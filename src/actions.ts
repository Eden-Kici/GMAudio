// Things the GM does with one press: switch scene, hit a pad, use a hotkey.

import { register, unregisterAll } from "@tauri-apps/plugin-global-shortcut";
import { api, type Doc } from "./api";
import { useDocs } from "./docs";
import { message, useStore } from "./store";
import type { Action, Board, Hotkey, Pad, Scene } from "./types";

const notify = (e: unknown) => useStore.getState().notify(message(e));

// ---- scenes ------------------------------------------------------------------

export async function activateScene(scene: Doc<Scene>) {
  try {
    const skipped = await api.sceneApply(scene.id, scene.data);
    if (skipped.length === 1) notify(skipped[0]);
    else if (skipped.length > 1) notify(`${skipped.length} sounds in “${scene.data.name}” could not be played. ${skipped[0]}`);
  } catch (e) {
    notify(e);
  }
}

/** What is playing right now, as a scene the GM can keep. */
export function sceneFromCurrentMix(name: string): Scene {
  const engine = useStore.getState().engine;
  const docs = useDocs.getState();
  const active = docs.scenes.find((s) => s.id === engine?.scene?.id);
  const music = engine?.sounds.filter((s) => s.channel === "music" && !s.stopping).map((s) => s.asset_id) ?? [];
  const queued = engine?.channels.find((c) => c.channel === "music")?.queue.map((q) => q.asset_id) ?? [];
  const tracks = [...music, ...queued];
  return {
    name,
    color: active?.data.color ?? "#f0a63a",
    transition_ms: active?.data.transition_ms ?? 3000,
    music: tracks.length > 0 ? { kind: "tracks", asset_ids: tracks } : { kind: "keep" },
    layers:
      engine?.sounds
        .filter((s) => s.channel === "ambience" && s.looping && !s.stopping)
        .map((s) => ({ asset_id: s.asset_id, volume_db: Math.round(s.trim_db) })) ?? [],
    // Random layers cannot be read back from the engine; carry over the active scene's.
    emitters: engine && engine.emitters.length > 0 ? (active?.data.emitters ?? []) : [],
    board_id: active?.data.board_id ?? null,
    campaign_id: active?.data.campaign_id ?? null,
  };
}

// ---- pads --------------------------------------------------------------------

export const padTag = (board: number, pad: number) => `pad:${board}:${pad}`;
const lastPick = new Map<string, number>();

/**
 * Presses (`down`) or releases (`up`) a pad.
 * One-shots fire on down. Loop pads toggle on down. Hold pads sound while down.
 */
export async function triggerPad(board: number, index: number, pad: Pad, phase: "down" | "up") {
  if (pad.asset_ids.length === 0) return;
  const tag = padTag(board, index);
  const playing = useStore.getState().engine?.sounds.filter((s) => s.tag === tag && !s.stopping) ?? [];
  try {
    if (phase === "up") {
      if (pad.mode === "hold") await Promise.all(playing.map((s) => api.stopSound(s.id)));
      return;
    }
    if (pad.mode === "loop" && playing.length > 0) {
      await Promise.all(playing.map((s) => api.stopSound(s.id)));
      return;
    }
    if (pad.mode === "hold" && playing.length > 0) return; // key repeat

    // From a pool, never the same sound twice running.
    let pick = Math.floor(Math.random() * pad.asset_ids.length);
    if (pad.asset_ids.length > 1 && pick === lastPick.get(tag)) pick = (pick + 1) % pad.asset_ids.length;
    lastPick.set(tag, pick);

    const semitones = (Math.random() * 2 - 1) * pad.pitch_var;
    await api.play(pad.asset_ids[pick], pad.channel, {
      looping: pad.mode !== "oneshot",
      trimDb: pad.volume_db - Math.random() * pad.volume_var,
      rate: Math.pow(2, semitones / 12),
      duck: pad.duck,
      tag,
    });
  } catch (e) {
    notify(e);
  }
}

/** Decodes every pad's sounds ahead of time so they start instantly. */
export function preloadBoards(boards: Doc<Board>[]) {
  const ids = [...new Set(boards.flatMap((b) => b.data.pads.flatMap((p) => p.asset_ids)))];
  if (ids.length > 0) void api.preload(ids).catch(() => {});
}

// ---- hotkeys -----------------------------------------------------------------

const MODIFIER_CODES = new Set(["ControlLeft", "ControlRight", "ShiftLeft", "ShiftRight", "AltLeft", "AltRight", "MetaLeft", "MetaRight"]);

/** "Ctrl+Shift+K" for a key press, or null for a bare modifier. */
export function comboOf(e: KeyboardEvent): string | null {
  if (MODIFIER_CODES.has(e.code) || !e.code) return null;
  let key = e.code;
  if (key.startsWith("Key")) key = key.slice(3);
  else if (key.startsWith("Digit")) key = key.slice(5);
  const parts = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  parts.push(key);
  return parts.join("+");
}

/** Keys that mean something while typing, so they need a modifier to be hotkeys there. */
const isPlainTypingKey = (combo: string) => !combo.includes("+") && !/^F\d+$/.test(combo);

/** Board shown right now; number keys press its first ten pads. */
let visibleBoard: Doc<Board> | null = null;
export const setVisibleBoard = (board: Doc<Board> | null) => {
  visibleBoard = board;
};

/** The built-in keys, used when no custom hotkey claims the combo. */
export const DEFAULT_PAD_KEYS = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];

export function defaultAction(combo: string): Action | null {
  const pad = DEFAULT_PAD_KEYS.indexOf(combo);
  if (pad >= 0 && visibleBoard && pad < visibleBoard.data.pads.length) {
    return { type: "pad", board: visibleBoard.id, pad };
  }
  const fn = /^F(\d+)$/.exec(combo);
  if (fn) {
    const scene = useDocs.getState().scenes[Number(fn[1]) - 1];
    if (scene) return { type: "scene", id: scene.id };
  }
  return null;
}

/** Hooks other parts of the app fill in (cue list, dynamic music). */
export const handlers: { go?: () => void; tier?: (delta: number) => void } = {};

export function runAction(action: Action, phase: "down" | "up" = "down") {
  const docs = useDocs.getState();
  if (action.type === "pad") {
    const board = docs.boards.find((b) => b.id === action.board);
    const pad = board?.data.pads[action.pad];
    if (board && pad) void triggerPad(board.id, action.pad, pad, phase);
    return;
  }
  if (phase === "up") return;
  switch (action.type) {
    case "scene": {
      const scene = docs.scenes.find((s) => s.id === action.id);
      if (scene) void activateScene(scene);
      break;
    }
    case "panic":
      void useStore.getState().panic();
      break;
    case "next_track":
      void api.nextTrack().catch(notify);
      break;
    case "stop_channel":
      void useStore.getState().stopChannel(action.channel);
      break;
    case "go":
      handlers.go?.();
      break;
    case "tier":
      handlers.tier?.(action.delta);
      break;
  }
}

export function describeAction(action: Action): string {
  const docs = useDocs.getState();
  switch (action.type) {
    case "scene":
      return `Scene: ${docs.scenes.find((s) => s.id === action.id)?.data.name ?? "(deleted)"}`;
    case "pad": {
      const board = docs.boards.find((b) => b.id === action.board);
      const pad = board?.data.pads[action.pad];
      return `Pad: ${pad?.label || "(deleted)"}${board ? ` on ${board.data.name}` : ""}`;
    }
    case "panic":
      return "Stop all";
    case "next_track":
      return "Next music track";
    case "go":
      return "GO: fire the next cue";
    case "tier":
      return action.delta > 0 ? "Music intensity up" : "Music intensity down";
    case "stop_channel":
      return `Stop ${action.channel}`;
  }
}

const sameAction = (a: Action, b: Action) => JSON.stringify(a) === JSON.stringify(b);

export function hotkeyFor(action: Action): Hotkey | undefined {
  return useDocs.getState().hotkeys.find((h) => sameAction(h.action, action));
}

/** Gives an action a key (or none). A key can only do one thing. */
export function setHotkey(action: Action, combo: string | null, global = false) {
  const docs = useDocs.getState();
  const rest = docs.hotkeys.filter((h) => !sameAction(h.action, action) && h.combo !== combo);
  docs.setHotkeys(combo ? [...rest, { combo, global, action }] : rest);
}

/** Handles a key event inside the app. Returns true when it did something. */
export function handleKey(e: KeyboardEvent, phase: "down" | "up", typing: boolean): boolean {
  const combo = comboOf(e);
  if (!combo) return false;
  if (typing && isPlainTypingKey(combo)) return false;
  const custom = useDocs.getState().hotkeys.find((h) => h.combo === combo);
  // A global hotkey also fires through the system while the app is focused.
  if (custom?.global && registered.has(combo)) return true;
  const action = custom?.action ?? defaultAction(combo);
  if (!action) return false;
  if (phase === "down" && e.repeat && action.type !== "pad") return true;
  runAction(action, phase);
  return true;
}

let registered = new Set<string>();

/** (Re)registers the hotkeys that work system-wide. */
export async function syncGlobalHotkeys(hotkeys: Hotkey[]) {
  try {
    await unregisterAll();
  } catch {
    // Nothing registered yet.
  }
  registered = new Set();
  for (const hotkey of hotkeys.filter((h) => h.global)) {
    try {
      await register(hotkey.combo, (event) => {
        runAction(hotkey.action, event.state === "Released" ? "up" : "down");
      });
      registered.add(hotkey.combo);
    } catch {
      notify(`${hotkey.combo} could not be made a system-wide hotkey. Another program may be using it. It still works inside GMAudio.`);
    }
  }
}
