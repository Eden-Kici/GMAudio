// Scenes, boards and other saved documents, plus interface preferences.

import { create } from "zustand";
import { api, events, type Asset, type Doc } from "./api";
import { message, useStore } from "./store";
import type { Board, Hotkey, Scene } from "./types";

export type View = "library" | "scenes" | "boards" | "settings";

interface Kinds {
  scene: Scene;
  board: Board;
}
type Kind = keyof Kinds;

const PLURAL: { [K in Kind]: "scenes" | "boards" } = { scene: "scenes", board: "boards" };

interface DocsStore {
  loaded: boolean;
  scenes: Doc<Scene>[];
  boards: Doc<Board>[];
  hotkeys: Hotkey[];
  view: View;
  /** Show every control. Off: only what a new GM needs. */
  advanced: boolean;
  /** Sounds by id, for showing names in editors. */
  assets: Record<number, Asset>;

  load: () => Promise<void>;
  /** Creates (`id: null`) or updates a document. Returns its id. */
  save: <K extends Kind>(kind: K, id: number | null, data: Kinds[K]) => Promise<number>;
  remove: (kind: Kind, id: number) => Promise<void>;
  setView: (view: View) => void;
  setAdvanced: (on: boolean) => void;
  setHotkeys: (hotkeys: Hotkey[]) => void;
  /** Makes sure these sounds are in `assets`. */
  needAssets: (ids: number[]) => void;
}

const notify = (e: unknown) => useStore.getState().notify(message(e));
const saveTimers = new Map<number, ReturnType<typeof setTimeout>>();
let requested = new Set<number>();

export const useDocs = create<DocsStore>((set, get) => ({
  loaded: false,
  scenes: [],
  boards: [],
  hotkeys: [],
  view: "library",
  advanced: false,
  assets: {},

  load: async () => {
    try {
      const [scenes, boards, hotkeys, view, advanced] = await Promise.all([
        api.docList<Scene>("scene"),
        api.docList<Board>("board"),
        api.uiGet("hotkeys"),
        api.uiGet("view"),
        api.uiGet("advanced"),
      ]);
      set({
        loaded: true,
        scenes,
        boards,
        hotkeys: hotkeys ? (JSON.parse(hotkeys) as Hotkey[]) : [],
        view: (view as View | null) ?? "library",
        advanced: advanced === "1",
      });
      // Sound names can change when files are renamed or rescanned.
      void events.libraryChanged(() => {
        const ids = Object.keys(get().assets).map(Number);
        requested = new Set();
        set({ assets: {} });
        get().needAssets(ids);
      });
    } catch (e) {
      notify(e);
    }
  },

  save: async (kind, id, data) => {
    const key = PLURAL[kind];
    if (id === null) {
      try {
        const created = await api.docSave(kind, null, data);
        set({ [key]: [...get()[key], { id: created, data }] } as Partial<DocsStore>);
        return created;
      } catch (e) {
        notify(e);
        return -1;
      }
    }
    // Show the change at once; write it shortly after, so dragging a
    // slider does not hit the disk on every step.
    set({
      [key]: (get()[key] as Doc<unknown>[]).map((d) => (d.id === id ? { id, data } : d)),
    } as Partial<DocsStore>);
    clearTimeout(saveTimers.get(id));
    saveTimers.set(
      id,
      setTimeout(() => void api.docSave(kind, id, data).catch(notify), 300),
    );
    return id;
  },

  remove: async (kind, id) => {
    const key = PLURAL[kind];
    clearTimeout(saveTimers.get(id));
    set({ [key]: (get()[key] as Doc<unknown>[]).filter((d) => d.id !== id) } as Partial<DocsStore>);
    try {
      await api.docDelete(id);
    } catch (e) {
      notify(e);
    }
  },

  setView: (view) => {
    set({ view });
    void api.uiSet("view", view).catch(() => {});
  },

  setAdvanced: (advanced) => {
    set({ advanced });
    void api.uiSet("advanced", advanced ? "1" : "0").catch(() => {});
  },

  setHotkeys: (hotkeys) => {
    set({ hotkeys });
    void api.uiSet("hotkeys", JSON.stringify(hotkeys)).catch(notify);
  },

  needAssets: (ids) => {
    const missing = ids.filter((id) => !requested.has(id));
    if (missing.length === 0) return;
    missing.forEach((id) => requested.add(id));
    void api
      .assets(missing)
      .then((found) => {
        const assets = { ...get().assets };
        for (const asset of found) assets[asset.id] = asset;
        set({ assets });
      })
      .catch(notify);
  },
}));

/** The name of a sound, or a placeholder while it loads or if it is gone. */
export function assetName(assets: Record<number, Asset>, id: number): string {
  return assets[id]?.name ?? "…";
}
