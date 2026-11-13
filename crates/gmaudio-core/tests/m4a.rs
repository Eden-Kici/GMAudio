//! M4A (AAC) is what the YouTube import saves, so it has to decode.

use std::path::Path;

use gmaudio_core::analyze::analyze;
use gmaudio_core::engine::decode;

#[test]
fn an_m4a_file_is_measured_and_decodes_for_playback() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tone.m4a");
    let a = analyze(&file).unwrap();
    assert!((2_900..3_200).contains(&a.duration_ms), "{} ms", a.duration_ms);
    assert!(a.loudness_lufs.is_some());
    decode(&file).unwrap();
}
