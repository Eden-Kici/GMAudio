import { useEffect, useMemo, useRef, useState } from "react";
import { api, type Asset, type AssetEdit } from "../api";
import { formatTime } from "../format";
import { message, useStore } from "../store";
import { CloseIcon, PlayIcon, PlusIcon, StopIcon } from "./icons";
import { buttonClass, inputBase, Modal, primaryClass } from "./ui";

/** 83500 → "1:23.5" */
export function formatExact(ms: number): string {
  const tenths = Math.round(ms / 100);
  const m = Math.floor(tenths / 600);
  const s = (tenths % 600) / 10;
  return `${m}:${s < 10 ? "0" : ""}${s.toFixed(1)}`;
}

/** "1:23.5", "83.5" or "1:02:03" → milliseconds; null when it is not a time. */
export function parseTime(text: string): number | null {
  const parts = text.trim().replace(",", ".").split(":");
  if (parts.length === 0 || parts.length > 3 || parts.some((p) => p === "" || !/^\d+(\.\d+)?$/.test(p))) return null;
  const seconds = parts.reduce((total, part) => total * 60 + Number(part), 0);
  return Math.round(seconds * 1000);
}

/** How long a trimmed sound plays for. */
export function editedLength(duration: number, edit: AssetEdit | null): number {
  if (!edit) return duration;
  const cut = edit.cuts.reduce((sum, [from, to]) => sum + (to - from), 0);
  return Math.max(0, (edit.end_ms ?? duration) - edit.start_ms - cut);
}

/** A time field: free typing, taken over when it is a valid time. */
function TimeBox({ value, onChange, placeholder, label }: { value: number | null; onChange: (ms: number | null) => void; placeholder?: string; label: string }) {
  const [text, setText] = useState<string | null>(null);
  const shown = text ?? (value === null ? "" : formatExact(value));
  const bad = text !== null && text.trim() !== "" && parseTime(text) === null;
  const commit = () => {
    if (text === null) return;
    if (text.trim() === "") onChange(null);
    else if (parseTime(text) !== null) onChange(parseTime(text));
    setText(null);
  };
  return (
    <input
      value={shown}
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
      }}
      placeholder={placeholder}
      aria-label={label}
      aria-invalid={bad}
      spellCheck={false}
      className={`${inputBase} h-8 w-20 text-center text-xs tabular-nums ${bad ? "border-danger" : ""}`}
    />
  );
}

type Grip = { kind: "start" } | { kind: "end" } | { kind: "cut"; index: number; edge: 0 | 1 } | { kind: "playhead" };

const MAX_ZOOM = 64;

/** The sound drawn out, with the kept and skipped parts marked and handles to drag. */
function Timeline({
  peaks,
  duration,
  start,
  end,
  cuts,
  playhead,
  onStart,
  onEnd,
  onCut,
  onScrub,
  onSeek,
  following,
}: {
  peaks: number[] | null;
  duration: number;
  start: number;
  end: number;
  cuts: [number, number][];
  playhead: number | null;
  onStart: (ms: number) => void;
  onEnd: (ms: number) => void;
  onCut: (index: number, edge: 0 | 1, ms: number) => void;
  /** The play marker is being dragged to `ms`. */
  onScrub: (ms: number) => void;
  /** The play marker was let go at `ms`: listen from there. */
  onSeek: (ms: number) => void;
  /** A preview is playing, so keep the play marker in view. */
  following: boolean;
}) {
  const box = useRef<HTMLDivElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const [grip, setGrip] = useState<Grip | null>(null);
  const [zoom, setZoom] = useState(1);
  /** Redraws the time labels when the picture is scrolled sideways. */
  const [, setScrolled] = useState(0);
  /** The time to keep under the pointer when the zoom changes, and where the pointer is. */
  const anchor = useRef<{ ms: number; x: number } | null>(null);
  const zoomTo = (next: number, around?: { ms: number; x: number }) => {
    const view = scroller.current;
    if (!view) return;
    const to = Math.min(MAX_ZOOM, Math.max(1, next));
    // Without a pointer, zoom around the middle of what is showing.
    const middle = view.clientWidth / 2;
    anchor.current = around ?? { ms: ((view.scrollLeft + middle) / (view.clientWidth * zoom)) * duration, x: middle };
    setZoom(to);
  };
  useEffect(() => {
    const view = scroller.current;
    if (!view || !anchor.current) return;
    view.scrollLeft = (anchor.current.ms / duration) * view.clientWidth * zoom - anchor.current.x;
    anchor.current = null;
  }, [zoom, duration]);
  // The mouse wheel zooms in and out around the pointer.
  const zoomNow = useRef(zoom);
  zoomNow.current = zoom;
  useEffect(() => {
    const view = scroller.current;
    if (!view) return;
    const wheel = (e: WheelEvent) => {
      if (Math.abs(e.deltaX) > Math.abs(e.deltaY) || e.shiftKey) return; // sideways: scroll
      e.preventDefault();
      const r = view.getBoundingClientRect();
      const x = e.clientX - r.left;
      const ms = ((view.scrollLeft + x) / (view.clientWidth * zoomNow.current)) * duration;
      zoomTo(zoomNow.current * (e.deltaY < 0 ? 1.5 : 1 / 1.5), { ms, x });
    };
    view.addEventListener("wheel", wheel, { passive: false });
    return () => view.removeEventListener("wheel", wheel);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [duration]);
  // While a preview plays, keep the play marker on screen.
  useEffect(() => {
    const view = scroller.current;
    if (!view || !following || playhead === null || grip) return;
    const x = (playhead / duration) * view.clientWidth * zoom;
    if (x < view.scrollLeft || x > view.scrollLeft + view.clientWidth - 24) view.scrollLeft = Math.max(0, x - 40);
  }, [playhead, following, zoom, duration, grip]);
  const pct = (ms: number) => `${(Math.min(duration, Math.max(0, ms)) / duration) * 100}%`;
  const at = (clientX: number) => {
    const r = box.current!.getBoundingClientRect();
    return Math.round((Math.min(1, Math.max(0, (clientX - r.left) / r.width)) * duration) / 100) * 100;
  };
  const shape = useMemo(() => {
    if (!peaks || peaks.length === 0) return "";
    const n = peaks.length;
    let top = "";
    let bottom = "";
    peaks.forEach((p, i) => {
      const x = ((i + 0.5) / n) * 1000;
      const h = Math.max(0.5, p * 46);
      top += `${i === 0 ? "M" : "L"}${x.toFixed(1)},${(50 - h).toFixed(1)}`;
      bottom = `L${x.toFixed(1)},${(50 + h).toFixed(1)}` + bottom;
    });
    return `${top}${bottom}Z`;
  }, [peaks]);

  // While a handle is held, follow the pointer anywhere in the window. The
  // latest values are read through a ref, so the listeners are set up once per drag.
  const latest = useRef({ start, end, onStart, onEnd, onCut, onScrub, onSeek });
  latest.current = { start, end, onStart, onEnd, onCut, onScrub, onSeek };
  const held = useRef(0);
  useEffect(() => {
    if (!grip) return;
    const move = (e: PointerEvent) => {
      const now = latest.current;
      const ms = at(e.clientX);
      held.current = ms;
      if (grip.kind === "start") now.onStart(Math.min(ms, now.end - 200));
      else if (grip.kind === "end") now.onEnd(Math.max(ms, now.start + 200));
      else if (grip.kind === "cut") now.onCut(grip.index, grip.edge, ms);
      else now.onScrub(ms);
    };
    const drop = () => {
      if (grip.kind === "playhead") latest.current.onSeek(held.current);
      setGrip(null);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", drop);
    window.addEventListener("pointercancel", drop);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", drop);
      window.removeEventListener("pointercancel", drop);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [grip]);
  const handle = (g: Grip, ms: number, color: string, label: string) => (
    <div
      key={label}
      role="slider"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={Math.round(duration / 1000)}
      aria-valuenow={Math.round(ms / 1000)}
      aria-valuetext={formatExact(ms)}
      tabIndex={0}
      title={`${label}: ${formatExact(ms)}. Drag, or use the arrow keys.`}
      onPointerDown={(e) => {
        e.stopPropagation();
        e.preventDefault();
        setGrip(g);
      }}
      onKeyDown={(e) => {
        const step = e.key === "ArrowRight" ? 100 : e.key === "ArrowLeft" ? -100 : 0;
        if (!step) return;
        e.preventDefault();
        e.stopPropagation();
        const to = ms + step * (e.shiftKey ? 10 : 1);
        if (g.kind === "start") onStart(Math.min(Math.max(0, to), end - 200));
        else if (g.kind === "end") onEnd(Math.max(Math.min(duration, to), start + 200));
        else if (g.kind === "cut") onCut(g.index, g.edge, to);
      }}
      style={{ left: pct(ms) }}
      className="absolute inset-y-0 z-20 -ml-2 w-4 cursor-ew-resize touch-none"
    >
      <span className="absolute inset-y-0 left-1/2 w-0.5 -translate-x-1/2" style={{ background: color }} />
      <span className="absolute left-1/2 top-1/2 h-6 w-2 -translate-x-1/2 -translate-y-1/2 rounded-sm" style={{ background: color }} />
    </div>
  );

  const zoomButton = "flex size-6 items-center justify-center rounded bg-raised text-sm font-medium leading-none hover:bg-line disabled:opacity-40";
  const shownFrom = scroller.current ? (scroller.current.scrollLeft / (scroller.current.clientWidth * zoom)) * duration : 0;
  return (
    <div>
      <div className="mb-1.5 flex items-center gap-1.5 text-[11px] text-faint">
        <button onClick={() => zoomTo(zoom / 2)} disabled={zoom <= 1} aria-label="Zoom out" title="Zoom out" className={zoomButton}>
          −
        </button>
        <button onClick={() => zoomTo(zoom * 2)} disabled={zoom >= MAX_ZOOM} aria-label="Zoom in" title="Zoom in" className={zoomButton}>
          +
        </button>
        <button onClick={() => zoomTo(1)} disabled={zoom === 1} className="rounded bg-raised px-2 py-1 font-medium hover:bg-line disabled:opacity-40">
          Show all
        </button>
        <span className="ml-1">
          {zoom > 1 ? `Zoomed ${zoom >= 10 ? Math.round(zoom) : Math.round(zoom * 10) / 10}×. ` : ""}
          Scroll the mouse wheel over the picture to zoom; Shift + wheel moves sideways.
        </span>
      </div>
      <div ref={scroller} className="overflow-x-auto overflow-y-hidden rounded-lg bg-bg" onScroll={() => setScrolled((n) => n + 1)}>
      <div
        ref={box}
        onPointerDown={(e) => {
          if (e.button !== 0) return;
          e.preventDefault();
          held.current = at(e.clientX);
          onScrub(held.current);
          setGrip({ kind: "playhead" });
        }}
        style={{ width: `${zoom * 100}%` }}
        className="relative h-28 cursor-pointer select-none overflow-hidden"
      >
        <svg viewBox="0 0 1000 100" preserveAspectRatio="none" className="absolute inset-0 size-full text-dim">
          {shape ? <path d={shape} fill="currentColor" /> : <line x1="0" y1="50" x2="1000" y2="50" stroke="currentColor" strokeWidth="1" />}
        </svg>
        {!peaks && <span className="absolute inset-0 flex items-center justify-center text-xs text-faint">Drawing the sound…</span>}
        {/* What does not play: before the start, after the end, and the skipped parts. */}
        <span className="absolute inset-y-0 left-0 bg-bg/80" style={{ width: pct(start) }} />
        <span className="absolute inset-y-0 right-0 bg-bg/80" style={{ left: pct(end) }} />
        {cuts.map(([from, to], i) => (
          <span
            key={i}
            className="absolute inset-y-0 border-x border-danger/60 bg-danger/25"
            style={{ left: pct(from), width: `${(Math.max(0, to - from) / duration) * 100}%` }}
          />
        ))}
        {playhead !== null && (
          <span className="pointer-events-none absolute inset-y-0 z-10 -ml-1.5 w-3" style={{ left: pct(playhead) }}>
            <span className="absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-ink" />
            <span className="absolute left-1/2 top-0 size-0 -translate-x-1/2 border-x-[6px] border-t-[8px] border-x-transparent border-t-ink" />
          </span>
        )}
        {handle({ kind: "start" }, start, "var(--color-accent)", "Start")}
        {handle({ kind: "end" }, end, "var(--color-accent)", "End")}
        {cuts.flatMap(([from, to], i) => [
          handle({ kind: "cut", index: i, edge: 0 }, from, "var(--color-danger)", `Skip ${i + 1} from`),
          handle({ kind: "cut", index: i, edge: 1 }, to, "var(--color-danger)", `Skip ${i + 1} to`),
        ])}
      </div>
      </div>
      <div className="mt-1 flex justify-between text-[11px] tabular-nums text-faint">
        <span>{formatExact(shownFrom)}</span>
        <span>{formatExact(shownFrom + duration / zoom / 2)}</span>
        <span>{formatExact(Math.min(duration, shownFrom + duration / zoom))}</span>
      </div>
    </div>
  );
}

/** Chooses which parts of a sound play. The file itself is not changed. */
export function TrimEditor({ asset, onClose }: { asset: Asset; onClose: () => void }) {
  const duration = Math.max(1, asset.duration_ms ?? 1);
  const [start, setStart] = useState(asset.edit?.start_ms ?? 0);
  const [end, setEnd] = useState<number | null>(asset.edit?.end_ms ?? null);
  const [cuts, setCuts] = useState<[number, number][]>(asset.edit?.cuts ?? []);
  const [peaks, setPeaks] = useState<number[] | null>(null);
  /** The preview that is sounding, if any. */
  const [previewId, setPreviewId] = useState<number | null>(null);
  /** Where the last click on the timeline was. */
  const [cursor, setCursor] = useState<number | null>(null);
  const previewing = useStore((s) => (previewId === null ? null : (s.engine?.sounds.find((x) => x.id === previewId && !x.stopping) ?? null)));
  /** The play marker is being dragged. */
  const [scrubbing, setScrubbing] = useState(false);
  const playhead = !scrubbing && previewing ? previewing.position_ms : cursor;
  const [resetting, setResetting] = useState(false);
  const [finding, setFinding] = useState(false);

  useEffect(() => {
    void api
      .waveform(asset.id, 3000)
      .then(setPeaks)
      .catch(() => setPeaks([]));
  }, [asset.id]);
  // Closing the editor ends the preview.
  const live = useRef<number | null>(null);
  live.current = previewing ? previewId : null;
  useEffect(
    () => () => {
      if (live.current !== null) void api.stopSound(live.current);
    },
    [],
  );

  const last = end ?? duration;
  const edit: AssetEdit = { start_ms: start, end_ms: end, cuts: cuts.filter(([a, b]) => b > a) };
  const whole = start === 0 && end === null && edit.cuts.length === 0;
  const fail = (e: unknown) => useStore.getState().notify(message(e));

  const stopPreview = async () => {
    if (previewId !== null) await api.stopSound(previewId).catch(() => {});
    setPreviewId(null);
  };
  const preview = async (from: number | null) => {
    try {
      await stopPreview();
      setPreviewId(await api.preview(asset.id, whole ? null : edit, from));
    } catch (e) {
      fail(e);
    }
  };
  const setCut = (index: number, edge: 0 | 1, ms: number) =>
    setCuts((list) =>
      list.map((c, i) => {
        if (i !== index) return c;
        const to = Math.min(duration, Math.max(0, ms));
        return edge === 0 ? [Math.min(to, c[1] - 100), c[1]] : [c[0], Math.max(to, c[0] + 100)];
      }),
    );
  const addCut = () => {
    // Start the new skip where the playhead is, a tenth of the sound long.
    const from = Math.min(Math.max(playhead ?? start + (last - start) / 2, start), last - 200);
    setCuts([...cuts, [Math.round(from), Math.round(Math.min(last, from + Math.max(500, duration / 10)))]]);
  };
  /** Sets the start and end to where the sound becomes audible and where it dies away. */
  const trimSilence = async () => {
    setFinding(true);
    try {
      const [from, to] = await api.audiblePart(asset.id);
      if (from === 0 && to === null) useStore.getState().notify("No silence found at the start or the end.", "info");
      setStart(from);
      setEnd(to);
      // Skips that now fall outside the kept part no longer mean anything.
      setCuts((list) => list.filter(([a, b]) => b > from && a < (to ?? duration)));
    } catch (e) {
      fail(e);
    }
    setFinding(false);
  };
  const save = async () => {
    try {
      await stopPreview();
      await api.setEdit(asset.id, whole ? null : edit);
      await useStore.getState().refresh();
      onClose();
    } catch (e) {
      fail(e);
    }
  };

  const row = "flex items-center gap-2 text-xs";
  return (
    <Modal title={`Trim “${asset.name}”`} onClose={onClose} wide>
      <div className="space-y-4 overflow-y-auto p-4">
        <p className="text-xs leading-relaxed text-faint">
          Drag the orange handles to set where the sound starts and ends. Red parts are skipped. Click or drag on the
          picture to move the play marker and listen from there. The file on disk is not changed.
        </p>

        <Timeline
          peaks={peaks}
          duration={duration}
          start={start}
          end={last}
          cuts={cuts}
          playhead={playhead}
          onStart={(ms) => setStart(Math.max(0, ms))}
          onEnd={(ms) => setEnd(ms >= duration ? null : ms)}
          onCut={setCut}
          onScrub={(ms) => {
            setScrubbing(true);
            setCursor(ms);
          }}
          onSeek={(ms) => {
            setScrubbing(false);
            setCursor(ms);
            void preview(ms);
          }}
          following={previewing !== null && !scrubbing}
        />

        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <button
            onClick={() => void (previewing ? stopPreview() : preview(null))}
            className={`${buttonClass} flex items-center gap-2`}
            title="Plays through the Sound effects channel, with the trim as it is set now"
          >
            {previewing ? <StopIcon width={12} height={12} /> : <PlayIcon width={12} height={12} />}
            {previewing ? "Stop" : "Listen from the start"}
          </button>
          <button
            onClick={() => void trimSilence()}
            disabled={finding}
            className={buttonClass}
            title="Sets the start and end to where the sound can first and last be heard, so it begins at once and stops when it is over"
          >
            {finding ? "Looking…" : "Trim silence"}
          </button>
          <span className="text-xs tabular-nums text-dim">
            {previewing ? `${formatExact(previewing.position_ms)} · ` : ""}
            Plays for {formatTime(editedLength(duration, edit))} of {formatTime(duration)}
          </span>
          <span className={`${row} ml-auto`}>
            <span className="text-dim">Start</span>
            <TimeBox value={start} onChange={(ms) => setStart(Math.min(Math.max(0, ms ?? 0), last - 200))} label="Start time" />
            <span className="text-dim">End</span>
            <TimeBox
              value={end}
              onChange={(ms) => setEnd(ms === null || ms >= duration ? null : Math.max(ms, start + 200))}
              placeholder={formatExact(duration)}
              label="End time"
            />
          </span>
        </div>

        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm text-dim">Parts to skip</span>
            <button onClick={addCut} className="flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-medium text-dim hover:bg-raised hover:text-ink">
              <PlusIcon width={12} height={12} /> Add a part to skip
            </button>
          </div>
          {cuts.length === 0 && <p className="text-xs text-faint">Nothing is skipped. A new skip appears where you last clicked; drag its red edges.</p>}
          {cuts.map(([from, to], i) => (
            <div key={i} className={row}>
              <span className="size-2.5 rounded-sm bg-danger/70" />
              <span className="text-faint">from</span>
              <TimeBox value={from} onChange={(ms) => ms !== null && setCut(i, 0, ms)} label={`Skip ${i + 1} from`} />
              <span className="text-faint">to</span>
              <TimeBox value={to} onChange={(ms) => ms !== null && setCut(i, 1, ms)} label={`Skip ${i + 1} to`} />
              <button
                onClick={() => void preview(Math.max(start, from - 2000))}
                className="rounded px-1.5 py-0.5 font-medium text-accent hover:bg-accent/10"
                title="Listen from two seconds before this skip, to hear how the join sounds"
              >
                Hear the join
              </button>
              <button onClick={() => setCuts(cuts.filter((_, j) => j !== i))} aria-label={`Remove skip ${i + 1}`} className="ml-auto flex size-7 items-center justify-center rounded text-faint hover:bg-raised hover:text-ink">
                <CloseIcon width={10} height={10} />
              </button>
            </div>
          ))}
        </div>

        <div className="flex items-center gap-2 border-t border-line pt-3">
          {resetting ? (
            <span className="mr-auto flex items-center gap-2 text-xs">
              <span className="text-dim">Remove the trim and all skips?</span>
              <button
                onClick={() => {
                  setStart(0);
                  setEnd(null);
                  setCuts([]);
                  setResetting(false);
                }}
                className="rounded bg-danger px-2 py-1 font-medium text-white"
              >
                Reset
              </button>
              <button onClick={() => setResetting(false)} className="rounded bg-line px-2 py-1 font-medium">
                Keep
              </button>
            </span>
          ) : (
            <button
              onClick={() => setResetting(true)}
              disabled={whole}
              title="Go back to playing the whole file"
              className="mr-auto text-xs font-medium text-dim underline-offset-2 hover:text-ink hover:underline disabled:opacity-40"
            >
              Reset
            </button>
          )}
          <button onClick={onClose} className={buttonClass}>
            Cancel
          </button>
          <button onClick={() => void save()} className={primaryClass}>
            Save
          </button>
        </div>
      </div>
    </Modal>
  );
}
