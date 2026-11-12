import { useEffect, useMemo, useRef, useState } from "react";
import { api, assetChannels, channelInfo, type Asset } from "../api";
import { activateScene, triggerPad } from "../actions";
import { useDocs } from "../docs";
import { useStore } from "../store";
import { SearchIcon } from "./icons";

interface Item {
  key: string;
  kind: string;
  name: string;
  detail: string;
  color: string;
  run: () => void;
}

/** Ctrl+K: type a few letters, press Enter, and it plays. */
export function Palette({ onClose }: { onClose: () => void }) {
  const scenes = useDocs((s) => s.scenes);
  const boards = useDocs((s) => s.boards);
  const play = useStore((s) => s.play);
  const [query, setQuery] = useState("");
  const [sounds, setSounds] = useState<Asset[]>([]);
  const [cursor, setCursor] = useState(0);
  const seq = useRef(0);

  useEffect(() => {
    const mine = ++seq.current;
    void api.search(query, null, [], 12).then((found) => {
      if (seq.current === mine) setSounds(found.assets.filter((a) => a.status !== "failed"));
    });
    setCursor(0);
  }, [query]);

  const items = useMemo(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    const matches = (name: string) => words.every((w) => name.toLowerCase().includes(w));
    const list: Item[] = [];
    for (const scene of scenes) {
      if (matches(scene.data.name)) {
        list.push({ key: `s${scene.id}`, kind: "Scene", name: scene.data.name, detail: "Switch to it", color: scene.data.color, run: () => void activateScene(scene) });
      }
    }
    for (const board of boards) {
      board.data.pads.forEach((pad, index) => {
        if (pad.asset_ids.length > 0 && matches(pad.label)) {
          list.push({
            key: `p${board.id}-${index}`,
            kind: "Pad",
            name: pad.label,
            detail: board.data.name,
            color: pad.color,
            run: () => void triggerPad(board.id, index, pad, "down"),
          });
        }
      });
    }
    for (const asset of sounds) {
      const channel = assetChannels(asset)[0] ?? "sfx";
      const info = channelInfo(channel);
      list.push({ key: `a${asset.id}`, kind: "Sound", name: asset.name, detail: `Play as ${info.label.toLowerCase()}`, color: info.color, run: () => void play(asset.id, channel) });
    }
    return list.slice(0, 14);
  }, [query, scenes, boards, sounds, play]);

  const fire = (item: Item | undefined) => {
    if (!item) return;
    item.run();
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/60 pt-[14vh]" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div role="dialog" aria-label="Find and fire" className="w-full max-w-xl overflow-hidden rounded-xl border border-line bg-panel shadow-2xl shadow-black/70">
        <label className="relative block border-b border-line">
          <SearchIcon className="pointer-events-none absolute left-4 top-1/2 -translate-y-1/2 text-faint" />
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setCursor((c) => Math.min(c + 1, items.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setCursor((c) => Math.max(c - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                fire(items[cursor]);
              } else if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                onClose();
              }
            }}
            placeholder="Find a scene, pad or sound, then press Enter"
            spellCheck={false}
            className="h-12 w-full select-text bg-transparent pl-11 pr-4 text-[15px] placeholder:text-faint focus:outline-none"
          />
        </label>
        <ul className="max-h-[50vh] overflow-y-auto p-1.5" role="listbox">
          {items.length === 0 && <li className="px-3 py-5 text-center text-sm text-dim">Nothing matches.</li>}
          {items.map((item, at) => (
            <li key={item.key} role="option" aria-selected={at === cursor}>
              <button
                onPointerMove={() => setCursor(at)}
                onClick={() => fire(item)}
                className={`flex h-10 w-full items-center gap-3 rounded-md px-3 text-left ${at === cursor ? "bg-raised" : ""}`}
              >
                <span className="size-2 shrink-0 rounded-full" style={{ background: item.color }} />
                <span className="w-12 shrink-0 text-[11px] font-medium uppercase tracking-wider text-faint">{item.kind}</span>
                <span className="min-w-0 flex-1 truncate text-sm font-medium">{item.name}</span>
                <span className="shrink-0 text-xs text-dim">{item.detail}</span>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
