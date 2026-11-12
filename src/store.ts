import { create } from "zustand";
import {
  api,
  events,
  type Asset,
  type Channel,
  type EngineState,
  type Folder,
  type ScanStatus,
  type Tag,
} from "./api";

/** The search asks for this many rows; the total is shown separately. */
export const RESULT_LIMIT = 300;

interface Store {
  engine: EngineState | null;
  scan: ScanStatus;
  folders: Folder[];
  foldersLoaded: boolean;

  query: string;
  folderFilter: number | null;
  results: Asset[];
  total: number;
  /** Every non-missing sound in the library, ignoring the search. */
  librarySize: number;
  selectedId: number | null;

  /** One message at a time, shown as a toast. */
  notice: { text: string; kind: "error" | "info"; id: number } | null;

  init: () => Promise<() => void>;
  setQuery: (text: string) => void;
  setFolderFilter: (id: number | null) => void;
  select: (id: number | null) => void;
  refresh: () => Promise<void>;
  addFolder: (path: string) => Promise<void>;
  removeFolder: (id: number) => Promise<void>;
  rescan: (id?: number) => Promise<void>;
  setTags: (assetId: number, tags: Tag[]) => Promise<void>;

  play: (assetId: number, channel: Channel) => Promise<void>;
  stopSound: (id: number) => Promise<void>;
  stopChannel: (channel: Channel) => Promise<void>;
  panic: () => Promise<void>;
  retryDevice: () => Promise<void>;

  notify: (text: string, kind?: "error" | "info") => void;
  dismissNotice: () => void;
}

const idleScan: ScanStatus = { scanning: null, files_found: 0, analyzed: 0, remaining: 0, failed: 0 };

let searchSeq = 0;
let searchTimer: ReturnType<typeof setTimeout> | undefined;
let noticeSeq = 0;

function message(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}

export const useStore = create<Store>((set, get) => {
  /** Runs the search for the current query. Stale answers are dropped. */
  const runSearch = async () => {
    const seq = ++searchSeq;
    const { query, folderFilter } = get();
    try {
      const [found, all] = await Promise.all([
        api.search(query, folderFilter, RESULT_LIMIT),
        api.search("", null, 1),
      ]);
      if (seq !== searchSeq) return;
      set({ results: found.assets, total: found.total, librarySize: all.total });
    } catch (e) {
      get().notify(message(e));
    }
  };

  const guard = async (action: () => Promise<unknown>) => {
    try {
      await action();
    } catch (e) {
      get().notify(message(e));
    }
  };

  return {
    engine: null,
    scan: idleScan,
    folders: [],
    foldersLoaded: false,
    query: "",
    folderFilter: null,
    results: [],
    total: 0,
    librarySize: 0,
    selectedId: null,
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
        const [engine, scan] = await Promise.all([api.engineState(), api.scanStatus()]);
        set({ engine, scan });
      });
      await get().refresh();
      return () => unlisten.forEach((off) => off());
    },

    setQuery: (text) => {
      set({ query: text });
      clearTimeout(searchTimer);
      // A short pause keeps typing smooth; the search itself takes ~1 ms.
      searchTimer = setTimeout(() => void runSearch(), 40);
    },

    setFolderFilter: (id) => {
      set({ folderFilter: id });
      void runSearch();
    },

    select: (id) => set({ selectedId: id }),

    refresh: async () => {
      await guard(async () => {
        const folders = await api.folders();
        const filter = get().folderFilter;
        set({
          folders,
          foldersLoaded: true,
          folderFilter: folders.some((f) => f.id === filter) ? filter : null,
        });
      });
      await runSearch();
    },

    addFolder: (path) =>
      guard(async () => {
        await api.addFolder(path);
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
        const updated = await api.setTags(assetId, tags);
        set({ results: get().results.map((a) => (a.id === assetId ? updated : a)) });
      }),

    play: (assetId, channel) => guard(() => api.play(assetId, channel)),
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
