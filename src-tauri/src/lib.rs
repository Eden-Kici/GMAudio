//! The Tauri shell: forwards UI commands to the core and core events to the UI.
//!
//! No audio or library logic lives here. The engine owns all playback state;
//! the UI only draws what the engine reports.

use std::fmt::Display;
use std::path::{Path, PathBuf};

use gmaudio_core::engine::{
    AudioProfile, Channel, Engine, EngineState, MixerSettings, PanicOutcome, PlayRequest, Quantize,
};
use gmaudio_core::library::{Library, LibraryEvent, ScanStatus};
use gmaudio_core::model::{Asset, Folder, Tag};
use gmaudio_core::project;
use gmaudio_core::{bundle, freesound, secrets};
use tauri_plugin_opener::OpenerExt;
use gmaudio_core::search::{Query, SearchResult};
use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

const MIXER_SETTINGS_KEY: &str = "mixer";
const OUTPUT_DEVICE_KEY: &str = "output_device";

/// The built-in sounds, compiled into the app.
static STARTER_SOUNDS: Dir = include_dir!("$CARGO_MANIFEST_DIR/starter-sounds");
const STARTER_FOLDER: &str = "Starter Sounds";

struct AppState {
    library: Library,
    engine: Engine,
}

type CmdResult<T> = Result<T, String>;

fn msg(e: impl Display) -> String {
    e.to_string()
}

/// Remembers fader positions, mutes and fade times for the next launch.
fn save_mixer(state: &AppState) {
    let Ok(s) = state.engine.state() else { return };
    let mut settings = MixerSettings {
        master_volume: s.master_volume,
        ..Default::default()
    };
    for (slot, c) in settings.channels.iter_mut().zip(&s.channels) {
        slot.volume = c.volume;
        slot.muted = c.muted;
        slot.fade_ms = c.fade_ms;
        slot.loop_queue = c.loop_queue;
        slot.solo = c.solo;
    }
    settings.duck_db = s.duck_db;
    settings.profile = s.profile;
    if let Ok(json) = serde_json::to_string(&settings) {
        let _ = state.library.set_setting(MIXER_SETTINGS_KEY, &json);
    }
}

// ---- library ---------------------------------------------------------------

#[tauri::command(async)]
fn library_folders(state: State<'_, AppState>) -> CmdResult<Vec<Folder>> {
    state.library.folders().map_err(msg)
}

#[tauri::command(async)]
fn library_add_folder(state: State<'_, AppState>, path: String) -> CmdResult<Folder> {
    state.library.add_folder(Path::new(&path)).map_err(msg)
}

#[tauri::command(async)]
fn library_remove_folder(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.library.remove_folder(id).map_err(msg)
}

#[tauri::command(async)]
fn library_rescan(state: State<'_, AppState>, folder_id: Option<i64>) -> CmdResult<()> {
    state.library.rescan(folder_id).map_err(msg)
}

#[tauri::command(async)]
fn library_search(state: State<'_, AppState>, query: Query) -> SearchResult {
    state.library.search(&query)
}

#[tauri::command(async)]
fn library_status(state: State<'_, AppState>) -> ScanStatus {
    state.library.status()
}

#[tauri::command(async)]
fn library_set_tags(state: State<'_, AppState>, asset_id: i64, tags: Vec<Tag>) -> CmdResult<Asset> {
    state.library.set_tags(asset_id, &tags).map_err(msg)
}

#[derive(Serialize)]
struct TagCount {
    name: String,
    category: String,
    count: i64,
}

#[tauri::command(async)]
fn library_tags(state: State<'_, AppState>) -> CmdResult<Vec<TagCount>> {
    Ok(state
        .library
        .tags()
        .map_err(msg)?
        .into_iter()
        .map(|(t, count)| TagCount {
            name: t.name,
            category: t.category,
            count,
        })
        .collect())
}

/// The sounds with these ids, skipping any that are no longer in the library.
#[tauri::command(async)]
fn library_assets(state: State<'_, AppState>, ids: Vec<i64>) -> Vec<Asset> {
    ids.into_iter().filter_map(|id| state.library.asset(id)).collect()
}

#[tauri::command(async)]
fn library_set_tag_on(
    state: State<'_, AppState>,
    asset_ids: Vec<i64>,
    tag: Tag,
    on: bool,
) -> CmdResult<()> {
    state.library.set_tag_on(&asset_ids, &tag, on).map_err(msg)
}

/// Copies the built-in sounds into the app's data folder and adds that
/// folder to the library. Safe to run again: missing files are restored.
#[tauri::command(async)]
fn library_add_starter_sounds(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let target = app.path().app_data_dir().map_err(msg)?.join(STARTER_FOLDER);
    std::fs::create_dir_all(&target).map_err(msg)?;
    STARTER_SOUNDS.extract(&target).map_err(msg)?;
    match state.library.add_folder(&target) {
        Ok(_) => Ok(()),
        Err(gmaudio_core::Error::FolderAlreadyAdded) => state.library.rescan(None).map_err(msg),
        Err(e) => Err(msg(e)),
    }
}

// ---- saved interface state and documents -----------------------------------

#[tauri::command(async)]
fn ui_get(state: State<'_, AppState>, key: String) -> CmdResult<Option<String>> {
    state.library.get_setting(&format!("ui.{key}")).map_err(msg)
}

#[tauri::command(async)]
fn ui_set(state: State<'_, AppState>, key: String, value: String) -> CmdResult<()> {
    state.library.set_setting(&format!("ui.{key}"), &value).map_err(msg)
}

#[derive(Serialize)]
struct Doc {
    id: i64,
    data: Value,
}

#[tauri::command(async)]
fn doc_list(state: State<'_, AppState>, kind: String) -> CmdResult<Vec<Doc>> {
    Ok(state
        .library
        .docs(&kind)
        .map_err(msg)?
        .into_iter()
        .filter_map(|(id, json)| Some(Doc { id, data: serde_json::from_str(&json).ok()? }))
        .collect())
}

#[tauri::command(async)]
fn doc_save(state: State<'_, AppState>, kind: String, id: Option<i64>, data: Value) -> CmdResult<i64> {
    state.library.put_doc(&kind, id, &data.to_string()).map_err(msg)
}

#[tauri::command(async)]
fn doc_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.library.delete_doc(id).map_err(msg)
}

// ---- engine ----------------------------------------------------------------

#[tauri::command(async)]
fn engine_state(state: State<'_, AppState>) -> CmdResult<EngineState> {
    state.engine.state().map_err(msg)
}

/// How one use of a sound differs from the plain "play it".
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PlayOptions {
    looping: Option<bool>,
    interrupt: bool,
    trim_db: f32,
    rate: f64,
    pan: f32,
    duck: Option<bool>,
    tag: Option<String>,
}

/// The outline of a sound, for the trim editor's timeline.
#[tauri::command(async)]
fn library_waveform(state: State<'_, AppState>, asset_id: i64, buckets: usize) -> CmdResult<Vec<f32>> {
    let asset = state.library.asset(asset_id).ok_or("That sound is no longer in the library.")?;
    gmaudio_core::analyze::waveform(Path::new(&asset.path), buckets, asset.duration_ms.unwrap_or(0)).map_err(msg)
}

/// Where a sound's audible part begins and ends, leaving out silence at
/// both ends. The end is null when the sound runs to the end of the file.
#[tauri::command(async)]
fn library_audible_part(state: State<'_, AppState>, asset_id: i64) -> CmdResult<(i64, Option<i64>)> {
    let asset = state.library.asset(asset_id).ok_or("That sound is no longer in the library.")?;
    gmaudio_core::analyze::audible_part(Path::new(&asset.path), asset.duration_ms.unwrap_or(0)).map_err(msg)
}

/// Trims the silence off both ends of several sounds, keeping any parts
/// they already skip. Returns how many were changed.
#[tauri::command(async)]
fn library_trim_silence(state: State<'_, AppState>, asset_ids: Vec<i64>) -> CmdResult<usize> {
    let mut changed = 0;
    for id in asset_ids {
        let Some(asset) = state.library.asset(id) else { continue };
        let Ok((start, end)) = gmaudio_core::analyze::audible_part(Path::new(&asset.path), asset.duration_ms.unwrap_or(0)) else {
            continue;
        };
        let cuts = asset.edit.as_ref().map(|e| e.cuts.clone()).unwrap_or_default();
        let edit = gmaudio_core::model::AssetEdit { start_ms: start, end_ms: end, cuts };
        if asset.edit.as_ref() != Some(&edit) && (start > 0 || end.is_some()) {
            state.library.set_edit(id, Some(edit)).map_err(msg)?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// Plays a sound once with a trim that has not been saved yet, from a
/// chosen point, on the effects channel.
#[tauri::command(async)]
fn engine_preview(
    state: State<'_, AppState>,
    asset_id: i64,
    edit: Option<gmaudio_core::model::AssetEdit>,
    from_ms: Option<i64>,
) -> CmdResult<u64> {
    let duration = state.library.asset(asset_id).and_then(|a| a.duration_ms);
    let edit = edit.and_then(|e| e.normalized(duration)).unwrap_or_default();
    let request = PlayRequest {
        looping: Some(false),
        always_loop: false,
        start_ms: edit.start_ms,
        end_ms: edit.end_ms,
        cuts: edit.cuts,
        resume_ms: from_ms,
        tag: Some("preview".into()),
        ..project::play_request(&state.library, asset_id, Channel::Sfx).map_err(msg)?
    };
    state.engine.play(request).map_err(msg)
}

#[tauri::command(async)]
fn engine_play(
    state: State<'_, AppState>,
    asset_id: i64,
    channel: Channel,
    options: Option<PlayOptions>,
) -> CmdResult<u64> {
    let o = options.unwrap_or_default();
    let request = PlayRequest {
        looping: o.looping,
        interrupt: o.interrupt,
        trim_db: o.trim_db,
        rate: o.rate,
        pan: o.pan,
        duck: o.duck,
        tag: o.tag,
        ..project::play_request(&state.library, asset_id, channel).map_err(msg)?
    };
    state.engine.play(request).map_err(msg)
}

/// Decodes short sounds ahead of time so pads and stingers start at once.
#[tauri::command(async)]
fn engine_preload(state: State<'_, AppState>, asset_ids: Vec<i64>) -> CmdResult<()> {
    let paths: Vec<PathBuf> = asset_ids
        .into_iter()
        .filter_map(|id| state.library.asset(id))
        .filter(|a| !a.missing && a.duration_ms.is_some_and(|d| d <= project::PRELOAD_MAX_MS))
        .map(|a| PathBuf::from(a.path))
        .collect();
    state.engine.preload(&paths);
    Ok(())
}

/// Switches the whole soundscape to a scene. Returns the sounds that had
/// to be left out (missing files), so the interface can say so.
#[tauri::command(async)]
fn scene_apply(state: State<'_, AppState>, id: i64, data: Value) -> CmdResult<Vec<String>> {
    let doc: project::SceneDoc = serde_json::from_value(data).map_err(msg)?;
    let mut resolved = project::scene_request(&state.library, id, &doc);
    state.engine.apply_scene(resolved.request).map_err(msg)?;

    // Dynamic music: start the set, or just move tier if it is already on.
    if let project::SceneMusicDoc::Set { set_id, tier } = doc.music {
        let playing = state.engine.state().map_err(msg)?.music_set.map(|s| s.id);
        if playing == Some(set_id) {
            state.engine.set_tier(tier, None).map_err(msg)?;
        } else {
            match state.library.doc(set_id).map_err(msg)? {
                Some((kind, json)) if kind == "set" => {
                    let set: project::MusicSetDoc = serde_json::from_str(&json).map_err(msg)?;
                    let set = project::set_request(&state.library, set_id, &set, tier).map_err(msg)?;
                    state.engine.play_set(set.request).map_err(msg)?;
                    resolved.skipped.extend(set.skipped);
                }
                _ => resolved.skipped.push("This scene's music set no longer exists.".into()),
            }
        }
    }
    Ok(resolved.skipped)
}

/// Starts a piece of dynamic music at one of its tiers.
#[tauri::command(async)]
fn set_play(state: State<'_, AppState>, id: i64, data: Value, tier: Option<usize>) -> CmdResult<Vec<String>> {
    let doc: project::MusicSetDoc = serde_json::from_value(data).map_err(msg)?;
    let resolved = project::set_request(&state.library, id, &doc, tier.unwrap_or(0)).map_err(msg)?;
    state.engine.play_set(resolved.request).map_err(msg)?;
    Ok(resolved.skipped)
}

/// Changes the tier of the music set that is playing. Without `quantize`
/// the set's own timing is used.
#[tauri::command(async)]
fn set_tier(state: State<'_, AppState>, tier: usize, quantize: Option<Quantize>) -> CmdResult<()> {
    state.engine.set_tier(tier, quantize).map_err(msg)
}

#[tauri::command(async)]
fn set_step(state: State<'_, AppState>, delta: i32) -> CmdResult<()> {
    state.engine.step_tier(delta).map_err(msg)
}

/// Plays a stinger in time with the music set, optionally changing tier
/// when it ends.
#[tauri::command(async)]
fn stinger_play(
    state: State<'_, AppState>,
    asset_id: i64,
    quantize: Option<Quantize>,
    then_tier: Option<usize>,
    duck: Option<bool>,
    tag: Option<String>,
) -> CmdResult<u64> {
    let request = PlayRequest {
        duck,
        tag,
        ..project::play_request(&state.library, asset_id, Channel::Stingers).map_err(msg)?
    };
    state.engine.play_stinger(request, quantize.unwrap_or(Quantize::Beat), then_tier).map_err(msg)
}

#[derive(Serialize)]
struct OutputDevices {
    devices: Vec<String>,
    /// `None`: follow the system default.
    selected: Option<String>,
}

#[tauri::command(async)]
fn engine_output_devices(state: State<'_, AppState>) -> CmdResult<OutputDevices> {
    Ok(OutputDevices {
        devices: gmaudio_core::engine::output_devices(),
        selected: state.library.get_setting(OUTPUT_DEVICE_KEY).map_err(msg)?.filter(|d| !d.is_empty()),
    })
}

/// Plays through another output from now on. `None` follows the system default.
#[tauri::command(async)]
fn engine_set_output_device(state: State<'_, AppState>, name: Option<String>) -> CmdResult<()> {
    state.library.set_setting(OUTPUT_DEVICE_KEY, name.as_deref().unwrap_or("")).map_err(msg)?;
    state.engine.set_output_device(name).map_err(msg)
}

#[tauri::command(async)]
fn engine_set_sound_trim(state: State<'_, AppState>, id: u64, trim_db: f32) -> CmdResult<()> {
    state.engine.set_sound_trim(id, trim_db).map_err(msg)
}

#[tauri::command(async)]
fn engine_set_duck_db(state: State<'_, AppState>, db: f32) -> CmdResult<()> {
    state.engine.set_duck_db(db).map_err(msg)?;
    save_mixer(&state);
    Ok(())
}

#[tauri::command(async)]
fn engine_stop_sound(state: State<'_, AppState>, id: u64, fade_ms: Option<u32>) -> CmdResult<()> {
    state.engine.stop_sound(id, fade_ms).map_err(msg)
}

#[tauri::command(async)]
fn engine_stop_channel(
    state: State<'_, AppState>,
    channel: Channel,
    fade_ms: Option<u32>,
) -> CmdResult<()> {
    state.engine.stop_channel(channel, fade_ms).map_err(msg)
}

#[tauri::command(async)]
fn engine_set_channel(
    state: State<'_, AppState>,
    channel: Channel,
    volume: Option<f32>,
    muted: Option<bool>,
    fade_ms: Option<u32>,
    solo: Option<bool>,
) -> CmdResult<()> {
    if let Some(v) = volume {
        state.engine.set_channel_volume(channel, v).map_err(msg)?;
    }
    if let Some(m) = muted {
        state.engine.set_channel_muted(channel, m).map_err(msg)?;
    }
    if let Some(f) = fade_ms {
        state.engine.set_channel_fade(channel, f).map_err(msg)?;
    }
    if let Some(s) = solo {
        state.engine.set_channel_solo(channel, s).map_err(msg)?;
    }
    save_mixer(&state);
    Ok(())
}

#[tauri::command(async)]
fn engine_set_master(state: State<'_, AppState>, volume: f32) -> CmdResult<()> {
    state.engine.set_master_volume(volume).map_err(msg)?;
    save_mixer(&state);
    Ok(())
}

#[tauri::command(async)]
fn engine_set_sound_looping(state: State<'_, AppState>, id: u64, looping: bool) -> CmdResult<()> {
    state.engine.set_sound_looping(id, looping).map_err(msg)
}

#[tauri::command(async)]
fn engine_set_loop_queue(state: State<'_, AppState>, on: bool) -> CmdResult<()> {
    state.engine.set_loop_queue(on).map_err(msg)?;
    save_mixer(&state);
    Ok(())
}

#[tauri::command(async)]
fn engine_next_track(state: State<'_, AppState>) -> CmdResult<()> {
    state.engine.next_track().map_err(msg)
}

#[tauri::command(async)]
fn engine_play_queued_now(state: State<'_, AppState>, id: u64) -> CmdResult<()> {
    state.engine.play_queued_now(id).map_err(msg)
}

#[tauri::command(async)]
fn engine_panic(state: State<'_, AppState>) -> CmdResult<PanicOutcome> {
    state.engine.panic().map_err(msg)
}

#[tauri::command(async)]
fn engine_retry_device(state: State<'_, AppState>) -> CmdResult<()> {
    state.engine.retry_device().map_err(msg)
}

#[tauri::command(async)]
fn engine_command_log(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    state.engine.command_log().map_err(msg)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[tauri::command]
fn engine_move_queued(state: State<'_, AppState>, id: u64, to: usize) -> CmdResult<()> {
    state.engine.move_queued(id, to).map_err(msg)
}

#[tauri::command]
fn engine_seek(state: State<'_, AppState>, id: u64, position_ms: i64) -> CmdResult<()> {
    state.engine.seek_sound(id, position_ms).map_err(msg)
}

#[tauri::command]
fn library_set_trim(state: State<'_, AppState>, asset_id: i64, trim_db: f64) -> CmdResult<Asset> {
    state.library.set_trim(asset_id, trim_db).map_err(msg)
}

#[tauri::command]
fn engine_set_profile(state: State<'_, AppState>, profile: AudioProfile) -> CmdResult<()> {
    state.engine.set_profile(profile).map_err(msg)?;
    save_mixer(&state);
    Ok(())
}

/// The credit lines of every downloaded sound, for pasting into notes.
#[tauri::command]
fn library_credits(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    let mut credits: Vec<String> = state
        .library
        .all_assets()
        .map_err(msg)?
        .into_iter()
        .filter(|a| !a.missing)
        .filter_map(|a| a.attribution)
        .collect();
    credits.sort();
    credits.dedup();
    Ok(credits)
}

// ---- Freesound ---------------------------------------------------------------

const FREESOUND_CLIENT_ID: &str = "freesound.client_id";
const FREESOUND_API_KEY: &str = "freesound.api_key";
const FREESOUND_TOKENS: &str = "freesound.tokens";
const FREESOUND_FOLDER: &str = "Freesound";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FreesoundStatus {
    /// An API key is saved: searching and downloading previews work.
    has_key: bool,
    /// A client id is saved too: signing in is possible.
    can_sign_in: bool,
    /// Signed in: original files can be downloaded.
    signed_in: bool,
}

fn freesound_key(state: &AppState) -> CmdResult<String> {
    secrets::get(&state.library, FREESOUND_API_KEY)
        .ok_or_else(|| "Add a Freesound API key first (Settings, Online sounds).".to_string())
}

fn freesound_credentials(state: &AppState) -> CmdResult<freesound::Credentials> {
    let client_id = secrets::get(&state.library, FREESOUND_CLIENT_ID)
        .ok_or_else(|| "Add the Freesound client id first (Settings, Online sounds).".to_string())?;
    Ok(freesound::Credentials { client_id, api_key: freesound_key(state)? })
}

/// A working access token, refreshed if the saved one has run out.
fn freesound_token(state: &AppState) -> Option<String> {
    let saved = secrets::get(&state.library, FREESOUND_TOKENS)?;
    let tokens: freesound::Tokens = serde_json::from_str(&saved).ok()?;
    if tokens.is_fresh() {
        return Some(tokens.access_token);
    }
    let credentials = freesound_credentials(state).ok()?;
    let renewed = freesound::refresh(&credentials, &tokens).ok()?;
    secrets::set(&state.library, FREESOUND_TOKENS, &serde_json::to_string(&renewed).ok()?);
    Some(renewed.access_token)
}

#[tauri::command]
fn freesound_status(state: State<'_, AppState>) -> FreesoundStatus {
    let has_key = secrets::get(&state.library, FREESOUND_API_KEY).is_some();
    FreesoundStatus {
        has_key,
        can_sign_in: has_key && secrets::get(&state.library, FREESOUND_CLIENT_ID).is_some(),
        signed_in: secrets::get(&state.library, FREESOUND_TOKENS).is_some(),
    }
}

#[tauri::command]
fn freesound_set_credentials(state: State<'_, AppState>, client_id: String, api_key: String) -> FreesoundStatus {
    for (name, value) in [(FREESOUND_CLIENT_ID, client_id.trim()), (FREESOUND_API_KEY, api_key.trim())] {
        // An empty box means "leave it as it is".
        if !value.is_empty() {
            secrets::set(&state.library, name, value);
        }
    }
    // Tokens belong to the old key.
    secrets::delete(&state.library, FREESOUND_TOKENS);
    freesound_status(state)
}

#[tauri::command]
fn freesound_remove_key(state: State<'_, AppState>) -> FreesoundStatus {
    for name in [FREESOUND_CLIENT_ID, FREESOUND_API_KEY, FREESOUND_TOKENS] {
        secrets::delete(&state.library, name);
    }
    freesound_status(state)
}

#[tauri::command(async)]
fn freesound_search(state: State<'_, AppState>, query: String, page: u32) -> CmdResult<freesound::SearchPage> {
    freesound::search(&freesound_key(&state)?, &query, page).map_err(msg)
}

/// Opens Freesound's sign-in page in the browser. It ends by showing a
/// code, which the user pastes back into the app.
#[tauri::command]
fn freesound_open_sign_in(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let credentials = freesound_credentials(&state)?;
    open_in_browser(&app, &freesound::authorize_url(&credentials.client_id))
}

#[tauri::command(async)]
fn freesound_sign_in(state: State<'_, AppState>, code: String) -> CmdResult<FreesoundStatus> {
    let tokens = freesound::exchange_code(&freesound_credentials(&state)?, &code).map_err(msg)?;
    secrets::set(&state.library, FREESOUND_TOKENS, &serde_json::to_string(&tokens).map_err(msg)?);
    Ok(freesound_status(state))
}

#[tauri::command]
fn freesound_sign_out(state: State<'_, AppState>) -> FreesoundStatus {
    secrets::delete(&state.library, FREESOUND_TOKENS);
    freesound_status(state)
}

#[derive(Serialize)]
struct Downloaded {
    /// The sound in the library, once it has been read.
    asset: Option<Asset>,
    /// The original file was fetched (false: the compressed preview).
    original: bool,
}

/// Downloads a sound into the library, with its credit and a channel tag.
#[tauri::command(async)]
fn freesound_download(
    app: AppHandle,
    state: State<'_, AppState>,
    sound: freesound::Sound,
    channel_tag: Option<String>,
) -> CmdResult<Downloaded> {
    let folder = app.path().app_data_dir().map_err(msg)?.join(FREESOUND_FOLDER);
    std::fs::create_dir_all(&folder).map_err(msg)?;
    let token = freesound_token(&state);
    let (path, original) = match freesound::download(&sound, token.as_deref(), &folder) {
        Ok(path) => {
            let original = token.is_some() && !path_is_preview(&sound, &path);
            (path, original)
        }
        // The original could not be had (signed out elsewhere, say): the preview still can.
        Err(_) if token.is_some() => (freesound::download(&sound, None, &folder).map_err(msg)?, false),
        Err(e) => return Err(msg(e)),
    };
    let path_str = path.to_string_lossy().to_string();
    state
        .library
        .set_credit(&path_str, &sound.license, &sound.attribution())
        .map_err(msg)?;
    let asset = add_download(&state, &folder, &path_str, channel_tag)?;
    Ok(Downloaded { asset, original })
}

/// Brings a freshly downloaded file into the library and tags it for a channel.
fn add_download(state: &AppState, folder: &Path, path: &str, channel_tag: Option<String>) -> CmdResult<Option<Asset>> {
    match state.library.add_folder(folder) {
        Ok(_) => {}
        Err(gmaudio_core::Error::FolderAlreadyAdded) => {
            let id = state.library.folders().map_err(msg)?.into_iter().find(|f| Path::new(&f.path) == folder).map(|f| f.id);
            state.library.rescan(id).map_err(msg)?;
        }
        Err(e) => return Err(msg(e)),
    }
    state.library.wait_idle(std::time::Duration::from_secs(120));
    let asset = state.library.all_assets().map_err(msg)?.into_iter().find(|a| a.path == path);
    Ok(match (asset, channel_tag) {
        (Some(asset), Some(tag)) if gmaudio_core::model::CHANNEL_TAGS.contains(&tag.as_str()) => {
            // Say where it belongs, replacing any guess made from the file's name.
            let mut tags: Vec<Tag> = asset
                .tags
                .iter()
                .filter(|t| t.category != gmaudio_core::model::CHANNEL_TAG_CATEGORY)
                .cloned()
                .collect();
            tags.push(Tag { name: tag, category: gmaudio_core::model::CHANNEL_TAG_CATEGORY.into() });
            Some(state.library.set_tags(asset.id, &tags).map_err(msg)?)
        }
        (asset, _) => asset,
    })
}

/// Opens a web address in the default browser.
fn open_in_browser(app: &AppHandle, url: &str) -> CmdResult<()> {
    // On Windows, go straight to the URL handler: a shell in between cuts
    // the address off at the first "&".
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = app;
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .creation_flags(0x0800_0000)
            .spawn()
            .map(|_| ())
            .map_err(msg)
    }
    #[cfg(not(windows))]
    {
        app.opener().open_url(url, None::<&str>).map_err(msg)
    }
}

/// Saves the audio of a YouTube video into the library.
#[tauri::command(async)]
fn youtube_import(app: AppHandle, state: State<'_, AppState>, url: String, channel_tag: Option<String>) -> CmdResult<Option<Asset>> {
    let data = app.path().app_data_dir().map_err(msg)?;
    let folder = data.join("YouTube");
    let path = gmaudio_core::youtube::download(&data.join("tools"), &url, &folder).map_err(msg)?;
    add_download(&state, &folder, &path.to_string_lossy(), channel_tag)
}

/// The app's version, and the commit it was built from when known.
#[tauri::command]
fn app_version(app: AppHandle) -> String {
    let version = app.package_info().version.to_string();
    match option_env!("GMAUDIO_COMMIT") {
        Some(commit) if !commit.is_empty() => format!("{version} ({commit})"),
        _ => version,
    }
}

#[tauri::command]
fn engine_move_to_channel(state: State<'_, AppState>, id: u64, channel: Channel) -> CmdResult<()> {
    state.engine.move_to_channel(id, channel).map_err(msg)
}

#[tauri::command]
fn engine_requeue_playing(state: State<'_, AppState>, id: u64, to: usize) -> CmdResult<()> {
    state.engine.requeue_playing(id, to).map_err(msg)
}

#[tauri::command]
fn library_set_title(state: State<'_, AppState>, asset_id: i64, title: String) -> CmdResult<Asset> {
    state.library.set_title(asset_id, &title).map_err(msg)
}

#[tauri::command]
fn library_set_always_loop(state: State<'_, AppState>, asset_id: i64, on: bool) -> CmdResult<Asset> {
    state.library.set_always_loop(asset_id, on).map_err(msg)
}

#[tauri::command]
fn library_set_edit(state: State<'_, AppState>, asset_id: i64, edit: Option<gmaudio_core::model::AssetEdit>) -> CmdResult<Asset> {
    state.library.set_edit(asset_id, edit).map_err(msg)
}

#[tauri::command]
fn engine_swap_with_playing(state: State<'_, AppState>, id: u64) -> CmdResult<()> {
    state.engine.swap_with_playing(id).map_err(msg)
}

#[tauri::command]
fn engine_pause(state: State<'_, AppState>, id: u64, paused: bool, fade_ms: u32) -> CmdResult<()> {
    state.engine.pause_sound(id, paused, fade_ms).map_err(msg)
}

/// The Freesound sign-in address, for copying when the browser did not open it right.
#[tauri::command]
fn freesound_sign_in_url(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(freesound::authorize_url(&freesound_credentials(&state)?.client_id))
}

fn path_is_preview(sound: &freesound::Sound, path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    !ext.eq_ignore_ascii_case(&sound.kind)
}

/// Opens a Freesound page in the browser (a sound's page, or where to get a key).
#[tauri::command]
fn freesound_open_page(app: AppHandle, url: String) -> CmdResult<()> {
    if !url.starts_with("https://freesound.org/") {
        return Err("Only Freesound pages can be opened from here.".into());
    }
    open_in_browser(&app, &url)
}

// ---- bundles -----------------------------------------------------------------

#[tauri::command(async)]
fn bundle_export(
    state: State<'_, AppState>,
    path: String,
    data: Value,
    asset_ids: Vec<i64>,
    include_audio: bool,
) -> CmdResult<bundle::ExportReport> {
    bundle::export(&state.library, Path::new(&path), data, &asset_ids, include_audio).map_err(msg)
}

#[tauri::command(async)]
fn bundle_import(app: AppHandle, state: State<'_, AppState>, path: String) -> CmdResult<bundle::ImportReport> {
    let source = PathBuf::from(&path);
    let name = source.file_stem().and_then(|s| s.to_str()).unwrap_or("bundle");
    let folder = app.path().app_data_dir().map_err(msg)?.join("Imported").join(name);
    bundle::import(&state.library, &source, &folder).map_err(msg)
}

/// Shows a file in Explorer.
#[tauri::command]
fn reveal_file(app: AppHandle, path: String) -> CmdResult<()> {
    app.opener().reveal_item_in_dir(path).map_err(msg)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let handle = app.handle().clone();
            let library = Library::open(&data_dir.join("library.db"), move |event| {
                let _ = match event {
                    LibraryEvent::Progress(status) => handle.emit("library://progress", status),
                    LibraryEvent::Changed => handle.emit("library://changed", ()),
                };
            })?;

            let settings = library
                .get_setting(MIXER_SETTINGS_KEY)
                .ok()
                .flatten()
                .and_then(|json| serde_json::from_str::<MixerSettings>(&json).ok())
                .unwrap_or_default();
            let handle = app.handle().clone();
            let device = library.get_setting(OUTPUT_DEVICE_KEY).ok().flatten().filter(|d| !d.is_empty());
            let engine = Engine::spawn(settings, device, move |state| {
                let _ = handle.emit("engine://state", state);
            });

            library.start()?;
            app.manage(AppState { library, engine });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            library_folders,
            library_add_folder,
            library_remove_folder,
            library_rescan,
            library_search,
            library_status,
            library_set_tags,
            library_set_tag_on,
            library_assets,
            library_add_starter_sounds,
            library_tags,
            ui_get,
            ui_set,
            doc_list,
            doc_save,
            doc_delete,
            engine_state,
            engine_play,
            engine_stop_sound,
            engine_stop_channel,
            engine_set_channel,
            engine_set_master,
            engine_set_sound_looping,
            engine_set_loop_queue,
            engine_next_track,
            engine_play_queued_now,
            engine_preload,
            engine_set_sound_trim,
            engine_set_duck_db,
            engine_output_devices,
            engine_set_output_device,
            scene_apply,
            set_play,
            set_tier,
            set_step,
            stinger_play,
            engine_panic,
            engine_retry_device,
            engine_command_log,
            library_set_trim,
            engine_move_queued,
            engine_swap_with_playing,
            library_waveform,
            library_audible_part,
            library_trim_silence,
            engine_preview,
            engine_move_to_channel,
            engine_requeue_playing,
            library_set_always_loop,
            library_set_title,
            library_set_edit,
            engine_pause,
            youtube_import,
            app_version,
            freesound_sign_in_url,
            freesound_remove_key,
            engine_seek,
            library_credits,
            engine_set_profile,
            freesound_status,
            freesound_set_credentials,
            freesound_search,
            freesound_open_sign_in,
            freesound_sign_in,
            freesound_sign_out,
            freesound_download,
            freesound_open_page,
            bundle_export,
            bundle_import,
            reveal_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GMAudio");
}
