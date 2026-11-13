import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { Folder } from "../api";
import { folderName, plural } from "../format";
import { useStore } from "../store";
import { CloseIcon, FolderIcon, PlusIcon, RefreshIcon, SearchIcon, WarningIcon } from "./icons";
import { useSizes } from "../layout";
import { OnlineSounds } from "./Online";
import { Divider } from "./Resize";
import { YouTubeImport } from "./YouTube";

export async function pickFolder(): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false, title: "Choose a folder of music or sounds" });
  return typeof picked === "string" ? picked : null;
}

export function Sidebar() {
  const folders = useStore((s) => s.folders);
  const activePane = useStore((s) => s.activePane);
  const filter = useStore((s) => s.panes.find((p) => p.id === s.activePane)?.folderId ?? null);
  const librarySize = useStore((s) => s.librarySize);
  const setPaneFolder = useStore((s) => s.setPaneFolder);
  const addFolder = useStore((s) => s.addFolder);
  const addStarterSounds = useStore((s) => s.addStarterSounds);
  const setFilter = (id: number | null) => setPaneFolder(activePane, id);
  const hasStarter = folders.some((f) => /[\\/]Starter Sounds$/i.test(f.path));
  const [online, setOnline] = useState(false);
  const [[width], setWidth] = useSizes("sidebar", [256]);
  const [youtube, setYoutube] = useState(false);

  const add = async () => {
    const path = await pickFolder();
    if (path) await addFolder(path);
  };

  return (
    <>
    <aside className="flex shrink-0 flex-col bg-panel" style={{ width }}>
      <div className="flex items-center justify-between px-4 pb-2 pt-4">
        <h2 className="text-xs font-semibold uppercase tracking-wider text-dim">Library</h2>
      </div>

      <nav className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        <button
          onClick={() => setFilter(null)}
          className={`flex h-10 w-full items-center justify-between rounded-md px-2.5 text-left text-sm ${
            filter === null ? "bg-raised text-ink" : "text-dim hover:bg-raised/60 hover:text-ink"
          }`}
        >
          <span className="font-medium">All sounds</span>
          <span className="text-xs tabular-nums text-faint">{librarySize.toLocaleString()}</span>
        </button>

        {folders.map((folder) => (
          <FolderRow key={folder.id} folder={folder} active={filter === folder.id} onSelect={() => setFilter(folder.id)} />
        ))}
      </nav>

      <div className="border-t border-line p-3">
        <button
          onClick={() => void add()}
          className="flex h-10 w-full items-center justify-center gap-2 rounded-md bg-raised text-sm font-medium hover:bg-line"
        >
          <PlusIcon /> Add folder
        </button>
        <button
          onClick={() => setOnline(true)}
          title="Search Freesound, a free library of sound effects and ambiences, and download into your library"
          className="mt-1.5 flex h-9 w-full items-center justify-center gap-2 rounded-md text-sm font-medium text-dim hover:bg-raised hover:text-ink"
        >
          <SearchIcon width={14} height={14} /> Find sounds online
        </button>
        {online && <OnlineSounds onClose={() => setOnline(false)} />}
        <button
          onClick={() => setYoutube(true)}
          title="Paste a YouTube link to save its audio into your library"
          className="flex h-9 w-full items-center justify-center gap-2 rounded-md text-sm font-medium text-dim hover:bg-raised hover:text-ink"
        >
          <PlusIcon width={14} height={14} /> Add from YouTube
        </button>
        {youtube && <YouTubeImport onClose={() => setYoutube(false)} />}
        {!hasStarter && (
          <button
            onClick={() => void addStarterSounds()}
            title="Adds a small set of built-in music, ambience, effects and stingers"
            className="mt-1.5 h-8 w-full rounded-md text-xs font-medium text-dim hover:bg-raised hover:text-ink"
          >
            Add the built-in sounds
          </button>
        )}
        <p className="mt-2 text-center text-[11px] leading-snug text-faint">
          MP3, OGG, FLAC and WAV. New files in these folders are picked up automatically.
        </p>
      </div>
    </aside>
    <Divider direction="x" onDrag={(d) => setWidth([Math.min(520, Math.max(180, width + d))])} onReset={() => setWidth([256])} label="Library list width" />
    </>
  );
}

function FolderRow({ folder, active, onSelect }: { folder: Folder; active: boolean; onSelect: () => void }) {
  const [confirming, setConfirming] = useState(false);
  const rescan = useStore((s) => s.rescan);
  const remove = useStore((s) => s.removeFolder);

  return (
    <div
      className={`group mt-0.5 rounded-md ${active ? "bg-raised text-ink" : "text-dim hover:bg-raised/60 hover:text-ink"}`}
      onMouseLeave={() => setConfirming(false)}
    >
      <div className="flex h-10 items-center">
        <button onClick={onSelect} title={folder.path} className="flex h-full min-w-0 flex-1 items-center gap-2 px-2.5 text-left text-sm">
          {folder.missing ? (
            <WarningIcon className="shrink-0 text-danger" />
          ) : (
            <FolderIcon className="shrink-0 text-faint" />
          )}
          <span className="truncate">{folderName(folder.path)}</span>
        </button>
        <span className="pr-2.5 text-xs tabular-nums text-faint group-hover:hidden group-focus-within:hidden">
          {folder.asset_count.toLocaleString()}
        </span>
        <div className="hidden items-center gap-0.5 pr-1.5 group-hover:flex group-focus-within:flex">
          <button
            onClick={() => void rescan(folder.id)}
            title="Check this folder for new or changed files"
            aria-label={`Rescan ${folderName(folder.path)}`}
            className="rounded p-1.5 hover:bg-line"
          >
            <RefreshIcon width={14} height={14} />
          </button>
          <button
            onClick={() => setConfirming(true)}
            title="Remove this folder from the library"
            aria-label={`Remove ${folderName(folder.path)}`}
            className="rounded p-1.5 hover:bg-line"
          >
            <CloseIcon width={14} height={14} />
          </button>
        </div>
      </div>

      {folder.missing && (
        <p className="px-2.5 pb-2 text-[11px] leading-snug text-danger/90">
          Folder not found. Is its drive connected? Your sounds and tags are kept.
        </p>
      )}

      {confirming && (
        <div className="px-2.5 pb-2.5 text-[11px] leading-snug text-dim">
          Remove {plural(folder.asset_count, "sound")} from the library? The files stay on your computer.
          <div className="mt-1.5 flex gap-1.5">
            <button onClick={() => void remove(folder.id)} className="rounded bg-danger px-2 py-1 font-medium text-white">
              Remove
            </button>
            <button onClick={() => setConfirming(false)} className="rounded bg-line px-2 py-1 font-medium text-ink">
              Keep
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
