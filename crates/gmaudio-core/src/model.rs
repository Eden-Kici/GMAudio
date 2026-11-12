use serde::{Deserialize, Serialize};

/// File extensions the library imports.
///
/// Opus is not in this list: the decoder (symphonia 0.6) has no Opus codec.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "ogg", "oga", "flac", "wav"];

/// Every asset is levelled towards this integrated loudness.
pub const TARGET_LUFS: f64 = -18.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisStatus {
    /// Imported, waiting for duration/loudness analysis.
    Pending,
    Ok,
    Failed,
}

impl AnalysisStatus {
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => Self::Ok,
            2 => Self::Failed,
            _ => Self::Pending,
        }
    }
    pub fn as_i64(self) -> i64 {
        match self {
            Self::Pending => 0,
            Self::Ok => 1,
            Self::Failed => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub id: i64,
    pub path: String,
    pub asset_count: i64,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    /// mood | biome | faction | intensity | custom
    #[serde(default = "default_category")]
    pub category: String,
}

fn default_category() -> String {
    "custom".into()
}

#[derive(Debug, Clone, Serialize)]
pub struct Asset {
    pub id: i64,
    pub folder_id: i64,
    pub path: String,
    /// File name without extension.
    pub name: String,
    pub ext: String,
    pub size: i64,
    pub mtime: i64,
    pub hash: Option<String>,
    pub duration_ms: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub loudness_lufs: Option<f64>,
    /// Gain applied at playback so assets sit at a similar level.
    pub gain_db: f64,
    pub bpm: Option<f64>,
    pub beat_offset_ms: Option<f64>,
    pub status: AnalysisStatus,
    pub error: Option<String>,
    /// The file was not found on the last scan.
    pub missing: bool,
    pub source: String,
    pub license: Option<String>,
    pub attribution: Option<String>,
    /// The user's own correction in dB, added to `gain_db` at playback.
    #[serde(default)]
    pub trim_db: f64,
    pub tags: Vec<Tag>,
}

/// Result of analysing one file.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub hash: String,
    pub duration_ms: i64,
    pub sample_rate: i64,
    pub channels: i64,
    /// `None` for silent or very short files.
    pub loudness_lufs: Option<f64>,
    pub gain_db: f64,
    /// Detected tempo, for sounds with a steady beat.
    pub bpm: Option<f64>,
    /// Where the first beat falls.
    pub beat_offset_ms: Option<f64>,
}

/// The four tags that say which mixer channel a sound belongs on, in
/// channel order. A sound with exactly one of them plays there with a
/// single click.
pub const CHANNEL_TAGS: [&str; 4] = ["Music", "Ambience", "Sound effects", "Stinger"];
pub const CHANNEL_TAG_CATEGORY: &str = "channel";

/// Guesses a channel tag from the folders a file sits in, deepest folder
/// first: `...\\Sounds\\Ambience\\Rain\\light.ogg` is ambience.
///
/// Only folder names count. File names are too varied to trust
/// ("Music Box Creak" is not music).
pub fn guess_channel_tag(path: &str) -> Option<&'static str> {
    let parts: Vec<&str> = path.split(['\\', '/']).collect();
    let folders = parts.len().saturating_sub(1);
    for folder in parts[..folders].iter().rev() {
        let name = folder.to_lowercase();
        let words: Vec<&str> = name
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let has = |needle: &str| name.contains(needle);
        let word = |w: &str| words.contains(&w);
        if has("sting") || has("jingle") || has("fanfare") {
            return Some(CHANNEL_TAGS[3]);
        }
        if has("ambien") || has("ambian") || has("atmos") || has("soundscape")
            || has("background") || has("weather") || has("nature") || has("environment")
        {
            return Some(CHANNEL_TAGS[1]);
        }
        if word("sfx") || word("fx") || word("foley") || has("sound effect")
            || has("soundeffect") || has("effects") || has("one shot") || has("oneshot")
            || has("one-shot")
        {
            return Some(CHANNEL_TAGS[2]);
        }
        if has("music") || has("song") || has("soundtrack") || word("ost") || word("bgm")
            || word("score") || word("scores") || word("theme") || word("themes")
        {
            return Some(CHANNEL_TAGS[0]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_channel_from_folder_names() {
        let g = guess_channel_tag;
        assert_eq!(g(r"C:\Sounds\Music\Taverns\reel.mp3"), Some("Music"));
        assert_eq!(g(r"C:\Sounds\Ambience\rain.ogg"), Some("Ambience"));
        assert_eq!(g(r"C:\Sounds\Ambiance\Weather\rain.ogg"), Some("Ambience"));
        assert_eq!(g("/home/x/SFX/sword.wav"), Some("Sound effects"));
        assert_eq!(g(r"D:\Audio\Sound Effects\door.wav"), Some("Sound effects"));
        assert_eq!(g(r"D:\Audio\Stingers\win.wav"), Some("Stinger"));
        assert_eq!(g(r"D:\OST\Disc 1\01.flac"), Some("Music"));
        // The deepest folder wins.
        assert_eq!(g(r"C:\Music\Battle\Stingers\hit.wav"), Some("Stinger"));
        // File names do not count, and short words must match whole.
        assert_eq!(g(r"C:\Stuff\Music Box Creak.wav"), None);
        assert_eq!(g(r"C:\Ghost Town\Frost\wind.wav"), None);
        assert_eq!(g("no-folders.wav"), None);
    }
}
