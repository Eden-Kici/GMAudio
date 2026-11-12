//! The audio engine.
//!
//! [`Mixer`] owns the audio device and every playing sound. [`Engine`] runs
//! a mixer on its own thread: the rest of the app only sends it commands
//! and receives state snapshots, so playback never depends on the UI.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use kira::backend::Backend;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{FromFileError, PlaybackState};
use kira::track::{TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Easing, Tween};
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
const COMMAND_LOG_SIZE: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Music,
    Ambience,
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

    /// Music and ambience loop unless told otherwise; one-shots do not.
    fn loops_by_default(self) -> bool {
        matches!(self, Channel::Music | Channel::Ambience)
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
}

/// Everything about the mixer that is remembered between sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixerSettings {
    pub master_volume: f32,
    /// In [`Channel::ALL`] order.
    pub channels: [ChannelSettings; 4],
}

impl Default for MixerSettings {
    fn default() -> Self {
        Self {
            master_volume: 0.8,
            channels: Channel::ALL.map(|c| ChannelSettings {
                volume: 0.8,
                muted: false,
                fade_ms: c.default_fade_ms(),
            }),
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

#[derive(Debug, Clone)]
pub struct PlayRequest {
    pub asset_id: i64,
    pub path: PathBuf,
    pub name: String,
    pub channel: Channel,
    /// Loudness-normalization gain for this file.
    pub gain_db: f32,
    pub duration_ms: Option<i64>,
    /// `None` uses the channel default.
    pub looping: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelState {
    pub channel: Channel,
    pub volume: f32,
    pub muted: bool,
    pub fade_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SoundState {
    pub id: u64,
    pub asset_id: i64,
    pub name: String,
    pub channel: Channel,
    pub position_ms: i64,
    pub duration_ms: Option<i64>,
    pub looping: bool,
    /// Fading out; it will be gone when the fade ends.
    pub stopping: bool,
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
}

struct ActiveSound {
    id: u64,
    asset_id: i64,
    name: String,
    channel: Channel,
    duration_ms: Option<i64>,
    looping: bool,
    stopping: bool,
    handle: Handle,
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
    tracks: Vec<TrackHandle>,
    settings: MixerSettings,
    sounds: Vec<ActiveSound>,
    next_id: u64,
    panic_armed: bool,
    last_error: Option<String>,
}

impl<B: Backend> Mixer<B> {
    pub fn new(mut manager: AudioManager<B>, settings: MixerSettings) -> Result<Self> {
        let settings = settings.sanitized();
        let mut tracks = Vec::with_capacity(4);
        for channel in Channel::ALL {
            let c = settings.channels[channel.index()];
            let volume = if c.muted { Decibels::SILENCE.0 } else { fader_to_db(c.volume) };
            tracks.push(
                manager
                    .add_sub_track(TrackBuilder::new().volume(volume))
                    .map_err(|e| Error::Playback(e.to_string()))?,
            );
        }
        manager
            .main_track()
            .set_volume(fader_to_db(settings.master_volume), Tween::default());
        Ok(Self {
            manager,
            tracks,
            settings,
            sounds: Vec::new(),
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

    /// Starts a sound and returns its id.
    ///
    /// Music is exclusive: a new track crossfades with whatever was playing.
    /// Ambience layers stack. One-shots overlap freely.
    pub fn play(&mut self, req: PlayRequest) -> Result<u64> {
        let channel = req.channel;
        let fade = ms(self.settings.channels[channel.index()].fade_ms);
        let looping = req.looping.unwrap_or(channel.loops_by_default());
        let one_shot = matches!(channel, Channel::Sfx | Channel::Stingers);

        let replacing_music = channel == Channel::Music
            && self
                .sounds
                .iter()
                .any(|s| s.channel == Channel::Music && !s.stopping);
        let fade_in_tween = match channel {
            // First track starts promptly; a replacement crossfades.
            Channel::Music if replacing_music => Some(fade_in(fade)),
            Channel::Music => Some(fade_in(Duration::from_millis(80))),
            Channel::Ambience => Some(fade_in(fade)),
            Channel::Sfx | Channel::Stingers => None,
        };

        let load_err = |e: FromFileError| Error::Playback(format!("{}: {e}", req.name));
        let in_memory = one_shot && req.duration_ms.is_some_and(|d| d <= STATIC_MAX_MS);
        let track = &mut self.tracks[channel.index()];
        let handle = if in_memory {
            let mut data = StaticSoundData::from_file(&req.path)
                .map_err(load_err)?
                .volume(req.gain_db)
                .fade_in_tween(fade_in_tween);
            if looping {
                data = data.loop_region(..);
            }
            Handle::Static(
                track
                    .play(data)
                    .map_err(|e| Error::Playback(e.to_string()))?,
            )
        } else {
            let mut data = StreamingSoundData::from_file(&req.path)
                .map_err(load_err)?
                .volume(req.gain_db)
                .fade_in_tween(fade_in_tween);
            if looping {
                data = data.loop_region(..);
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
            for s in self.sounds.iter_mut().filter(|s| s.channel == Channel::Music) {
                if !s.stopping {
                    s.handle.stop(fade_out(fade));
                    s.stopping = true;
                }
            }
        }

        let id = self.next_id;
        self.next_id += 1;
        self.sounds.push(ActiveSound {
            id,
            asset_id: req.asset_id,
            name: req.name,
            channel,
            duration_ms: req.duration_ms,
            looping,
            stopping: false,
            handle,
        });
        self.panic_armed = false;
        Ok(id)
    }

    /// Fades one sound out. `fade_ms: None` uses the channel's fade time.
    pub fn stop_sound(&mut self, id: u64, fade_ms: Option<u32>) {
        if let Some(s) = self.sounds.iter_mut().find(|s| s.id == id) {
            let fade = fade_ms.unwrap_or(self.settings.channels[s.channel.index()].fade_ms);
            s.handle.stop(fade_out(ms(fade)));
            s.stopping = true;
        }
    }

    pub fn stop_channel(&mut self, channel: Channel, fade_ms: Option<u32>) {
        let fade = fade_ms.unwrap_or(self.settings.channels[channel.index()].fade_ms);
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
        let c = self.settings.channels[channel.index()];
        let db = if c.muted { Decibels::SILENCE.0 } else { fader_to_db(c.volume) };
        self.tracks[channel.index()].set_volume(db, smooth());
    }

    pub fn set_master_volume(&mut self, volume: f32) {
        self.settings.master_volume = clamp_unit(volume);
        self.manager
            .main_track()
            .set_volume(fader_to_db(self.settings.master_volume), smooth());
    }

    /// First press fades everything out over one second. A second press
    /// while that fade is still running cuts instantly.
    pub fn panic(&mut self) -> PanicOutcome {
        if self.sounds.is_empty() {
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
        self.panic_armed = outcome == PanicOutcome::Fading;
        outcome
    }

    /// Housekeeping: forgets finished sounds and collects stream errors.
    pub fn tick(&mut self) {
        for s in &mut self.sounds {
            if let Some(e) = s.handle.pop_error() {
                self.last_error = Some(format!("{}: {e}", s.name));
            }
        }
        self.sounds
            .retain(|s| s.handle.state() != PlaybackState::Stopped);
        if self.sounds.is_empty() {
            self.panic_armed = false;
        }
    }

    pub fn sound_count(&self) -> usize {
        self.sounds.len()
    }

    pub fn state(&self) -> EngineState {
        EngineState {
            device_ok: true,
            device_error: None,
            master_volume: self.settings.master_volume,
            channels: channel_states(&self.settings),
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
                })
                .collect(),
            panic_armed: self.panic_armed,
            last_error: self.last_error.clone(),
        }
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
}

impl Engine {
    /// Starts the engine on the default audio output.
    ///
    /// `on_state` is called from the engine thread whenever the state
    /// changes (up to ~30 times a second while something is playing).
    pub fn spawn(settings: MixerSettings, on_state: impl Fn(&EngineState) + Send + 'static) -> Self {
        Self::spawn_with(settings, on_state, || {
            AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
                .map_err(|e| e.to_string())
        })
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

                            if matches!(cmd, Command::RetryDevice) {
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
        Command::Panic => Ok(Reply::Panic(mixer.panic())),
        Command::RetryDevice => Ok(Reply::Done),
    }
}

fn run_offline(
    settings: &mut MixerSettings,
    command: Command,
    device_error: &Option<String>,
) -> Result<Reply> {
    match command {
        Command::Play(_) => Err(Error::NoDevice(
            device_error.clone().unwrap_or_else(|| "no output device".into()),
        )),
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
        Command::Panic => Ok(Reply::Panic(PanicOutcome::Idle)),
        Command::StopSound { .. } | Command::StopChannel { .. } | Command::RetryDevice => {
            Ok(Reply::Done)
        }
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
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::testutil::write_sine_wav;
    use kira::backend::mock::{MockBackend, MockBackendSettings};
    use std::sync::{Arc, Mutex};

    const RATE: u32 = 48_000;

    fn manager() -> AudioManager<MockBackend> {
        AudioManager::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: RATE },
            ..Default::default()
        })
        .unwrap()
    }

    fn mixer() -> Mixer<MockBackend> {
        Mixer::new(manager(), MixerSettings::default()).unwrap()
    }

    /// Renders `millis` of audio through the mock device.
    fn render(m: &mut Mixer<MockBackend>, millis: u64) {
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
    fn render_until(m: &mut Mixer<MockBackend>, mut done: impl FnMut(&Mixer<MockBackend>) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(m) {
            assert!(Instant::now() < deadline, "timed out; state: {:?}", m.state());
            render(m, 20);
            thread::sleep(Duration::from_millis(2));
        }
    }

    struct Files {
        _dir: tempfile::TempDir,
        long: PathBuf,
        short: PathBuf,
    }

    fn files() -> Files {
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

    fn req(path: &PathBuf, channel: Channel, duration_ms: i64) -> PlayRequest {
        PlayRequest {
            asset_id: 7,
            path: path.clone(),
            name: "test".into(),
            channel,
            gain_db: 0.0,
            duration_ms: Some(duration_ms),
            looping: None,
        }
    }

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
        assert!(m.state().sounds[0].looping, "music loops by default");

        let second = m.play(req(&f.long, Channel::Music, 30_000)).unwrap();
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
            m.play(req(&missing, Channel::Music, 1000)),
            Err(Error::Playback(_))
        ));
        let s = m.state();
        assert_eq!(s.sounds.len(), 1);
        assert!(!s.sounds[0].stopping);
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
