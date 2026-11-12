//! The Tauri shell: forwards UI commands to the core and core events to the UI.
//!
//! No audio or library logic lives here. The engine owns all playback state;
//! the UI only draws what the engine reports.

use std::fmt::Display;
use std::path::Path;

use gmaudio_core::engine::{
    Channel, Engine, EngineState, MixerSettings, PanicOutcome, PlayRequest,
};
use gmaudio_core::library::{Library, LibraryEvent, ScanStatus};
use gmaudio_core::model::{AnalysisStatus, Asset, Folder, Tag};
use gmaudio_core::search::{Query, SearchResult};
use serde::Serialize;
use tauri::{Emitter, Manager, State};

const MIXER_SETTINGS_KEY: &str = "mixer";

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
    }
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

// ---- engine ----------------------------------------------------------------

#[tauri::command(async)]
fn engine_state(state: State<'_, AppState>) -> CmdResult<EngineState> {
    state.engine.state().map_err(msg)
}

#[tauri::command(async)]
fn engine_play(
    state: State<'_, AppState>,
    asset_id: i64,
    channel: Channel,
    looping: Option<bool>,
) -> CmdResult<u64> {
    let asset = state
        .library
        .asset(asset_id)
        .ok_or("That sound is no longer in the library.")?;
    if asset.missing {
        return Err(format!("\"{}\" was not found on disk.", asset.name));
    }
    if asset.status == AnalysisStatus::Failed {
        return Err(format!(
            "\"{}\" could not be read: {}",
            asset.name,
            asset.error.as_deref().unwrap_or("unknown problem")
        ));
    }
    state
        .engine
        .play(PlayRequest {
            asset_id,
            path: asset.path.into(),
            name: asset.name,
            channel,
            gain_db: asset.gain_db as f32,
            duration_ms: asset.duration_ms,
            looping,
        })
        .map_err(msg)
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
            let engine = Engine::spawn(settings, move |state| {
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
            library_tags,
            engine_state,
            engine_play,
            engine_stop_sound,
            engine_stop_channel,
            engine_set_channel,
            engine_set_master,
            engine_panic,
            engine_retry_device,
            engine_command_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GMAudio");
}
