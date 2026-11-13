// Small building blocks shared by the editors.

import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, CHANNELS, type Asset, type Channel } from "../api";
import { comboOf } from "../actions";
import { assetName, useDocs } from "../docs";
import { formatTime } from "../format";
import { COLORS } from "../types";
import { useStore } from "../store";
import { ArrowIcon, CloseIcon, PlayIcon, PlusIcon, SearchIcon, StopIcon, TrashIcon } from "./icons";

export const buttonClass =
  "inline-flex h-9 items-center justify-center gap-2 rounded-md bg-raised px-3 text-sm font-medium text-ink hover:bg-line disabled:opacity-40";
export const primaryClass =
  "inline-flex h-9 items-center justify-center gap-2 rounded-md bg-accent px-3.5 text-sm font-semibold text-bg hover:brightness-110 disabled:opacity-40";
/** A text field's look, without a width or height. */
export const inputBase =
  "select-text rounded-md border border-line bg-bg px-2.5 text-sm text-ink placeholder:text-faint focus:border-accent focus:outline-none";
/** A full-width text field of standard height. */
export const inputClass = `${inputBase} h-9 w-full`;

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline justify-between gap-3">
        <span className="text-xs font-semibold uppercase tracking-wider text-dim">{label}</span>
        {hint && <span className="text-right text-[11px] text-faint">{hint}</span>}
      </div>
      {children}
    </div>
  );
}

/** A row of mutually exclusive choices. */
export function Segmented<T extends string | number>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: string; title?: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <div className="flex flex-wrap gap-1 rounded-lg bg-bg p-1" role="radiogroup">
      {options.map((o) => (
        <button
          key={String(o.value)}
          role="radio"
          aria-checked={o.value === value}
          title={o.title}
          onClick={() => onChange(o.value)}
          className={`h-7 flex-1 whitespace-nowrap rounded-md px-2.5 text-xs font-medium ${
            o.value === value ? "bg-raised text-ink shadow-sm" : "text-dim hover:text-ink"
          }`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Toggle({ on, onChange, label, hint }: { on: boolean; onChange: (on: boolean) => void; label: string; hint?: string }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      onClick={() => onChange(!on)}
      className="flex w-full items-center gap-3 rounded-md py-1 text-left"
    >
      <span className={`relative h-5 w-9 shrink-0 rounded-full transition-colors ${on ? "bg-accent" : "bg-line"}`}>
        <span className={`absolute top-0.5 size-4 rounded-full bg-ink transition-[left] ${on ? "left-[18px]" : "left-0.5"}`} />
      </span>
      <span className="min-w-0">
        <span className="block text-sm">{label}</span>
        {hint && <span className="block text-xs text-faint">{hint}</span>}
      </span>
    </button>
  );
}

/** A plain slider with its value shown beside it. */
export function Slider({
  value,
  min,
  max,
  step = 1,
  onChange,
  format,
  label,
  className,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  format: (value: number) => string;
  label: string;
  className?: string;
}) {
  const percent = ((value - min) / (max - min)) * 100;
  return (
    <div className={`flex items-center gap-2.5 ${className ?? ""}`}>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        aria-label={label}
        title={`${label}: ${format(value)}`}
        onChange={(e) => onChange(Number(e.target.value))}
        className="fader min-w-0 flex-1"
        style={{ "--c": "var(--color-dim)", "--v": `${percent}%` } as React.CSSProperties}
      />
      <span className="w-14 shrink-0 text-right text-xs tabular-nums text-dim">{format(value)}</span>
    </div>
  );
}

/** Volume in plain words: "Normal", "Quieter (-6 dB)". */
export const formatDb = (db: number) => (db === 0 ? "Normal" : `${db > 0 ? "+" : "−"}${Math.abs(db)} dB`);

export function ColorPicker({ value, onChange }: { value: string; onChange: (color: string) => void }) {
  return (
    <div className="flex gap-1.5" role="radiogroup" aria-label="Colour">
      {COLORS.map((c) => (
        <button
          key={c}
          role="radio"
          aria-checked={c === value}
          aria-label={c}
          onClick={() => onChange(c)}
          className={`size-6 rounded-full ${c === value ? "ring-2 ring-ink ring-offset-2 ring-offset-panel" : "opacity-70 hover:opacity-100"}`}
          style={{ background: c }}
        />
      ))}
    </div>
  );
}

const openModals: object[] = [];

export function Modal({ title, onClose, children, wide }: { title: string; onClose: () => void; children: ReactNode; wide?: boolean }) {
  // The latest onClose, without re-registering (which would reorder the stack).
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const token = {};
    openModals.push(token);
    const onKey = (e: KeyboardEvent) => {
      // With one dialog on top of another, Esc closes only the top one.
      if (e.key === "Escape" && openModals[openModals.length - 1] === token) {
        e.stopImmediatePropagation();
        e.preventDefault();
        close.current();
      }
    };
    // Capture, so Esc closes the dialog instead of stopping all sound.
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      openModals.splice(openModals.indexOf(token), 1);
    };
  }, []);
  return (
    <div className="fixed inset-0 z-40 flex items-start justify-center bg-black/60 p-6 pt-[10vh]" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        role="dialog"
        aria-label={title}
        className={`flex max-h-[76vh] w-full flex-col rounded-xl border border-line bg-panel shadow-2xl shadow-black/70 ${wide ? "max-w-2xl" : "max-w-lg"}`}
      >
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <h2 className="text-sm font-semibold">{title}</h2>
          <button onClick={onClose} aria-label="Close" className="rounded p-1.5 text-dim hover:bg-raised hover:text-ink">
            <CloseIcon />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

/** Search the library and pick sounds. Stays open so several can be added. */
export function AssetPicker({
  title,
  channel,
  chosen,
  onPick,
  onRemove,
  onClose,
}: {
  title: string;
  /** Start filtered to sounds tagged for this channel. */
  channel?: Channel;
  chosen: number[];
  onPick: (asset: Asset) => void;
  /** Takes a chosen sound out again. Without it, chosen sounds only show as added. */
  onRemove?: (asset: Asset) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  /** The sound being listened to: its asset, and the engine's id for it. */
  const [listening, setListening] = useState<{ asset: number; sound: number } | null>(null);
  const stillSounding = useStore((s) => listening !== null && (s.engine?.sounds.some((x) => x.id === listening.sound && !x.stopping) ?? false));
  const heard = useRef<number | null>(null);
  heard.current = listening && stillSounding ? listening.sound : null;
  // Closing the window ends the listening.
  useEffect(
    () => () => {
      if (heard.current !== null) void api.stopSound(heard.current);
    },
    [],
  );
  const listen = async (asset: Asset) => {
    const was = listening && stillSounding ? listening : null;
    if (was) await api.stopSound(was.sound).catch(() => {});
    if (was?.asset === asset.id) return setListening(null);
    try {
      setListening({ asset: asset.id, sound: await api.preview(asset.id, asset.edit, null) });
    } catch (e) {
      useStore.getState().notify(String(e));
    }
  };
  const [filter, setFilter] = useState<Channel | null>(channel ?? null);
  const [results, setResults] = useState<Asset[]>([]);
  const seq = useRef(0);

  useEffect(() => {
    const mine = ++seq.current;
    const tag = CHANNELS.find((c) => c.id === filter)?.tag;
    void api.search(query, null, tag ? [tag] : [], 80).then((found) => {
      if (seq.current === mine) setResults(found.assets);
    });
  }, [query, filter]);

  return (
    <Modal title={title} onClose={onClose} wide>
      <div className="space-y-2 px-4 pt-3">
        <label className="relative block">
          <SearchIcon className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-faint" />
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search your library"
            spellCheck={false}
            className={`${inputClass} pl-9`}
          />
        </label>
        <div className="flex flex-wrap gap-1.5 text-xs">
          {[null, ...CHANNELS.map((c) => c.id)].map((id) => (
            <button
              key={id ?? "all"}
              onClick={() => setFilter(id)}
              aria-pressed={filter === id}
              className={`h-6 rounded-full border px-2.5 font-medium ${
                filter === id ? "border-ink bg-ink text-bg" : "border-line text-dim hover:text-ink"
              }`}
            >
              {id ? CHANNELS.find((c) => c.id === id)!.label : "All"}
            </button>
          ))}
        </div>
      </div>
      <div className="mt-2 min-h-40 flex-1 overflow-y-auto px-2 pb-2">
        {results.length === 0 && <p className="px-2 py-6 text-center text-sm text-dim">Nothing matches.</p>}
        {results.map((asset) => {
          const added = chosen.includes(asset.id);
          const on = stillSounding && listening?.asset === asset.id;
          const failed = asset.status === "failed";
          return (
            <div key={asset.id} className={`flex h-10 items-center gap-1 rounded-md pl-1.5 hover:bg-raised ${failed ? "opacity-40" : ""}`}>
              <button
                disabled={failed}
                onClick={() => void listen(asset)}
                aria-label={on ? `Stop ${asset.name}` : `Listen to ${asset.name}`}
                title={on ? "Stop" : "Listen without adding it (plays on the Sound effects channel)"}
                className={`flex size-7 shrink-0 items-center justify-center rounded-full ${on ? "bg-accent text-bg" : "bg-raised text-ink hover:bg-line"}`}
              >
                {on ? <StopIcon width={10} height={10} /> : <PlayIcon width={10} height={10} />}
              </button>
              <button
                disabled={failed}
                onClick={() => (added && onRemove ? onRemove(asset) : onPick(asset))}
                title={added && onRemove ? "Take it out again" : undefined}
                className="group flex h-full min-w-0 flex-1 items-center gap-3 px-2 text-left"
              >
                <span className="min-w-0 flex-1 truncate text-sm">{asset.name}</span>
                <span className="text-xs tabular-nums text-faint">{formatTime(asset.duration_ms)}</span>
                <span className={`flex w-20 items-center justify-end gap-1 text-xs font-medium ${added ? "text-accent" : "text-dim"}`}>
                  {!added ? (
                    <>
                      <PlusIcon width={12} height={12} /> Add
                    </>
                  ) : onRemove ? (
                    <>
                      <span className="group-hover:hidden">Added</span>
                      <span className="hidden items-center gap-1 text-danger group-hover:flex">
                        <CloseIcon width={10} height={10} /> Remove
                      </span>
                    </>
                  ) : (
                    "Added"
                  )}
                </span>
              </button>
            </div>
          );
        })}
      </div>
      <div className="flex justify-end border-t border-line px-4 py-3">
        <button onClick={onClose} className={primaryClass}>
          Done
        </button>
      </div>
    </Modal>
  );
}

/** A list of chosen sounds with remove (and optionally reorder) buttons, plus "add". */
export function AssetList({
  ids,
  onChange,
  addLabel,
  pickerTitle,
  channel,
  ordered,
  empty,
}: {
  ids: number[];
  onChange: (ids: number[]) => void;
  addLabel: string;
  pickerTitle: string;
  channel?: Channel;
  ordered?: boolean;
  empty?: string;
}) {
  const assets = useDocs((s) => s.assets);
  const needAssets = useDocs((s) => s.needAssets);
  const [picking, setPicking] = useState(false);
  useEffect(() => needAssets(ids), [ids, needAssets]);

  const move = (at: number, by: number) => {
    const next = [...ids];
    const [item] = next.splice(at, 1);
    next.splice(at + by, 0, item);
    onChange(next);
  };

  return (
    <div className="space-y-1">
      {ids.length === 0 && empty && <p className="text-xs text-faint">{empty}</p>}
      {ids.map((id, at) => (
        <div key={`${id}-${at}`} className="flex h-8 items-center gap-1 rounded-md bg-bg pl-2.5 pr-1">
          <span className="min-w-0 flex-1 truncate text-sm">{assetName(assets, id)}</span>
          <span className="text-[11px] tabular-nums text-faint">{formatTime(assets[id]?.duration_ms ?? null)}</span>
          {ordered && ids.length > 1 && (
            <>
              <button disabled={at === 0} onClick={() => move(at, -1)} aria-label="Move up" className="rounded p-1 text-dim enabled:hover:bg-line disabled:opacity-25">
                <ArrowIcon up width={12} height={12} />
              </button>
              <button disabled={at === ids.length - 1} onClick={() => move(at, 1)} aria-label="Move down" className="rounded p-1 text-dim enabled:hover:bg-line disabled:opacity-25">
                <ArrowIcon width={12} height={12} />
              </button>
            </>
          )}
          <button onClick={() => onChange(ids.filter((_, i) => i !== at))} aria-label={`Remove ${assetName(assets, id)}`} className="rounded p-1 text-dim hover:bg-line hover:text-ink">
            <CloseIcon width={12} height={12} />
          </button>
        </div>
      ))}
      <button onClick={() => setPicking(true)} className="flex h-8 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-dim hover:bg-raised hover:text-ink">
        <PlusIcon width={12} height={12} /> {addLabel}
      </button>
      {picking && (
        <AssetPicker
          title={pickerTitle}
          channel={channel}
          chosen={ids}
          onPick={(asset) => {
            useDocs.setState((s) => ({ assets: { ...s.assets, [asset.id]: asset } }));
            if (!ids.includes(asset.id)) onChange([...ids, asset.id]);
          }}
          onRemove={(asset) => onChange(ids.filter((id) => id !== asset.id))}
          onClose={() => setPicking(false)}
        />
      )}
    </div>
  );
}

/** Click, then press a key: that key now does the thing. */
export function HotkeyInput({
  combo,
  global,
  onChange,
  fallback,
}: {
  combo: string | null;
  global: boolean;
  onChange: (combo: string | null, global: boolean) => void;
  /** The built-in key, shown when no custom one is set. */
  fallback?: string;
}) {
  const advanced = useDocs((s) => s.advanced);
  const [listening, setListening] = useState(false);

  useEffect(() => {
    if (!listening) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") return setListening(false);
      const pressed = comboOf(e);
      if (!pressed) return;
      setListening(false);
      onChange(pressed, global);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [listening, global, onChange]);

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2">
        <button
          onClick={() => setListening(!listening)}
          className={`flex h-9 min-w-32 items-center justify-center rounded-md border px-3 text-sm ${
            listening ? "border-accent text-accent" : "border-line text-ink hover:border-dim"
          }`}
        >
          {listening ? "Press a key…" : combo ? <kbd className="font-sans font-semibold">{combo}</kbd> : fallback ? <span className="text-dim">{fallback} (built in)</span> : <span className="text-faint">Click to set a key</span>}
        </button>
        {combo && (
          <button onClick={() => onChange(null, false)} className="text-xs text-dim hover:text-ink">
            Clear
          </button>
        )}
      </div>
      {advanced && combo && (
        <Toggle
          on={global}
          onChange={(on) => onChange(combo, on)}
          label="Works in other programs too"
          hint="Off: only while GMAudio is the window in front."
        />
      )}
    </div>
  );
}

/** Asks before doing something that cannot be undone. */
export function ConfirmButton({ label, confirm, onConfirm }: { label: string; confirm: string; onConfirm: () => void }) {
  const [asking, setAsking] = useState(false);
  if (!asking) {
    return (
      <button onClick={() => setAsking(true)} className="flex items-center gap-1.5 text-xs font-medium text-dim hover:text-danger">
        <TrashIcon width={13} height={13} />
        {label}
      </button>
    );
  }
  return (
    <span className="flex items-center gap-2 text-xs">
      <span className="text-dim">{confirm}</span>
      <button onClick={onConfirm} className="flex items-center gap-1 rounded bg-danger px-2 py-1 font-medium text-white">
        <TrashIcon width={12} height={12} />
        Delete
      </button>
      <button onClick={() => setAsking(false)} className="rounded bg-line px-2 py-1 font-medium">
        Keep
      </button>
    </span>
  );
}
