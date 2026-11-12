//! Campaign bundles: one file holding a campaign's scenes, boards, music
//! sets and cue lists, optionally with the audio they use. For backups and
//! for moving to another computer.
//!
//! A bundle is a zip file:
//!
//! ```text
//! gmaudio.json     what the interface exported, plus a list of sounds
//! audio/12.ogg     the sounds themselves (only if asked for)
//! ```
//!
//! This module looks after the file and the sounds. The interface decides
//! what goes into a campaign and re-creates it on import, using the map of
//! old sound ids to new ones that [`import`] returns.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::library::Library;
use crate::model::Tag;
use crate::{Error, Result};

const FORMAT: &str = "gmaudio-bundle";
const VERSION: u32 = 1;
const MANIFEST: &str = "gmaudio.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BundleAsset {
    /// The sound's id on the computer that made the bundle.
    id: i64,
    name: String,
    ext: String,
    /// Fingerprint of the file's contents, to recognise a copy.
    hash: Option<String>,
    #[serde(default)]
    tags: Vec<Tag>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    attribution: Option<String>,
    /// Where the audio is inside the bundle, if it was included.
    #[serde(default)]
    file: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Contents {
    format: String,
    version: u32,
    /// Whatever the interface exported. Not interpreted here.
    data: Value,
    assets: Vec<BundleAsset>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    pub sounds: usize,
    /// Sounds whose files could not be read and so were left out.
    pub skipped: Vec<String>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    /// What the interface exported.
    pub data: Value,
    /// Sound ids in the bundle to sound ids in this library.
    pub asset_map: HashMap<i64, i64>,
    /// Sounds the bundle refers to that are neither in this library nor in
    /// the bundle.
    pub missing: Vec<String>,
    /// How many sounds were copied out of the bundle.
    pub extracted: usize,
}

fn zip_err(e: zip::result::ZipError) -> Error {
    Error::Bundle(e.to_string())
}

/// Writes a bundle. `asset_ids` are the sounds the exported data refers to.
pub fn export(library: &Library, path: &Path, data: Value, asset_ids: &[i64], include_audio: bool) -> Result<ExportReport> {
    let mut zip = ZipWriter::new(File::create(path)?);
    // Audio is already compressed (or large); storing it as is keeps export fast.
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored).large_file(true);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut assets = Vec::new();
    let mut skipped = Vec::new();
    let mut ids: Vec<i64> = asset_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    for id in ids {
        let Some(asset) = library.asset(id) else { continue };
        let mut file = None;
        if include_audio {
            match File::open(&asset.path) {
                Ok(mut source) => {
                    let name = format!("audio/{id}.{}", asset.ext);
                    zip.start_file(&name, stored).map_err(zip_err)?;
                    std::io::copy(&mut source, &mut zip)?;
                    file = Some(name);
                }
                Err(_) => skipped.push(asset.name.clone()),
            }
        }
        assets.push(BundleAsset {
            id,
            name: asset.name,
            ext: asset.ext,
            hash: asset.hash,
            tags: asset.tags,
            license: asset.license,
            attribution: asset.attribution,
            file,
        });
    }

    let sounds = assets.len();
    let contents = Contents { format: FORMAT.into(), version: VERSION, data, assets };
    zip.start_file(MANIFEST, deflated).map_err(zip_err)?;
    zip.write_all(serde_json::to_string_pretty(&contents).map_err(|e| Error::Bundle(e.to_string()))?.as_bytes())?;
    zip.finish().map_err(zip_err)?;
    Ok(ExportReport { sounds, skipped, bytes: std::fs::metadata(path)?.len() })
}

/// Reads a bundle and makes sure its sounds are in the library.
///
/// A sound already in the library (the same file contents, wherever it
/// lives) is reused. Otherwise it is copied out of the bundle into
/// `audio_folder`, which is added to the library.
pub fn import(library: &Library, path: &Path, audio_folder: &Path) -> Result<ImportReport> {
    let mut zip = ZipArchive::new(File::open(path)?).map_err(|_| Error::Bundle("That file is not a GMAudio bundle.".into()))?;
    let contents: Contents = {
        let mut manifest = zip.by_name(MANIFEST).map_err(|_| Error::Bundle("That file is not a GMAudio bundle.".into()))?;
        let mut json = String::new();
        manifest.read_to_string(&mut json)?;
        serde_json::from_str(&json).map_err(|e| Error::Bundle(format!("The bundle could not be read: {e}")))?
    };
    if contents.format != FORMAT {
        return Err(Error::Bundle("That file is not a GMAudio bundle.".into()));
    }
    if contents.version > VERSION {
        return Err(Error::Bundle("That bundle was made by a newer version of GMAudio. Update the app to open it.".into()));
    }

    let by_hash = |library: &Library| -> Result<HashMap<String, i64>> {
        Ok(library
            .all_assets()?
            .into_iter()
            .filter(|a| !a.missing)
            .filter_map(|a| Some((a.hash?, a.id)))
            .collect())
    };
    let known = by_hash(library)?;

    let mut asset_map = HashMap::new();
    let mut extracted: Vec<(BundleAsset, PathBuf)> = Vec::new();
    let mut missing = Vec::new();
    for asset in contents.assets {
        if let Some(local) = asset.hash.as_ref().and_then(|h| known.get(h)) {
            asset_map.insert(asset.id, *local);
            continue;
        }
        let Some(inside) = asset.file.clone() else {
            missing.push(asset.name.clone());
            continue;
        };
        let Ok(mut source) = zip.by_name(&inside) else {
            missing.push(asset.name.clone());
            continue;
        };
        std::fs::create_dir_all(audio_folder)?;
        // Keep the sound's own name; tell equal names apart by id.
        let safe: String = asset.name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { ' ' } else { c }).collect();
        let mut target = audio_folder.join(format!("{}.{}", safe.trim(), asset.ext));
        if target.exists() {
            target = audio_folder.join(format!("{} ({}).{}", safe.trim(), asset.id, asset.ext));
        }
        let mut out = File::create(&target)?;
        std::io::copy(&mut source, &mut out)?;
        extracted.push((asset, target));
    }

    if !extracted.is_empty() {
        match library.add_folder(audio_folder) {
            Ok(_) => {}
            Err(Error::FolderAlreadyAdded) => library.rescan(None)?,
            Err(e) => return Err(e),
        }
        // Wait for the new files to be read, so they can be found by path.
        library.wait_idle(Duration::from_secs(300));
        let by_path: HashMap<String, i64> = library.all_assets()?.into_iter().map(|a| (a.path, a.id)).collect();
        for (asset, target) in &extracted {
            match by_path.get(target.to_string_lossy().as_ref()) {
                Some(local) => {
                    asset_map.insert(asset.id, *local);
                    // Carry the sound's tags over, unless folder names already tagged it.
                    if !asset.tags.is_empty() && library.asset(*local).is_some_and(|a| a.tags.is_empty()) {
                        library.set_tags(*local, &asset.tags)?;
                    }
                    if let Some(attribution) = &asset.attribution {
                        library.set_credit(&target.to_string_lossy(), asset.license.as_deref().unwrap_or(""), attribution)?;
                    }
                }
                None => missing.push(asset.name.clone()),
            }
        }
    }

    Ok(ImportReport { data: contents.data, asset_map, missing, extracted: extracted.len() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::testutil::write_sine_wav;
    use crate::search::Query;

    fn library_with(dir: &Path, sounds: &[(&str, f32)]) -> Library {
        std::fs::create_dir_all(dir).unwrap();
        for (name, freq) in sounds {
            write_sine_wav(&dir.join(format!("{name}.wav")), 0.4, *freq, -12.0, 1);
        }
        let library = Library::in_memory(|_| {}).unwrap();
        library.start().unwrap();
        library.add_folder(dir).unwrap();
        assert!(library.wait_idle(Duration::from_secs(20)));
        library
    }

    fn id_of(library: &Library, name: &str) -> i64 {
        library.search(&Query { text: name.into(), ..Default::default() }).assets[0].id
    }

    #[test]
    fn a_bundle_moves_a_campaign_to_another_library() {
        let home = tempfile::tempdir().unwrap();
        let source = library_with(home.path(), &[("rain", 300.0), ("thunder", 80.0), ("tavern", 440.0)]);
        let (rain, thunder, tavern) = (id_of(&source, "rain"), id_of(&source, "thunder"), id_of(&source, "tavern"));
        source.set_tags(rain, &[Tag { name: "storm".into(), category: "custom".into() }]).unwrap();

        let bundle = home.path().join("campaign.gmaudio");
        let data = serde_json::json!({ "campaign": { "name": "Lich" }, "scenes": [{ "layers": [{ "asset_id": rain }] }] });
        let report = export(&source, &bundle, data.clone(), &[rain, thunder, tavern, rain, 9999], true).unwrap();
        assert_eq!(report.sounds, 3, "duplicates and unknown ids are dropped");
        assert!(report.skipped.is_empty());

        // Another computer: it already has the tavern track (same contents,
        // other name and place), but not the rest.
        let away = tempfile::tempdir().unwrap();
        let other = library_with(&away.path().join("mine"), &[]);
        std::fs::create_dir_all(away.path().join("mine")).unwrap();
        std::fs::copy(home.path().join("tavern.wav"), away.path().join("mine/inn music.wav")).unwrap();
        other.rescan(None).unwrap();
        assert!(other.wait_idle(Duration::from_secs(20)));

        let imported = import(&other, &bundle, &away.path().join("imported")).unwrap();
        assert_eq!(imported.data, data);
        assert_eq!(imported.extracted, 2);
        assert!(imported.missing.is_empty());
        assert_eq!(imported.asset_map.len(), 3);
        assert_eq!(imported.asset_map[&tavern], id_of(&other, "inn music"), "the copy already here is reused");
        let new_rain = other.asset(imported.asset_map[&rain]).unwrap();
        assert!(new_rain.path.contains("imported"));
        assert_eq!(new_rain.tags[0].name, "storm", "tags travel with the sound");
        assert_eq!(other.asset_count(), 3);

        // Importing again changes nothing: everything is recognised.
        let again = import(&other, &bundle, &away.path().join("imported")).unwrap();
        assert_eq!((again.extracted, other.asset_count()), (0, 3));
    }

    #[test]
    fn a_bundle_without_audio_reports_what_is_missing() {
        let home = tempfile::tempdir().unwrap();
        let source = library_with(home.path(), &[("rain", 300.0), ("tavern", 440.0)]);
        let bundle = home.path().join("light.gmaudio");
        export(&source, &bundle, Value::Null, &[id_of(&source, "rain"), id_of(&source, "tavern")], false).unwrap();

        let away = tempfile::tempdir().unwrap();
        let other = library_with(away.path(), &[("tavern", 440.0)]);
        let imported = import(&other, &bundle, &away.path().join("imported")).unwrap();
        assert_eq!(imported.asset_map.len(), 1);
        assert_eq!(imported.missing, ["rain"]);
        assert!(!away.path().join("imported").exists());

        // Not a bundle at all.
        let junk = away.path().join("junk.gmaudio");
        std::fs::write(&junk, b"hello").unwrap();
        assert!(matches!(import(&other, &junk, away.path()), Err(Error::Bundle(_))));
    }
}
