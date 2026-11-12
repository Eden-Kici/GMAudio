//! Tempo detection: finds a track's beats per minute and where its first
//! beat falls, so music can be switched in time.
//!
//! The method is deliberately simple and has no dependencies:
//! 1. Track how loudness jumps over time (an "onset envelope", 200 values
//!    a second). Drum hits and plucked notes show up as spikes.
//! 2. Lay a grid of evenly spaced beats over the envelope and see how well
//!    the spikes sit on it, for every tempo from 50 to 220 BPM. Prefer
//!    ordinary tempos where half and double speed fit equally well.
//! 3. Fine-tune the winner and note where its first beat falls.
//!
//! It finds the beat, not the bar: which beat is "one" is left to the GM.

/// Envelope values per second, roughly. The exact rate depends on the
/// file's sample rate, because each value covers a whole number of samples.
const TARGET_ENV_RATE: f64 = 200.0;
/// Only the first two minutes are looked at; tempo rarely needs more.
const MAX_SECONDS: f64 = 120.0;
const MIN_SECONDS: f64 = 8.0;
const MIN_BPM: f64 = 50.0;
const MAX_BPM: f64 = 220.0;
/// How much better than chance the beat grid must fit before we trust it.
const MIN_CONFIDENCE: f64 = 1.8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub bpm: f64,
    /// Time of the first beat, in milliseconds from the start.
    pub offset_ms: f64,
    /// How clearly the beat stood out. Above 3 is a strong, steady beat.
    pub confidence: f64,
}

/// Collects the onset envelope while a file is being decoded.
pub struct OnsetTracker {
    channels: usize,
    hop: usize,
    /// Envelope values per second for this file.
    rate: f64,
    acc: f64,
    count: usize,
    previous: f64,
    envelope: Vec<f32>,
}

impl OnsetTracker {
    pub fn new(sample_rate: u32, channels: u32) -> Self {
        let hop = ((sample_rate as f64 / TARGET_ENV_RATE).round() as usize).max(1);
        Self {
            channels: channels.max(1) as usize,
            hop,
            rate: sample_rate as f64 / hop as f64,
            acc: 0.0,
            count: 0,
            previous: f64::NEG_INFINITY,
            envelope: Vec::new(),
        }
    }

    /// Feeds interleaved samples.
    pub fn push(&mut self, samples: &[f32]) {
        if self.envelope.len() as f64 >= MAX_SECONDS * self.rate {
            return;
        }
        for frame in samples.chunks_exact(self.channels) {
            let mono = frame.iter().sum::<f32>() as f64 / self.channels as f64;
            self.acc += mono * mono;
            self.count += 1;
            if self.count == self.hop {
                // Loudness on a log scale, so quiet and loud passages count alike.
                let level = (self.acc / self.hop as f64 + 1e-7).log10();
                let rise = if self.previous.is_finite() { (level - self.previous).max(0.0) } else { 0.0 };
                self.envelope.push(rise as f32);
                self.previous = level;
                self.acc = 0.0;
                self.count = 0;
            }
        }
    }

    pub fn finish(self) -> Option<Tempo> {
        detect(&self.envelope, self.rate)
    }
}

/// Reads the envelope at a fractional position.
fn at(env: &[f32], position: f64) -> f64 {
    let i = position.floor() as usize;
    if i + 1 >= env.len() {
        return 0.0;
    }
    let t = position - i as f64;
    env[i] as f64 * (1.0 - t) + env[i + 1] as f64 * t
}

/// Average envelope height on a grid of beats: high when beats sit on onsets.
fn grid_fit(env: &[f32], period: f64, phase: f64) -> f64 {
    let mut sum = 0.0;
    let mut beats = 0usize;
    let mut position = phase;
    while position < (env.len() - 1) as f64 {
        sum += at(env, position);
        beats += 1;
        position += period;
    }
    if beats == 0 { 0.0 } else { sum / beats as f64 }
}

fn detect(raw: &[f32], rate: f64) -> Option<Tempo> {
    let n = raw.len();
    if (n as f64) < MIN_SECONDS * rate {
        return None;
    }
    // A little smoothing makes the fit tolerant of onsets a frame early or late.
    let env: Vec<f32> = (0..n)
        .map(|i| {
            let a = raw[i.saturating_sub(1)];
            let c = raw[(i + 1).min(n - 1)];
            0.25 * a + 0.5 * raw[i] + 0.25 * c
        })
        .collect();
    let mean = env.iter().map(|v| *v as f64).sum::<f64>() / n as f64;
    if mean <= 1e-9 {
        return None;
    }

    // Step 2: try every tempo on a coarse scale. For each, lay a beat grid
    // over the envelope and slide it to its best position. The envelope is
    // judged in ten-second stretches, each with its own grid position, so a
    // tempo that is slightly off still scores well at this stage.
    let chunk = (rate * 10.0) as usize;
    let chunks: Vec<&[f32]> = env.chunks(chunk).filter(|c| c.len() >= chunk / 2).collect();
    let best_fit = |part: &[f32], period: f64, phase_step: f64| -> (f64, f64) {
        let mut top = (0.0, f64::MIN);
        let mut phase = 0.0;
        while phase < period {
            let fit = grid_fit(part, period, phase);
            if fit > top.1 {
                top = (phase, fit);
            }
            phase += phase_step;
        }
        top
    };
    let mut rough = (0.0, f64::MIN);
    let mut bpm = MIN_BPM;
    while bpm <= MAX_BPM {
        let period = rate * 60.0 / bpm;
        let fit = chunks.iter().map(|c| best_fit(c, period, 1.0).1).sum::<f64>() / chunks.len() as f64;
        // Half and double tempo fit a steady beat equally well; favour
        // everyday tempos to settle "is it 70 or 140?".
        let octaves_off = (bpm / 118.0).log2();
        let prior = (-0.5 * (octaves_off / 0.85).powi(2)).exp();
        if fit * prior > rough.1 {
            rough = (bpm, fit * prior);
        }
        bpm += 0.5;
    }
    let rough_bpm = rough.0;

    // Step 3: fine-tune over the whole envelope with one grid position.
    let search = |from: f64, to: f64, step: f64, phase_step: f64| -> (f64, f64, f64) {
        let mut top = (from, 0.0, f64::MIN);
        let mut bpm = from;
        while bpm <= to {
            let (phase, fit) = best_fit(&env, rate * 60.0 / bpm, phase_step);
            if fit > top.2 {
                top = (bpm, phase, fit);
            }
            bpm += step;
        }
        top
    };
    let (coarse, _, _) = search(rough_bpm - 0.8, rough_bpm + 0.8, 0.02, 1.0);
    let (mut bpm, phase, fit) = search(coarse - 0.03, coarse + 0.03, 0.005, 0.5);

    let confidence = fit / mean;
    if confidence < MIN_CONFIDENCE {
        return None;
    }
    // Most music is recorded at a whole-number tempo.
    if (bpm - bpm.round()).abs() < 0.12 {
        bpm = bpm.round();
    }
    // An onset shows up one hop after it happens, and smoothing spreads it;
    // step back half a hop to compensate.
    let offset_ms = ((phase - 0.5) / rate * 1000.0).max(0.0);
    Some(Tempo {
        bpm: (bpm * 100.0).round() / 100.0,
        offset_ms: (offset_ms * 10.0).round() / 10.0,
        confidence: (confidence * 100.0).round() / 100.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// A drum-like click on every beat, over a bed of noise, with a short
    /// tone on the off-beats to make it less trivial.
    fn click_track(bpm: f64, seconds: f64, first_beat_s: f64, noise: f32) -> Vec<f32> {
        click_track_with(bpm, seconds, first_beat_s, noise, true)
    }

    fn click_track_with(bpm: f64, seconds: f64, first_beat_s: f64, noise: f32, off_beats: bool) -> Vec<f32> {
        let n = (seconds * RATE as f64) as usize;
        let mut out = vec![0.0f32; n];
        let mut seed = 12345u32;
        for sample in out.iter_mut() {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *sample = ((seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * noise;
        }
        let beat = 60.0 / bpm;
        let mut k = 0;
        loop {
            let t = first_beat_s + k as f64 * beat;
            let start = (t * RATE as f64) as usize;
            if start >= n {
                break;
            }
            let strength = if k % 4 == 0 { 0.9 } else { 0.6 };
            for i in 0..(0.05 * RATE as f64) as usize {
                if start + i >= n {
                    break;
                }
                let x = i as f32 / RATE as f32;
                out[start + i] += strength * (x * 2.0 * std::f32::consts::PI * 90.0).sin() * (-x / 0.015).exp();
            }
            // Off-beat tone, quieter.
            let off = start + (beat * 0.5 * RATE as f64) as usize;
            for i in 0..if off_beats { (0.03 * RATE as f64) as usize } else { 0 } {
                if off + i >= n {
                    break;
                }
                let x = i as f32 / RATE as f32;
                out[off + i] += 0.15 * (x * 2.0 * std::f32::consts::PI * 880.0).sin() * (-x / 0.01).exp();
            }
            k += 1;
        }
        out
    }

    fn tempo_of(samples: &[f32]) -> Option<Tempo> {
        let mut tracker = OnsetTracker::new(RATE, 1);
        for chunk in samples.chunks(4096) {
            tracker.push(chunk);
        }
        tracker.finish()
    }

    #[test]
    fn finds_tempo_within_one_bpm_across_the_test_set() {
        for bpm in [72.0, 90.0, 100.0, 120.0, 128.0, 140.0, 160.0] {
            // Below 90 BPM the track has no off-beat notes; with them, slow
            // music reads just as well at double speed (see below).
            let track = click_track_with(bpm, 40.0, 0.0, 0.02, bpm >= 90.0);
            let found = tempo_of(&track).unwrap_or_else(|| panic!("no tempo at {bpm}"));
            assert!((found.bpm - bpm).abs() <= 1.0, "{bpm} BPM detected as {found:?}");
            assert!(found.confidence > 3.0, "{found:?}");
        }
        // Very slow music with off-beats reads equally well at double speed.
        // Either answer is right; the editor has a "half / double" button.
        let found = tempo_of(&click_track(60.0, 40.0, 0.0, 0.02)).unwrap();
        assert!((found.bpm - 60.0).abs() <= 1.0 || (found.bpm - 120.0).abs() <= 1.0, "{found:?}");
        // Likewise very fast music with accented bars reads well at half speed.
        let found = tempo_of(&click_track(174.0, 40.0, 0.0, 0.02)).unwrap();
        assert!((found.bpm - 174.0).abs() <= 1.0 || (found.bpm - 87.0).abs() <= 0.5, "{found:?}");
        // A fractional tempo is kept as measured, not rounded to a whole number.
        let found = tempo_of(&click_track(133.5, 60.0, 0.0, 0.02)).unwrap();
        assert!((found.bpm - 133.5).abs() <= 0.25, "{found:?}");
    }

    #[test]
    fn finds_where_the_first_beat_falls() {
        for first_beat in [0.0, 0.11, 0.25] {
            let found = tempo_of(&click_track(120.0, 30.0, first_beat, 0.02)).unwrap();
            let beat_ms = 500.0;
            // The beat grid repeats, so compare positions within one beat.
            let error = (found.offset_ms - first_beat * 1000.0).rem_euclid(beat_ms);
            let error = error.min(beat_ms - error);
            assert!(error <= 12.0, "first beat at {first_beat}s, found {found:?}");
        }
    }

    #[test]
    fn says_nothing_for_sound_without_a_beat() {
        // Steady noise: rain, wind.
        assert_eq!(tempo_of(&click_track(120.0, 30.0, 1e9, 0.3)), None);
        // Silence, and anything too short to judge.
        assert_eq!(tempo_of(&vec![0.0; RATE as usize * 20]), None);
        assert_eq!(tempo_of(&click_track(120.0, 4.0, 0.0, 0.02)), None);
    }
}
