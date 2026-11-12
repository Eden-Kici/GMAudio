//! The Tauri shell: forwards UI commands to the core and core events to the UI.
//!
//! No audio or library logic lives here. The engine owns all playback state;
//! the UI only draws what the engine reports.

use std::fmt::Display;
use std::path::{Path, PathBuf};

use gmaudio_core::engine::{
    Channel, Engine, EngineState, MixerSettings, PanicOutcome, PlayRequest, Quantize,
};
use gmaudio_core::library::{Library, LibraryEvent, ScanStatus};
use gmaudio_core::model::{Asset, Folder, Tag};
use gmaudio_core::project;
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
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running GMAudio");
}
