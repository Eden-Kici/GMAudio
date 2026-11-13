// Typed wrappers around the Rust commands and events.
// Field names are snake_case because they mirror the Rust structs.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Channel = "music" | "ambience" | "sfx" | "stingers";

export interface ChannelInfo {
  id: Channel;
  label: string;
  /** Short label for buttons. */
  verb: string;
  hint: string;
  /** The tag that marks a sound as belonging on this channel. */
  tag: string;
  color: string;
}

export const CHANNELS: ChannelInfo[] = [
  {
    id: "music",
    label: "Music",
    verb: "Music",
    hint: "One track at a time. More tracks wait in the queue.",
    tag: "Music",
    color: "var(--color-music)",
  },
  {
    id: "ambience",
    label: "Ambience",
    verb: "Ambience",
    hint: "Background layers that stack and repeat.",
    tag: "Ambience",
    color: "var(--color-ambience)",
  },
  {
    id: "sfx",
    label: "Sound effects",
    verb: "SFX",
    hint: "Plays once. Sounds can overlap.",
    tag: "Sound effects",
    color: "var(--color-sfx)",
  },
  {
    id: "stingers",
    label: "Stingers",
    verb: "Stinger",
    hint: "Short musical cues. Plays once.",
    tag: "Stinger",
    color: "var(--color-stingers)",
  },
];

export const channelInfo = (id: Channel): ChannelInfo => CHANNELS.find((c) => c.id === id)!;

/** Other spellings people type for the channel tags. */
const TAG_ALIASES: Record<string, Channel> = {
  music: "music",
  ambience: "ambience",
  ambiance: "ambience",
  "sound effects": "sfx",
  "sound effect": "sfx",
  sfx: "sfx",
  stinger: "stingers",
  stingers: "stingers",
};

export const channelOfTag = (name: string): Channel | undefined => TAG_ALIASES[name.trim().toLowerCase()];

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
  bpm: number | null;
  beat_offset_ms: number | null;
  status: "pending" | "ok" | "failed";
  error: string | null;
  missing: boolean;
  /** "local", or "freesound" for a downloaded sound. */
  source: string;
  license: string | null;
  /** The credit line a downloaded sound asks for. */
  attribution: string | null;
  /** The user's own volume correction in dB. */
  trim_db: number;
  /** Repeats whenever it is played, on any channel. */
  always_loop: boolean;
  /** Which parts of the file play; null plays all of it. */
  edit: AssetEdit | null;
  tags: Tag[];
}

/** A trim of a sound. Times are milliseconds into the file. */
export interface AssetEdit {
  start_ms: number;
  end_ms: number | null;
  /** Skipped stretches as [from, to]. */
  cuts: [number, number][];
}

/** How the whole mix is shaped for the speakers in use. */
export interface AudioProfile {
  bass_db: number;
  treble_db: number;
  gain_db: number;
  /** 0 to 1: how much loud and quiet passages are pulled together. */
  compress: number;
  limit: boolean;
}

export interface FreesoundStatus {
  hasKey: boolean;
  canSignIn: boolean;
  signedIn: boolean;
}

export interface FreesoundSound {
  id: number;
  name: string;
  username: string;
  license: string;
  license_url: string;
  duration: number;
  tags: string[];
  kind: string;
  filesize: number;
  preview_url: string;
}

export interface FreesoundPage {
  count: number;
  sounds: FreesoundSound[];
  has_more: boolean;
}

export interface ImportReport {
  data: unknown;
  /** Sound ids in the bundle to sound ids in this library. */
  asset_map: Record<string, number>;
  missing: string[];
  extracted: number;
}

/** The channels a sound is tagged for, in mixer order. */
export function assetChannels(asset: Asset): Channel[] {
  const found = new Set(asset.tags.map((t) => channelOfTag(t.name)).filter(Boolean) as Channel[]);
  return CHANNELS.map((c) => c.id).filter((id) => found.has(id));
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

export interface QueuedState {
  id: number;
  asset_id: number;
  name: string;
  duration_ms: number | null;
  looping: boolean;
}

export interface ChannelState {
  channel: Channel;
  volume: number;
  muted: boolean;
  fade_ms: number;
  loop_queue: boolean;
  solo: boolean;
  queue: QueuedState[];
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
  /** Held where it is until resumed. */
  paused: boolean;
  trim_db: number;
  tag: string | null;
}

export interface EmitterState {
  id: number;
  name: string;
  min_ms: number;
  max_ms: number;
  tag: string | null;
}

export interface PlayOptions {
  looping?: boolean;
  interrupt?: boolean;
  /** This use's own volume in dB, on top of loudness levelling. */
  trimDb?: number;
  /** Playback speed; also shifts pitch. */
  rate?: number;
  pan?: number;
  /** Lower the music while this plays. */
  duck?: boolean;
  tag?: string;
}

export type Quantize = "now" | "beat" | "bar" | "phrase" | "loop";

export interface SetState {
  id: number;
  name: string;
  tiers: string[];
  tier: number;
  /** A tier change waiting for its beat. */
  pending: number | null;
  bpm: number;
  beats_per_bar: number;
  beat: number;
  bar: number;
}

export interface EngineState {
  device_ok: boolean;
  device_error: string | null;
  master_volume: number;
  channels: ChannelState[];
  sounds: SoundState[];
  panic_armed: boolean;
  last_error: string | null;
  emitters: EmitterState[];
  scene: { id: number; name: string } | null;
  ducked: boolean;
  duck_db: number;
  /** Peak levels: music, ambience, sfx, stingers, master. */
  levels: [number, number, number, number, number];
  music_set: SetState | null;
  profile: AudioProfile;
}

export type PanicOutcome = "idle" | "fading" | "cut";

export interface Doc<T> {
  id: number;
  data: T;
}

export const api = {
  folders: () => invoke<Folder[]>("library_folders"),
  addFolder: (path: string) => invoke<Folder>("library_add_folder", { path }),
  removeFolder: (id: number) => invoke<void>("library_remove_folder", { id }),
  rescan: (folderId?: number) => invoke<void>("library_rescan", { folderId: folderId ?? null }),
  addStarterSounds: () => invoke<void>("library_add_starter_sounds"),
  search: (text: string, folderId: number | null, tags: string[], limit: number) =>
    invoke<SearchResult>("library_search", {
      query: { text, folder_id: folderId, tags, limit },
    }),
  scanStatus: () => invoke<ScanStatus>("library_status"),
  setTags: (assetId: number, tags: Tag[]) => invoke<Asset>("library_set_tags", { assetId, tags }),
  setTagOn: (assetIds: number[], tag: Tag, on: boolean) =>
    invoke<void>("library_set_tag_on", { assetIds, tag, on }),
  assets: (ids: number[]) => invoke<Asset[]>("library_assets", { ids }),

  uiGet: (key: string) => invoke<string | null>("ui_get", { key }),
  uiSet: (key: string, value: string) => invoke<void>("ui_set", { key, value }),
  docList: <T>(kind: string) => invoke<Doc<T>[]>("doc_list", { kind }),
  docSave: <T>(kind: string, id: number | null, data: T) => invoke<number>("doc_save", { kind, id, data }),
  docDelete: (id: number) => invoke<void>("doc_delete", { id }),

  engineState: () => invoke<EngineState>("engine_state"),
  play: (assetId: number, channel: Channel, options: PlayOptions = {}) =>
    invoke<number>("engine_play", { assetId, channel, options }),
  preload: (assetIds: number[]) => invoke<void>("engine_preload", { assetIds }),
  sceneApply: (id: number, data: unknown) => invoke<string[]>("scene_apply", { id, data }),
  setPlay: (id: number, data: unknown, tier?: number) => invoke<string[]>("set_play", { id, data, tier: tier ?? null }),
  setTier: (tier: number, quantize?: Quantize) => invoke<void>("set_tier", { tier, quantize: quantize ?? null }),
  setStep: (delta: number) => invoke<void>("set_step", { delta }),
  stingerPlay: (assetId: number, options: { quantize?: Quantize; thenTier?: number | null; duck?: boolean; tag?: string } = {}) =>
    invoke<number>("stinger_play", { assetId, ...options }),
  setSoundTrim: (id: number, trimDb: number) => invoke<void>("engine_set_sound_trim", { id, trimDb }),
  setDuckDb: (db: number) => invoke<void>("engine_set_duck_db", { db }),
  stopSound: (id: number) => invoke<void>("engine_stop_sound", { id }),
  stopChannel: (channel: Channel, fadeMs?: number) => invoke<void>("engine_stop_channel", { channel, fadeMs: fadeMs ?? null }),
  outputDevices: () => invoke<{ devices: string[]; selected: string | null }>("engine_output_devices"),
  setOutputDevice: (name: string | null) => invoke<void>("engine_set_output_device", { name }),
  setChannel: (channel: Channel, change: { volume?: number; muted?: boolean; fadeMs?: number; solo?: boolean }) =>
    invoke<void>("engine_set_channel", { channel, ...change }),
  setMaster: (volume: number) => invoke<void>("engine_set_master", { volume }),
  setSoundLooping: (id: number, looping: boolean) => invoke<void>("engine_set_sound_looping", { id, looping }),
  setLoopQueue: (on: boolean) => invoke<void>("engine_set_loop_queue", { on }),
  nextTrack: () => invoke<void>("engine_next_track"),
  playQueuedNow: (id: number) => invoke<void>("engine_play_queued_now", { id }),
  moveToChannel: (id: number, channel: Channel) => invoke<void>("engine_move_to_channel", { id, channel }),
  requeuePlaying: (id: number, to: number) => invoke<void>("engine_requeue_playing", { id, to }),
  setAlwaysLoop: (assetId: number, on: boolean) => invoke<Asset>("library_set_always_loop", { assetId, on }),
  setEdit: (assetId: number, edit: AssetEdit | null) => invoke<Asset>("library_set_edit", { assetId, edit }),
  /** [start, end] of the audible part in ms; end is null at the end of the file. */
  audiblePart: (assetId: number) => invoke<[number, number | null]>("library_audible_part", { assetId }),
  trimSilence: (assetIds: number[]) => invoke<number>("library_trim_silence", { assetIds }),
  waveform: (assetId: number, buckets: number) => invoke<number[]>("library_waveform", { assetId, buckets }),
  /** Plays once on the effects channel with an unsaved trim, from `fromMs`. */
  preview: (assetId: number, edit: AssetEdit | null, fromMs: number | null) =>
    invoke<number>("engine_preview", { assetId, edit, fromMs }),
  swapWithPlaying: (id: number) => invoke<void>("engine_swap_with_playing", { id }),
  pauseSound: (id: number, paused: boolean, fadeMs: number) => invoke<void>("engine_pause", { id, paused, fadeMs }),
  youtubeImport: (url: string, channelTag: string | null) => invoke<Asset | null>("youtube_import", { url, channelTag }),
  appVersion: () => invoke<string>("app_version"),
  freesoundSignInUrl: () => invoke<string>("freesound_sign_in_url"),
  freesoundRemoveKey: () => invoke<FreesoundStatus>("freesound_remove_key"),
  moveQueued: (id: number, to: number) => invoke<void>("engine_move_queued", { id, to }),
  seek: (id: number, positionMs: number) => invoke<void>("engine_seek", { id, positionMs }),
  setTrim: (assetId: number, trimDb: number) => invoke<Asset>("library_set_trim", { assetId, trimDb }),
  credits: () => invoke<string[]>("library_credits"),
  revealFile: (path: string) => invoke<void>("reveal_file", { path }),
  setProfile: (profile: AudioProfile) => invoke<void>("engine_set_profile", { profile }),

  freesoundStatus: () => invoke<FreesoundStatus>("freesound_status"),
  freesoundSetCredentials: (clientId: string, apiKey: string) =>
    invoke<FreesoundStatus>("freesound_set_credentials", { clientId, apiKey }),
  freesoundSearch: (query: string, page: number) => invoke<FreesoundPage>("freesound_search", { query, page }),
  freesoundOpenSignIn: () => invoke<void>("freesound_open_sign_in"),
  freesoundSignIn: (code: string) => invoke<FreesoundStatus>("freesound_sign_in", { code }),
  freesoundSignOut: () => invoke<FreesoundStatus>("freesound_sign_out"),
  freesoundDownload: (sound: FreesoundSound, channelTag: string | null) =>
    invoke<{ asset: Asset | null; original: boolean }>("freesound_download", { sound, channelTag }),
  freesoundOpenPage: (url: string) => invoke<void>("freesound_open_page", { url }),

  bundleExport: (path: string, data: unknown, assetIds: number[], includeAudio: boolean) =>
    invoke<{ sounds: number; skipped: string[]; bytes: number }>("bundle_export", { path, data, assetIds, includeAudio }),
  bundleImport: (path: string) => invoke<ImportReport>("bundle_import", { path }),

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
