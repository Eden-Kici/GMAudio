//! Dynamic music: a piece that changes intensity in time with its beat.
//!
//! A *music set* has tiers (Calm, Tense, Combat...). It comes in two kinds:
//!
//! - **Tracks**: each tier is its own recording. Changing tier crossfades
//!   to the new recording, started on a beat.
//! - **Stems**: one piece split into layers of identical length (drums,
//!   strings, brass) that play together in lockstep. A tier is a mix of
//!   those layers; changing tier fades layers in and out.
//!
//! Changes can wait for the next beat, bar, phrase or the end of the loop.
//! The wait is worked out from where the music actually is (its playback
//! position), not from a free-running clock, so it stays right when a file
//! loops or is not a whole number of bars long.

use kira::clock::{ClockHandle, ClockSpeed};
use kira::sound::PlaybackPosition;
use kira::StartTime;

use super::*;

/// When a change should happen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quantize {
    Now,
    Beat,
    #[default]
    Bar,
    Phrase,
    /// When the music reaches its end and starts over.
    Loop,
}

/// One synced layer of a stems set.
#[derive(Clone)]
pub struct StemRequest {
    pub name: String,
    pub request: PlayRequest,
    /// Decoded ahead of time, off the engine thread. Stems are held in
    /// memory whole, which is what keeps them sample-aligned.
    pub data: Option<StaticSoundData>,
}

impl std::fmt::Debug for StemRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stem({:?}, decoded: {})", self.name, self.data.is_some())
    }
}

#[derive(Debug, Clone, Default)]
pub struct TierRequest {
    pub name: String,
    /// Tracks kind: this tier's recording. `None` is a silent tier.
    pub track: Option<PlayRequest>,
    /// Stems kind: each stem's volume in dB in this tier, in stem order.
    /// [`STEM_OFF_DB`] or lower means the stem is silent.
    pub stem_db: Vec<f32>,
}

pub const STEM_OFF_DB: f32 = -60.0;

#[derive(Debug, Clone, Default)]
pub struct SetRequest {
    pub id: i64,
    pub name: String,
    pub bpm: f64,
    pub beats_per_bar: u32,
    pub bars_per_phrase: u32,
    /// Where the first beat falls in the audio.
    pub offset_ms: f64,
    /// How long a tier change takes once it starts.
    pub crossfade_ms: u32,
    /// When tier changes happen unless told otherwise.
    pub quantize: Quantize,
    pub tiers: Vec<TierRequest>,
    /// Empty for a tracks set.
    pub stems: Vec<StemRequest>,
    pub start_tier: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SetState {
    pub id: i64,
    pub name: String,
    pub tiers: Vec<String>,
    pub tier: usize,
    /// A tier change that is waiting for its beat.
    pub pending: Option<usize>,
    pub bpm: f64,
    pub beats_per_bar: u32,
    /// Position in the bar, counting from 0.
    pub beat: u32,
    pub bar: u64,
}

pub(super) struct ActiveSet {
    request: SetRequest,
    tier: usize,
    /// A tier change on its way: the tier, and the seconds of music still
    /// to play before it happens.
    pending: Option<(usize, f64)>,
    /// Where the music was at the last tick, to measure how much has played.
    last_position: f64,
    last_tick: Instant,
    /// Stems kind: one handle per stem, all started on the same sample.
    stems: Vec<Handle>,
    /// Tracks kind: the current tier's recording.
    track: Option<Handle>,
    /// Tracks kind: the next tier's recording, waiting for its beat.
    next_track: Option<Handle>,
    /// Holds the stems back until all are loaded, then releases them together.
    _gate: Option<ClockHandle>,
}

impl ActiveSet {
    fn reference(&self) -> Option<&Handle> {
        self.stems.first().or(self.track.as_ref())
    }

    /// Seconds into the reference recording.
    fn position(&self) -> f64 {
        self.reference().map_or(0.0, Handle::position)
    }

    /// Length of the reference recording in seconds, when known.
    fn duration(&self) -> Option<f64> {
        let request = match self.request.stems.first() {
            Some(stem) => Some(&stem.request),
            None => self.request.tiers.get(self.tier).and_then(|t| t.track.as_ref()),
        };
        request.and_then(|r| r.duration_ms).map(|ms| ms as f64 / 1000.0)
    }

    fn beat_seconds(&self) -> f64 {
        60.0 / self.request.bpm.clamp(20.0, 400.0)
    }

    /// Seconds until the next beat, bar, phrase or loop point.
    fn time_to(&self, quantize: Quantize) -> f64 {
        let position = self.position();
        let offset = self.request.offset_ms / 1000.0;
        let to_loop = self.duration().map(|d| (d - position).max(0.0));
        let unit = match quantize {
            Quantize::Now => return 0.0,
            Quantize::Loop => return to_loop.unwrap_or(0.0),
            Quantize::Beat => self.beat_seconds(),
            Quantize::Bar => self.beat_seconds() * self.request.beats_per_bar.max(1) as f64,
            Quantize::Phrase => {
                self.beat_seconds()
                    * self.request.beats_per_bar.max(1) as f64
                    * self.request.bars_per_phrase.max(1) as f64
            }
        };
        let since_first_beat = position - offset;
        let wait = if since_first_beat < 0.0 {
            -since_first_beat
        } else {
            // A hair past a boundary still counts as being on it.
            ((since_first_beat + 1e-4) / unit).floor() * unit + unit - since_first_beat
        };
        match to_loop {
            // The music starts over before that boundary: go when it does.
            Some(to_loop) if wait > to_loop + 1e-3 => to_loop + offset.max(0.0),
            _ => wait,
        }
    }

    /// Seconds of music played since this was last called.
    ///
    /// Measured from the music itself, not the wall clock, so a tier change
    /// stays on the beat even if the audio device stalls for a moment.
    fn played(&mut self) -> f64 {
        let now = Instant::now();
        let position = self.position();
        let mut played = position - self.last_position;
        if played < 0.0 {
            // The music looped.
            played = (played + self.duration().unwrap_or(0.0)).max(0.0);
        }
        if self.reference().is_none() {
            // A silent tier has no position to go by.
            played = now.duration_since(self.last_tick).as_secs_f64();
        }
        self.last_position = position;
        self.last_tick = now;
        played
    }

    fn state(&self) -> SetState {
        let since_first_beat = self.position() - self.request.offset_ms / 1000.0;
        let beats = if since_first_beat <= 0.0 { 0 } else { (since_first_beat / self.beat_seconds()) as u64 };
        let per_bar = self.request.beats_per_bar.max(1) as u64;
        SetState {
            id: self.request.id,
            name: self.request.name.clone(),
            tiers: self.request.tiers.iter().map(|t| t.name.clone()).collect(),
            tier: self.tier,
            pending: self.pending.map(|(tier, _)| tier),
            bpm: self.request.bpm,
            beats_per_bar: self.request.beats_per_bar.max(1),
            beat: (beats % per_bar) as u32,
            bar: beats / per_bar,
        }
    }

    fn handles(&mut self) -> impl Iterator<Item = &mut Handle> {
        self.stems.iter_mut().chain(self.track.iter_mut()).chain(self.next_track.iter_mut())
    }
}

fn delayed(seconds: f64, duration: Duration) -> Tween {
    Tween {
        start_time: if seconds > 0.0 { StartTime::Delayed(Duration::from_secs_f64(seconds)) } else { StartTime::Immediate },
        duration,
        easing: Easing::Linear,
    }
}

impl<B: Backend> Mixer<B> {
    pub fn set_state(&self) -> Option<SetState> {
        self.set.as_ref().map(ActiveSet::state)
    }

    pub(super) fn set_is_sounding(&self) -> bool {
        self.set.is_some() || !self.set_tails.is_empty()
    }

    /// Fades the music set out. Its sounds are kept until they fall silent.
    pub fn stop_set(&mut self, fade: Duration) {
        if let Some(mut set) = self.set.take() {
            let tween = Tween { duration: fade, ..fade_out(fade) };
            for handle in set.handles() {
                handle.stop(tween);
            }
            self.set_tails.extend(set.stems);
            self.set_tails.extend(set.track);
            self.set_tails.extend(set.next_track);
        }
        self.after_stingers.clear();
    }

    /// Cuts everything left of a stopped set (panic, second press).
    pub(super) fn cut_set_tails(&mut self) {
        for handle in &mut self.set_tails {
            handle.stop(Tween { duration: Duration::ZERO, ..Default::default() });
        }
    }

    /// Starts a music set at its starting tier, replacing any other music.
    pub fn play_set(&mut self, request: SetRequest) -> Result<()> {
        if request.tiers.is_empty() {
            return Err(Error::Unplayable(format!("\"{}\" has no levels yet.", request.name)));
        }
        let tier = request.start_tier.min(request.tiers.len() - 1);
        let fade = ms(self.settings.channels[Channel::Music.index()].fade_ms);
        let over_music = self.set_is_sounding() || self.sounds.iter().any(|s| s.channel == Channel::Music);
        let fade_in_tween = if over_music { fade_in(fade) } else { fade_in(Duration::from_millis(80)) };

        let mut set = ActiveSet {
            request,
            tier,
            pending: None,
            last_position: 0.0,
            last_tick: Instant::now(),
            stems: Vec::new(),
            track: None,
            next_track: None,
            _gate: None,
        };

        if !set.request.stems.is_empty() {
            // Every stem waits on a stopped clock; starting the clock
            // releases them in the same instant.
            let mut gate = self
                .manager
                .add_clock(ClockSpeed::TicksPerMinute(set.request.bpm.clamp(20.0, 400.0)))
                .map_err(|e| Error::Playback(e.to_string()))?;
            let start = StartTime::ClockTime(gate.time());
            let levels = set.request.tiers[tier].stem_db.clone();
            for (index, stem) in set.request.stems.iter().enumerate() {
                let data = match &stem.data {
                    Some(data) => data.clone(),
                    None => decode(&stem.request.path)?,
                };
                let level = levels.get(index).copied().unwrap_or(STEM_OFF_DB);
                let data = data
                    .volume(stem_volume(&stem.request, level))
                    .loop_region(..)
                    .start_time(start)
                    .fade_in_tween(Some(fade_in_tween));
                let handle = self.tracks[Channel::Music.index()]
                    .play(data)
                    .map_err(|e| Error::Playback(e.to_string()))?;
                set.stems.push(Handle::Static(handle));
            }
            gate.start();
            set._gate = Some(gate);
        } else if let Some(track) = set.request.tiers[tier].track.clone() {
            set.track = Some(self.start_tier_track(&track, 0.0, fade_in_tween, 0.0)?);
        }

        // Only now let go of the old music, so a set that fails to load
        // never leaves the table in silence.
        self.stop_set(fade);
        self.queue.clear();
        for s in self.sounds.iter_mut().filter(|s| s.channel == Channel::Music && !s.stopping) {
            s.handle.stop(fade_out(fade));
            s.stopping = true;
        }
        self.set = Some(set);
        self.panic_armed = false;
        Ok(())
    }

    fn start_tier_track(&mut self, track: &PlayRequest, delay: f64, fade: Tween, from: f64) -> Result<Handle> {
        let data = StreamingSoundData::from_file(&track.path)
            .map_err(|e| Error::Playback(format!("{}: {e}", track.name)))?
            .volume(track.gain_db + track.trim_db)
            .loop_region(..)
            .start_position(PlaybackPosition::Seconds(from))
            .start_time(if delay > 0.0 { StartTime::Delayed(Duration::from_secs_f64(delay)) } else { StartTime::Immediate })
            .fade_in_tween(Some(Tween { start_time: StartTime::Immediate, ..fade }));
        self.tracks[Channel::Music.index()]
            .play(data)
            .map(Handle::Streaming)
            .map_err(|e| Error::Playback(e.to_string()))
    }

    /// Changes tier. `quantize: None` uses the set's own setting.
    pub fn set_tier(&mut self, tier: usize, quantize: Option<Quantize>) -> Result<()> {
        let Some(set) = self.set.as_ref() else { return Ok(()) };
        let tier = tier.min(set.request.tiers.len().saturating_sub(1));
        let target_now = set.pending.map_or(set.tier, |(pending, _)| pending);
        if tier == target_now {
            return Ok(());
        }
        let wait = set.time_to(quantize.unwrap_or(set.request.quantize));
        let crossfade = ms(set.request.crossfade_ms);
        let offset = (set.request.offset_ms / 1000.0).max(0.0);
        let is_stems = !set.stems.is_empty();
        let next_track = set.request.tiers[tier].track.clone();

        if is_stems {
            let set = self.set.as_mut().expect("checked above");
            let levels = set.request.tiers[tier].stem_db.clone();
            for (index, handle) in set.stems.iter_mut().enumerate() {
                let level = levels.get(index).copied().unwrap_or(STEM_OFF_DB);
                let volume = stem_volume(&set.request.stems[index].request, level);
                handle.set_volume(volume, delayed(wait, crossfade));
            }
        } else {
            // On a beat, the new recording starts from its own first beat.
            let from = if wait > 0.0 { offset } else { 0.0 };
            let incoming = match &next_track {
                Some(track) => Some(self.start_tier_track(track, wait, fade_in(crossfade), from)?),
                None => None,
            };
            let set = self.set.as_mut().expect("checked above");
            // A change that was still waiting is called off.
            if let Some(mut cancelled) = set.next_track.take() {
                cancelled.stop(Tween { duration: Duration::ZERO, ..Default::default() });
                self.set_tails.push(cancelled);
            }
            if let Some(current) = set.track.as_mut() {
                current.stop(delayed(wait, crossfade));
            }
            set.next_track = incoming;
        }
        let set = self.set.as_mut().expect("checked above");
        set.played(); // count the wait from this moment
        set.pending = Some((tier, wait));
        self.settle_set();
        Ok(())
    }

    /// Moves up or down by a number of tiers.
    pub fn step_tier(&mut self, delta: i32) -> Result<()> {
        let Some(set) = self.set.as_ref() else { return Ok(()) };
        let from = set.pending.map_or(set.tier, |(pending, _)| pending) as i32;
        let last = set.request.tiers.len() as i32 - 1;
        self.set_tier((from + delta).clamp(0, last.max(0)) as usize, None)
    }

    /// Plays a stinger on the next beat or bar of the music set (at once if
    /// none is playing). `then_tier` changes tier when the stinger ends.
    pub fn play_stinger(&mut self, mut request: PlayRequest, quantize: Quantize, then_tier: Option<usize>) -> Result<u64> {
        request.channel = Channel::Stingers;
        request.looping = Some(false);
        let wait = self.set.as_ref().map_or(0.0, |set| set.time_to(quantize));
        request.delay_ms = (wait * 1000.0) as u32;
        let id = self.play(request)?;
        if let Some(tier) = then_tier {
            if self.set.is_some() {
                self.after_stingers.push((id, tier));
            }
        }
        Ok(id)
    }

    /// Makes a tier change that has reached its beat the current tier.
    fn settle_set(&mut self) {
        let Some(set) = self.set.as_mut() else { return };
        let played = set.played();
        if let Some((tier, remaining)) = set.pending {
            let remaining = remaining - played;
            if remaining > 0.005 {
                set.pending = Some((tier, remaining));
                return;
            }
            set.pending = None;
            set.tier = tier;
            if set.stems.is_empty() {
                if let Some(old) = set.track.take() {
                    self.set_tails.push(old);
                }
                set.track = set.next_track.take();
                // The new recording is the reference from here on.
                set.last_position = set.position();
            }
        }
    }

    /// Housekeeping for the music set, once per engine tick.
    pub(super) fn tick_set(&mut self) {
        self.settle_set();
        self.set_tails.retain(|h| h.state() != PlaybackState::Stopped);

        // Stingers that have finished hand over to their tier.
        let mut due = Vec::new();
        self.after_stingers.retain(|(id, tier)| {
            let playing = self.sounds.iter().any(|s| s.id == *id);
            if !playing {
                due.push(*tier);
            }
            playing
        });
        if let Some(tier) = due.last() {
            if let Err(e) = self.set_tier(*tier, Some(Quantize::Now)) {
                self.last_error = Some(e.to_string());
            }
        }

        // A streamed tier that broke mid-play.
        if let Some(set) = self.set.as_mut() {
            let name = set.request.name.clone();
            for handle in set.handles() {
                if let Some(e) = handle.pop_error() {
                    self.last_error = Some(format!("{name}: {e}"));
                }
            }
        }
    }
}

/// A stem's volume: its levelling gain plus its level in the tier.
fn stem_volume(stem: &PlayRequest, level_db: f32) -> f32 {
    if level_db <= STEM_OFF_DB {
        Decibels::SILENCE.0
    } else {
        stem.gain_db + stem.trim_db + level_db
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    /// A set at 120 BPM (a beat is 500 ms, a bar 2 s) over 30-second files.
    fn tracks_set(f: &Files) -> SetRequest {
        let tier = |name: &str, asset_id: i64| TierRequest {
            name: name.into(),
            track: Some(PlayRequest { asset_id, ..req(&f.long, Channel::Music, 30_000) }),
            stem_db: vec![],
        };
        SetRequest {
            id: 1,
            name: "Chase".into(),
            bpm: 120.0,
            beats_per_bar: 4,
            bars_per_phrase: 4,
            crossfade_ms: 100,
            quantize: Quantize::Bar,
            tiers: vec![tier("Calm", 1), tier("Tense", 2), tier("Combat", 3)],
            ..Default::default()
        }
    }

    fn stems_set(f: &Files) -> SetRequest {
        let stem = |name: &str| StemRequest {
            name: name.into(),
            request: req(&f.long, Channel::Music, 30_000),
            data: None,
        };
        let tier = |name: &str, levels: [f32; 3]| TierRequest { name: name.into(), track: None, stem_db: levels.to_vec() };
        SetRequest {
            id: 2,
            name: "Battle".into(),
            bpm: 120.0,
            beats_per_bar: 4,
            bars_per_phrase: 4,
            crossfade_ms: 100,
            quantize: Quantize::Bar,
            stems: vec![stem("Drums"), stem("Strings"), stem("Brass")],
            tiers: vec![
                tier("Calm", [0.0, STEM_OFF_DB, STEM_OFF_DB]),
                tier("Tense", [0.0, 0.0, STEM_OFF_DB]),
                tier("Combat", [0.0, 0.0, -3.0]),
            ],
            ..Default::default()
        }
    }

    fn set(m: &Mixer<MockBackend>) -> SetState {
        m.set_state().expect("a set is playing")
    }

    #[test]
    fn a_set_replaces_other_music_and_reports_its_beat() {
        let f = files();
        let mut m = mixer();
        m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        m.play_set(tracks_set(&f)).unwrap();

        let s = m.state();
        assert!(s.sounds.iter().all(|x| x.stopping), "the old track fades out");
        assert!(s.channels[Channel::Music.index()].queue.is_empty());
        let state = s.music_set.unwrap();
        assert_eq!(state.tiers, ["Calm", "Tense", "Combat"]);
        assert_eq!((state.tier, state.pending, state.bar, state.beat), (0, None, 0, 0));

        // 2.6 s in at 120 BPM: second bar, second beat.
        play_for(&mut m, 2600);
        let state = set(&m);
        assert_eq!((state.bar, state.beat), (1, 1), "{state:?}");

        // Ordinary music takes over again, and Stop ends a set.
        m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        assert!(m.set_state().is_none());
        m.play_set(tracks_set(&f)).unwrap();
        m.stop_channel(Channel::Music, Some(50));
        assert!(m.set_state().is_none());
        render_until(&mut m, |m| m.sound_count() == 0 && !m.set_is_sounding());
    }

    #[test]
    fn a_tier_change_waits_for_the_bar_and_lands_on_the_downbeat() {
        let f = files();
        let mut m = mixer();
        m.play_set(tracks_set(&f)).unwrap();
        play_for(&mut m, 700); // 0.7 s into a 2 s bar

        m.set_tier(2, None).unwrap();
        let state = set(&m);
        assert_eq!((state.tier, state.pending), (0, Some(2)), "waiting for the bar line");

        // Just before the bar line nothing has changed...
        play_for(&mut m, 1100);
        assert_eq!(set(&m).pending, Some(2));
        // ...and just after it the new tier is in, starting from its top.
        render_until(&mut m, |m| m.set_state().is_some_and(|s| s.pending.is_none()));
        let state = set(&m);
        assert_eq!(state.tier, 2);
        let landed = m.set.as_ref().unwrap().position();
        assert!(landed < 0.12, "new tier is {landed:.3}s past its downbeat");

        // "Now" does not wait.
        m.set_tier(0, Some(Quantize::Now)).unwrap();
        assert_eq!((set(&m).tier, set(&m).pending), (0, None));

        // Stepping clamps at both ends and counts from a pending change.
        m.step_tier(-1).unwrap();
        assert_eq!(set(&m).pending, None);
        m.step_tier(1).unwrap();
        m.step_tier(1).unwrap();
        assert_eq!(set(&m).pending, Some(2));
        m.step_tier(5).unwrap();
        assert_eq!(set(&m).pending, Some(2));
    }

    #[test]
    fn waits_are_measured_from_the_first_beat() {
        let f = files();
        let mut m = mixer();
        let mut request = tracks_set(&f);
        request.offset_ms = 250.0;
        m.play_set(request).unwrap();
        play_for(&mut m, 1000);
        let active = m.set.as_ref().unwrap();
        // About a second in: the first beat was at 0.25 s, beats are 0.5 s
        // apart, bars 2 s, phrases 8 s, and the file is 30 s long.
        let position = active.position();
        assert!((0.9..1.2).contains(&position), "{position}");
        let close = |found: f64, expected: f64| (found - expected).abs() < 1e-6;
        assert!(close(active.time_to(Quantize::Beat), 1.25 - position), "{}", active.time_to(Quantize::Beat));
        assert!(close(active.time_to(Quantize::Bar), 2.25 - position), "{}", active.time_to(Quantize::Bar));
        assert!(close(active.time_to(Quantize::Phrase), 8.25 - position), "{}", active.time_to(Quantize::Phrase));
        assert!(close(active.time_to(Quantize::Loop), 30.0 - position), "{}", active.time_to(Quantize::Loop));
        assert_eq!(active.time_to(Quantize::Now), 0.0);
    }

    #[test]
    fn stems_start_together_and_stay_together() {
        let f = files();
        let mut m = mixer();
        m.play_set(stems_set(&f)).unwrap();
        assert_eq!(set(&m).tiers.len(), 3);

        // Thirty minutes of music, rendered as fast as the machine allows.
        // Stems are held in memory, so no disk reads can pull them apart.
        for minute in 0..30 {
            render(&mut m, 60_000);
            let positions: Vec<f64> = m.set.as_ref().unwrap().stems.iter().map(Handle::position).collect();
            assert!(
                positions.iter().all(|p| *p == positions[0]),
                "minute {minute}: stems drifted apart: {positions:?}"
            );
        }
        // Having looped many times, the stems are back inside the file.
        assert!(m.set.as_ref().unwrap().position() < 30.0);
        render(&mut m, 5_000);

        // A tier change fades stems; nothing restarts.
        let before = m.set.as_ref().unwrap().position();
        m.set_tier(2, Some(Quantize::Now)).unwrap();
        render(&mut m, 200);
        let after = m.set.as_ref().unwrap().position();
        assert_eq!(set(&m).tier, 2);
        assert!((after - before - 0.2).abs() < 0.03, "{before} -> {after}");

        // A change on the bar line is counted in music played, so it lands
        // on time however fast or slowly the audio is rendered.
        m.set_tier(0, Some(Quantize::Bar)).unwrap();
        assert_eq!(set(&m).pending, Some(0));
        let wait = m.set.as_ref().unwrap().time_to(Quantize::Bar);
        render(&mut m, ((wait - 0.05) * 1000.0) as u64);
        assert_eq!(set(&m).pending, Some(0), "not before the bar line");
        render(&mut m, 100);
        assert_eq!((set(&m).tier, set(&m).pending), (0, None));
        assert_eq!(set(&m).beat, 0, "it is the first beat of a bar");
    }

    #[test]
    fn a_stinger_waits_for_the_beat_and_can_change_tier_afterwards() {
        let f = files();
        let mut m = mixer();
        m.play_set(tracks_set(&f)).unwrap();
        m.set_tier(2, Some(Quantize::Now)).unwrap();
        play_for(&mut m, 300); // 0.3 s into a 0.5 s beat

        let id = m.play_stinger(req(&f.short, Channel::Sfx, 300), Quantize::Beat, Some(0)).unwrap();
        let stinger = |m: &Mixer<MockBackend>| m.state().sounds.into_iter().find(|s| s.id == id);
        assert_eq!(stinger(&m).unwrap().channel, Channel::Stingers);
        assert_eq!(stinger(&m).unwrap().position_ms, 0, "held back until the beat");
        play_for(&mut m, 100);
        assert_eq!(stinger(&m).unwrap().position_ms, 0);
        assert_eq!(set(&m).tier, 2, "the tier holds while the stinger waits and plays");

        // It plays on the beat, and when it ends the music drops to Calm.
        render_until(&mut m, |m| m.state().sounds.iter().all(|s| s.id != id));
        render_until(&mut m, |m| m.set_state().is_some_and(|s| s.tier == 0));

        // With no set playing, a stinger just plays.
        m.stop_set(Duration::ZERO);
        let id = m.play_stinger(req(&f.short, Channel::Sfx, 300), Quantize::Bar, Some(1)).unwrap();
        play_for(&mut m, 100);
        assert!(m.state().sounds.iter().any(|s| s.id == id && s.position_ms > 0));
    }

    #[test]
    fn panic_silences_a_set() {
        let f = files();
        let mut m = mixer();
        m.play_set(stems_set(&f)).unwrap();
        assert_eq!(m.panic(), PanicOutcome::Fading);
        assert!(m.set_state().is_none());
        assert_eq!(m.panic(), PanicOutcome::Cut);
        render_until(&mut m, |m| !m.set_is_sounding());
        assert_eq!(m.panic(), PanicOutcome::Idle);

        // A set with nothing in it is refused with a plain message.
        let empty = SetRequest { name: "Empty".into(), ..Default::default() };
        assert!(matches!(m.play_set(empty), Err(Error::Unplayable(_))));
    }
}
