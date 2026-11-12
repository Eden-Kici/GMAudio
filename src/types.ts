// The things a GM builds. Each is saved as one JSON document; the Rust
// side reads only the parts that decide what is played.

import type { Channel } from "./api";

export type SceneMusic =
  | { kind: "keep" }
  | { kind: "silence" }
  | { kind: "tracks"; asset_ids: number[] }
  | { kind: "set"; set_id: number; tier: number };

export interface Layer {
  asset_id: number;
  volume_db: number;
}

/** A sound that plays every so often: distant thunder, a creak. */
export interface RandomLayer {
  name: string;
  asset_ids: number[];
  min_s: number;
  max_s: number;
  volume_db: number;
  random_pan: boolean;
}

export interface Scene {
  name: string;
  color: string;
  transition_ms: number;
  music: SceneMusic;
  layers: Layer[];
  emitters: RandomLayer[];
  /** The soundboard shown while this scene is active. */
  board_id: number | null;
  /** The campaign this scene belongs to. `null`: shared by all. */
  campaign_id: number | null;
}

export type PadMode = "oneshot" | "loop" | "hold";

export interface Pad {
  label: string;
  /** One sound, or several to pick from at random. */
  asset_ids: number[];
  mode: PadMode;
  channel: Channel;
  volume_db: number;
  /** Lower the music while it plays. */
  duck: boolean;
  /** Random pitch change per press, in semitones either way. */
  pitch_var: number;
  /** Random volume drop per press, in dB. */
  volume_var: number;
  color: string;
}

export interface Board {
  name: string;
  columns: number;
  pads: Pad[];
  campaign_id: number | null;
}

/** What a hotkey does. */
export type Action =
  | { type: "scene"; id: number }
  | { type: "pad"; board: number; pad: number }
  | { type: "panic" }
  | { type: "next_track" }
  | { type: "go" }
  | { type: "tier"; delta: number }
  | { type: "stop_channel"; channel: Channel };

export interface Hotkey {
  /** e.g. "Ctrl+Shift+K", "F5", "Numpad1". */
  combo: string;
  /** Works even when GMAudio is not the active window. */
  global: boolean;
  action: Action;
}

export const COLORS = ["#f0a63a", "#e5704a", "#f2708a", "#a78bfa", "#6aa5f0", "#4fc4b0", "#8fc45a", "#a39a8a"];

export const newScene = (name: string): Scene => ({
  name,
  color: COLORS[0],
  transition_ms: 3000,
  music: { kind: "keep" },
  layers: [],
  emitters: [],
  board_id: null,
  campaign_id: null,
});

export const newPad = (): Pad => ({
  label: "",
  asset_ids: [],
  mode: "oneshot",
  channel: "sfx",
  volume_db: 0,
  duck: false,
  pitch_var: 0,
  volume_var: 0,
  color: COLORS[3],
});

export const newBoard = (name: string): Board => ({ name, columns: 5, pads: [], campaign_id: null });
