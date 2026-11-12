import { memo, useMemo, useState } from "react";
import { assetChannels, channelInfo, CHANNELS, channelOfTag, type Asset, type Channel } from "../api";
import { folderName, formatTime, plural, relativeFolder } from "../format";
import { MAX_PANES, playingKey, RESULT_LIMIT, useStore, type Pane } from "../store";
import { ChevronIcon, CloseIcon, FolderIcon, PlayIcon, PlusIcon, SearchIcon, SplitIcon, WarningIcon } from "./icons";
import { pickFolder } from "./Sidebar";

/** What a dragged sound carries, so the mixer knows it when it lands. */
export const DRAG_TYPE = "application/x-gmaudio-asset";

/** The library: one to four panes side by side, each with its own filters. */
export function Browser() {
  const panes = useStore((s) => s.panes);
  const folders = useStore((s) => s.folders);
  const foldersLoaded = useStore((s) => s.foldersLoaded);

  if (foldersLoaded && folders.length === 0) return <Welcome />;

  const grid = panes.length === 4 ? "grid-cols-2 grid-rows-2" : panes.length === 3 ? "grid-cols-3" : panes.length === 2 ? "grid-cols-2" : "grid-cols-1";
  return (
    <main className={`grid min-h-0 min-w-0 flex-1 gap-px bg-line ${grid}`}>
      {panes.map((pane) => (
        <PaneView key={pane.id} pane={pane} canSplit={panes.length < MAX_PANES} canClose={panes.length > 1} />
      ))}
    </main>
  );
}

function PaneView({ pane, canSplit, canClose }: { pane: Pane; canSplit: boolean; canClose: boolean }) {
  const setQuery = useStore((s) => s.setQuery);
  const setPaneChannel = useStore((s) => s.setPaneChannel);
  const setPaneFolder = useStore((s) => s.setPaneFolder);
  const setActivePane = useStore((s) => s.setActivePane);
  const splitPane = useStore((s) => s.splitPane);
  const closePane = useStore((s) => s.closePane);
  const active = useStore((s) => s.activePane === pane.id && s.panes.length > 1);
  const folders = useStore((s) => s.folders);
  const scanning = useStore((s) => s.scan.scanning !== null);
  const selectedId = useStore((s) => s.selectedId);
  const playing = useStore((s) => playingKey(s.engine));

  const roots = useMemo(() => folders.map((f) => f.path), [folders]);
  const folder = folders.find((f) => f.id === pane.folderId);
  /** asset id -> channels it is playing on */
  const playingByAsset = useMemo(() => {
    const map = new Map<number, string>();
    for (const part of playing ? playing.split(",") : []) {
      const [id, channel] = part.split(":");
      const key = Number(id);
      map.set(key, map.has(key) ? `${map.get(key)} ${channel}` : channel);
    }
    return map;
  }, [playing]);

  return (
    <section
      className={`flex min-h-0 min-w-0 flex-col bg-bg ${active ? "shadow-[inset_0_2px_0_var(--color-accent)]" : ""}`}
      onPointerDownCapture={() => setActivePane(pane.id)}
      onFocusCapture={() => setActivePane(pane.id)}
      aria-label="Library pane"
    >
      <div className="flex items-center gap-2 px-3 pb-2 pt-3">
        <label className="relative min-w-0 flex-1">
          <SearchIcon className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-faint" />
          <input
            data-pane-search={pane.id}
            value={pane.query}
            onChange={(e) => setQuery(pane.id, e.target.value)}
            placeholder="Search by name, folder or #tag"
            spellCheck={false}
            autoComplete="off"
            className="h-10 w-full select-text rounded-lg border border-line bg-panel pl-9 pr-9 text-sm placeholder:text-faint focus:border-accent focus:outline-none"
          />
          {pane.query && (
            <button
              onClick={() => setQuery(pane.id, "")}
              aria-label="Clear search"
              className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-1.5 text-dim hover:bg-raised hover:text-ink"
            >
              <CloseIcon width={14} height={14} />
            </button>
          )}
        </label>
        {canSplit && (
          <button
            onClick={() => splitPane(pane.id)}
            title="Split: open another pane beside this one, with its own search and filters"
            aria-label="Split pane"
            className="flex size-9 shrink-0 items-center justify-center rounded-md text-dim hover:bg-raised hover:text-ink"
          >
            <SplitIcon />
          </button>
        )}
        {canClose && (
          <button
            onClick={() => closePane(pane.id)}
            title="Close this pane"
            aria-label="Close pane"
            className="flex size-9 shrink-0 items-center justify-center rounded-md text-dim hover:bg-raised hover:text-ink"
          >
            <CloseIcon />
          </button>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-1.5 px-3 pb-2 text-xs" role="group" aria-label="Show only">
        <FilterChip on={pane.channel === null} onClick={() => setPaneChannel(pane.id, null)}>
          All
        </FilterChip>
        {CHANNELS.map((c) => (
          <FilterChip
            key={c.id}
            on={pane.channel === c.id}
            color={c.color}
            onClick={() => setPaneChannel(pane.id, pane.channel === c.id ? null : c.id)}
            title={`Show only sounds tagged ${c.tag}`}
          >
            {c.label}
          </FilterChip>
        ))}
        {folder && (
          <span className="flex items-center gap-1 rounded-full bg-raised py-0.5 pl-2 pr-1 text-dim" title={folder.path}>
            <FolderIcon width={12} height={12} />
            {folderName(folder.path)}
            <button
              onClick={() => setPaneFolder(pane.id, null)}
              aria-label="Show all folders"
              className="rounded-full p-0.5 hover:bg-line hover:text-ink"
            >
              <CloseIcon width={10} height={10} />
            </button>
          </span>
        )}
        <span className="ml-auto shrink-0 tabular-nums text-faint">
          {pane.total > pane.results.length
            ? `${pane.results.length.toLocaleString()} of ${pane.total.toLocaleString()}`
            : plural(pane.total, "sound")}
        </span>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2" role="list">
        {pane.results.length === 0 ? (
          <Empty pane={pane} scanning={scanning} />
        ) : (
          <>
            {pane.results.map((asset) => (
              <Row
                key={asset.id}
                asset={asset}
                folder={relativeFolder(asset.path, roots)}
                playing={playingByAsset.get(asset.id) ?? ""}
                selected={asset.id === selectedId}
              />
            ))}
            {pane.total > RESULT_LIMIT && (
              <p className="py-4 text-center text-xs text-faint">
                {(pane.total - RESULT_LIMIT).toLocaleString()} more. Type a few letters to narrow it down.
              </p>
            )}
          </>
        )}
      </div>
    </section>
  );
}

function FilterChip({
  on,
  color,
  onClick,
  title,
  children,
}: {
  on: boolean;
  color?: string;
  onClick: () => void;
  title?: string;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      aria-pressed={on}
      title={title}
      style={{ "--c": color ?? "var(--color-ink)" } as React.CSSProperties}
      className={`flex h-6 items-center gap-1.5 rounded-full border px-2.5 font-medium transition-colors ${
        on ? "border-(--c) bg-(--c) text-bg" : "border-line text-dim hover:border-(--c) hover:text-(--c)"
      }`}
    >
      {children}
    </button>
  );
}

interface RowProps {
  asset: Asset;
  folder: string;
  /** Space-separated channels this asset is playing on right now. */
  playing: string;
  selected: boolean;
}

const Row = memo(function Row({ asset, folder, playing, selected }: RowProps) {
  const select = useStore((s) => s.select);
  const play = useStore((s) => s.play);
  const openMenu = useStore((s) => s.openMenu);
  const setDragging = useStore((s) => s.setDragging);
  const failed = asset.status === "failed";
  const active = playing ? playing.split(" ") : [];
  const channels = assetChannels(asset);
  const otherTags = asset.tags.filter((t) => !channelOfTag(t.name));

  const menuAt = (x: number, y: number) => openMenu({ x, y, asset });
  /** One channel tag: play there. Otherwise ask. */
  const playDefault = (e: { clientX: number; clientY: number }) => {
    if (failed) return;
    if (channels.length === 1) void play(asset.id, channels[0]);
    else menuAt(e.clientX, e.clientY);
  };

  return (
    <div
      role="listitem"
      draggable={!failed}
      onDragStart={(e) => {
        e.dataTransfer.setData(DRAG_TYPE, String(asset.id));
        e.dataTransfer.setData("text/plain", asset.name);
        e.dataTransfer.effectAllowed = "copy";
        setDragging(asset.id);
      }}
      onDragEnd={() => setDragging(null)}
      onContextMenu={(e) => {
        e.preventDefault();
        if (!failed) menuAt(e.clientX, e.clientY);
      }}
      className={`rounded-lg ${selected ? "bg-panel ring-1 ring-line" : "hover:bg-panel"}`}
    >
      <div className="flex min-h-11 items-center gap-2 px-2">
        <button
          onClick={() => select(selected ? null : asset.id)}
          onDoubleClick={playDefault}
          aria-expanded={selected}
          title={selected ? "Hide details" : "Click for details and tags. Double-click to play. Drag onto a mixer channel."}
          className="flex min-h-11 min-w-0 flex-1 items-center gap-2.5 text-left"
        >
          <span className="flex w-4 shrink-0 justify-center">
            {failed ? (
              <WarningIcon className="text-danger" />
            ) : active.length > 0 ? (
              <Equalizer color={channelInfo(active[0] as Channel).color} />
            ) : (
              <span className="size-1.5 rounded-full bg-line" />
            )}
          </span>
          <span className="min-w-0 flex-1">
            <span className={`block truncate text-[13.5px] font-medium ${failed ? "text-dim" : ""}`}>{asset.name}</span>
            {(folder || otherTags.length > 0) && (
              <span className="mt-0.5 flex items-center gap-1.5 text-xs text-faint">
                {folder && <span className="truncate">{folder}</span>}
                {otherTags.slice(0, 3).map((t) => (
                  <span key={t.name} className="shrink-0 rounded bg-raised px-1.5 py-px text-[11px] text-dim">
                    {t.name}
                  </span>
                ))}
                {otherTags.length > 3 && <span className="shrink-0">+{otherTags.length - 3}</span>}
              </span>
            )}
          </span>
          <span className="w-10 shrink-0 text-right text-xs tabular-nums text-dim">{formatTime(asset.duration_ms)}</span>
        </button>

        <div className="flex shrink-0 gap-1" role="group" aria-label={`Play ${asset.name}`}>
          {channels.length === 0 ? (
            <button
              disabled={failed}
              onClick={(e) => {
                const box = e.currentTarget.getBoundingClientRect();
                menuAt(box.left, box.bottom + 4);
              }}
              title={failed ? "This file could not be read" : "Choose where to play this sound"}
              className="flex h-8 items-center gap-1 rounded-md border border-line px-2 text-xs font-medium text-dim enabled:hover:border-ink enabled:hover:text-ink disabled:cursor-not-allowed disabled:opacity-30"
            >
              <PlayIcon width={11} height={11} /> Play <ChevronIcon width={11} height={11} />
            </button>
          ) : (
            channels.map((id) => {
              const c = channelInfo(id);
              const on = active.includes(id);
              return (
                <button
                  key={id}
                  disabled={failed}
                  onClick={() => void play(asset.id, id)}
                  title={`Play as ${c.label.toLowerCase()}. ${c.hint}`}
                  style={{ "--c": c.color } as React.CSSProperties}
                  className={`flex h-8 items-center gap-1 rounded-md border px-2 text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-30 ${
                    on ? "border-(--c) bg-(--c) text-bg" : "border-(--c)/50 text-(--c) enabled:hover:bg-(--c)/15"
                  }`}
                >
                  <PlayIcon width={11} height={11} /> {c.verb}
                </button>
              );
            })
          )}
        </div>
      </div>

      {selected && <Details asset={asset} />}
    </div>
  );
});

function Equalizer({ color }: { color: string }) {
  return (
    <span className="flex h-3.5 items-end gap-px" aria-label="Playing">
      {[0, 0.3, 0.15].map((delay) => (
        <span key={delay} className="eq-bar h-full w-[3px] rounded-sm" style={{ background: color, animationDelay: `${delay}s` }} />
      ))}
    </span>
  );
}

function Details({ asset }: { asset: Asset }) {
  const setTags = useStore((s) => s.setTags);
  const toggleChannelTag = useStore((s) => s.toggleChannelTag);
  const [draft, setDraft] = useState("");
  const channels = assetChannels(asset);
  const otherTags = asset.tags.filter((t) => !channelOfTag(t.name));

  const add = () => {
    const name = draft.trim().replace(/^#/, "");
    setDraft("");
    if (!name || asset.tags.some((t) => t.name.toLowerCase() === name.toLowerCase())) return;
    const channel = channelOfTag(name);
    if (channel) void toggleChannelTag(asset, channel);
    else void setTags(asset.id, [...asset.tags, { name, category: "custom" }]);
  };

  const level =
    asset.status === "pending"
      ? "Loudness not measured yet"
      : asset.loudness_lufs == null
        ? "Too short to measure loudness; plays at its own level"
        : Math.abs(asset.gain_db) < 0.5
          ? "Already at the standard level"
          : `Turned ${asset.gain_db > 0 ? "up" : "down"} ${Math.abs(asset.gain_db).toFixed(1)} dB to match other sounds`;

  return (
    <div className="space-y-2.5 border-t border-line px-8 pb-3.5 pt-3 text-xs">
      {asset.status === "failed" && (
        <p className="text-danger">This file could not be read: {asset.error ?? "unknown problem"}.</p>
      )}

      <div className="flex flex-wrap items-center gap-1.5">
        <span className="mr-1 w-14 text-dim">Plays as</span>
        {CHANNELS.map((c) => {
          const on = channels.includes(c.id);
          return (
            <button
              key={c.id}
              onClick={() => void toggleChannelTag(asset, c.id)}
              aria-pressed={on}
              style={{ "--c": c.color } as React.CSSProperties}
              className={`h-6 rounded-full border px-2.5 font-medium transition-colors ${
                on ? "border-(--c) bg-(--c) text-bg" : "border-line text-dim hover:border-(--c) hover:text-(--c)"
              }`}
            >
              {c.label}
            </button>
          );
        })}
        <span className="text-faint">
          {channels.length === 1 ? "One click plays it there." : channels.length === 0 ? "Pick one for one-click play." : ""}
        </span>
      </div>

      <div className="flex flex-wrap items-center gap-1.5">
        <span className="mr-1 w-14 text-dim">Tags</span>
        {otherTags.map((t) => (
          <span key={t.name} className="flex items-center gap-1 rounded bg-raised py-0.5 pl-2 pr-1 text-ink">
            {t.name}
            <button
              onClick={() => void setTags(asset.id, asset.tags.filter((x) => x.name !== t.name))}
              aria-label={`Remove tag ${t.name}`}
              className="rounded p-0.5 text-dim hover:bg-line hover:text-ink"
            >
              <CloseIcon width={10} height={10} />
            </button>
          </span>
        ))}
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") add();
          }}
          onBlur={add}
          placeholder="Add a tag, press Enter"
          spellCheck={false}
          className="h-6 w-44 select-text rounded border border-line bg-bg px-2 placeholder:text-faint focus:border-accent focus:outline-none"
        />
      </div>

      <p className="text-faint">
        <span className="text-dim">{level}.</span> <span className="select-text break-all">{asset.path}</span>
      </p>
    </div>
  );
}

function Empty({ pane, scanning }: { pane: Pane; scanning: boolean }) {
  const filtered = pane.channel !== null;
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 px-4 text-center text-dim">
      {pane.query ? (
        <>
          <p className="text-[15px] text-ink">Nothing matches “{pane.query}”</p>
          <p className="text-xs">Try fewer letters, or a #tag{filtered ? ", or switch the filter back to All" : ""}.</p>
        </>
      ) : scanning ? (
        <p>Looking through your folder…</p>
      ) : filtered ? (
        <>
          <p className="text-[15px] text-ink">No sounds tagged {channelInfo(pane.channel!).tag} yet</p>
          <p className="text-xs">Right-click a sound and choose “Tag as”, or open its details.</p>
        </>
      ) : (
        <>
          <p className="text-[15px] text-ink">No sounds here yet</p>
          <p className="text-xs">This folder has no MP3, OGG, FLAC or WAV files.</p>
        </>
      )}
    </div>
  );
}

/** First launch: nothing imported yet. */
function Welcome() {
  const addFolder = useStore((s) => s.addFolder);
  const addStarterSounds = useStore((s) => s.addStarterSounds);
  const add = async () => {
    const path = await pickFolder();
    if (path) await addFolder(path);
  };
  return (
    <main className="flex min-w-0 flex-1 items-center justify-center p-8">
      <div className="max-w-md text-center">
        <div className="mx-auto mb-5 flex size-14 items-center justify-center rounded-2xl bg-raised text-accent">
          <FolderIcon width={26} height={26} />
        </div>
        <h1 className="text-xl font-semibold">Add your music and sounds</h1>
        <p className="mt-2 text-sm leading-relaxed text-dim">
          Pick the folder where you keep them. GMAudio lists everything inside, including subfolders, and leaves the
          files where they are.
        </p>
        <div className="mt-6 flex flex-col items-center gap-3">
          <button
            onClick={() => void add()}
            className="inline-flex h-11 items-center gap-2 rounded-lg bg-accent px-5 text-sm font-semibold text-bg hover:brightness-110"
          >
            <PlusIcon /> Choose a folder
          </button>
          <button
            onClick={() => void addStarterSounds()}
            className="inline-flex h-9 items-center gap-2 rounded-lg px-4 text-sm font-medium text-dim hover:bg-raised hover:text-ink"
          >
            Or start with the built-in sounds
          </button>
        </div>
      </div>
    </main>
  );
}
