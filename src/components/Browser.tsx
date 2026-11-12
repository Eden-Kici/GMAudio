import { memo, useMemo, useState } from "react";
import { CHANNELS, type Asset, type Channel } from "../api";
import { formatTime, plural, relativeFolder } from "../format";
import { playingKey, RESULT_LIMIT, useStore } from "../store";
import { CloseIcon, FolderIcon, PlusIcon, SearchIcon, WarningIcon } from "./icons";
import { pickFolder } from "./Sidebar";

export function Browser() {
  const query = useStore((s) => s.query);
  const setQuery = useStore((s) => s.setQuery);
  const results = useStore((s) => s.results);
  const total = useStore((s) => s.total);
  const folders = useStore((s) => s.folders);
  const foldersLoaded = useStore((s) => s.foldersLoaded);
  const scanning = useStore((s) => s.scan.scanning !== null);
  const selectedId = useStore((s) => s.selectedId);
  const playing = useStore((s) => playingKey(s.engine));

  const roots = useMemo(() => folders.map((f) => f.path), [folders]);
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

  if (foldersLoaded && folders.length === 0) return <Welcome />;

  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <div className="flex items-center gap-4 px-5 pb-3 pt-4">
        <label className="relative flex-1">
          <SearchIcon className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-faint" width={18} height={18} />
          <input
            id="library-search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search by name, folder or #tag"
            spellCheck={false}
            autoComplete="off"
            className="h-11 w-full select-text rounded-lg border border-line bg-panel pl-11 pr-10 text-[15px] placeholder:text-faint focus:border-accent focus:outline-none"
          />
          {query ? (
            <button
              onClick={() => setQuery("")}
              aria-label="Clear search"
              className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-1.5 text-dim hover:bg-raised hover:text-ink"
            >
              <CloseIcon width={14} height={14} />
            </button>
          ) : (
            <kbd className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 rounded border border-line px-1.5 text-[11px] text-faint">
              /
            </kbd>
          )}
        </label>
        <span className="shrink-0 text-xs tabular-nums text-dim">
          {total > results.length
            ? `Showing ${results.length.toLocaleString()} of ${total.toLocaleString()}`
            : plural(total, "sound")}
        </span>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-3" role="list">
        {results.length === 0 ? (
          <Empty query={query} scanning={scanning} />
        ) : (
          <>
            {results.map((asset) => (
              <Row
                key={asset.id}
                asset={asset}
                folder={relativeFolder(asset.path, roots)}
                playing={playingByAsset.get(asset.id) ?? ""}
                selected={asset.id === selectedId}
              />
            ))}
            {total > RESULT_LIMIT && (
              <p className="py-4 text-center text-xs text-faint">
                {(total - RESULT_LIMIT).toLocaleString()} more. Type a few letters to narrow it down.
              </p>
            )}
          </>
        )}
      </div>
    </main>
  );
}

const channelColor: Record<Channel, string> = {
  music: "var(--color-music)",
  ambience: "var(--color-ambience)",
  sfx: "var(--color-sfx)",
  stingers: "var(--color-stingers)",
};

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
  const failed = asset.status === "failed";
  const active = playing.split(" ");

  return (
    <div role="listitem" className={`rounded-lg ${selected ? "bg-panel ring-1 ring-line" : "hover:bg-panel"}`}>
      <div className="flex min-h-12 items-center gap-3 px-2.5">
        <button
          onClick={() => select(selected ? null : asset.id)}
          aria-expanded={selected}
          title={selected ? "Hide details" : "Show details and tags"}
          className="flex min-h-12 min-w-0 flex-1 items-center gap-3 text-left"
        >
          <span className="flex w-4 shrink-0 justify-center">
            {failed ? (
              <WarningIcon className="text-danger" />
            ) : playing ? (
              <Equalizer color={channelColor[active[0] as Channel]} />
            ) : (
              <span className="size-1.5 rounded-full bg-line" />
            )}
          </span>
          <span className="min-w-0 flex-1">
            <span className={`block truncate text-[14px] font-medium ${failed ? "text-dim" : ""}`}>{asset.name}</span>
            {(folder || asset.tags.length > 0) && (
              <span className="mt-0.5 flex items-center gap-2 text-xs text-faint">
                {folder && <span className="truncate">{folder}</span>}
                {asset.tags.slice(0, 4).map((t) => (
                  <span key={t.name} className="shrink-0 rounded bg-raised px-1.5 py-px text-[11px] text-dim">
                    {t.name}
                  </span>
                ))}
                {asset.tags.length > 4 && <span className="shrink-0">+{asset.tags.length - 4}</span>}
              </span>
            )}
          </span>
          <span className="w-12 shrink-0 text-right text-xs tabular-nums text-dim">{formatTime(asset.duration_ms)}</span>
        </button>

        <div className="flex shrink-0 gap-1.5" role="group" aria-label={`Play ${asset.name}`}>
          {CHANNELS.map((c) => {
            const on = active.includes(c.id);
            return (
              <button
                key={c.id}
                disabled={failed}
                onClick={() => void play(asset.id, c.id)}
                title={failed ? "This file could not be read" : `Play as ${c.label.toLowerCase()}. ${c.hint}`}
                style={{ "--c": channelColor[c.id] } as React.CSSProperties}
                className={`h-8 rounded-md border px-2.5 text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-30 ${
                  on
                    ? "border-(--c) bg-(--c) text-bg"
                    : "border-line text-dim enabled:hover:border-(--c) enabled:hover:text-(--c)"
                }`}
              >
                {c.verb}
              </button>
            );
          })}
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
  const [draft, setDraft] = useState("");

  const add = () => {
    const name = draft.trim().replace(/^#/, "");
    setDraft("");
    if (!name || asset.tags.some((t) => t.name.toLowerCase() === name.toLowerCase())) return;
    void setTags(asset.id, [...asset.tags, { name, category: "custom" }]);
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
    <div className="space-y-2.5 border-t border-line px-9 pb-3.5 pt-3 text-xs">
      {asset.status === "failed" && (
        <p className="text-danger">This file could not be read: {asset.error ?? "unknown problem"}.</p>
      )}

      <div className="flex flex-wrap items-center gap-1.5">
        <span className="mr-1 text-dim">Tags</span>
        {asset.tags.map((t) => (
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
        <span className="text-dim">{level}.</span>{" "}
        <span className="select-text break-all">{asset.path}</span>
      </p>
    </div>
  );
}

function Empty({ query, scanning }: { query: string; scanning: boolean }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 text-center text-dim">
      {query ? (
        <>
          <p className="text-[15px] text-ink">Nothing matches “{query}”</p>
          <p className="text-xs">Try fewer letters, or a #tag.</p>
        </>
      ) : scanning ? (
        <p>Looking through your folder…</p>
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
        <button
          onClick={() => void add()}
          className="mt-6 inline-flex h-11 items-center gap-2 rounded-lg bg-accent px-5 text-sm font-semibold text-bg hover:brightness-110"
        >
          <PlusIcon /> Choose a folder
        </button>
      </div>
    </main>
  );
}
