import { create } from "zustand";
import {
  api,
  channelInfo,
  events,
  type Asset,
  type Channel,
  type EngineState,
  type Folder,
  type ScanStatus,
  type Tag,
} from "./api";

/** Each pane asks for this many rows; the total is shown separately. */
export const RESULT_LIMIT = 300;
export const MAX_PANES = 4;

/** One library pane: its own search, channel filter and folder. */
export interface Pane {
  id: number;
  query: string;
  /** Only sounds tagged for this channel. `null` shows everything. */
  channel: Channel | null;
  folderId: number | null;
  results: Asset[];
  total: number;
}

interface SavedPane {
  query: string;
  channel: Channel | null;
  folderId: number | null;
}

export interface MenuState {
  x: number;
  y: number;
  asset: Asset;
}

interface Store {
  engine: EngineState | null;
  scan: ScanStatus;
  folders: Folder[];
  foldersLoaded: boolean;

  panes: Pane[];
  /** The pane the sidebar and the search shortcut act on. */
  activePane: number;
  /** Every non-missing sound in the library, ignoring the search. */
  librarySize: number;
  selectedId: number | null;
  /** Sounds ticked for tagging several at once. */
  picked: number[];
  /** The right-click menu, when open. */
  menu: MenuState | null;
  /** A sound being dragged towards the mixer. */
  dragging: number | null;

  /** One message at a time, shown as a toast. */
  notice: { text: string; kind: "error" | "info"; id: number } | null;

  init: () => Promise<() => void>;
  setQuery: (pane: number, text: string) => void;
  setPaneChannel: (pane: number, channel: Channel | null) => void;
  setPaneFolder: (pane: number, folderId: number | null) => void;
  setActivePane: (pane: number) => void;
  splitPane: (after: number) => void;
  closePane: (pane: number) => void;
  select: (id: number | null) => void;
  /** Ticks or unticks one sound. With `range`, everything from the last ticked one to it. */
  pick: (pane: number, id: number, range?: boolean) => void;
  pickAll: (pane: number) => void;
  clearPicked: () => void;
  /** Adds a tag to, or removes it from, every ticked sound. */
  tagPicked: (tag: Tag, on: boolean) => Promise<void>;
  openMenu: (menu: MenuState | null) => void;
  setDragging: (assetId: number | null) => void;
  refresh: () => Promise<void>;
  addFolder: (path: string) => Promise<void>;
  addStarterSounds: () => Promise<void>;
  removeFolder: (id: number) => Promise<void>;
  rescan: (id?: number) => Promise<void>;
  setTags: (assetId: number, tags: Tag[]) => Promise<void>;
  toggleChannelTag: (asset: Asset, channel: Channel) => Promise<void>;

  play: (assetId: number, channel: Channel, interrupt?: boolean) => Promise<void>;
  stopSound: (id: number) => Promise<void>;
  stopChannel: (channel: Channel) => Promise<void>;
  panic: () => Promise<void>;
  retryDevice: () => Promise<void>;

  notify: (text: string, kind?: "error" | "info") => void;
  dismissNotice: () => void;
}

const idleScan: ScanStatus = { scanning: null, files_found: 0, analyzed: 0, remaining: 0, failed: 0 };

let paneSeq = 1;
const newPane = (from?: Partial<SavedPane>): Pane => ({
  id: paneSeq++,
  query: from?.query ?? "",
  channel: from?.channel ?? null,
  folderId: from?.folderId ?? null,
  results: [],
  total: 0,
});

const searchSeq = new Map<number, number>();
const searchTimers = new Map<number, ReturnType<typeof setTimeout>>();
let saveTimer: ReturnType<typeof setTimeout> | undefined;
let noticeSeq = 0;

export function message(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}

export const useStore = create<Store>((set, get) => {
  const patchPane = (id: number, change: Partial<Pane>) =>
    set({ panes: get().panes.map((p) => (p.id === id ? { ...p, ...change } : p)) });

  /** Runs one pane's search. Stale answers are dropped. */
  const runSearch = async (id: number) => {
    const pane = get().panes.find((p) => p.id === id);
    if (!pane) return;
    const seq = (searchSeq.get(id) ?? 0) + 1;
    searchSeq.set(id, seq);
    try {
      const tags = pane.channel ? [channelInfo(pane.channel).tag] : [];
      const found = await api.search(pane.query, pane.folderId, tags, RESULT_LIMIT);
      if (searchSeq.get(id) !== seq) return;
      patchPane(id, { results: found.assets, total: found.total });
    } catch (e) {
      get().notify(message(e));
    }
  };

  const runAllSearches = async () => {
    await Promise.all(get().panes.map((p) => runSearch(p.id)));
    try {
      const all = await api.search("", null, [], 1);
      set({ librarySize: all.total });
    } catch (e) {
      get().notify(message(e));
    }
  };

  /** Remembers the pane layout for the next launch. */
  const saveLayout = () => {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      const saved: SavedPane[] = get().panes.map(({ query, channel, folderId }) => ({ query, channel, folderId }));
      void api.uiSet("panes", JSON.stringify(saved)).catch(() => {});
    }, 400);
  };

  const guard = async (action: () => Promise<unknown>) => {
    try {
      await action();
    } catch (e) {
      get().notify(message(e));
    }
  };

  const first = newPane();

  return {
    engine: null,
    scan: idleScan,
    folders: [],
    foldersLoaded: false,
    panes: [first],
    activePane: first.id,
    librarySize: 0,
    selectedId: null,
    picked: [],
    menu: null,
    dragging: null,
    notice: null,

    init: async () => {
      const unlisten = await Promise.all([
        events.engineState((engine) => {
          const previous = get().engine?.last_error;
          set({ engine });
          if (engine.last_error && engine.last_error !== previous) {
            get().notify(engine.last_error);
          }
        }),
        events.scanProgress((scan) => set({ scan })),
        events.libraryChanged(() => void get().refresh()),
      ]);
      await guard(async () => {
        const [engine, scan, layout] = await Promise.all([api.engineState(), api.scanStatus(), api.uiGet("panes")]);
        set({ engine, scan });
        if (layout) {
          const saved = (JSON.parse(layout) as SavedPane[]).slice(0, MAX_PANES);
          if (saved.length > 0) {
            const panes = saved.map((p) => newPane(p));
            set({ panes, activePane: panes[0].id });
          }
        }
      });
      await get().refresh();
      return () => unlisten.forEach((off) => off());
    },

    setQuery: (pane, text) => {
      patchPane(pane, { query: text });
      clearTimeout(searchTimers.get(pane));
      // A short pause keeps typing smooth; the search itself takes ~1 ms.
      searchTimers.set(
        pane,
        setTimeout(() => void runSearch(pane), 40),
      );
      saveLayout();
    },

    setPaneChannel: (pane, channel) => {
      patchPane(pane, { channel });
      void runSearch(pane);
      saveLayout();
    },

    setPaneFolder: (pane, folderId) => {
      patchPane(pane, { folderId });
      void runSearch(pane);
      saveLayout();
    },

    setActivePane: (pane) => {
      if (get().activePane !== pane) set({ activePane: pane });
    },

    splitPane: (after) => {
      const panes = get().panes;
      if (panes.length >= MAX_PANES) return;
      const at = panes.findIndex((p) => p.id === after);
      const source = panes[at];
      const added = newPane({ folderId: source?.folderId ?? null });
      set({ panes: [...panes.slice(0, at + 1), added, ...panes.slice(at + 1)], activePane: added.id });
      void runSearch(added.id);
      saveLayout();
    },

    closePane: (pane) => {
      const panes = get().panes.filter((p) => p.id !== pane);
      if (panes.length === 0) return;
      set({ panes, activePane: get().activePane === pane ? panes[0].id : get().activePane });
      saveLayout();
    },

    select: (id) => set({ selectedId: id }),

    pick: (pane, id, range) => {
      const picked = get().picked;
      const rows = get().panes.find((p) => p.id === pane)?.results.map((a) => a.id) ?? [];
      const last = picked[picked.length - 1];
      const from = rows.indexOf(last);
      const to = rows.indexOf(id);
      if (range && from >= 0 && to >= 0) {
        const span = rows.slice(Math.min(from, to), Math.max(from, to) + 1);
        // Keep the clicked row last, so the next range starts from it.
        set({ picked: [...picked.filter((x) => !span.includes(x)), ...span.filter((x) => x !== id), id] });
      } else {
        set({ picked: picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id] });
      }
    },

    pickAll: (pane) => {
      const rows = get().panes.find((p) => p.id === pane)?.results.map((a) => a.id) ?? [];
      const picked = get().picked;
      set({ picked: [...picked, ...rows.filter((id) => !picked.includes(id))] });
    },

    clearPicked: () => set({ picked: [] }),

    tagPicked: (tag, on) =>
      guard(async () => {
        await api.setTagOn(get().picked, tag, on);
        await runAllSearches();
      }),
    openMenu: (menu) => set({ menu }),
    setDragging: (assetId) => set({ dragging: assetId }),

    refresh: async () => {
      await guard(async () => {
        const folders = await api.folders();
        const known = new Set(folders.map((f) => f.id));
        set({
          folders,
          foldersLoaded: true,
          // A pane looking at a folder that was removed shows everything again.
          panes: get().panes.map((p) => (p.folderId !== null && !known.has(p.folderId) ? { ...p, folderId: null } : p)),
        });
      });
      await runAllSearches();
    },

    addFolder: (path) =>
      guard(async () => {
        await api.addFolder(path);
        await get().refresh();
      }),

    addStarterSounds: () =>
      guard(async () => {
        await api.addStarterSounds();
        await get().refresh();
      }),

    removeFolder: (id) =>
      guard(async () => {
        await api.removeFolder(id);
        await get().refresh();
      }),

    rescan: (id) => guard(() => api.rescan(id)),

    setTags: (assetId, tags) =>
      guard(async () => {
        await api.setTags(assetId, tags);
        // Other panes may show (or filter on) the same sound.
        await runAllSearches();
      }),

    toggleChannelTag: (asset, channel) => {
      const info = channelInfo(channel);
      const has = asset.tags.some((t) => t.name.toLowerCase() === info.tag.toLowerCase());
      const tags = has
        ? asset.tags.filter((t) => t.name.toLowerCase() !== info.tag.toLowerCase())
        : [...asset.tags, { name: info.tag, category: "channel" }];
      return get().setTags(asset.id, tags);
    },

    play: (assetId, channel, interrupt) => guard(() => api.play(assetId, channel, { interrupt })),
    stopSound: (id) => guard(() => api.stopSound(id)),
    stopChannel: (channel) => guard(() => api.stopChannel(channel)),
    panic: () => guard(() => api.panic()),
    retryDevice: () => guard(() => api.retryDevice()),

    notify: (text, kind = "error") => set({ notice: { text, kind, id: ++noticeSeq } }),
    dismissNotice: () => set({ notice: null }),
  };
});

/**
 * Which channels each asset is currently playing on, as a string that only
 * changes when that set changes. Lets list rows ignore the 30 Hz position
 * updates.
 */
export function playingKey(engine: EngineState | null): string {
  if (!engine) return "";
  return engine.sounds
    .filter((s) => !s.stopping)
    .map((s) => `${s.asset_id}:${s.channel}`)
    .sort()
    .join(",");
}
