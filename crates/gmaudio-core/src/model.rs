use serde::{Deserialize, Serialize};

/// File extensions the library imports.
///
/// Opus is not in this list: the decoder (symphonia 0.6) has no Opus codec.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "ogg", "oga", "flac", "wav", "m4a"];

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
    /// Repeat whenever it is played, on any channel.
    #[serde(default)]
    pub always_loop: bool,
    /// Which parts of the file play. The file itself is never changed.
    #[serde(default)]
    pub edit: Option<AssetEdit>,
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
/// A trim of a sound: where it starts and ends, and stretches to skip.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssetEdit {
    #[serde(default)]
    pub start_ms: i64,
    #[serde(default)]
    pub end_ms: Option<i64>,
    /// Skipped stretches as (from, to).
    #[serde(default)]
    pub cuts: Vec<(i64, i64)>,
}

impl AssetEdit {
    /// Tidies an edit for a file of the given length: times inside the
    /// file, cuts in order without overlaps and inside the kept part.
    /// `None` when nothing is left to change.
    pub fn normalized(mut self, duration_ms: Option<i64>) -> Option<AssetEdit> {
        let length = duration_ms.unwrap_or(i64::MAX);
        self.start_ms = self.start_ms.clamp(0, length);
        self.end_ms = self.end_ms.map(|end| end.min(length)).filter(|end| *end < length && *end > self.start_ms + 100);
        let (first, last) = (self.start_ms, self.end_ms.unwrap_or(length));
        let mut cuts: Vec<(i64, i64)> = self
            .cuts
            .iter()
            .map(|(a, b)| ((*a).min(*b).max(first), (*a).max(*b).min(last)))
            .filter(|(a, b)| b - a >= 50)
            .collect();
        cuts.sort_unstable();
        let mut merged: Vec<(i64, i64)> = Vec::new();
        for (from, to) in cuts {
            match merged.last_mut() {
                Some(previous) if from <= previous.1 => previous.1 = previous.1.max(to),
                _ => merged.push((from, to)),
            }
        }
        // A cut touching either edge is just a later start or an earlier end.
        if merged.first().is_some_and(|c| c.0 <= first) {
            self.start_ms = merged.remove(0).1;
        }
        if merged.last().is_some_and(|c| c.1 >= last) {
            self.end_ms = Some(merged.pop().unwrap().0);
        }
        self.cuts = merged;
        let unchanged = self.start_ms == 0 && self.end_ms.is_none() && self.cuts.is_empty();
        (!unchanged).then_some(self)
    }
}

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
    fn an_edit_is_tidied_to_fit_the_file() {
        let edit = AssetEdit { start_ms: 2_000, end_ms: Some(50_000), cuts: vec![(9_000, 8_000), (8_500, 12_000), (1_000, 3_000), (20_000, 20_010)] };
        let tidy = edit.normalized(Some(30_000)).unwrap();
        assert_eq!(tidy, AssetEdit { start_ms: 3_000, end_ms: None, cuts: vec![(8_000, 12_000)] });
        assert_eq!(AssetEdit::default().normalized(Some(30_000)), None);
        assert_eq!(AssetEdit { start_ms: 0, end_ms: Some(30_000), cuts: vec![] }.normalized(Some(30_000)), None);
        let tail = AssetEdit { start_ms: 0, end_ms: None, cuts: vec![(25_000, 40_000)] }.normalized(Some(30_000)).unwrap();
        assert_eq!(tail, AssetEdit { start_ms: 0, end_ms: Some(25_000), cuts: vec![] });
    }

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
