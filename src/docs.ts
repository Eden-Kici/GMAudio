// Scenes, boards and other saved documents, plus interface preferences.

import { create } from "zustand";
import { api, events, searchPrefs, type Asset, type Doc } from "./api";
import { message, useStore } from "./store";
import { useMemo } from "react";
import type { Board, Campaign, Hotkey, MusicSet, Scene, Session } from "./types";

export type View = "perform" | "library" | "scenes" | "boards" | "sets" | "cues" | "settings";

interface Kinds {
  scene: Scene;
  board: Board;
  set: MusicSet;
  campaign: Campaign;
  session: Session;
}
type Kind = keyof Kinds;
type ListKey = "scenes" | "boards" | "sets" | "campaigns" | "sessions";

const PLURAL: { [K in Kind]: ListKey } = { scene: "scenes", board: "boards", set: "sets", campaign: "campaigns", session: "sessions" };

interface DocsStore {
  loaded: boolean;
  scenes: Doc<Scene>[];
  boards: Doc<Board>[];
  sets: Doc<MusicSet>[];
  campaigns: Doc<Campaign>[];
  sessions: Doc<Session>[];
  /** The campaign and session being prepared or run. */
  campaignId: number | null;
  sessionId: number | null;
  /** First-run setup has been finished or skipped. */
  setupDone: boolean;
  hotkeys: Hotkey[];
  view: View;
  /** Show every control. Off: only what a new GM needs. */
  advanced: boolean;
  /** Under a renamed sound, show the file's own name. */
  showFileName: boolean;
  /** Searching also finds renamed sounds by their file's name. */
  searchFileNames: boolean;
  setShowFileName: (on: boolean) => void;
  setSearchFileNames: (on: boolean) => void;
  /** Sounds by id, for showing names in editors. */
  assets: Record<number, Asset>;

  load: () => Promise<void>;
  /** Creates (`id: null`) or updates a document. Returns its id. */
  save: <K extends Kind>(kind: K, id: number | null, data: Kinds[K]) => Promise<number>;
  remove: (kind: Kind, id: number) => Promise<void>;
  setView: (view: View) => void;
  setAdvanced: (on: boolean) => void;
  setHotkeys: (hotkeys: Hotkey[]) => void;
  selectSession: (campaignId: number | null, sessionId: number | null) => void;
  setSetupDone: (done: boolean) => void;
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
  sets: [],
  campaigns: [],
  sessions: [],
  campaignId: null,
  sessionId: null,
  setupDone: true,
  hotkeys: [],
  view: "library",
  advanced: false,
  showFileName: true,
  searchFileNames: true,
  assets: {},

  load: async () => {
    try {
      const [scenes, boards, sets, campaigns, sessions, scope, setup, hotkeys, view, advanced, showFileName, searchFileNames] = await Promise.all([
        api.docList<Scene>("scene"),
        api.docList<Board>("board"),
        api.docList<MusicSet>("set"),
        api.docList<Campaign>("campaign"),
        api.docList<Session>("session"),
        api.uiGet("scope"),
        api.uiGet("setup_done"),
        api.uiGet("hotkeys"),
        api.uiGet("view"),
        api.uiGet("advanced"),
        api.uiGet("show_file_name"),
        api.uiGet("search_file_names"),
      ]);
      searchPrefs.fileNames = searchFileNames !== "0";
      set({
        loaded: true,
        scenes,
        boards,
        sets,
        campaigns,
        sessions,
        ...restoreScope(scope, campaigns, sessions),
        setupDone: setup === "1",
        hotkeys: hotkeys ? (JSON.parse(hotkeys) as Hotkey[]) : [],
        view: (view as View | null) ?? "library",
        advanced: advanced === "1",
        showFileName: showFileName !== "0",
        searchFileNames: searchFileNames !== "0",
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

  setShowFileName: (showFileName) => {
    set({ showFileName });
    void api.uiSet("show_file_name", showFileName ? "1" : "0").catch(() => {});
  },

  setSearchFileNames: (searchFileNames) => {
    set({ searchFileNames });
    searchPrefs.fileNames = searchFileNames;
    void api.uiSet("search_file_names", searchFileNames ? "1" : "0").catch(() => {});
    void useStore.getState().refresh();
  },

  setAdvanced: (advanced) => {
    set({ advanced });
    void api.uiSet("advanced", advanced ? "1" : "0").catch(() => {});
  },

  selectSession: (campaignId, sessionId) => {
    set({ campaignId, sessionId });
    void api.uiSet("scope", JSON.stringify({ campaignId, sessionId })).catch(() => {});
  },

  setSetupDone: (setupDone) => {
    set({ setupDone });
    void api.uiSet("setup_done", setupDone ? "1" : "0").catch(() => {});
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

/** The saved campaign and session, if they still exist. */
function restoreScope(saved: string | null, campaigns: Doc<Campaign>[], sessions: Doc<Session>[]) {
  let campaignId: number | null = null;
  let sessionId: number | null = null;
  try {
    const scope = saved ? (JSON.parse(saved) as { campaignId: number | null; sessionId: number | null }) : null;
    if (scope && campaigns.some((c) => c.id === scope.campaignId)) {
      campaignId = scope.campaignId;
      if (sessions.some((s) => s.id === scope.sessionId && s.data.campaign_id === campaignId)) sessionId = scope.sessionId;
    }
  } catch {
    // A damaged setting just means starting with no campaign chosen.
  }
  return { campaignId, sessionId };
}

interface Scoped {
  campaign_id: number | null;
  session_id?: number | null;
}

/** Whether something shows up while this campaign and session are chosen. */
export const inScope = (item: Scoped, campaignId: number | null, sessionId: number | null) =>
  (item.campaign_id == null || item.campaign_id === campaignId) && (item.session_id == null || item.session_id === sessionId);

/** The scenes, boards or music sets of the chosen campaign and session, plus shared ones. */
export function useScoped<K extends "scenes" | "boards" | "sets">(key: K): DocsStore[K] {
  const all = useDocs((s) => s[key]);
  const campaignId = useDocs((s) => s.campaignId);
  const sessionId = useDocs((s) => s.sessionId);
  return useMemo(
    () => (all as Doc<Scoped>[]).filter((d) => inScope(d.data, campaignId, sessionId)) as DocsStore[K],
    [all, campaignId, sessionId],
  );
}

export function scoped<K extends "scenes" | "boards" | "sets">(key: K): DocsStore[K] {
  const s = useDocs.getState();
  return (s[key] as Doc<Scoped>[]).filter((d) => inScope(d.data, s.campaignId, s.sessionId)) as DocsStore[K];
}

/** The name of a sound, or a placeholder while it loads or if it is gone. */
export function assetName(assets: Record<number, Asset>, id: number): string {
  return assets[id]?.name ?? "…";
}
