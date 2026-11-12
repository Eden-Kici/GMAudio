// Typed wrappers around the Rust commands and events.
// Field names are snake_case because they mirror the Rust structs.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Channel = "music" | "ambience" | "sfx" | "stingers";

export const CHANNELS: { id: Channel; label: string; verb: string; hint: string }[] = [
  { id: "music", label: "Music", verb: "Music", hint: "One track at a time. A new track crossfades in. Loops." },
  { id: "ambience", label: "Ambience", verb: "Ambience", hint: "Background layers that stack. Loops." },
  { id: "sfx", label: "Sound effects", verb: "SFX", hint: "Plays once. Sounds can overlap." },
  { id: "stingers", label: "Stingers", verb: "Stinger", hint: "Short musical cues. Plays once." },
];

export interface Tag {
  name: string;
  category: string;
}

export interface Asset {
  id: number;
  folder_id: number;
  path: string;
  name: string;
  ext: string;
  size: number;
  duration_ms: number | null;
  sample_rate: number | null;
  channels: number | null;
  loudness_lufs: number | null;
  gain_db: number;
  status: "pending" | "ok" | "failed";
  error: string | null;
  missing: boolean;
  tags: Tag[];
}

export interface Folder {
  id: number;
  path: string;
  asset_count: number;
  missing: boolean;
}

export interface SearchResult {
  total: number;
  assets: Asset[];
}

export interface ScanStatus {
  scanning: string | null;
  files_found: number;
  analyzed: number;
  remaining: number;
  failed: number;
}

export interface ChannelState {
  channel: Channel;
  volume: number;
  muted: boolean;
  fade_ms: number;
}

export interface SoundState {
  id: number;
  asset_id: number;
  name: string;
  channel: Channel;
  position_ms: number;
  duration_ms: number | null;
  looping: boolean;
  stopping: boolean;
}

export interface EngineState {
  device_ok: boolean;
  device_error: string | null;
  master_volume: number;
  channels: ChannelState[];
  sounds: SoundState[];
  panic_armed: boolean;
  last_error: string | null;
}

export type PanicOutcome = "idle" | "fading" | "cut";

export const api = {
  folders: () => invoke<Folder[]>("library_folders"),
  addFolder: (path: string) => invoke<Folder>("library_add_folder", { path }),
  removeFolder: (id: number) => invoke<void>("library_remove_folder", { id }),
  rescan: (folderId?: number) => invoke<void>("library_rescan", { folderId: folderId ?? null }),
  search: (text: string, folderId: number | null, limit: number) =>
    invoke<SearchResult>("library_search", {
      query: { text, folder_id: folderId, limit },
    }),
  scanStatus: () => invoke<ScanStatus>("library_status"),
  setTags: (assetId: number, tags: Tag[]) => invoke<Asset>("library_set_tags", { assetId, tags }),

  engineState: () => invoke<EngineState>("engine_state"),
  play: (assetId: number, channel: Channel) => invoke<number>("engine_play", { assetId, channel }),
  stopSound: (id: number) => invoke<void>("engine_stop_sound", { id }),
  stopChannel: (channel: Channel) => invoke<void>("engine_stop_channel", { channel }),
  setChannel: (channel: Channel, change: { volume?: number; muted?: boolean; fadeMs?: number }) =>
    invoke<void>("engine_set_channel", { channel, ...change }),
  setMaster: (volume: number) => invoke<void>("engine_set_master", { volume }),
  panic: () => invoke<PanicOutcome>("engine_panic"),
  retryDevice: () => invoke<void>("engine_retry_device"),
};

export const events = {
  engineState: (fn: (s: EngineState) => void): Promise<UnlistenFn> =>
    listen<EngineState>("engine://state", (e) => fn(e.payload)),
  scanProgress: (fn: (s: ScanStatus) => void): Promise<UnlistenFn> =>
    listen<ScanStatus>("library://progress", (e) => fn(e.payload)),
  libraryChanged: (fn: () => void): Promise<UnlistenFn> => listen("library://changed", () => fn()),
};
