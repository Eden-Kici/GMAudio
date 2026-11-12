//! What the GM builds on top of the library: scenes, and later boards,
//! music sets and cue lists.
//!
//! Each one is stored as a JSON document (see [`crate::db`]). The interface
//! owns the full shape of those documents; this module reads only the parts
//! that decide what is played, and turns them into engine requests.

use serde::{Deserialize, Serialize};

use crate::engine::{
    self, Channel, EmitterRequest, PlayRequest, Quantize, STEM_OFF_DB, SceneMusic, SceneRequest,
    SetRequest, StemRequest, TierRequest,
};
use crate::library::Library;
use crate::model::AnalysisStatus;
use crate::{Error, Result};

/// Sounds longer than this are streamed, so there is no point preloading them.
pub const PRELOAD_MAX_MS: i64 = 20_000;

/// Looks a sound up and prepares it for a channel.
///
/// Fails, with a message fit to show the GM, when the file is gone or was
/// never readable.
pub fn play_request(library: &Library, asset_id: i64, channel: Channel) -> Result<PlayRequest> {
    let asset = library
        .asset(asset_id)
        .ok_or_else(|| Error::Unplayable("That sound is no longer in the library.".into()))?;
    if asset.missing {
        return Err(Error::Unplayable(format!("\"{}\" was not found on disk.", asset.name)));
    }
    if asset.status == AnalysisStatus::Failed {
        return Err(Error::Unplayable(format!(
            "\"{}\" could not be read: {}",
            asset.name,
            asset.error.as_deref().unwrap_or("unknown problem")
        )));
    }
    Ok(PlayRequest {
        asset_id,
        path: asset.path.into(),
        name: asset.name,
        channel,
        gain_db: (asset.gain_db + asset.trim_db) as f32,
        duration_ms: asset.duration_ms,
        ..Default::default()
    })
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneDoc {
    pub name: String,
    /// How long switching to this scene takes. `None`: three seconds.
    pub transition_ms: Option<u32>,
    pub music: SceneMusicDoc,
    pub layers: Vec<LayerDoc>,
    pub emitters: Vec<EmitterDoc>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SceneMusicDoc {
    /// Leave the music alone.
    #[default]
    Keep,
    Silence,
    Tracks {
        #[serde(default)]
        asset_ids: Vec<i64>,
    },
    /// A dynamic music set, started at one of its tiers.
    Set {
        set_id: i64,
        #[serde(default)]
        tier: usize,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerDoc {
    pub asset_id: i64,
    pub volume_db: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmitterDoc {
    pub name: String,
    pub asset_ids: Vec<i64>,
    pub min_s: f32,
    pub max_s: f32,
    pub volume_db: f32,
    pub random_pan: bool,
}

impl Default for EmitterDoc {
    fn default() -> Self {
        Self {
            name: String::new(),
            asset_ids: Vec::new(),
            min_s: 20.0,
            max_s: 90.0,
            volume_db: 0.0,
            random_pan: true,
        }
    }
}

/// An engine request, plus what had to be left out of it.
#[derive(Debug, Clone)]
pub struct Resolved<T> {
    pub request: T,
    /// Problems with individual sounds, ready to show. The rest still plays.
    pub skipped: Vec<String>,
}

/// Turns a scene document into what the engine should do.
///
/// Sounds that are missing or unreadable are skipped and reported, so one
/// lost file never stops a scene change mid-session.
pub fn scene_request(library: &Library, id: i64, doc: &SceneDoc) -> Resolved<SceneRequest> {
    let mut skipped = Vec::new();
    let tag = Some(format!("scene:{id}"));
    let mut resolve = |asset_id: i64, channel: Channel| match play_request(library, asset_id, channel) {
        Ok(mut request) => {
            request.tag = tag.clone();
            Some(request)
        }
        Err(e) => {
            skipped.push(e.to_string());
            None
        }
    };

    let layers = doc
        .layers
        .iter()
        .filter_map(|layer| {
            let mut request = resolve(layer.asset_id, Channel::Ambience)?;
            request.trim_db = layer.volume_db;
            Some(request)
        })
        .collect();

    let emitters = doc
        .emitters
        .iter()
        .filter_map(|emitter| {
            let sounds: Vec<PlayRequest> = emitter
                .asset_ids
                .iter()
                .filter_map(|asset_id| {
                    let mut request = resolve(*asset_id, Channel::Ambience)?;
                    request.trim_db = emitter.volume_db;
                    Some(request)
                })
                .collect();
            if sounds.is_empty() {
                return None;
            }
            let min_ms = (emitter.min_s.max(0.5) * 1000.0) as u32;
            let max_ms = ((emitter.max_s.max(emitter.min_s) * 1000.0) as u32).max(min_ms);
            Some(EmitterRequest {
                name: if emitter.name.trim().is_empty() {
                    sounds[0].name.clone()
                } else {
                    emitter.name.clone()
                },
                sounds,
                min_ms,
                max_ms,
                random_pan: emitter.random_pan,
                tag: tag.clone(),
            })
        })
        .collect();

    let music = match &doc.music {
        // A music set is started separately, by the dynamic music engine.
        SceneMusicDoc::Keep | SceneMusicDoc::Set { .. } => SceneMusic::Keep,
        SceneMusicDoc::Silence => SceneMusic::Silence,
        SceneMusicDoc::Tracks { asset_ids } => {
            let tracks: Vec<PlayRequest> = asset_ids
                .iter()
                .filter_map(|asset_id| resolve(*asset_id, Channel::Music))
                .collect();
            // If every track is gone, keep what is playing rather than go quiet.
            if tracks.is_empty() { SceneMusic::Keep } else { SceneMusic::Tracks(tracks) }
        }
    };

    Resolved {
        request: SceneRequest {
            id,
            name: doc.name.clone(),
            transition_ms: doc.transition_ms.unwrap_or(3000).min(60_000),
            music,
            layers,
            emitters,
        },
        skipped,
    }
}

/// A piece of dynamic music, as the interface saves it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicSetDoc {
    pub name: String,
    /// `stems`: synced layers mixed per tier. Otherwise one track per tier.
    pub kind: String,
    pub bpm: f64,
    pub beats_per_bar: u32,
    pub bars_per_phrase: u32,
    pub offset_ms: f64,
    pub crossfade_ms: u32,
    pub quantize: Quantize,
    pub tiers: Vec<TierDoc>,
    pub stems: Vec<StemDoc>,
}

impl Default for MusicSetDoc {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: "tracks".into(),
            bpm: 120.0,
            beats_per_bar: 4,
            bars_per_phrase: 4,
            offset_ms: 0.0,
            crossfade_ms: 600,
            quantize: Quantize::Bar,
            tiers: Vec::new(),
            stems: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TierDoc {
    pub name: String,
    /// Tracks kind: this tier's recording.
    pub asset_id: Option<i64>,
    /// Stems kind: per stem, its volume in dB, or `null` for off.
    pub stems: Vec<Option<f32>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StemDoc {
    pub name: String,
    pub asset_id: i64,
}

/// Turns a music set document into an engine request.
///
/// Stems are decoded here, on the caller's thread, because they are held
/// in memory whole. A stem that cannot be read is an error (the mix would
/// be wrong without it); an unreadable tier track becomes a silent tier.
pub fn set_request(library: &Library, id: i64, doc: &MusicSetDoc, start_tier: usize) -> Result<Resolved<SetRequest>> {
    let mut skipped = Vec::new();
    let is_stems = doc.kind == "stems";

    let stems = if is_stems {
        doc.stems
            .iter()
            .map(|stem| {
                let request = play_request(library, stem.asset_id, Channel::Music)?;
                let data = engine::decode(&request.path)?;
                Ok(StemRequest { name: stem.name.clone(), request, data: Some(data) })
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    if is_stems && stems.is_empty() {
        return Err(Error::Unplayable(format!("\"{}\" has no layers yet.", doc.name)));
    }

    let tiers: Vec<TierRequest> = doc
        .tiers
        .iter()
        .map(|tier| {
            let track = if is_stems {
                None
            } else {
                tier.asset_id.and_then(|asset_id| match play_request(library, asset_id, Channel::Music) {
                    Ok(request) => Some(request),
                    Err(e) => {
                        skipped.push(e.to_string());
                        None
                    }
                })
            };
            TierRequest {
                name: tier.name.clone(),
                track,
                stem_db: (0..stems.len())
                    .map(|i| tier.stems.get(i).copied().flatten().unwrap_or(STEM_OFF_DB))
                    .collect(),
            }
        })
        .collect();
    if tiers.is_empty() {
        return Err(Error::Unplayable(format!("\"{}\" has no levels yet.", doc.name)));
    }

    Ok(Resolved {
        request: SetRequest {
            id,
            name: doc.name.clone(),
            bpm: if doc.bpm.is_finite() { doc.bpm.clamp(20.0, 400.0) } else { 120.0 },
            beats_per_bar: doc.beats_per_bar.clamp(1, 16),
            bars_per_phrase: doc.bars_per_phrase.clamp(1, 64),
            offset_ms: doc.offset_ms.max(0.0),
            crossfade_ms: doc.crossfade_ms.min(20_000),
            quantize: doc.quantize,
            start_tier: start_tier.min(tiers.len() - 1),
            tiers,
            stems,
        },
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::testutil::write_sine_wav;
    use crate::search::Query;
    use std::time::Duration;

    fn library_with(names: &[&str]) -> (tempfile::TempDir, Library, Vec<i64>) {
        let dir = tempfile::tempdir().unwrap();
        for name in names {
            write_sine_wav(&dir.path().join(format!("{name}.wav")), 0.4, 220.0, -12.0, 1);
        }
        let library = Library::in_memory(|_| {}).unwrap();
        library.start().unwrap();
        library.add_folder(dir.path()).unwrap();
        assert!(library.wait_idle(Duration::from_secs(20)));
        let ids = names
            .iter()
            .map(|name| library.search(&Query { text: (*name).into(), ..Default::default() }).assets[0].id)
            .collect();
        (dir, library, ids)
    }

    #[test]
    fn a_scene_document_becomes_an_engine_request() {
        let (dir, library, ids) = library_with(&["rain", "wind", "thunder", "tavern", "gone"]);
        std::fs::remove_file(dir.path().join("gone.wav")).unwrap();
        library.rescan(None).unwrap();
        assert!(library.wait_idle(Duration::from_secs(20)));

        // As the interface would save it, with fields this module ignores.
        let json = serde_json::json!({
            "name": "Stormy inn",
            "color": "#445566",
            "music": { "kind": "tracks", "asset_ids": [ids[3], ids[4]] },
            "layers": [
                { "asset_id": ids[0], "volume_db": -6.0 },
                { "asset_id": ids[1] },
                { "asset_id": ids[4] },
                { "asset_id": 99999 }
            ],
            "emitters": [
                { "name": "", "asset_ids": [ids[2]], "min_s": 20, "max_s": 90 },
                { "name": "Nothing left", "asset_ids": [ids[4]] }
            ]
        });
        let doc: SceneDoc = serde_json::from_value(json).unwrap();
        let resolved = scene_request(&library, 7, &doc);
        let scene = resolved.request;

        assert_eq!(scene.id, 7);
        assert_eq!(scene.transition_ms, 3000);
        assert_eq!(scene.layers.len(), 2);
        assert_eq!(scene.layers[0].trim_db, -6.0);
        assert_eq!(scene.layers[0].tag.as_deref(), Some("scene:7"));
        assert_eq!(scene.emitters.len(), 1, "a random layer with no sounds left is dropped");
        assert_eq!(scene.emitters[0].name, "thunder", "unnamed layers take their sound's name");
        assert_eq!((scene.emitters[0].min_ms, scene.emitters[0].max_ms), (20_000, 90_000));
        assert!(scene.emitters[0].random_pan);
        match scene.music {
            SceneMusic::Tracks(tracks) => assert_eq!(tracks.len(), 1),
            other => panic!("expected tracks, got {other:?}"),
        }
        // Missing track, missing layer, unknown layer, missing random sound.
        assert_eq!(resolved.skipped.len(), 4, "{:?}", resolved.skipped);
        assert!(resolved.skipped[0].contains("gone"));
    }

    #[test]
    fn a_music_set_document_becomes_an_engine_request() {
        let (dir, library, ids) = library_with(&["calm", "combat", "drums", "strings", "gone"]);
        std::fs::remove_file(dir.path().join("gone.wav")).unwrap();
        library.rescan(None).unwrap();
        assert!(library.wait_idle(Duration::from_secs(20)));

        // One track per tier; the tier whose file is gone goes silent.
        let doc: MusicSetDoc = serde_json::from_value(serde_json::json!({
            "name": "Chase", "bpm": 140, "quantize": "beat",
            "tiers": [
                { "name": "Calm", "asset_id": ids[0] },
                { "name": "Lost", "asset_id": ids[4] },
                { "name": "Combat", "asset_id": ids[1] }
            ]
        }))
        .unwrap();
        let resolved = set_request(&library, 3, &doc, 9).unwrap();
        let set = resolved.request;
        assert_eq!((set.bpm, set.beats_per_bar, set.quantize), (140.0, 4, Quantize::Beat));
        assert_eq!(set.start_tier, 2, "clamped to the last tier");
        assert!(set.stems.is_empty());
        assert!(set.tiers[0].track.is_some() && set.tiers[1].track.is_none());
        assert_eq!(resolved.skipped.len(), 1);

        // Layered stems, decoded ahead of time.
        let doc: MusicSetDoc = serde_json::from_value(serde_json::json!({
            "name": "Battle", "kind": "stems",
            "stems": [{ "name": "Drums", "asset_id": ids[2] }, { "name": "Strings", "asset_id": ids[3] }],
            "tiers": [
                { "name": "Calm", "stems": [0, null] },
                { "name": "Full", "stems": [0, -3.0] }
            ]
        }))
        .unwrap();
        let set = set_request(&library, 4, &doc, 0).unwrap().request;
        assert_eq!(set.stems.len(), 2);
        assert!(set.stems.iter().all(|s| s.data.is_some()));
        assert_eq!(set.tiers[0].stem_db, [0.0, STEM_OFF_DB]);
        assert_eq!(set.tiers[1].stem_db, [0.0, -3.0]);

        // A missing stem, or a set with nothing in it, is refused in plain words.
        let broken: MusicSetDoc = serde_json::from_value(serde_json::json!({
            "kind": "stems", "stems": [{ "asset_id": ids[4] }], "tiers": [{ "name": "A", "stems": [0] }]
        }))
        .unwrap();
        assert!(matches!(set_request(&library, 5, &broken, 0), Err(Error::Unplayable(_))));
        assert!(matches!(set_request(&library, 6, &MusicSetDoc::default(), 0), Err(Error::Unplayable(_))));
    }

    #[test]
    fn an_empty_document_changes_nothing_but_the_ambience() {
        let (_dir, library, _) = library_with(&["rain"]);
        let doc: SceneDoc = serde_json::from_str("{}").unwrap();
        let resolved = scene_request(&library, 1, &doc);
        assert!(matches!(resolved.request.music, SceneMusic::Keep));
        assert!(resolved.request.layers.is_empty());
        assert!(resolved.skipped.is_empty());

        // All music tracks unreadable: keep what is playing.
        let doc: SceneDoc =
            serde_json::from_value(serde_json::json!({ "music": { "kind": "tracks", "asset_ids": [42] } })).unwrap();
        assert!(matches!(scene_request(&library, 1, &doc).request.music, SceneMusic::Keep));
    }
}
