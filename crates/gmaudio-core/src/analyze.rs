//! Per-file analysis: duration, loudness (EBU R128) and a content hash.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use ebur128::{EbuR128, Mode};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::TrackType;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSourceStream;

use crate::model::{Analysis, TARGET_LUFS};
use crate::tempo::OnsetTracker;
use crate::{Error, Result};

/// How much of the start and end of a file goes into its hash.
const HASH_WINDOW: u64 = 256 * 1024;
/// Never boost a file so far that its peaks would pass this level (dBFS).
const PEAK_CEILING_DB: f64 = -1.0;
const MAX_BOOST_DB: f64 = 12.0;
const MAX_CUT_DB: f64 = -24.0;

/// Cheap content hash: file size plus the first and last 256 KiB.
///
/// Enough to recognise a file that was moved or renamed without reading
/// whole multi-hundred-megabyte files.
pub fn content_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    hasher.update(&len.to_le_bytes());

    let mut buf = vec![0u8; HASH_WINDOW as usize];
    let head = read_up_to(&mut file, &mut buf)?;
    hasher.update(&buf[..head]);
    if len > HASH_WINDOW * 2 {
        file.seek(SeekFrom::Start(len - HASH_WINDOW))?;
        let tail = read_up_to(&mut file, &mut buf)?;
        hasher.update(&buf[..tail]);
    } else if len > HASH_WINDOW {
        let tail = read_up_to(&mut file, &mut buf)?;
        hasher.update(&buf[..tail]);
    }
    Ok(format!("{:016x}", hasher.digest()))
}

fn read_up_to(file: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// Playback gain that brings a file to [`TARGET_LUFS`].
///
/// `peak` is the highest absolute sample value (1.0 = full scale). Boosts
/// are limited so peaks stay under [`PEAK_CEILING_DB`]; cuts are not.
pub fn gain_for(loudness_lufs: Option<f64>, peak: f64) -> f64 {
    let Some(loudness) = loudness_lufs else {
        return 0.0;
    };
    let mut gain = (TARGET_LUFS - loudness).clamp(MAX_CUT_DB, MAX_BOOST_DB);
    if gain > 0.0 && peak > 0.0 {
        let headroom = PEAK_CEILING_DB - 20.0 * peak.log10();
        gain = gain.min(headroom.max(0.0));
    }
    (gain * 100.0).round() / 100.0
}

fn decode_err(e: impl std::fmt::Display) -> Error {
    Error::Decode(e.to_string())
}

/// Decodes the whole file once to measure it.
pub fn analyze(path: &Path) -> Result<Analysis> {
    let hash = content_hash(path)?;

    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, Default::default(), Default::default())
        .map_err(decode_err)?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| Error::Decode("no audio track in file".into()))?;
    let track_id = track.id;
    let params = match track.codec_params.as_ref() {
        Some(CodecParameters::Audio(p)) => p.clone(),
        _ => return Err(Error::Decode("no audio track in file".into())),
    };
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &Default::default())
        .map_err(decode_err)?;

    let mut meter: Option<EbuR128> = None;
    let mut rate = 0u32;
    let mut channels = 0u32;
    let mut frames: u64 = 0;
    let mut samples: Vec<f32> = Vec::new();
    let mut onsets: Option<OnsetTracker> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            // A truncated file still has everything before the cut.
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(decode_err(e)),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(b) => b,
            // One bad packet is not a bad file.
            Err(SymError::DecodeError(_)) => continue,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(decode_err(e)),
        };

        let buf_rate = buffer.spec().rate();
        let buf_channels = buffer.num_planes() as u32;
        if meter.is_none() {
            if buf_channels == 0 || buf_channels > 2 {
                return Err(Error::Decode(format!(
                    "only mono and stereo files are supported (this one has {buf_channels} channels)"
                )));
            }
            rate = buf_rate;
            channels = buf_channels;
            meter = Some(
                EbuR128::new(channels, rate, Mode::I | Mode::SAMPLE_PEAK | Mode::HISTOGRAM)
                    .map_err(decode_err)?,
            );
            onsets = Some(OnsetTracker::new(rate, channels));
        } else if buf_rate != rate || buf_channels != channels {
            // Format changed mid-stream (chained Ogg): measure the first part only.
            break;
        }

        samples.clear();
        buffer.copy_to_vec_interleaved::<f32>(&mut samples);
        frames += buffer.frames() as u64;
        if let Some(m) = meter.as_mut() {
            m.add_frames_f32(&samples).map_err(decode_err)?;
        }
        if let Some(o) = onsets.as_mut() {
            o.push(&samples);
        }
    }

    let Some(meter) = meter else {
        return Err(Error::Decode("file contains no audio".into()));
    };
    if frames == 0 {
        return Err(Error::Decode("file contains no audio".into()));
    }

    let tempo = onsets.and_then(OnsetTracker::finish);
    let loudness = meter
        .loudness_global()
        .ok()
        .filter(|l| l.is_finite() && *l > -70.0);
    let peak = (0..channels)
        .filter_map(|c| meter.sample_peak(c).ok())
        .fold(0.0f64, f64::max);

    Ok(Analysis {
        hash,
        duration_ms: (frames * 1000 / rate as u64) as i64,
        sample_rate: rate as i64,
        channels: channels as i64,
        loudness_lufs: loudness.map(|l| (l * 100.0).round() / 100.0),
        gain_db: gain_for(loudness, peak),
        bpm: tempo.map(|t| t.bpm),
        beat_offset_ms: tempo.map(|t| t.offset_ms),
    })
}

#[cfg(test)]
pub(crate) mod testutil {
    use std::f32::consts::TAU;
    use std::io::Write;
    use std::path::Path;

    /// Writes a 16-bit PCM WAV holding a sine tone.
    pub fn write_sine_wav(path: &Path, seconds: f32, freq: f32, peak_dbfs: f32, channels: u16) {
        let rate = 48_000u32;
        let frames = (seconds * rate as f32) as u32;
        let amp = 10f32.powf(peak_dbfs / 20.0) * i16::MAX as f32;
        let data_len = frames * channels as u32 * 2;

        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let s = ((i as f32 / rate as f32) * freq * TAU).sin() * amp;
            for _ in 0..channels {
                out.extend_from_slice(&(s as i16).to_le_bytes());
            }
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::File::create(path).unwrap().write_all(&out).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::write_sine_wav;
    use super::*;

    #[test]
    fn measures_duration_and_loudness_of_a_sine() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        // EBU reference: a 997 Hz stereo sine peaking at -23 dBFS reads -23 LUFS.
        write_sine_wav(&path, 5.0, 997.0, -23.0, 2);

        let a = analyze(&path).unwrap();
        assert_eq!(a.duration_ms, 5000);
        assert_eq!((a.sample_rate, a.channels), (48_000, 2));
        let lufs = a.loudness_lufs.unwrap();
        assert!((lufs + 23.0).abs() < 0.3, "loudness was {lufs}");
        // Brought up to the -18 LUFS target.
        assert!((a.gain_db - 5.0).abs() < 0.3, "gain was {}", a.gain_db);
    }

    #[test]
    fn loud_files_are_turned_down() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("loud.wav");
        write_sine_wav(&path, 3.0, 997.0, -6.0, 2);
        let a = analyze(&path).unwrap();
        assert!((a.gain_db + 12.0).abs() < 0.4, "gain was {}", a.gain_db);
    }

    #[test]
    fn boost_never_pushes_peaks_past_the_ceiling() {
        // Quiet on average (-30 LUFS) but with peaks at -4 dBFS:
        // the full +12 dB would clip, so only 3 dB of boost is allowed.
        let g = gain_for(Some(-30.0), 10f64.powf(-4.0 / 20.0));
        assert!((g - 3.0).abs() < 0.01, "gain was {g}");
        // Already peaking above the ceiling: no boost at all.
        assert_eq!(gain_for(Some(-30.0), 1.0), 0.0);
        // Unknown loudness: leave untouched.
        assert_eq!(gain_for(None, 0.5), 0.0);
    }

    #[test]
    fn very_short_files_get_no_loudness_but_still_analyze() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blip.wav");
        write_sine_wav(&path, 0.1, 440.0, -12.0, 1);
        let a = analyze(&path).unwrap();
        assert_eq!(a.duration_ms, 100);
        assert_eq!(a.channels, 1);
        assert_eq!(a.loudness_lufs, None);
        assert_eq!(a.gain_db, 0.0);
    }

    #[test]
    fn rejects_non_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fake.mp3");
        std::fs::write(&path, b"this is not audio at all, just text").unwrap();
        assert!(matches!(analyze(&path), Err(Error::Decode(_))));
    }

    #[test]
    fn hash_follows_content_not_name() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.wav");
        let b = dir.path().join("b.wav");
        let c = dir.path().join("c.wav");
        write_sine_wav(&a, 1.0, 440.0, -12.0, 2);
        std::fs::copy(&a, &b).unwrap();
        write_sine_wav(&c, 1.0, 441.0, -12.0, 2);
        assert_eq!(content_hash(&a).unwrap(), content_hash(&b).unwrap());
        assert_ne!(content_hash(&a).unwrap(), content_hash(&c).unwrap());
    }
}
