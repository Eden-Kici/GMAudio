//! The audio engine.
//!
//! [`Mixer`] owns the audio device and every playing sound. [`Engine`] runs
//! a mixer on its own thread: the rest of the app only sends it commands
//! and receives state snapshots, so playback never depends on the UI.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use kira::backend::Backend;
use kira::effect::compressor::{CompressorBuilder, CompressorHandle};
use kira::effect::eq_filter::{EqFilterBuilder, EqFilterHandle, EqFilterKind};
use kira::effect::{Effect, EffectBuilder};
use kira::info::Info;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{FromFileError, PlaybackState, Region};
use kira::track::{TrackBuilder, TrackHandle};
use kira::{
    AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Easing, Frame, Mix, Panning, Tween,
};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// How often the engine thread wakes to update state (~30 Hz).
const TICK: Duration = Duration::from_millis(33);
/// Volume and mute changes glide over this long so they never click.
const SMOOTH: Duration = Duration::from_millis(40);
/// First panic press fades everything out over this long.
const PANIC_FADE: Duration = Duration::from_millis(1000);
/// While there is no audio device, look for one again this often.
const DEVICE_RETRY: Duration = Duration::from_secs(3);
/// One-shots longer than this are streamed from disk instead of being
/// decoded into memory up front.
const STATIC_MAX_MS: i64 = 20_000;
mod music_set;
pub use music_set::{Quantize, STEM_OFF_DB, SetRequest, SetState, StemRequest, TierRequest};
use music_set::ActiveSet;

const COMMAND_LOG_SIZE: usize = 300;
/// How fast music drops when a ducking sound starts, and recovers after.
const DUCK_ATTACK: Duration = Duration::from_millis(120);
const DUCK_RELEASE: Duration = Duration::from_millis(700);
/// Preloaded sounds are kept up to this many frames in total (~160 MB).
const CACHE_MAX_FRAMES: usize = 20_000_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Music,
    Ambience,
    #[default]
    Sfx,
    Stingers,
}

impl Channel {
    pub const ALL: [Channel; 4] = [
        Channel::Music,
        Channel::Ambience,
        Channel::Sfx,
        Channel::Stingers,
    ];

    fn index(self) -> usize {
        self as usize
    }

    /// Ambience layers repeat unless told otherwise. Music tracks play
    /// through a queue (which repeats as a whole, see
    /// [`ChannelSettings::loop_queue`]); one-shots play once.
    fn loops_by_default(self) -> bool {
        matches!(self, Channel::Ambience)
    }

    fn default_fade_ms(self) -> u32 {
        match self {
            Channel::Music | Channel::Ambience => 3000,
            Channel::Sfx | Channel::Stingers => 150,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChannelSettings {
    /// Fader position, 0.0 to 1.0.
    pub volume: f32,
    pub muted: bool,
    /// Default fade time for starting, stopping and crossfading.
    pub fade_ms: u32,
    /// Music only: when the queue runs out, start it again from the top.
    /// With a single track this simply repeats that track.
    #[serde(default = "default_true")]
    pub loop_queue: bool,
    /// While any channel is soloed, only soloed channels are heard.
    #[serde(default)]
    pub solo: bool,
}

fn default_true() -> bool {
    true
}

/// How the final mix is shaped for the speakers or headphones in use.
///
/// Applied on the master, after every channel. Each file's own loudness
/// levelling still applies underneath.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioProfile {
    /// Bass boost or cut in dB (a shelf below about 120 Hz).
    pub bass_db: f32,
    /// Treble boost or cut in dB (a shelf above about 6 kHz).
    pub treble_db: f32,
    /// Overall level in dB.
    pub gain_db: f32,
    /// 0 to 1: how much loud and quiet passages are pulled together.
    pub compress: f32,
    /// Catch peaks just before they would distort.
    pub limit: bool,
}

impl Default for AudioProfile {
    fn default() -> Self {
        Self { bass_db: 0.0, treble_db: 0.0, gain_db: 0.0, compress: 0.0, limit: true }
    }
}

impl AudioProfile {
    fn sanitized(self) -> Self {
        let clean = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { 0.0 };
        Self {
            bass_db: clean(self.bass_db, -18.0, 12.0),
            treble_db: clean(self.treble_db, -18.0, 12.0),
            gain_db: clean(self.gain_db, -24.0, 12.0),
            compress: clean(self.compress, 0.0, 1.0),
            limit: self.limit,
        }
    }
}

/// Everything about the mixer that is remembered between sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixerSettings {
    pub master_volume: f32,
    /// In [`Channel::ALL`] order.
    pub channels: [ChannelSettings; 4],
    /// How far music drops (in dB, negative) while a ducking sound plays.
    #[serde(default = "default_duck_db")]
    pub duck_db: f32,
    #[serde(default)]
    pub profile: AudioProfile,
}

fn default_duck_db() -> f32 {
    -8.0
}

impl Default for MixerSettings {
    fn default() -> Self {
        Self {
            master_volume: 0.8,
            channels: Channel::ALL.map(|c| ChannelSettings {
                volume: 0.8,
                muted: false,
                fade_ms: c.default_fade_ms(),
                loop_queue: true,
                solo: false,
            }),
            duck_db: default_duck_db(),
            profile: AudioProfile::default(),
        }
    }
}

impl MixerSettings {
    /// Clamps values loaded from disk into their valid ranges.
    pub fn sanitized(mut self) -> Self {
        self.master_volume = clamp_unit(self.master_volume);
        for c in &mut self.channels {
            c.volume = clamp_unit(c.volume);
            c.fade_ms = c.fade_ms.min(60_000);
        }
        self.profile = self.profile.sanitized();
        self.duck_db = if self.duck_db.is_finite() { self.duck_db.clamp(-40.0, 0.0) } else { default_duck_db() };
        self
    }
}

fn clamp_unit(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
}

/// Maps a fader position (0..1) to decibels.
///
/// The curve is amplitude = position², which feels even to the ear: half
/// way down is -12 dB, not the barely audible -6 dB of a linear fader.
pub fn fader_to_db(position: f32) -> f32 {
    let p = clamp_unit(position);
    if p <= 0.0 {
        return Decibels::SILENCE.0;
    }
    (40.0 * p.log10()).max(Decibels::SILENCE.0)
}

#[derive(Debug, Clone, Default)]
pub struct PlayRequest {
    pub asset_id: i64,
    pub path: PathBuf,
    pub name: String,
    pub channel: Channel,
    /// Loudness-normalization gain for this file.
    pub gain_db: f32,
    pub duration_ms: Option<i64>,
    /// Repeat this sound until it is stopped. `None` uses the channel default.
    pub looping: Option<bool>,
    /// Music only: crossfade to this track right now instead of queueing
    /// it behind the one that is playing.
    pub interrupt: bool,
    /// Volume of this use of the sound, on top of `gain_db`: a pad's or a
    /// scene layer's own level.
    pub trim_db: f32,
    /// Playback speed, which also shifts pitch. 0 means normal speed.
    pub rate: f64,
    /// -1 (left) to 1 (right).
    pub pan: f32,
    /// Lower the music while this plays. `None`: stingers do, others do not.
    pub duck: Option<bool>,
    /// Who started it (e.g. `pad:12:3`), so the UI can light the right button.
    pub tag: Option<String>,
    /// Hold the sound back this long before it starts (a stinger waiting
    /// for the beat).
    pub delay_ms: u32,
    /// The sound is set to always repeat, whatever started it.
    pub always_loop: bool,
    /// Where the sound begins, when its opening has been trimmed off.
    pub start_ms: i64,
    /// Where the sound ends, when its tail has been trimmed off.
    pub end_ms: Option<i64>,
    /// Stretches in between that are skipped, as (from, to), in order.
    pub cuts: Vec<(i64, i64)>,
    /// Begin here instead of at the start: a sound moved while playing.
    pub resume_ms: Option<i64>,
}

impl PlayRequest {
    /// The part of the file that plays, in seconds, for looping.
    fn region(&self) -> Region {
        let start = self.start_ms.max(0) as f64 / 1000.0;
        match self.end_ms {
            Some(end) if end > self.start_ms => Region::from(start..end as f64 / 1000.0),
            _ => Region::from(start..),
        }
    }
}

/// A layer that plays a sound from its pool every so often: distant
/// thunder, a creaking floorboard.
#[derive(Debug, Clone)]
pub struct EmitterRequest {
    pub name: String,
    pub sounds: Vec<PlayRequest>,
    pub min_ms: u32,
    pub max_ms: u32,
    /// Place each occurrence somewhere different between the speakers.
    pub random_pan: bool,
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub enum SceneMusic {
    /// Leave whatever music is playing alone.
    #[default]
    Keep,
    /// Fade the music out.
    Silence,
    /// Play these tracks in order (and round again if the queue repeats).
    Tracks(Vec<PlayRequest>),
}

/// A whole soundscape, applied in one go.
#[derive(Debug, Clone, Default)]
pub struct SceneRequest {
    pub id: i64,
    pub name: String,
    /// How long the change takes.
    pub transition_ms: u32,
    pub music: SceneMusic,
    /// Looping ambience layers.
    pub layers: Vec<PlayRequest>,
    pub emitters: Vec<EmitterRequest>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EmitterState {
    pub id: u64,
    pub name: String,
    pub min_ms: u32,
    pub max_ms: u32,
    pub tag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SceneState {
    pub id: i64,
    pub name: String,
}

/// A music track waiting for its turn.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueuedState {
    pub id: u64,
    pub asset_id: i64,
    pub name: String,
    pub duration_ms: Option<i64>,
    pub looping: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelState {
    pub channel: Channel,
    pub volume: f32,
    pub muted: bool,
    pub fade_ms: u32,
    pub loop_queue: bool,
    pub solo: bool,
    /// Tracks waiting to play, next first. Only music has a queue.
    pub queue: Vec<QueuedState>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SoundState {
    pub id: u64,
    pub asset_id: i64,
    pub name: String,
    pub channel: Channel,
    pub position_ms: i64,
    pub duration_ms: Option<i64>,
    /// Set to repeat until stopped.
    pub looping: bool,
    /// Fading out; it will be gone when the fade ends.
    pub stopping: bool,
    /// Held where it is until resumed.
    pub paused: bool,
    pub trim_db: f32,
    pub tag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EngineState {
    pub device_ok: bool,
    pub device_error: Option<String>,
    pub master_volume: f32,
    pub channels: Vec<ChannelState>,
    pub sounds: Vec<SoundState>,
    /// A panic fade is running: the next panic press cuts instantly.
    pub panic_armed: bool,
    /// The most recent playback problem (e.g. a file that failed mid-stream).
    pub last_error: Option<String>,
    /// Random-interval layers that are running.
    pub emitters: Vec<EmitterState>,
    /// The scene applied last, until something stops everything.
    pub scene: Option<SceneState>,
    /// Music is lowered right now because a ducking sound is playing.
    pub ducked: bool,
    pub duck_db: f32,
    /// Peak level since the last update, 0 to 1 and beyond: the four
    /// channels in order, then the master.
    pub levels: [f32; 5],
    /// The dynamic music set that is playing, if any.
    pub music_set: Option<SetState>,
    pub profile: AudioProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PanicOutcome {
    /// Nothing was playing.
    Idle,
    /// Everything is fading out.
    Fading,
    /// Everything was cut instantly.
    Cut,
}

enum Handle {
    Static(StaticSoundHandle),
    Streaming(StreamingSoundHandle<FromFileError>),
}

impl Handle {
    fn state(&self) -> PlaybackState {
        match self {
            Handle::Static(h) => h.state(),
            Handle::Streaming(h) => h.state(),
        }
    }
    fn position(&self) -> f64 {
        match self {
            Handle::Static(h) => h.position(),
            Handle::Streaming(h) => h.position(),
        }
    }
    fn pause(&mut self, tween: Tween) {
        match self {
            Handle::Static(h) => h.pause(tween),
            Handle::Streaming(h) => h.pause(tween),
        }
    }
    fn resume(&mut self, tween: Tween) {
        match self {
            Handle::Static(h) => h.resume(tween),
            Handle::Streaming(h) => h.resume(tween),
        }
    }
    fn seek_to(&mut self, seconds: f64) {
        match self {
            Handle::Static(h) => h.seek_to(seconds),
            Handle::Streaming(h) => h.seek_to(seconds),
        }
    }
    fn stop(&mut self, tween: Tween) {
        match self {
            Handle::Static(h) => h.stop(tween),
            Handle::Streaming(h) => h.stop(tween),
        }
    }
    fn pop_error(&mut self) -> Option<String> {
        match self {
            Handle::Static(_) => None,
            Handle::Streaming(h) => h.pop_error().map(|e| e.to_string()),
        }
    }
    fn set_volume(&mut self, db: f32, tween: Tween) {
        match self {
            Handle::Static(h) => h.set_volume(db, tween),
            Handle::Streaming(h) => h.set_volume(db, tween),
        }
    }
    /// Repeats the whole sound, or lets it run to its end.
    fn set_repeat(&mut self, on: bool, part: Region) {
        let region = on.then_some(part);
        match self {
            Handle::Static(h) => h.set_loop_region(region),
            Handle::Streaming(h) => h.set_loop_region(region),
        }
    }
}

struct ActiveSound {
    id: u64,
    asset_id: i64,
    name: String,
    channel: Channel,
    duration_ms: Option<i64>,
    /// What the GM asked for: repeat this sound until stopped.
    looping: bool,
    /// What the audio backend is doing right now. Differs from `looping`
    /// for a lone music track that repeats because the queue does.
    repeating: bool,
    stopping: bool,
    /// The stream broke; never put this sound back in the queue.
    failed: bool,
    /// Reached its trimmed end: being faded out, but it counts as having
    /// played through.
    finished: bool,
    duck: bool,
    handle: Handle,
    /// Kept so a finished music track can rejoin a repeating queue.
    request: PlayRequest,
}

struct Queued {
    id: u64,
    looping: bool,
    request: PlayRequest,
}

struct Emitter {
    id: u64,
    request: EmitterRequest,
    next_at: Instant,
    last: Option<usize>,
}

/// The master's tone and dynamics controls.
struct Shaping {
    bass: EqFilterHandle,
    treble: EqFilterHandle,
    compressor: CompressorHandle,
    limiter: CompressorHandle,
}

impl Shaping {
    fn apply(&mut self, profile: &AudioProfile, tween: Tween) {
        self.bass.set_gain(profile.bass_db, tween);
        self.treble.set_gain(profile.treble_db, tween);
        // More compression: a lower threshold, a steeper ratio, and gain
        // to make up for what is taken off the loud parts.
        let c = profile.compress as f64;
        self.compressor.set_threshold(-12.0 - 18.0 * c, tween);
        self.compressor.set_ratio(1.0 + 3.0 * c, tween);
        self.compressor.set_makeup_gain((6.0 * c) as f32, tween);
        self.compressor.set_mix(if c > 0.0 { Mix::WET } else { Mix::DRY }, tween);
        self.limiter.set_mix(if profile.limit { Mix::WET } else { Mix::DRY }, tween);
    }
}

/// Measures the peak level passing through a track.
struct Meter(Arc<AtomicU32>);

struct MeterBuilder;

impl EffectBuilder for MeterBuilder {
    type Handle = Arc<AtomicU32>;
    fn build(self) -> (Box<dyn Effect>, Self::Handle) {
        let peak = Arc::new(AtomicU32::new(0));
        (Box::new(Meter(peak.clone())), peak)
    }
}

impl Effect for Meter {
    fn process(&mut self, input: &mut [Frame], _dt: f64, _info: &Info) {
        let mut peak = f32::from_bits(self.0.load(Ordering::Relaxed));
        for frame in input.iter() {
            peak = peak.max(frame.left.abs()).max(frame.right.abs());
        }
        self.0.store(peak.to_bits(), Ordering::Relaxed);
    }
}

/// Reads a meter and resets it, rounded so tiny changes do not count.
fn take_level(meter: &AtomicU32) -> f32 {
    let peak = f32::from_bits(meter.swap(0, Ordering::Relaxed));
    (peak * 100.0).round() / 100.0
}

// Volume fades are linear in decibels, so a plain linear tween would spend
// most of a fade-in inaudible and dip in the middle of a crossfade. These
// two curves keep the summed power of a crossfade close to constant
// (about -0.7 dB at the midpoint).
fn fade_in(duration: Duration) -> Tween {
    Tween {
        duration,
        easing: Easing::OutPowi(4),
        ..Default::default()
    }
}

fn fade_out(duration: Duration) -> Tween {
    Tween {
        duration,
        easing: Easing::InPowi(4),
        ..Default::default()
    }
}

fn smooth() -> Tween {
    Tween {
        duration: SMOOTH,
        ..Default::default()
    }
}

fn ms(v: u32) -> Duration {
    Duration::from_millis(v as u64)
}

/// The mixer: four channels feeding a master, and the sounds on them.
pub struct Mixer<B: Backend = DefaultBackend> {
    manager: AudioManager<B>,
    /// Everything passes through this track on its way out: the master.
    bus: TrackHandle,
    tracks: Vec<TrackHandle>,
    shaping: Shaping,
    meters: Vec<Arc<AtomicU32>>,
    levels: [f32; 5],
    emitters: Vec<Emitter>,
    scene: Option<SceneState>,
    ducked: bool,
    /// The dynamic music set that is playing.
    set: Option<ActiveSet>,
    /// Sounds of a stopped set, fading out.
    set_tails: Vec<Handle>,
    /// (stinger sound id, tier to change to when it ends)
    after_stingers: Vec<(u64, usize)>,
    /// Short sounds decoded ahead of time, for instant starts.
    cache: HashMap<PathBuf, StaticSoundData>,
    cache_order: VecDeque<PathBuf>,
    settings: MixerSettings,
    sounds: Vec<ActiveSound>,
    /// Music tracks waiting for their turn.
    queue: VecDeque<Queued>,
    next_id: u64,
    panic_armed: bool,
    last_error: Option<String>,
}

impl<B: Backend> Mixer<B> {
    pub fn new(mut manager: AudioManager<B>, settings: MixerSettings) -> Result<Self> {
        let settings = settings.sanitized();
        let limit = |e: kira::ResourceLimitReached| Error::Playback(e.to_string());
        let mut meters = Vec::with_capacity(5);
        let mut bus_builder =
            TrackBuilder::new().volume(fader_to_db(settings.master_volume) + settings.profile.gain_db);
        let mut shaping = Shaping {
            bass: bus_builder.add_effect(EqFilterBuilder::new(EqFilterKind::LowShelf, 120.0, 0.0, 0.7)),
            treble: bus_builder.add_effect(EqFilterBuilder::new(EqFilterKind::HighShelf, 6000.0, 0.0, 0.7)),
            compressor: bus_builder.add_effect(
                CompressorBuilder::new()
                    .attack_duration(Duration::from_millis(15))
                    .release_duration(Duration::from_millis(250)),
            ),
            limiter: bus_builder.add_effect(
                CompressorBuilder::new()
                    .threshold(-1.5)
                    .ratio(20.0)
                    .attack_duration(Duration::from_millis(1))
                    .release_duration(Duration::from_millis(80)),
            ),
        };
        shaping.apply(&settings.profile, Tween { duration: Duration::ZERO, ..Default::default() });
        let master_meter = bus_builder.add_effect(MeterBuilder);
        let mut bus = manager.add_sub_track(bus_builder).map_err(limit)?;
        let mut tracks = Vec::with_capacity(4);
        for channel in Channel::ALL {
            let mut builder = TrackBuilder::new().volume(channel_db(&settings, channel, false));
            meters.push(builder.add_effect(MeterBuilder));
            tracks.push(bus.add_sub_track(builder).map_err(limit)?);
        }
        meters.push(master_meter);
        Ok(Self {
            manager,
            bus,
            shaping,
            meters,
            levels: [0.0; 5],
            emitters: Vec::new(),
            scene: None,
            ducked: false,
            set: None,
            set_tails: Vec::new(),
            after_stingers: Vec::new(),
            cache: HashMap::new(),
            cache_order: VecDeque::new(),
            tracks,
            settings,
            sounds: Vec::new(),
            queue: VecDeque::new(),
            next_id: 1,
            panic_armed: false,
            last_error: None,
        })
    }

    pub fn settings(&self) -> &MixerSettings {
        &self.settings
    }

    pub fn manager_mut(&mut self) -> &mut AudioManager<B> {
        &mut self.manager
    }

    /// Plays a sound and returns its id.
    ///
    /// Music plays one track at a time: a new track waits in the queue
    /// behind the current one, unless the request says to interrupt, in
    /// which case it crossfades in right away. Ambience layers stack.
    /// One-shots overlap freely.
    pub fn play(&mut self, req: PlayRequest) -> Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        if req.channel == Channel::Music && !req.interrupt && self.music_is_playing() {
            self.queue.push_back(Queued {
                id,
                looping: req.always_loop || req.looping.unwrap_or(false),
                request: req,
            });
            self.refresh_repeats();
            return Ok(id);
        }
        self.start(id, req, None)?;
        self.refresh_repeats();
        self.refresh_duck();
        Ok(id)
    }

    /// A music track that is playing and not on its way out.
    fn music_is_playing(&self) -> bool {
        self.sounds
            .iter()
            .any(|s| s.channel == Channel::Music && !s.stopping)
    }

    fn loop_queue(&self) -> bool {
        self.settings.channels[Channel::Music.index()].loop_queue
    }

    /// `fade` overrides the channel's fade time (a scene's transition).
    fn start(&mut self, id: u64, req: PlayRequest, fade: Option<Duration>) -> Result<()> {
        let channel = req.channel;
        let fade = fade.unwrap_or(ms(self.settings.channels[channel.index()].fade_ms));
        let looping = req.always_loop || req.looping.unwrap_or(channel.loops_by_default());
        let begin = req.resume_ms.unwrap_or(req.start_ms).max(0) as f64 / 1000.0;
        let repeating = match channel {
            Channel::Music => looping || (self.loop_queue() && self.queue.is_empty()),
            _ => looping,
        };
        // An ambience sound that does not repeat is an occasional one-off
        // (a random-interval layer), so it starts like an effect.
        let one_shot = matches!(channel, Channel::Sfx | Channel::Stingers)
            || (channel == Channel::Ambience && !looping);
        let duck = req.duck.unwrap_or(channel == Channel::Stingers);
        let volume = req.gain_db + req.trim_db;
        let rate = if req.rate > 0.0 { req.rate } else { 1.0 };
        let pan = Panning(req.pan.clamp(-1.0, 1.0));

        // Crossfade when other music is sounding (playing or fading out);
        // otherwise the track starts promptly.
        let over_music = channel == Channel::Music
            && (self.sounds.iter().any(|s| s.channel == Channel::Music) || self.set_is_sounding());
        let start_time = if req.delay_ms > 0 {
            kira::StartTime::Delayed(ms(req.delay_ms))
        } else {
            kira::StartTime::Immediate
        };
        let fade_in_tween = match channel {
            Channel::Music if over_music => Some(fade_in(fade)),
            Channel::Music => Some(fade_in(Duration::from_millis(80))),
            Channel::Ambience if !one_shot => Some(fade_in(fade)),
            _ => None,
        };

        let load_err = |e: FromFileError| Error::Playback(format!("{}: {e}", req.name));
        let in_memory = one_shot && req.duration_ms.is_some_and(|d| d <= STATIC_MAX_MS);
        let track = &mut self.tracks[channel.index()];
        let handle = if in_memory {
            let cached = match self.cache.get(&req.path) {
                Some(data) => data.clone(),
                None => {
                    let data = StaticSoundData::from_file(&req.path).map_err(load_err)?;
                    remember(&mut self.cache, &mut self.cache_order, req.path.clone(), data.clone());
                    data
                }
            };
            let mut data = cached
                .volume(volume)
                .playback_rate(rate)
                .panning(pan)
                .start_time(start_time)
                .start_position(begin)
                .fade_in_tween(fade_in_tween);
            if repeating {
                data = data.loop_region(req.region());
            }
            Handle::Static(
                track
                    .play(data)
                    .map_err(|e| Error::Playback(e.to_string()))?,
            )
        } else {
            let mut data = StreamingSoundData::from_file(&req.path)
                .map_err(load_err)?
                .volume(volume)
                .playback_rate(rate)
                .panning(pan)
                .start_time(start_time)
                .start_position(begin)
                .fade_in_tween(fade_in_tween);
            if repeating {
                data = data.loop_region(req.region());
            }
            Handle::Streaming(
                track
                    .play(data)
                    .map_err(|e| Error::Playback(e.to_string()))?,
            )
        };

        // Only fade the old music out once the new track is really playing,
        // so a file that fails to load never leaves the table in silence.
        if channel == Channel::Music {
            self.stop_set(fade);
            for s in self.sounds.iter_mut().filter(|s| s.channel == Channel::Music) {
                if !s.stopping {
                    s.handle.stop(fade_out(fade));
                    s.stopping = true;
                }
            }
        }

        self.sounds.push(ActiveSound {
            id,
            asset_id: req.asset_id,
            name: req.name.clone(),
            channel,
            duration_ms: req.duration_ms,
            looping,
            repeating,
            stopping: false,
            failed: false,
            finished: false,
            duck,
            handle,
            request: req,
        });
        self.panic_armed = false;
        Ok(())
    }

    /// Makes each sound repeat, or not, to match what was asked for.
    ///
    /// A lone music track repeats while the queue is set to repeat and has
    /// nothing else in it; as soon as another track is queued it plays to
    /// its end and hands over.
    fn refresh_repeats(&mut self) {
        let lone_music_repeats = self.loop_queue() && self.queue.is_empty();
        for s in self.sounds.iter_mut().filter(|s| !s.stopping) {
            let want = match s.channel {
                Channel::Music => s.looping || lone_music_repeats,
                _ => s.looping,
            };
            if want != s.repeating {
                s.handle.set_repeat(want, s.request.region());
                s.repeating = want;
            }
        }
    }

    /// Starts the next queued track when no music is playing.
    fn advance_music(&mut self) {
        while !self.music_is_playing() {
            let Some(next) = self.queue.pop_front() else { break };
            let mut request = next.request;
            request.looping = Some(next.looping);
            request.interrupt = false;
            if let Err(e) = self.start(next.id, request, None) {
                // Skip a track that cannot be read and try the one after.
                self.last_error = Some(e.to_string());
            }
        }
        self.refresh_repeats();
    }

    fn requeue(&mut self, request: PlayRequest, looping: bool) {
        let id = self.next_id;
        self.next_id += 1;
        self.queue.push_back(Queued { id, looping, request });
    }

    /// Crossfades to the next queued music track. With a repeating queue
    /// the current track goes to the back of the line.
    pub fn next_track(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        let fade = ms(self.settings.channels[Channel::Music.index()].fade_ms);
        let loop_queue = self.loop_queue();
        let mut back = Vec::new();
        for s in self.sounds.iter_mut().filter(|s| s.channel == Channel::Music) {
            if !s.stopping {
                s.handle.stop(fade_out(fade));
                s.stopping = true;
                if loop_queue {
                    back.push((s.request.clone(), s.looping));
                }
            }
        }
        for (request, looping) in back {
            self.requeue(request, looping);
        }
        self.advance_music();
    }

    /// Jumps a queued track to the front and crossfades to it.
    pub fn play_queued_now(&mut self, id: u64) {
        let Some(at) = self.queue.iter().position(|q| q.id == id) else { return };
        if let Some(item) = self.queue.remove(at) {
            self.queue.push_front(item);
        }
        self.next_track();
    }

    /// Moves a queued track to another place in the queue (0 = next up).
    pub fn move_queued(&mut self, id: u64, to: usize) {
        let Some(at) = self.queue.iter().position(|q| q.id == id) else { return };
        if let Some(item) = self.queue.remove(at) {
            let to = to.min(self.queue.len());
            self.queue.insert(to, item);
        }
    }

    /// Plays a queued track now and puts the playing one in its place in
    /// the queue.
    pub fn swap_with_playing(&mut self, id: u64) {
        let Some(at) = self.queue.iter().position(|q| q.id == id) else { return };
        let Some(item) = self.queue.remove(at) else { return };
        let fade = ms(self.settings.channels[Channel::Music.index()].fade_ms);
        let mut back = Vec::new();
        for s in self.sounds.iter_mut().filter(|s| s.channel == Channel::Music && !s.stopping) {
            s.handle.stop(fade_out(fade));
            s.stopping = true;
            back.push((s.request.clone(), s.looping));
        }
        for (request, looping) in back {
            let id = self.next_id;
            self.next_id += 1;
            self.queue.insert(at.min(self.queue.len()), Queued { id, looping, request });
        }
        self.queue.push_front(item);
        self.advance_music();
    }

    /// Moves a playing or queued sound to another channel. A playing one
    /// carries on from where it was.
    pub fn move_to_channel(&mut self, id: u64, channel: Channel) -> Result<()> {
        let mut request = if let Some(at) = self.queue.iter().position(|q| q.id == id) {
            let Some(item) = self.queue.remove(at) else { return Ok(()) };
            let mut request = item.request;
            request.looping = item.looping.then_some(true);
            request
        } else if let Some(s) = self.sounds.iter_mut().find(|s| s.id == id && !s.stopping) {
            if s.channel == channel {
                return Ok(());
            }
            let mut request = s.request.clone();
            request.resume_ms = Some((s.handle.position() * 1000.0) as i64);
            // Keep "repeat" only where the GM switched it on.
            request.looping = (s.looping && s.channel == Channel::Music || request.looping == Some(true)).then_some(true);
            s.handle.stop(fade_out(Duration::from_millis(150)));
            s.stopping = true;
            request
        } else {
            return Ok(());
        };
        request.channel = channel;
        request.interrupt = true;
        request.delay_ms = 0;
        request.tag = None;
        self.play(request).map(|_| ())
    }

    /// Puts the playing music track into the queue at `to` and moves on
    /// to the next one.
    pub fn requeue_playing(&mut self, id: u64, to: usize) {
        if self.queue.is_empty() {
            return;
        }
        let fade = ms(self.settings.channels[Channel::Music.index()].fade_ms);
        let Some(s) = self.sounds.iter_mut().find(|s| s.id == id && s.channel == Channel::Music && !s.stopping) else {
            return;
        };
        s.handle.stop(fade_out(fade));
        s.stopping = true;
        let (request, looping) = (s.request.clone(), s.looping);
        // The slot is counted in the queue as shown, before the next track leaves it.
        let next = self.queue.pop_front();
        let id = self.next_id;
        self.next_id += 1;
        let to = to.saturating_sub(1).min(self.queue.len());
        self.queue.insert(to, Queued { id, looping, request });
        if let Some(next) = next {
            self.queue.push_front(next);
        }
        self.advance_music();
    }

    /// Holds a playing sound where it is (`fade_ms` 0: at once), or lets
    /// it carry on.
    pub fn pause_sound(&mut self, id: u64, paused: bool, fade_ms: u32) {
        if let Some(s) = self.sounds.iter_mut().find(|s| s.id == id && !s.stopping) {
            let tween = fade_out(ms(fade_ms));
            if paused {
                s.handle.pause(tween);
            } else {
                s.handle.resume(tween);
            }
        }
    }

    /// Jumps a playing sound to another point in the file.
    pub fn seek_sound(&mut self, id: u64, position_ms: i64) {
        if let Some(s) = self.sounds.iter_mut().find(|s| s.id == id && !s.stopping) {
            // Stay clear of the very end, where a seek would just finish the sound.
            let end = s.duration_ms.map_or(i64::MAX, |d| (d - 200).max(0));
            s.handle.seek_to(position_ms.clamp(0, end) as f64 / 1000.0);
        }
    }

    /// Sets whether a playing or queued sound repeats until stopped.
    pub fn set_sound_looping(&mut self, id: u64, looping: bool) {
        if let Some(s) = self.sounds.iter_mut().find(|s| s.id == id) {
            s.looping = looping;
        } else if let Some(q) = self.queue.iter_mut().find(|q| q.id == id) {
            q.looping = looping;
        }
        self.refresh_repeats();
    }

    pub fn set_loop_queue(&mut self, on: bool) {
        self.settings.channels[Channel::Music.index()].loop_queue = on;
        self.refresh_repeats();
    }

    /// Fades one sound out. `fade_ms: None` uses the channel's fade time.
    ///
    /// A queued track is simply taken out of the queue. Stopping the music
    /// that is playing moves on to the next queued track, if there is one.
    pub fn stop_sound(&mut self, id: u64, fade_ms: Option<u32>) {
        if let Some(s) = self.sounds.iter_mut().find(|s| s.id == id) {
            let fade = fade_ms.unwrap_or(self.settings.channels[s.channel.index()].fade_ms);
            s.handle.stop(fade_out(ms(fade)));
            s.stopping = true;
        } else {
            self.queue.retain(|q| q.id != id);
            self.emitters.retain(|e| e.id != id);
        }
        self.advance_music();
    }

    /// Sets the level of one playing sound (a pad's or layer's own volume).
    pub fn set_sound_trim(&mut self, id: u64, trim_db: f32) {
        if let Some(s) = self.sounds.iter_mut().find(|s| s.id == id && !s.stopping) {
            let trim_db = trim_db.clamp(-60.0, 12.0);
            s.request.trim_db = trim_db;
            s.handle.set_volume(s.request.gain_db + trim_db, smooth());
        }
    }

    /// Keeps a decoded copy of a short sound so it starts instantly.
    pub fn cache_sound(&mut self, path: PathBuf, data: StaticSoundData) {
        remember(&mut self.cache, &mut self.cache_order, path, data);
    }

    pub fn is_cached(&self, path: &PathBuf) -> bool {
        self.cache.contains_key(path)
    }

    // ---- random-interval layers ------------------------------------------

    /// Starts a layer that plays one of its sounds every so often.
    pub fn add_emitter(&mut self, request: EmitterRequest) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        // The first one comes soon, so the GM hears that it works.
        let first = fastrand::u32(300..=request.min_ms.clamp(301, 4000));
        self.emitters.push(Emitter {
            id,
            request,
            next_at: Instant::now() + ms(first),
            last: None,
        });
        id
    }

    fn run_emitters(&mut self) {
        let now = Instant::now();
        let mut due = Vec::new();
        for e in &mut self.emitters {
            if now < e.next_at || e.request.sounds.is_empty() {
                continue;
            }
            let count = e.request.sounds.len();
            let mut pick = fastrand::usize(..count);
            if count > 1 && Some(pick) == e.last {
                pick = (pick + 1) % count; // never the same one twice running
            }
            e.last = Some(pick);
            let (lo, hi) = (e.request.min_ms.max(50), e.request.max_ms.max(e.request.min_ms).max(50));
            e.next_at = now + ms(fastrand::u32(lo..=hi));
            let mut play = e.request.sounds[pick].clone();
            play.channel = Channel::Ambience;
            play.looping = Some(false);
            play.interrupt = false;
            play.tag = e.request.tag.clone();
            if e.request.random_pan {
                play.pan = fastrand::f32() * 1.6 - 0.8;
            }
            due.push(play);
        }
        for play in due {
            let id = self.next_id;
            self.next_id += 1;
            if let Err(e) = self.start(id, play, None) {
                self.last_error = Some(e.to_string());
            }
        }
    }

    // ---- scenes ----------------------------------------------------------

    /// Moves the whole soundscape to a scene over its transition time.
    ///
    /// Ambience layers the old and new scene share keep playing without a
    /// restart; the rest crossfade, so there is never a gap.
    pub fn apply_scene(&mut self, scene: SceneRequest) {
        let fade = ms(scene.transition_ms);
        let mut wanted: Vec<Option<PlayRequest>> = scene.layers.into_iter().map(Some).collect();

        for s in self.sounds.iter_mut().filter(|s| s.channel == Channel::Ambience && !s.stopping) {
            let keep = s.looping
                .then(|| wanted.iter().position(|w| w.as_ref().is_some_and(|w| w.asset_id == s.asset_id)))
                .flatten();
            match keep {
                Some(at) => {
                    let layer = wanted[at].take().unwrap();
                    s.request.trim_db = layer.trim_db;
                    s.request.tag = layer.tag;
                    s.handle.set_volume(s.request.gain_db + layer.trim_db, fade_in(fade));
                }
                None => {
                    s.handle.stop(fade_out(fade));
                    s.stopping = true;
                }
            }
        }
        for mut layer in wanted.into_iter().flatten() {
            layer.channel = Channel::Ambience;
            layer.looping = Some(true);
            let id = self.next_id;
            self.next_id += 1;
            if let Err(e) = self.start(id, layer, Some(fade)) {
                self.last_error = Some(e.to_string());
            }
        }

        self.emitters.clear();
        for emitter in scene.emitters {
            self.add_emitter(emitter);
        }

        match scene.music {
            SceneMusic::Keep => {}
            SceneMusic::Silence => self.stop_channel(Channel::Music, Some(scene.transition_ms)),
            SceneMusic::Tracks(tracks) => {
                let mut tracks: VecDeque<PlayRequest> = tracks.into();
                let current = self
                    .sounds
                    .iter()
                    .find(|s| s.channel == Channel::Music && !s.stopping)
                    .map(|s| s.asset_id);
                self.queue.clear();
                if let Some(mut first) = tracks.pop_front() {
                    if current != Some(first.asset_id) {
                        first.channel = Channel::Music;
                        let id = self.next_id;
                        self.next_id += 1;
                        if let Err(e) = self.start(id, first, Some(fade)) {
                            self.last_error = Some(e.to_string());
                        }
                    }
                }
                for mut track in tracks {
                    track.channel = Channel::Music;
                    let looping = track.looping.unwrap_or(false);
                    let id = self.next_id;
                    self.next_id += 1;
                    self.queue.push_back(Queued { id, looping, request: track });
                }
            }
        }
        self.scene = Some(SceneState { id: scene.id, name: scene.name });
        self.refresh_repeats();
        self.refresh_duck();
    }

    // ---- ducking and solo ------------------------------------------------

    /// Lowers the music while a ducking sound plays, and brings it back.
    fn refresh_duck(&mut self) {
        let ducked = self.sounds.iter().any(|s| s.duck && !s.stopping);
        if ducked != self.ducked {
            self.ducked = ducked;
            let db = channel_db(&self.settings, Channel::Music, ducked);
            let tween = if ducked { fade_out(DUCK_ATTACK) } else { fade_in(DUCK_RELEASE) };
            self.tracks[Channel::Music.index()].set_volume(db, tween);
        }
    }

    pub fn set_duck_db(&mut self, db: f32) {
        self.settings.duck_db = db.clamp(-40.0, 0.0);
        self.apply_channel(Channel::Music);
    }

    fn apply_master(&mut self) {
        let db = fader_to_db(self.settings.master_volume) + self.settings.profile.gain_db;
        self.bus.set_volume(db, smooth());
    }

    /// Shapes the final mix for the speakers or headphones in use.
    pub fn set_profile(&mut self, profile: AudioProfile) {
        self.settings.profile = profile.sanitized();
        let profile = self.settings.profile;
        self.shaping.apply(&profile, smooth());
        self.apply_master();
    }

    pub fn set_channel_solo(&mut self, channel: Channel, solo: bool) {
        self.settings.channels[channel.index()].solo = solo;
        for c in Channel::ALL {
            self.apply_channel(c);
        }
    }

    /// Fades out everything on a channel. For music this also empties the
    /// queue: stop means stop.
    pub fn stop_channel(&mut self, channel: Channel, fade_ms: Option<u32>) {
        if channel == Channel::Music {
            self.queue.clear();
        }
        if channel == Channel::Ambience {
            self.emitters.clear();
        }
        let fade = fade_ms.unwrap_or(self.settings.channels[channel.index()].fade_ms);
        if channel == Channel::Music {
            self.stop_set(ms(fade));
        }
        for s in self.sounds.iter_mut().filter(|s| s.channel == channel) {
            s.handle.stop(fade_out(ms(fade)));
            s.stopping = true;
        }
    }

    pub fn set_channel_volume(&mut self, channel: Channel, volume: f32) {
        self.settings.channels[channel.index()].volume = clamp_unit(volume);
        self.apply_channel(channel);
    }

    pub fn set_channel_muted(&mut self, channel: Channel, muted: bool) {
        self.settings.channels[channel.index()].muted = muted;
        self.apply_channel(channel);
    }

    pub fn set_channel_fade(&mut self, channel: Channel, fade_ms: u32) {
        self.settings.channels[channel.index()].fade_ms = fade_ms.min(60_000);
    }

    fn apply_channel(&mut self, channel: Channel) {
        let ducked = channel == Channel::Music && self.ducked;
        let db = channel_db(&self.settings, channel, ducked);
        self.tracks[channel.index()].set_volume(db, smooth());
    }

    pub fn set_master_volume(&mut self, volume: f32) {
        self.settings.master_volume = clamp_unit(volume);
        self.apply_master();
    }

    /// First press fades everything out over one second. A second press
    /// while that fade is still running cuts instantly.
    pub fn panic(&mut self) -> PanicOutcome {
        self.queue.clear();
        self.emitters.clear();
        self.scene = None;
        if self.sounds.is_empty() && !self.set_is_sounding() {
            self.panic_armed = false;
            return PanicOutcome::Idle;
        }
        let (tween, outcome) = if self.panic_armed {
            (Tween { duration: Duration::ZERO, ..Default::default() }, PanicOutcome::Cut)
        } else {
            (fade_out(PANIC_FADE), PanicOutcome::Fading)
        };
        for s in &mut self.sounds {
            s.handle.stop(tween);
            s.stopping = true;
        }
        self.stop_set(PANIC_FADE);
        if outcome == PanicOutcome::Cut {
            self.cut_set_tails();
        }
        self.panic_armed = outcome == PanicOutcome::Fading;
        outcome
    }

    /// Housekeeping: forgets finished sounds and collects stream errors.
    pub fn tick(&mut self) {
        for s in &mut self.sounds {
            if let Some(e) = s.handle.pop_error() {
                self.last_error = Some(format!("{}: {e}", s.name));
                s.failed = true;
            }
        }
        // Trimmed sounds: jump over the cut stretches, and end early.
        for s in self.sounds.iter_mut().filter(|s| !s.stopping) {
            if s.request.cuts.is_empty() && s.request.end_ms.is_none() {
                continue;
            }
            let at = (s.handle.position() * 1000.0) as i64;
            if let Some(&(_, to)) = s.request.cuts.iter().find(|(from, to)| at >= *from && at < *to) {
                s.handle.seek_to(to as f64 / 1000.0);
            } else if s.request.end_ms.is_some_and(|end| at >= end) && !s.repeating {
                s.handle.stop(fade_out(Duration::from_millis(40)));
                s.stopping = true;
                s.finished = true;
            }
        }
        // A music track that played to its end rejoins a repeating queue.
        let loop_queue = self.loop_queue();
        let mut ended = Vec::new();
        self.sounds.retain(|s| {
            if s.handle.state() != PlaybackState::Stopped {
                return true;
            }
            if s.channel == Channel::Music && loop_queue && (!s.stopping || s.finished) && !s.failed {
                ended.push((s.request.clone(), s.looping));
            }
            false
        });
        for (request, looping) in ended {
            self.requeue(request, looping);
        }
        self.advance_music();
        self.run_emitters();
        self.tick_set();
        self.refresh_duck();
        for (level, meter) in self.levels.iter_mut().zip(&self.meters) {
            *level = take_level(meter);
        }
        if self.sounds.is_empty() && !self.set_is_sounding() {
            self.panic_armed = false;
        }
    }

    pub fn sound_count(&self) -> usize {
        self.sounds.len()
    }

    fn channel_states(&self) -> Vec<ChannelState> {
        let mut states = channel_states(&self.settings);
        states[Channel::Music.index()].queue = self
            .queue
            .iter()
            .map(|q| QueuedState {
                id: q.id,
                asset_id: q.request.asset_id,
                name: q.request.name.clone(),
                duration_ms: q.request.duration_ms,
                looping: q.looping,
            })
            .collect();
        states
    }

    pub fn state(&self) -> EngineState {
        EngineState {
            device_ok: true,
            device_error: None,
            master_volume: self.settings.master_volume,
            channels: self.channel_states(),
            sounds: self
                .sounds
                .iter()
                .map(|s| SoundState {
                    id: s.id,
                    asset_id: s.asset_id,
                    name: s.name.clone(),
                    channel: s.channel,
                    position_ms: (s.handle.position() * 1000.0) as i64,
                    duration_ms: s.duration_ms,
                    looping: s.looping,
                    stopping: s.stopping,
                    paused: matches!(s.handle.state(), PlaybackState::Pausing | PlaybackState::Paused),
                    trim_db: s.request.trim_db,
                    tag: s.request.tag.clone(),
                })
                .collect(),
            panic_armed: self.panic_armed,
            last_error: self.last_error.clone(),
            emitters: self
                .emitters
                .iter()
                .map(|e| EmitterState {
                    id: e.id,
                    name: e.request.name.clone(),
                    min_ms: e.request.min_ms,
                    max_ms: e.request.max_ms,
                    tag: e.request.tag.clone(),
                })
                .collect(),
            scene: self.scene.clone(),
            ducked: self.ducked,
            duck_db: self.settings.duck_db,
            levels: self.levels,
            music_set: self.set_state(),
            profile: self.settings.profile,
        }
    }
}

/// A channel track's volume in dB: its fader, unless it is muted or
/// another channel is soloed, and lowered while ducked.
fn channel_db(settings: &MixerSettings, channel: Channel, ducked: bool) -> f32 {
    let c = settings.channels[channel.index()];
    let any_solo = settings.channels.iter().any(|c| c.solo);
    if c.muted || (any_solo && !c.solo) {
        return Decibels::SILENCE.0;
    }
    fader_to_db(c.volume) + if ducked { settings.duck_db } else { 0.0 }
}

/// Adds a decoded sound to the cache, dropping the oldest ones when full.
fn remember(
    cache: &mut HashMap<PathBuf, StaticSoundData>,
    order: &mut VecDeque<PathBuf>,
    path: PathBuf,
    data: StaticSoundData,
) {
    if cache.insert(path.clone(), data).is_none() {
        order.push_back(path);
    }
    let mut total: usize = cache.values().map(|d| d.frames.len()).sum();
    while total > CACHE_MAX_FRAMES && order.len() > 1 {
        if let Some(oldest) = order.pop_front() {
            if let Some(dropped) = cache.remove(&oldest) {
                total -= dropped.frames.len();
            }
        }
    }
}

/// Decodes a short sound ahead of time. Run this off the engine thread.
pub fn decode(path: &std::path::Path) -> Result<StaticSoundData> {
    StaticSoundData::from_file(path).map_err(|e| Error::Playback(format!("{}: {e}", path.display())))
}

/// A decoded sound on its way into the cache. Printed without its samples.
#[derive(Clone)]
pub struct Decoded(pub PathBuf, pub StaticSoundData);

impl std::fmt::Debug for Decoded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} ({} frames)", self.0, self.1.frames.len())
    }
}

fn channel_states(settings: &MixerSettings) -> Vec<ChannelState> {
    Channel::ALL
        .iter()
        .map(|&channel| {
            let c = settings.channels[channel.index()];
            ChannelState {
                channel,
                volume: c.volume,
                muted: c.muted,
                fade_ms: c.fade_ms,
                loop_queue: c.loop_queue,
                solo: c.solo,
                queue: Vec::new(),
            }
        })
        .collect()
}

// ---- engine thread ---------------------------------------------------------

/// Everything the UI can ask the engine to do. Each one is logged.
#[derive(Debug, Clone)]
pub enum Command {
    Play(PlayRequest),
    StopSound { id: u64, fade_ms: Option<u32> },
    StopChannel { channel: Channel, fade_ms: Option<u32> },
    SetChannelVolume { channel: Channel, volume: f32 },
    SetChannelMuted { channel: Channel, muted: bool },
    SetChannelFade { channel: Channel, fade_ms: u32 },
    SetMasterVolume { volume: f32 },
    /// Repeat (or stop repeating) one playing or queued sound.
    SetSoundLooping { id: u64, looping: bool },
    /// Repeat the music queue when it runs out.
    SetLoopQueue { on: bool },
    /// Crossfade to the next queued music track.
    NextTrack,
    /// Crossfade to this queued music track now.
    PlayQueuedNow { id: u64 },
    MoveQueued { id: u64, to: usize },
    SwapWithPlaying { id: u64 },
    MoveToChannel { id: u64, channel: Channel },
    RequeuePlaying { id: u64, to: usize },
    PauseSound { id: u64, paused: bool, fade_ms: u32 },
    SeekSound { id: u64, position_ms: i64 },
    SetSoundTrim { id: u64, trim_db: f32 },
    SetChannelSolo { channel: Channel, solo: bool },
    SetDuckDb { db: f32 },
    SetProfile(AudioProfile),
    /// Keep a decoded short sound ready for an instant start.
    Cache(Decoded),
    AddEmitter(EmitterRequest),
    ApplyScene(SceneRequest),
    PlaySet(SetRequest),
    SetTier { tier: usize, quantize: Option<Quantize> },
    StepTier { delta: i32 },
    PlayStinger { request: PlayRequest, quantize: Quantize, then_tier: Option<usize> },
    /// Close the audio output and open it again (after choosing another).
    Reopen,
    Panic,
    /// Try to open the audio device again right now.
    RetryDevice,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Done,
    Started(u64),
    Panic(PanicOutcome),
}

enum Msg {
    Command(Command, Sender<Result<Reply>>),
    GetState(Sender<EngineState>),
    GetLog(Sender<Vec<String>>),
    Shutdown,
}

type StateSink = Box<dyn Fn(&EngineState) + Send>;

/// Handle to the engine thread.
pub struct Engine {
    tx: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
    /// The chosen audio output's name, read each time the device is opened.
    device: Option<Arc<std::sync::Mutex<Option<String>>>>,
}

impl Engine {
    /// Starts the engine on an audio output: the one named, or the
    /// system default when `device` is `None` or no longer plugged in.
    ///
    /// `on_state` is called from the engine thread whenever the state
    /// changes (up to ~30 times a second while something is playing).
    pub fn spawn(
        settings: MixerSettings,
        device: Option<String>,
        on_state: impl Fn(&EngineState) + Send + 'static,
    ) -> Self {
        let chosen = Arc::new(std::sync::Mutex::new(device));
        let for_thread = chosen.clone();
        let mut engine = Self::spawn_with(settings, on_state, move || {
            let wanted = for_thread.lock().unwrap().clone();
            let backend_settings = kira::backend::cpal::CpalBackendSettings {
                device: wanted.as_deref().and_then(find_output_device),
                ..Default::default()
            };
            AudioManager::<DefaultBackend>::new(AudioManagerSettings { backend_settings, ..Default::default() })
                .map_err(|e| e.to_string())
        });
        engine.device = Some(chosen);
        engine
    }

    /// Switches to another audio output (`None`: the system default).
    /// Everything that is playing stops.
    pub fn set_output_device(&self, name: Option<String>) -> Result<()> {
        if let Some(device) = &self.device {
            *device.lock().unwrap() = name;
        }
        self.send(Command::Reopen).map(|_| ())
    }

    /// Like [`Engine::spawn`], with a custom way of opening the device.
    pub fn spawn_with<B, F>(
        settings: MixerSettings,
        on_state: impl Fn(&EngineState) + Send + 'static,
        open: F,
    ) -> Self
    where
        B: Backend + 'static,
        F: Fn() -> std::result::Result<AudioManager<B>, String> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<Msg>();
        let on_state: StateSink = Box::new(on_state);
        let thread = thread::Builder::new()
            .name("gmaudio-engine".into())
            .spawn(move || {
                let mut settings = settings.sanitized();
                let mut mixer: Option<Mixer<B>> = None;
                let mut device_error: Option<String> = None;
                let mut last_attempt: Option<Instant> = None;
                let mut last_state: Option<EngineState> = None;
                let mut log: VecDeque<String> = VecDeque::with_capacity(COMMAND_LOG_SIZE);
                let started = Instant::now();

                loop {
                    // (Re)open the device when there is none.
                    if mixer.is_none()
                        && last_attempt.is_none_or(|t| t.elapsed() >= DEVICE_RETRY)
                    {
                        last_attempt = Some(Instant::now());
                        match open().and_then(|m| {
                            Mixer::new(m, settings.clone()).map_err(|e| e.to_string())
                        }) {
                            Ok(m) => {
                                mixer = Some(m);
                                device_error = None;
                            }
                            Err(e) => device_error = Some(e),
                        }
                    }

                    match rx.recv_timeout(TICK) {
                        Ok(Msg::Command(cmd, reply)) => {
                            if log.len() == COMMAND_LOG_SIZE {
                                log.pop_front();
                            }
                            log.push_back(format!("{:>10.3}s {cmd:?}", started.elapsed().as_secs_f64()));

                            if matches!(cmd, Command::Reopen) {
                                mixer = None;
                            }
                            if matches!(cmd, Command::RetryDevice | Command::Reopen) {
                                last_attempt = None;
                                let _ = reply.send(Ok(Reply::Done));
                                continue;
                            }
                            let result = match mixer.as_mut() {
                                Some(m) => {
                                    let r = run(m, cmd);
                                    settings = m.settings().clone();
                                    r
                                }
                                // Without a device, settings still change so
                                // they apply as soon as one appears.
                                None => run_offline(&mut settings, cmd, &device_error),
                            };
                            let _ = reply.send(result);
                        }
                        Ok(Msg::GetState(reply)) => {
                            let _ = reply.send(current_state(&mixer, &settings, &device_error));
                        }
                        Ok(Msg::GetLog(reply)) => {
                            let _ = reply.send(log.iter().cloned().collect());
                        }
                        Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
                        Err(RecvTimeoutError::Timeout) => {}
                    }

                    if let Some(m) = mixer.as_mut() {
                        m.tick();
                    }
                    let state = current_state(&mixer, &settings, &device_error);
                    if last_state.as_ref() != Some(&state) {
                        on_state(&state);
                        last_state = Some(state);
                    }
                }
            })
            .expect("could not start the audio engine thread");
        Self {
            tx,
            thread: Some(thread),
            device: None,
        }
    }

    fn send(&self, command: Command) -> Result<Reply> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(Msg::Command(command, reply_tx))
            .map_err(|_| Error::EngineGone)?;
        reply_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| Error::EngineGone)?
    }

    pub fn play(&self, request: PlayRequest) -> Result<u64> {
        match self.send(Command::Play(request))? {
            Reply::Started(id) => Ok(id),
            _ => Err(Error::EngineGone),
        }
    }

    pub fn stop_sound(&self, id: u64, fade_ms: Option<u32>) -> Result<()> {
        self.send(Command::StopSound { id, fade_ms }).map(|_| ())
    }

    pub fn stop_channel(&self, channel: Channel, fade_ms: Option<u32>) -> Result<()> {
        self.send(Command::StopChannel { channel, fade_ms }).map(|_| ())
    }

    pub fn set_channel_volume(&self, channel: Channel, volume: f32) -> Result<()> {
        self.send(Command::SetChannelVolume { channel, volume }).map(|_| ())
    }

    pub fn set_channel_muted(&self, channel: Channel, muted: bool) -> Result<()> {
        self.send(Command::SetChannelMuted { channel, muted }).map(|_| ())
    }

    pub fn set_channel_fade(&self, channel: Channel, fade_ms: u32) -> Result<()> {
        self.send(Command::SetChannelFade { channel, fade_ms }).map(|_| ())
    }

    pub fn set_master_volume(&self, volume: f32) -> Result<()> {
        self.send(Command::SetMasterVolume { volume }).map(|_| ())
    }

    pub fn set_sound_looping(&self, id: u64, looping: bool) -> Result<()> {
        self.send(Command::SetSoundLooping { id, looping }).map(|_| ())
    }

    pub fn set_loop_queue(&self, on: bool) -> Result<()> {
        self.send(Command::SetLoopQueue { on }).map(|_| ())
    }

    pub fn next_track(&self) -> Result<()> {
        self.send(Command::NextTrack).map(|_| ())
    }

    pub fn play_queued_now(&self, id: u64) -> Result<()> {
        self.send(Command::PlayQueuedNow { id }).map(|_| ())
    }

    pub fn move_queued(&self, id: u64, to: usize) -> Result<()> {
        self.send(Command::MoveQueued { id, to }).map(|_| ())
    }

    pub fn move_to_channel(&self, id: u64, channel: Channel) -> Result<()> {
        self.send(Command::MoveToChannel { id, channel }).map(|_| ())
    }

    pub fn requeue_playing(&self, id: u64, to: usize) -> Result<()> {
        self.send(Command::RequeuePlaying { id, to }).map(|_| ())
    }

    pub fn swap_with_playing(&self, id: u64) -> Result<()> {
        self.send(Command::SwapWithPlaying { id }).map(|_| ())
    }

    pub fn pause_sound(&self, id: u64, paused: bool, fade_ms: u32) -> Result<()> {
        self.send(Command::PauseSound { id, paused, fade_ms }).map(|_| ())
    }

    pub fn seek_sound(&self, id: u64, position_ms: i64) -> Result<()> {
        self.send(Command::SeekSound { id, position_ms }).map(|_| ())
    }

    pub fn set_sound_trim(&self, id: u64, trim_db: f32) -> Result<()> {
        self.send(Command::SetSoundTrim { id, trim_db }).map(|_| ())
    }

    pub fn set_channel_solo(&self, channel: Channel, solo: bool) -> Result<()> {
        self.send(Command::SetChannelSolo { channel, solo }).map(|_| ())
    }

    pub fn set_duck_db(&self, db: f32) -> Result<()> {
        self.send(Command::SetDuckDb { db }).map(|_| ())
    }

    pub fn set_profile(&self, profile: AudioProfile) -> Result<()> {
        self.send(Command::SetProfile(profile)).map(|_| ())
    }

    /// Decodes short sounds here, on the caller's thread, and hands them
    /// to the engine so pads and stingers start without a disk read.
    pub fn preload(&self, paths: &[PathBuf]) {
        for path in paths {
            if let Ok(data) = decode(path) {
                let _ = self.send(Command::Cache(Decoded(path.clone(), data)));
            }
        }
    }

    pub fn add_emitter(&self, request: EmitterRequest) -> Result<u64> {
        match self.send(Command::AddEmitter(request))? {
            Reply::Started(id) => Ok(id),
            _ => unreachable!("add emitter always replies with an id"),
        }
    }

    pub fn apply_scene(&self, scene: SceneRequest) -> Result<()> {
        self.send(Command::ApplyScene(scene)).map(|_| ())
    }

    pub fn play_set(&self, request: SetRequest) -> Result<()> {
        self.send(Command::PlaySet(request)).map(|_| ())
    }

    pub fn set_tier(&self, tier: usize, quantize: Option<Quantize>) -> Result<()> {
        self.send(Command::SetTier { tier, quantize }).map(|_| ())
    }

    pub fn step_tier(&self, delta: i32) -> Result<()> {
        self.send(Command::StepTier { delta }).map(|_| ())
    }

    pub fn play_stinger(&self, request: PlayRequest, quantize: Quantize, then_tier: Option<usize>) -> Result<u64> {
        match self.send(Command::PlayStinger { request, quantize, then_tier })? {
            Reply::Started(id) => Ok(id),
            _ => unreachable!("play stinger always replies with an id"),
        }
    }

    pub fn panic(&self) -> Result<PanicOutcome> {
        match self.send(Command::Panic)? {
            Reply::Panic(outcome) => Ok(outcome),
            _ => Err(Error::EngineGone),
        }
    }

    pub fn retry_device(&self) -> Result<()> {
        self.send(Command::RetryDevice).map(|_| ())
    }

    pub fn state(&self) -> Result<EngineState> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(Msg::GetState(tx)).map_err(|_| Error::EngineGone)?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| Error::EngineGone)
    }

    /// The most recent commands, oldest first, for debugging.
    pub fn command_log(&self) -> Result<Vec<String>> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(Msg::GetLog(tx)).map_err(|_| Error::EngineGone)?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| Error::EngineGone)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run<B: Backend>(mixer: &mut Mixer<B>, command: Command) -> Result<Reply> {
    match command {
        Command::Play(req) => mixer.play(req).map(Reply::Started),
        Command::StopSound { id, fade_ms } => {
            mixer.stop_sound(id, fade_ms);
            Ok(Reply::Done)
        }
        Command::StopChannel { channel, fade_ms } => {
            mixer.stop_channel(channel, fade_ms);
            Ok(Reply::Done)
        }
        Command::SetChannelVolume { channel, volume } => {
            mixer.set_channel_volume(channel, volume);
            Ok(Reply::Done)
        }
        Command::SetChannelMuted { channel, muted } => {
            mixer.set_channel_muted(channel, muted);
            Ok(Reply::Done)
        }
        Command::SetChannelFade { channel, fade_ms } => {
            mixer.set_channel_fade(channel, fade_ms);
            Ok(Reply::Done)
        }
        Command::SetMasterVolume { volume } => {
            mixer.set_master_volume(volume);
            Ok(Reply::Done)
        }
        Command::SetSoundLooping { id, looping } => {
            mixer.set_sound_looping(id, looping);
            Ok(Reply::Done)
        }
        Command::SetLoopQueue { on } => {
            mixer.set_loop_queue(on);
            Ok(Reply::Done)
        }
        Command::NextTrack => {
            mixer.next_track();
            Ok(Reply::Done)
        }
        Command::PlayQueuedNow { id } => {
            mixer.play_queued_now(id);
            Ok(Reply::Done)
        }
        Command::MoveQueued { id, to } => {
            mixer.move_queued(id, to);
            Ok(Reply::Done)
        }
        Command::MoveToChannel { id, channel } => {
            mixer.move_to_channel(id, channel)?;
            Ok(Reply::Done)
        }
        Command::RequeuePlaying { id, to } => {
            mixer.requeue_playing(id, to);
            Ok(Reply::Done)
        }
        Command::SwapWithPlaying { id } => {
            mixer.swap_with_playing(id);
            Ok(Reply::Done)
        }
        Command::PauseSound { id, paused, fade_ms } => {
            mixer.pause_sound(id, paused, fade_ms);
            Ok(Reply::Done)
        }
        Command::SeekSound { id, position_ms } => {
            mixer.seek_sound(id, position_ms);
            Ok(Reply::Done)
        }
        Command::SetSoundTrim { id, trim_db } => {
            mixer.set_sound_trim(id, trim_db);
            Ok(Reply::Done)
        }
        Command::SetChannelSolo { channel, solo } => {
            mixer.set_channel_solo(channel, solo);
            Ok(Reply::Done)
        }
        Command::SetDuckDb { db } => {
            mixer.set_duck_db(db);
            Ok(Reply::Done)
        }
        Command::SetProfile(profile) => {
            mixer.set_profile(profile);
            Ok(Reply::Done)
        }
        Command::Cache(Decoded(path, data)) => {
            mixer.cache_sound(path, data);
            Ok(Reply::Done)
        }
        Command::AddEmitter(request) => Ok(Reply::Started(mixer.add_emitter(request))),
        Command::ApplyScene(scene) => {
            mixer.apply_scene(scene);
            Ok(Reply::Done)
        }
        Command::PlaySet(request) => mixer.play_set(request).map(|_| Reply::Done),
        Command::SetTier { tier, quantize } => mixer.set_tier(tier, quantize).map(|_| Reply::Done),
        Command::StepTier { delta } => mixer.step_tier(delta).map(|_| Reply::Done),
        Command::PlayStinger { request, quantize, then_tier } => {
            mixer.play_stinger(request, quantize, then_tier).map(Reply::Started)
        }
        Command::Panic => Ok(Reply::Panic(mixer.panic())),
        Command::RetryDevice | Command::Reopen => Ok(Reply::Done),
    }
}

/// The names of the audio outputs on this computer.
pub fn output_devices() -> Vec<String> {
    use kira::backend::cpal::cpal::traits::{DeviceTrait, HostTrait};
    let host = kira::backend::cpal::cpal::default_host();
    let Ok(devices) = host.output_devices() else { return Vec::new() };
    let mut names: Vec<String> = devices
        .filter_map(|d| d.description().ok().map(|d| d.name().to_string()))
        .collect();
    names.sort();
    names.dedup();
    names
}

fn find_output_device(name: &str) -> Option<kira::backend::cpal::cpal::Device> {
    use kira::backend::cpal::cpal::traits::{DeviceTrait, HostTrait};
    kira::backend::cpal::cpal::default_host()
        .output_devices()
        .ok()?
        .find(|d| d.description().is_ok_and(|d| d.name() == name))
}

fn run_offline(
    settings: &mut MixerSettings,
    command: Command,
    device_error: &Option<String>,
) -> Result<Reply> {
    let no_device =
        || Error::NoDevice(device_error.clone().unwrap_or_else(|| "no output device".into()));
    match command {
        Command::Play(_) => Err(no_device()),
        Command::SetChannelVolume { channel, volume } => {
            settings.channels[channel.index()].volume = clamp_unit(volume);
            Ok(Reply::Done)
        }
        Command::SetChannelMuted { channel, muted } => {
            settings.channels[channel.index()].muted = muted;
            Ok(Reply::Done)
        }
        Command::SetChannelFade { channel, fade_ms } => {
            settings.channels[channel.index()].fade_ms = fade_ms.min(60_000);
            Ok(Reply::Done)
        }
        Command::SetMasterVolume { volume } => {
            settings.master_volume = clamp_unit(volume);
            Ok(Reply::Done)
        }
        Command::SetLoopQueue { on } => {
            settings.channels[Channel::Music.index()].loop_queue = on;
            Ok(Reply::Done)
        }
        Command::SetChannelSolo { channel, solo } => {
            settings.channels[channel.index()].solo = solo;
            Ok(Reply::Done)
        }
        Command::SetDuckDb { db } => {
            settings.duck_db = db.clamp(-40.0, 0.0);
            Ok(Reply::Done)
        }
        Command::SetProfile(profile) => {
            settings.profile = profile.sanitized();
            Ok(Reply::Done)
        }
        Command::AddEmitter(_) | Command::ApplyScene(_) | Command::PlaySet(_) | Command::PlayStinger { .. } => {
            Err(no_device())
        }
        Command::SetTier { .. } | Command::StepTier { .. } => Ok(Reply::Done),
        Command::Panic => Ok(Reply::Panic(PanicOutcome::Idle)),
        Command::StopSound { .. }
        | Command::SetSoundTrim { .. }
        | Command::Cache(_)
        | Command::StopChannel { .. }
        | Command::SetSoundLooping { .. }
        | Command::NextTrack
        | Command::PlayQueuedNow { .. }
        | Command::MoveQueued { .. }
        | Command::SwapWithPlaying { .. }
        | Command::MoveToChannel { .. }
        | Command::RequeuePlaying { .. }
        | Command::PauseSound { .. }
        | Command::SeekSound { .. }
        | Command::RetryDevice
        | Command::Reopen => Ok(Reply::Done),
    }
}

fn current_state<B: Backend>(
    mixer: &Option<Mixer<B>>,
    settings: &MixerSettings,
    device_error: &Option<String>,
) -> EngineState {
    match mixer {
        Some(m) => m.state(),
        None => EngineState {
            device_ok: false,
            device_error: device_error.clone(),
            master_volume: settings.master_volume,
            channels: channel_states(settings),
            sounds: Vec::new(),
            panic_armed: false,
            last_error: None,
            emitters: Vec::new(),
            scene: None,
            ducked: false,
            duck_db: settings.duck_db,
            levels: [0.0; 5],
            music_set: None,
            profile: settings.profile,
        },
    }
}

/// Helpers shared by the engine's tests: a mixer on kira's mock device,
/// which renders audio only when told to.
#[cfg(test)]
pub(in crate::engine) mod test_support {
    pub(in crate::engine) use super::*;
    use crate::analyze::testutil::write_sine_wav;
    pub(in crate::engine) use kira::backend::mock::{MockBackend, MockBackendSettings};


    pub(in crate::engine) const RATE: u32 = 48_000;

    pub(in crate::engine) fn manager() -> AudioManager<MockBackend> {
        AudioManager::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: RATE },
            ..Default::default()
        })
        .unwrap()
    }

    pub(in crate::engine) fn mixer() -> Mixer<MockBackend> {
        Mixer::new(manager(), MixerSettings::default()).unwrap()
    }

    /// Renders `millis` of audio through the mock device.
    pub(in crate::engine) fn render(m: &mut Mixer<MockBackend>, millis: u64) {
        // Each process() call renders one internal buffer (128 frames).
        let calls = (millis * RATE as u64 / 1000).div_ceil(128);
        for _ in 0..calls {
            let backend = m.manager_mut().backend_mut();
            backend.on_start_processing();
            backend.process();
        }
        m.tick();
    }

    /// Streaming sounds decode on their own thread: give them real time
    /// while rendering, until `done` holds.
    pub(in crate::engine) fn render_until(m: &mut Mixer<MockBackend>, mut done: impl FnMut(&Mixer<MockBackend>) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(m) {
            assert!(Instant::now() < deadline, "timed out; state: {:?}", m.state());
            render(m, 20);
            thread::sleep(Duration::from_millis(2));
        }
    }

    pub(in crate::engine) struct Files {
        pub _dir: tempfile::TempDir,
        pub long: PathBuf,
        pub short: PathBuf,
    }

    pub(in crate::engine) fn files() -> Files {
        let dir = tempfile::tempdir().unwrap();
        let long = dir.path().join("long.wav");
        let short = dir.path().join("short.wav");
        write_sine_wav(&long, 30.0, 220.0, -12.0, 2);
        write_sine_wav(&short, 0.3, 880.0, -12.0, 1);
        Files {
            _dir: dir,
            long,
            short,
        }
    }

    pub(in crate::engine) fn req(path: &PathBuf, channel: Channel, duration_ms: i64) -> PlayRequest {
        PlayRequest {
            asset_id: 7,
            path: path.clone(),
            name: "test".into(),
            channel,
            gain_db: 0.0,
            duration_ms: Some(duration_ms),
            looping: None,
            ..Default::default()
        }
    }

    pub(in crate::engine) fn now(mut request: PlayRequest) -> PlayRequest {
        request.interrupt = true;
        request
    }

    pub(in crate::engine) fn music(m: &Mixer<MockBackend>) -> ChannelState {
        m.state().channels[Channel::Music.index()].clone()
    }

    /// Renders about `millis` of audio, giving streams real time to decode.
    pub(in crate::engine) fn play_for(m: &mut Mixer<MockBackend>, millis: u64) {
        for _ in 0..millis / 20 {
            render(m, 20);
            thread::sleep(Duration::from_millis(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use std::sync::{Arc, Mutex};
    use music as music_state;

    #[test]
    fn fader_curve() {
        assert_eq!(fader_to_db(1.0), 0.0);
        assert!((fader_to_db(0.5) + 12.04).abs() < 0.01);
        assert_eq!(fader_to_db(0.0), -60.0);
        assert_eq!(fader_to_db(0.01), -60.0);
        assert_eq!(fader_to_db(f32::NAN), -60.0);
        assert_eq!(fader_to_db(7.0), 0.0);
    }

    #[test]
    fn one_shot_plays_to_the_end_and_is_forgotten() {
        let f = files();
        let mut m = mixer();
        m.play(req(&f.short, Channel::Sfx, 300)).unwrap();
        assert_eq!(m.sound_count(), 1);
        assert!(!m.state().sounds[0].looping);

        render(&mut m, 150);
        let pos = m.state().sounds[0].position_ms;
        assert!((100..=200).contains(&pos), "position was {pos} ms");

        render(&mut m, 300);
        assert_eq!(m.sound_count(), 0);
    }

    #[test]
    fn one_shots_overlap() {
        let f = files();
        let mut m = mixer();
        for _ in 0..5 {
            m.play(req(&f.short, Channel::Sfx, 300)).unwrap();
        }
        render(&mut m, 50);
        assert_eq!(m.sound_count(), 5);
    }

    #[test]
    fn music_crossfades_to_the_new_track() {
        let f = files();
        let mut m = mixer();
        m.set_channel_fade(Channel::Music, 500);
        let first = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        render_until(&mut m, |m| m.state().sounds[0].position_ms > 100);
        assert!(music(&m).loop_queue, "music repeats by default");

        let second = m.play(now(req(&f.long, Channel::Music, 30_000))).unwrap();
        let s = m.state();
        assert_eq!(s.sounds.len(), 2, "both tracks sound during the crossfade");
        assert!(s.sounds.iter().find(|x| x.id == first).unwrap().stopping);
        assert!(!s.sounds.iter().find(|x| x.id == second).unwrap().stopping);

        // After the fade only the new track is left, still playing.
        render_until(&mut m, |m| m.sound_count() == 1);
        assert_eq!(m.state().sounds[0].id, second);
        render_until(&mut m, |m| m.state().sounds[0].position_ms > 600);
        assert_eq!(m.sound_count(), 1);
    }

    #[test]
    fn a_file_that_fails_to_load_leaves_the_music_playing() {
        let f = files();
        let mut m = mixer();
        m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        let missing = f.long.with_file_name("nope.wav");
        assert!(matches!(
            m.play(now(req(&missing, Channel::Music, 1000))),
            Err(Error::Playback(_))
        ));
        let s = m.state();
        assert_eq!(s.sounds.len(), 1);
        assert!(!s.sounds[0].stopping);
    }

    #[test]
    fn music_queues_behind_the_current_track() {
        let f = files();
        let mut m = mixer();
        let first = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        let second = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        let third = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        play_for(&mut m, 100);

        let s = m.state();
        assert_eq!(s.sounds.len(), 1, "the current track keeps playing alone");
        assert_eq!(s.sounds[0].id, first);
        assert!(!s.sounds[0].stopping);
        let queue: Vec<u64> = music(&m).queue.iter().map(|q| q.id).collect();
        assert_eq!(queue, [second, third]);

        // Taking a track out of the queue.
        m.stop_sound(second, None);
        let queue: Vec<u64> = music(&m).queue.iter().map(|q| q.id).collect();
        assert_eq!(queue, [third]);
        assert_eq!(m.sound_count(), 1);
    }

    #[test]
    fn queued_tracks_can_be_reordered_and_a_playing_one_seeked() {
        let f = files();
        let mut m = mixer();
        let first = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        let ids: Vec<u64> = (0..3).map(|_| m.play(req(&f.long, Channel::Music, 30_000)).unwrap()).collect();
        let queue = |m: &Mixer<MockBackend>| music(m).queue.iter().map(|q| q.id).collect::<Vec<u64>>();

        m.move_queued(ids[2], 0);
        assert_eq!(queue(&m), [ids[2], ids[0], ids[1]]);
        m.move_queued(ids[2], 99);
        assert_eq!(queue(&m), [ids[0], ids[1], ids[2]], "past the end means last");
        m.move_queued(first, 1);
        assert_eq!(queue(&m), [ids[0], ids[1], ids[2]], "the playing track is not in the queue");

        play_for(&mut m, 100);
        m.seek_sound(first, 20_000);
        render_until(&mut m, |m| m.state().sounds[0].position_ms >= 20_000);
        let pos = m.state().sounds[0].position_ms;
        assert!((20_000..21_000).contains(&pos), "jumped to {pos} ms");
        m.seek_sound(first, 999_999);
        render_until(&mut m, |m| m.state().sounds[0].position_ms >= 29_000);
        assert_eq!(m.state().sounds[0].id, first, "a seek past the end stops just short of it");
    }

    #[test]
    fn a_queued_track_swaps_with_the_playing_one_and_sounds_pause() {
        let f = files();
        let mut m = mixer();
        let first = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        let second = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        let third = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        play_for(&mut m, 100);

        m.swap_with_playing(third);
        let s = m.state();
        let live: Vec<u64> = s.sounds.iter().filter(|s| !s.stopping).map(|s| s.id).collect();
        assert_eq!(live, [third], "the dragged track plays");
        assert!(s.sounds.iter().any(|s| s.id == first && s.stopping), "the old one fades out");
        let queue = &music(&m).queue;
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].id, second);
        assert_ne!(queue[1].id, third, "the old track took the dragged one's place");

        // Pause holds the position; resume carries on from it.
        play_for(&mut m, 300);
        m.pause_sound(third, true, 0);
        play_for(&mut m, 100);
        let held = m.state().sounds.iter().find(|s| s.id == third).unwrap().clone();
        assert!(held.paused);
        play_for(&mut m, 500);
        let later = m.state().sounds.iter().find(|s| s.id == third).unwrap().clone();
        assert_eq!(later.position_ms, held.position_ms);
        assert_eq!(music(&m).queue.len(), 2, "a paused track does not let the queue move on");
        m.pause_sound(third, false, 0);
        play_for(&mut m, 300);
        let resumed = m.state().sounds.iter().find(|s| s.id == third).unwrap().clone();
        assert!(!resumed.paused && resumed.position_ms > held.position_ms);
    }

    #[test]
    fn a_trimmed_sound_starts_late_skips_its_cuts_and_ends_early() {
        let f = files();
        let mut m = mixer();
        let trimmed = PlayRequest {
            start_ms: 5_000,
            end_ms: Some(12_000),
            cuts: vec![(6_000, 10_000)],
            ..req(&f.long, Channel::Music, 30_000)
        };
        let id = m.play(trimmed).unwrap();
        let next = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        play_for(&mut m, 200);
        let pos = m.state().sounds[0].position_ms;
        assert!((5_000..5_600).contains(&pos), "started at {pos} ms");
        // 5 s to 6 s plays, then the cut is skipped.
        render_until(&mut m, |m| m.state().sounds[0].position_ms >= 10_000);
        let pos = m.state().sounds[0].position_ms;
        assert!((10_000..10_300).contains(&pos), "after the cut: {pos} ms");
        // At 12 s it ends and the queue moves on; the track rejoins the queue.
        render_until(&mut m, |m| m.state().sounds.iter().any(|s| s.id == next));
        assert!(m.state().sounds.iter().all(|s| s.id != id || s.stopping));
        play_for(&mut m, 300);
        assert_eq!(music(&m).queue.len(), 1, "the trimmed track went back into the repeating queue");
    }

    #[test]
    fn sounds_move_between_channels_and_always_loop_when_set() {
        let f = files();
        let mut m = mixer();
        // A sound set to always loop repeats even as an effect.
        let beat = m.play(PlayRequest { always_loop: true, ..req(&f.short, Channel::Sfx, 300) }).unwrap();
        play_for(&mut m, 1_500);
        assert!(m.state().sounds.iter().any(|s| s.id == beat && s.looping), "still going after five times its length");

        // A playing music track moves to ambience and carries on from where it was.
        let track = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        let queued = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        play_for(&mut m, 2_000);
        m.move_to_channel(track, Channel::Ambience).unwrap();
        play_for(&mut m, 300);
        let s = m.state();
        let moved = s.sounds.iter().find(|s| s.channel == Channel::Ambience && !s.stopping).expect("now on ambience");
        assert!(moved.position_ms >= 2_000, "carried on at {} ms", moved.position_ms);
        assert!(s.sounds.iter().any(|s| s.id == queued && !s.stopping), "the queue moved on");

        // A queued track moves to another channel and starts there.
        let waiting = m.play(req(&f.short, Channel::Music, 300)).unwrap();
        m.move_to_channel(waiting, Channel::Stingers).unwrap();
        assert!(music(&m).queue.is_empty());
        assert!(m.state().sounds.iter().any(|s| s.channel == Channel::Stingers));

        // The playing track goes back into the queue, behind the next one.
        let a = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        let b = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        m.requeue_playing(queued, 2);
        let s = m.state();
        assert!(s.sounds.iter().any(|s| s.id == a && !s.stopping), "the next track plays");
        let ids: Vec<u64> = music(&m).queue.iter().map(|q| q.id).collect();
        assert_eq!(ids[0], b);
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn queue_moves_on_when_a_track_ends_and_repeats_as_a_whole() {
        let f = files();
        let mut m = mixer();
        let first = m.play(req(&f.short, Channel::Music, 300)).unwrap();
        let second = m.play(req(&f.short, Channel::Music, 300)).unwrap();

        // The first track ends, the second starts, the first rejoins the line.
        render_until(&mut m, |m| m.state().sounds.first().is_some_and(|s| s.id == second));
        assert_eq!(m.sound_count(), 1);
        assert_eq!(music(&m).queue.len(), 1);
        let rejoined = music(&m).queue[0].id;
        assert_ne!(rejoined, first, "a requeued track gets a fresh id");

        // And round again.
        render_until(&mut m, |m| m.state().sounds.first().is_some_and(|s| s.id == rejoined));
        assert_eq!(music(&m).queue.len(), 1);
    }

    #[test]
    fn queue_stops_at_the_end_when_repeat_is_off() {
        let f = files();
        let mut m = mixer();
        m.set_loop_queue(false);
        m.play(req(&f.short, Channel::Music, 300)).unwrap();
        let second = m.play(req(&f.short, Channel::Music, 300)).unwrap();
        render_until(&mut m, |m| m.state().sounds.first().is_some_and(|s| s.id == second));
        assert!(music(&m).queue.is_empty());
        render_until(&mut m, |m| m.sound_count() == 0);
        assert!(music(&m).queue.is_empty());
    }

    #[test]
    fn a_lone_music_track_repeats_until_another_is_queued() {
        let f = files();
        let mut m = mixer();
        let first = m.play(req(&f.short, Channel::Music, 300)).unwrap();
        // Three times its length later it is still the same sound.
        play_for(&mut m, 1000);
        let s = m.state();
        assert_eq!(s.sounds.len(), 1);
        assert_eq!(s.sounds[0].id, first);
        assert!(s.sounds[0].position_ms < 300);

        // Queue another: the first now plays to its end and hands over.
        let second = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        render_until(&mut m, |m| m.state().sounds.first().is_some_and(|s| s.id == second));
        // The long track does not repeat on its own while the short one waits.
        assert_eq!(music(&m).queue.len(), 1);
    }

    #[test]
    fn any_sound_can_be_set_to_repeat_and_released() {
        let f = files();
        let mut m = mixer();
        let id = m.play(req(&f.short, Channel::Sfx, 300)).unwrap();
        m.set_sound_looping(id, true);
        assert!(m.state().sounds[0].looping);
        play_for(&mut m, 1000);
        assert_eq!(m.sound_count(), 1, "a repeating one-shot keeps going");

        m.set_sound_looping(id, false);
        render_until(&mut m, |m| m.sound_count() == 0);

        // A music track set to repeat holds the queue back.
        m.set_channel_fade(Channel::Music, 100);
        let held = m.play(req(&f.short, Channel::Music, 300)).unwrap();
        let waiting = m.play(req(&f.short, Channel::Music, 300)).unwrap();
        m.set_sound_looping(held, true);
        play_for(&mut m, 1000);
        assert_eq!(m.state().sounds[0].id, held);
        assert_eq!(music(&m).queue[0].id, waiting);

        // The flag can be set on a queued track too, and survives the wait.
        m.set_sound_looping(waiting, true);
        assert!(music(&m).queue[0].looping);
        m.set_sound_looping(held, false);
        render_until(&mut m, |m| m.state().sounds.iter().any(|s| s.id == waiting));
        assert!(m.state().sounds.iter().find(|s| s.id == waiting).unwrap().looping);
    }

    #[test]
    fn next_and_play_now_crossfade_through_the_queue() {
        let f = files();
        let mut m = mixer();
        m.set_channel_fade(Channel::Music, 300);
        m.next_track(); // nothing queued: nothing happens
        let a = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        m.next_track();
        assert!(!m.state().sounds[0].stopping, "next with an empty queue keeps playing");

        let b = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        let c = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        play_for(&mut m, 60);

        // Jump straight to the last one.
        m.play_queued_now(c);
        let s = m.state();
        assert_eq!(s.sounds.len(), 2, "old and new sound together during the crossfade");
        assert!(s.sounds.iter().find(|x| x.id == a).unwrap().stopping);
        assert!(!s.sounds.iter().find(|x| x.id == c).unwrap().stopping);
        // The queue repeats, so the interrupted track goes to the back.
        let queue = music(&m).queue;
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].id, b);
        render_until(&mut m, |m| m.sound_count() == 1);

        m.next_track();
        assert!(m.state().sounds.iter().any(|x| x.id == b && !x.stopping));
        render_until(&mut m, |m| m.sound_count() == 1);

        // Interrupting from the library replaces the track and keeps the queue.
        let before = music(&m).queue.len();
        let d = m.play(now(req(&f.long, Channel::Music, 30_000))).unwrap();
        assert!(m.state().sounds.iter().any(|x| x.id == d && !x.stopping));
        assert_eq!(music(&m).queue.len(), before);
    }

    #[test]
    fn stopping_the_current_track_moves_on_but_stop_all_empties_the_queue() {
        let f = files();
        let mut m = mixer();
        m.set_channel_fade(Channel::Music, 100);
        let a = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        let b = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        m.play(req(&f.long, Channel::Music, 30_000)).unwrap();

        m.stop_sound(a, None);
        assert!(m.state().sounds.iter().any(|x| x.id == b && !x.stopping));
        // A stopped track does not rejoin the queue.
        assert_eq!(music(&m).queue.len(), 1);

        m.stop_channel(Channel::Music, None);
        assert!(music(&m).queue.is_empty());
        render_until(&mut m, |m| m.sound_count() == 0);

        // Panic empties the queue as well.
        m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        m.panic();
        assert!(music(&m).queue.is_empty());
        render_until(&mut m, |m| m.sound_count() == 0);
    }

    #[test]
    fn an_unreadable_queued_track_is_skipped() {
        let f = files();
        let mut m = mixer();
        m.set_loop_queue(false);
        m.play(req(&f.short, Channel::Music, 300)).unwrap();
        let missing = f.long.with_file_name("nope.wav");
        m.play(req(&missing, Channel::Music, 1000)).unwrap();
        let good = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        render_until(&mut m, |m| m.state().sounds.first().is_some_and(|s| s.id == good));
        assert!(m.state().last_error.is_some());
        assert!(music(&m).queue.is_empty());
    }

    fn count_on(m: &Mixer<MockBackend>, channel: Channel, stopping: bool) -> usize {
        m.state().sounds.iter().filter(|s| s.channel == channel && s.stopping == stopping).count()
    }

    #[test]
    fn stingers_duck_the_music_and_it_recovers() {
        let f = files();
        let mut m = mixer();
        m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        m.play(req(&f.short, Channel::Sfx, 300)).unwrap();
        assert!(!m.state().ducked, "plain effects do not duck");

        m.play(req(&f.short, Channel::Stingers, 300)).unwrap();
        assert!(m.state().ducked);
        render_until(&mut m, |m| m.sound_count() == 1);
        assert!(!m.state().ducked, "music comes back when the stinger ends");

        // Any sound can ask for it, and a stinger can decline.
        let mut pad = req(&f.short, Channel::Sfx, 300);
        pad.duck = Some(true);
        m.play(pad).unwrap();
        assert!(m.state().ducked);
        render_until(&mut m, |m| m.sound_count() == 1);
        let mut quiet = req(&f.short, Channel::Stingers, 300);
        quiet.duck = Some(false);
        m.play(quiet).unwrap();
        assert!(!m.state().ducked);
    }

    #[test]
    fn meters_follow_the_sound_and_solo_silences_the_rest() {
        let f = files();
        let mut m = mixer();
        play_for(&mut m, 60);
        assert_eq!(m.state().levels, [0.0; 5]);

        m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        play_for(&mut m, 400);
        let levels = m.state().levels;
        assert!(levels[Channel::Music.index()] > 0.05, "{levels:?}");
        assert!(levels[4] > 0.02, "master shows the mix: {levels:?}");
        assert_eq!(levels[Channel::Sfx.index()], 0.0);

        // Solo another channel: the music still runs but nothing reaches the master.
        m.set_channel_solo(Channel::Ambience, true);
        assert!(m.state().channels[Channel::Ambience.index()].solo);
        play_for(&mut m, 300);
        let levels = m.state().levels;
        assert!(levels[Channel::Music.index()] > 0.05);
        assert_eq!(levels[4], 0.0, "{levels:?}");

        m.set_channel_solo(Channel::Ambience, false);
        play_for(&mut m, 300);
        assert!(m.state().levels[4] > 0.02);
    }

    #[test]
    fn an_audio_profile_shapes_the_master() {
        let f = files();
        let mut m = mixer();
        m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        play_for(&mut m, 400);
        let flat = m.state().levels[4];
        assert!(flat > 0.02);

        // The 220 Hz test tone sits above the bass shelf, so cutting bass
        // barely touches it, while turning the level down clearly does.
        m.set_profile(AudioProfile { bass_db: -12.0, ..Default::default() });
        play_for(&mut m, 400);
        let bass_cut = m.state().levels[4];
        assert!(bass_cut > flat * 0.5, "{flat} -> {bass_cut}");

        // The master meter sits before the master's own volume, so check
        // the setting took and that out-of-range values are tamed.
        m.set_profile(AudioProfile { gain_db: 99.0, compress: 5.0, bass_db: f32::NAN, ..Default::default() });
        let profile = m.state().profile;
        assert_eq!((profile.gain_db, profile.compress, profile.bass_db), (12.0, 1.0, 0.0));
        play_for(&mut m, 200);
        assert!(m.state().levels[4].is_finite());

        // Old saved settings without a profile load with the default one.
        let old: MixerSettings = serde_json::from_str(
            r#"{"master_volume":0.5,"channels":[{"volume":1,"muted":false,"fade_ms":0},{"volume":1,"muted":false,"fade_ms":0},{"volume":1,"muted":false,"fade_ms":0},{"volume":1,"muted":false,"fade_ms":0}]}"#,
        )
        .unwrap();
        assert_eq!(old.profile, AudioProfile::default());
        assert!(old.channels[0].loop_queue);
    }

    #[test]
    fn a_sound_has_its_own_level_and_short_sounds_are_cached() {
        let f = files();
        let mut m = mixer();
        let mut pad = req(&f.short, Channel::Sfx, 300);
        pad.trim_db = -6.0;
        pad.rate = 1.2;
        pad.pan = -0.5;
        pad.tag = Some("pad:1:0".into());
        assert!(!m.is_cached(&f.short));
        let id = m.play(pad.clone()).unwrap();
        assert!(m.is_cached(&f.short), "decoded once, kept for next time");
        let s = m.state();
        assert_eq!(s.sounds[0].trim_db, -6.0);
        assert_eq!(s.sounds[0].tag.as_deref(), Some("pad:1:0"));
        m.set_sound_trim(id, -12.0);
        assert_eq!(m.state().sounds[0].trim_db, -12.0);

        // A cached pad starts well inside the 30 ms budget.
        let started = Instant::now();
        m.play(pad).unwrap();
        let took = started.elapsed();
        assert!(took < Duration::from_millis(30), "pad took {took:?}");
    }

    #[test]
    fn random_interval_layers_keep_firing_until_stopped() {
        let f = files();
        let mut m = mixer();
        let id = m.add_emitter(EmitterRequest {
            name: "Drips".into(),
            sounds: vec![req(&f.short, Channel::Ambience, 300)],
            min_ms: 80,
            max_ms: 120,
            random_pan: true,
            tag: None,
        });
        assert_eq!(m.state().emitters.len(), 1);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..50 {
            play_for(&mut m, 20);
            thread::sleep(Duration::from_millis(18));
            for s in m.state().sounds {
                assert!(!s.looping, "each occurrence plays once");
                seen.insert(s.id);
            }
        }
        assert!(seen.len() >= 3, "fired {} times in about a second", seen.len());

        m.stop_sound(id, None);
        assert!(m.state().emitters.is_empty());
        render_until(&mut m, |m| m.sound_count() == 0);

        // Stopping the ambience channel also ends the layers.
        m.add_emitter(EmitterRequest {
            name: "Drips".into(),
            sounds: vec![req(&f.short, Channel::Ambience, 300)],
            min_ms: 80,
            max_ms: 120,
            random_pan: false,
            tag: None,
        });
        m.stop_channel(Channel::Ambience, None);
        assert!(m.state().emitters.is_empty());
    }

    fn layer(path: &PathBuf, asset_id: i64, trim_db: f32) -> PlayRequest {
        PlayRequest { asset_id, trim_db, ..req(path, Channel::Ambience, 30_000) }
    }

    #[test]
    fn scenes_switch_without_a_gap() {
        let f = files();
        let mut m = mixer();
        let music = |asset_id| PlayRequest { asset_id, ..req(&f.long, Channel::Music, 30_000) };

        m.apply_scene(SceneRequest {
            id: 1,
            name: "Tavern".into(),
            transition_ms: 300,
            music: SceneMusic::Tracks(vec![music(100), music(101)]),
            layers: (1..=4).map(|i| layer(&f.long, i, 0.0)).collect(),
            emitters: vec![],
        });
        assert_eq!(count_on(&m, Channel::Ambience, false), 4);
        assert_eq!(count_on(&m, Channel::Music, false), 1);
        assert_eq!(music_state(&m).queue.len(), 1);
        assert_eq!(m.state().scene.unwrap().name, "Tavern");
        play_for(&mut m, 400);
        let kept: Vec<u64> = m.state().sounds.iter().filter(|s| s.asset_id == 3 || s.asset_id == 4).map(|s| s.id).collect();

        // Two layers stay, two go, two arrive; the music changes.
        m.apply_scene(SceneRequest {
            id: 2,
            name: "Storm".into(),
            transition_ms: 300,
            music: SceneMusic::Tracks(vec![music(200)]),
            layers: vec![layer(&f.long, 3, -6.0), layer(&f.long, 4, 0.0), layer(&f.long, 5, 0.0), layer(&f.long, 6, 0.0)],
            emitters: vec![EmitterRequest {
                name: "Thunder".into(),
                sounds: vec![req(&f.short, Channel::Ambience, 300)],
                min_ms: 20_000,
                max_ms: 90_000,
                random_pan: true,
                tag: None,
            }],
        });
        let s = m.state();
        // During the transition old and new sound together: never fewer than four.
        assert_eq!(count_on(&m, Channel::Ambience, false), 4);
        assert_eq!(count_on(&m, Channel::Ambience, true), 2);
        for id in &kept {
            let layer = s.sounds.iter().find(|x| x.id == *id).unwrap();
            assert!(!layer.stopping, "shared layers are not restarted");
        }
        assert_eq!(s.sounds.iter().find(|x| x.asset_id == 3).unwrap().trim_db, -6.0);
        assert_eq!(count_on(&m, Channel::Music, true), 1);
        assert_eq!(count_on(&m, Channel::Music, false), 1);
        assert!(music_state(&m).queue.is_empty());
        assert_eq!(s.emitters.len(), 1);
        assert_eq!(s.scene.unwrap().id, 2);

        render_until(&mut m, |m| m.sound_count() == 5);

        // Same first track: the music carries on untouched. Keep: likewise.
        let playing = m.state().sounds.iter().find(|s| s.channel == Channel::Music).unwrap().id;
        for music in [SceneMusic::Tracks(vec![music(200), music(201)]), SceneMusic::Keep] {
            m.apply_scene(SceneRequest { id: 3, name: "Calm".into(), transition_ms: 100, music, ..Default::default() });
            let s = m.state();
            let track = s.sounds.iter().find(|s| s.id == playing).unwrap();
            assert!(!track.stopping);
        }
        assert_eq!(music_state(&m).queue.len(), 1, "keep leaves the queue the earlier scene set");
        assert!(m.state().emitters.is_empty(), "a scene without random layers ends the old ones");
        assert_eq!(count_on(&m, Channel::Ambience, false), 0);

        m.apply_scene(SceneRequest { id: 4, name: "Silence".into(), transition_ms: 100, music: SceneMusic::Silence, ..Default::default() });
        render_until(&mut m, |m| m.sound_count() == 0);

        m.apply_scene(SceneRequest { id: 5, name: "X".into(), ..Default::default() });
        m.play(req(&f.long, Channel::Sfx, 30_000)).unwrap();
        m.panic();
        assert!(m.state().scene.is_none(), "panic clears the active scene");
    }

    #[test]
    fn ambience_layers_stack_and_stop_individually() {
        let f = files();
        let mut m = mixer();
        m.set_channel_fade(Channel::Ambience, 200);
        let a = m.play(req(&f.long, Channel::Ambience, 30_000)).unwrap();
        let b = m.play(req(&f.long, Channel::Ambience, 30_000)).unwrap();
        render(&mut m, 50);
        assert_eq!(m.sound_count(), 2);

        m.stop_sound(a, None);
        render_until(&mut m, |m| m.sound_count() == 1);
        assert_eq!(m.state().sounds[0].id, b);

        m.stop_channel(Channel::Ambience, Some(0));
        render_until(&mut m, |m| m.sound_count() == 0);
    }

    #[test]
    fn panic_fades_then_second_press_cuts() {
        let f = files();
        let mut m = mixer();
        assert_eq!(m.panic(), PanicOutcome::Idle);

        m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        m.play(req(&f.long, Channel::Ambience, 30_000)).unwrap();
        m.play(req(&f.short, Channel::Sfx, 300)).unwrap();
        render(&mut m, 50);

        assert_eq!(m.panic(), PanicOutcome::Fading);
        assert!(m.state().panic_armed);
        assert!(m.state().sounds.iter().all(|s| s.stopping));
        // Half way through the one-second fade the long sounds are still there.
        render(&mut m, 500);
        assert!(m.sound_count() >= 2);

        assert_eq!(m.panic(), PanicOutcome::Cut);
        render(&mut m, 20);
        assert_eq!(m.sound_count(), 0);
        assert!(!m.state().panic_armed);
    }

    #[test]
    fn panic_fade_alone_silences_within_about_a_second() {
        let f = files();
        let mut m = mixer();
        m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
        render(&mut m, 50);
        assert_eq!(m.panic(), PanicOutcome::Fading);
        render(&mut m, 1100);
        assert_eq!(m.sound_count(), 0);
        // Playing again after a panic works and disarms it.
        m.play(req(&f.short, Channel::Sfx, 300)).unwrap();
        assert!(!m.state().panic_armed);
    }

    #[test]
    fn settings_are_tracked_and_sanitized() {
        let mut m = mixer();
        m.set_master_volume(2.0);
        m.set_channel_volume(Channel::Sfx, -1.0);
        m.set_channel_muted(Channel::Music, true);
        m.set_channel_fade(Channel::Ambience, 5000);
        let s = m.state();
        assert_eq!(s.master_volume, 1.0);
        assert_eq!(s.channels[Channel::Sfx.index()].volume, 0.0);
        assert!(s.channels[Channel::Music.index()].muted);
        assert_eq!(s.channels[Channel::Ambience.index()].fade_ms, 5000);

        let wild = MixerSettings {
            master_volume: f32::NAN,
            ..Default::default()
        };
        assert_eq!(wild.sanitized().master_volume, 0.0);
    }

    #[test]
    fn engine_thread_runs_commands_and_reports_state() {
        let f = files();
        let seen: Arc<Mutex<Vec<EngineState>>> = Arc::default();
        let sink = seen.clone();
        let engine = Engine::spawn_with(
            MixerSettings::default(),
            move |s| sink.lock().unwrap().push(s.clone()),
            || Ok(manager()),
        );

        assert!(engine.state().unwrap().device_ok);
        let id = engine.play(req(&f.short, Channel::Sfx, 300)).unwrap();
        engine.set_master_volume(0.5).unwrap();
        engine.set_channel_muted(Channel::Music, true).unwrap();
        let state = engine.state().unwrap();
        assert_eq!(state.sounds[0].id, id);
        assert_eq!(state.master_volume, 0.5);
        assert!(state.channels[0].muted);

        assert_eq!(engine.panic().unwrap(), PanicOutcome::Fading);
        assert_eq!(engine.panic().unwrap(), PanicOutcome::Cut);

        let log = engine.command_log().unwrap();
        assert_eq!(log.len(), 5);
        assert!(log[0].contains("Play"));
        assert!(log[4].contains("Panic"));

        // State changes were pushed without being asked for.
        thread::sleep(Duration::from_millis(120));
        assert!(seen.lock().unwrap().iter().any(|s| !s.sounds.is_empty()));
    }

    #[test]
    fn engine_without_a_device_reports_it_and_recovers() {
        let f = files();
        let available = Arc::new(Mutex::new(false));
        let flag = available.clone();
        let engine = Engine::spawn_with(
            MixerSettings::default(),
            |_| {},
            move || {
                if *flag.lock().unwrap() {
                    Ok(manager())
                } else {
                    Err("no speakers".to_string())
                }
            },
        );

        let s = engine.state().unwrap();
        assert!(!s.device_ok);
        assert_eq!(s.device_error.as_deref(), Some("no speakers"));
        assert!(matches!(
            engine.play(req(&f.short, Channel::Sfx, 300)),
            Err(Error::NoDevice(_))
        ));
        // Settings changed while offline are kept…
        engine.set_master_volume(0.25).unwrap();

        // …and applied once a device shows up.
        *available.lock().unwrap() = true;
        engine.retry_device().unwrap();
        let s = engine.state().unwrap();
        assert!(s.device_ok);
        assert_eq!(s.master_volume, 0.25);
        engine.play(req(&f.short, Channel::Sfx, 300)).unwrap();
    }
}
