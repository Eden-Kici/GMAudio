//! The built-in starter sounds must all be readable by the app's own decoder.

use std::path::{Path, PathBuf};

use gmaudio_core::analyze::analyze;

fn pack() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/starter-sounds")
}

fn oggs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            oggs(&path, out);
        } else if path.extension().is_some_and(|e| e == "ogg") {
            out.push(path);
        }
    }
}

#[test]
fn every_starter_sound_decodes_and_has_a_level() {
    let mut files = Vec::new();
    oggs(&pack(), &mut files);
    assert!(files.len() >= 25, "found only {} starter sounds", files.len());
    for file in &files {
        let a = analyze(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        assert!(a.duration_ms > 500, "{} is too short", file.display());
        assert_eq!(a.channels, 2);
        let lufs = a.loudness_lufs.unwrap_or_else(|| panic!("{}: no loudness", file.display()));
        assert!((-40.0..-5.0).contains(&lufs), "{}: {lufs} LUFS", file.display());
    }
}

#[test]
fn battle_march_stems_are_exactly_the_same_length() {
    let mut files = Vec::new();
    oggs(&pack().join("Music/Battle March (stems)"), &mut files);
    assert_eq!(files.len(), 3);
    let lengths: Vec<i64> = files.iter().map(|f| analyze(f).unwrap().duration_ms).collect();
    assert!(lengths.iter().all(|l| *l == lengths[0]), "{lengths:?}");
    // 16 bars of 4/4 at 140 BPM.
    assert!((lengths[0] - 27_429).abs() <= 2, "{lengths:?}");
}

#[test]
fn starter_music_tempos_are_detected() {
    // (file, true tempo). The slow pad pieces may read at double speed.
    for (name, bpm) in [("Tavern Evening", 100.0), ("Battle March", 140.0), ("Battle March (stems)/Battle March - Drums", 140.0)] {
        let a = analyze(&pack().join(format!("Music/{name}.ogg"))).unwrap();
        let found = a.bpm.unwrap_or_else(|| panic!("{name}: no tempo found"));
        assert!((found - bpm).abs() <= 1.0, "{name}: {found} BPM, expected {bpm}");
    }
    for name in ["Rain", "Wind", "River"] {
        let a = analyze(&pack().join(format!("Ambience/{name}.ogg"))).unwrap();
        assert_eq!(a.bpm, None, "{name} has no beat");
    }
}
