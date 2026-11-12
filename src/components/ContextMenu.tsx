import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { assetChannels, CHANNELS } from "../api";
import { useStore } from "../store";
import { CheckIcon, PlayIcon } from "./icons";

/** The right-click menu for a sound: play it somewhere, or tag it. */
export function ContextMenu() {
  const menu = useStore((s) => s.menu);
  const close = useStore((s) => s.openMenu);
  const play = useStore((s) => s.play);
  const toggleChannelTag = useStore((s) => s.toggleChannelTag);
  const musicBusy = useStore((s) => s.engine?.sounds.some((x) => x.channel === "music" && !x.stopping) ?? false);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: 0, top: 0 });

  // Keep the menu inside the window.
  useLayoutEffect(() => {
    if (!menu || !ref.current) return;
    const box = ref.current.getBoundingClientRect();
    setPos({
      left: Math.max(8, Math.min(menu.x, window.innerWidth - box.width - 8)),
      top: Math.max(8, Math.min(menu.y, window.innerHeight - box.height - 8)),
    });
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const dismiss = (e: Event) => {
      if (e.target instanceof Node && ref.current?.contains(e.target)) return;
      close(null);
    };
    window.addEventListener("pointerdown", dismiss, true);
    window.addEventListener("wheel", dismiss, true);
    window.addEventListener("blur", dismiss);
    return () => {
      window.removeEventListener("pointerdown", dismiss, true);
      window.removeEventListener("wheel", dismiss, true);
      window.removeEventListener("blur", dismiss);
    };
  }, [menu, close]);

  if (!menu) return null;
  const { asset } = menu;
  const tagged = assetChannels(asset);
  const item = "flex h-8 w-full items-center gap-2.5 rounded px-2.5 text-left text-[13px] hover:bg-line";

  return (
    <div
      ref={ref}
      role="menu"
      aria-label={`Actions for ${asset.name}`}
      style={pos}
      className="fixed z-50 w-60 rounded-lg border border-line bg-raised p-1.5 shadow-2xl shadow-black/60"
      onContextMenu={(e) => e.preventDefault()}
    >
      <p className="truncate px-2.5 pb-1 pt-1 text-xs font-semibold text-dim">{asset.name}</p>
      <p className="px-2.5 pb-1 pt-1 text-[11px] font-medium uppercase tracking-wider text-faint">Play on</p>
      {CHANNELS.map((c) => (
        <div key={c.id}>
          <button
            role="menuitem"
            className={item}
            onClick={() => {
              void play(asset.id, c.id);
              close(null);
            }}
          >
            <PlayIcon width={11} height={11} style={{ color: c.color }} />
            <span className="flex-1">{c.label}</span>
            {c.id === "music" && musicBusy && <span className="text-[11px] text-faint">adds to queue</span>}
          </button>
          {c.id === "music" && musicBusy && (
            <button
              role="menuitem"
              className={item}
              onClick={() => {
                void play(asset.id, "music", true);
                close(null);
              }}
            >
              <PlayIcon width={11} height={11} style={{ color: c.color }} />
              <span className="flex-1">Music, right now</span>
              <span className="text-[11px] text-faint">crossfade</span>
            </button>
          )}
        </div>
      ))}
      <div className="my-1.5 border-t border-line" />
      <p className="px-2.5 pb-1 text-[11px] font-medium uppercase tracking-wider text-faint">Tag as</p>
      {CHANNELS.map((c) => {
        const on = tagged.includes(c.id);
        return (
          <button
            key={c.id}
            role="menuitemcheckbox"
            aria-checked={on}
            className={item}
            onClick={() => {
              void toggleChannelTag(asset, c.id);
              close(null);
            }}
          >
            <span className="flex w-[11px] justify-center">{on && <CheckIcon width={12} height={12} style={{ color: c.color }} />}</span>
            {c.tag}
          </button>
        );
      })}
    </div>
  );
}
