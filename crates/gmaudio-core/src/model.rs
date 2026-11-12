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
}
