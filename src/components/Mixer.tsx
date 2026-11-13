import { useEffect, useState } from "react";
import { api, CHANNELS, type Channel, type ChannelState, type EmitterState, type QueuedState, type SoundState } from "../api";
import { useDocs } from "../docs";
import { SetPanel } from "./SetsView";
import { formatTime } from "../format";
import { useStore } from "../store";
import { DRAG_TYPE } from "./Browser";
import { Fader } from "./Fader";
import { CloseIcon, LoopIcon, NextIcon, PauseIcon, PlayIcon, StopIcon, VolumeIcon } from "./icons";

const FADES = [0, 150, 500, 1000, 2000, 3000, 5000, 8000, 12000];

function fadeLabel(ms: number): string {
  if (ms === 0) return "No fade";
  return `${(ms / 1000).toLocaleString(undefined, { maximumFractionDigits: 2 })} s fade`;
}

export function Mixer() {
  const channels = useStore((s) => s.engine?.channels);
  const sounds = useStore((s) => s.engine?.sounds);
  const emitters = useStore((s) => s.engine?.emitters);

  return (
    <section aria-label="Mixer" className="grid h-60 shrink-0 grid-cols-4 gap-px border-t border-line bg-line">
      {CHANNELS.map((c) => (
        <Strip
          key={c.id}
          id={c.id}
          label={c.label}
          hint={c.hint}
          color={c.color}
          state={channels?.find((x) => x.channel === c.id)}
          sounds={sounds?.filter((s) => s.channel === c.id) ?? []}
          emitters={c.id === "ambience" ? (emitters ?? []) : []}
          index={CHANNELS.indexOf(c)}
        />
      ))}
    </section>
  );
}

interface StripProps {
  id: Channel;
  label: string;
  hint: string;
  color: string;
  state: ChannelState | undefined;
  sounds: SoundState[];
  emitters: EmitterState[];
  index: number;
}

/** A thin bar showing how loud a channel is right now. */
function Meter({ index, color }: { index: number; color: string }) {
  const level = useStore((s) => s.engine?.levels[index] ?? 0);
  // Peak 0..1 on a dB-like curve, so quiet sounds still show.
  const width = level <= 0 ? 0 : Math.max(0, Math.min(1, 1 + Math.log10(level) / 2.5));
  return (
    <div className="mt-1 h-1 overflow-hidden rounded-full bg-line" aria-hidden>
      <div className="h-full rounded-full transition-[width] duration-75" style={{ width: `${width * 100}%`, background: level > 0.98 ? "var(--color-danger)" : color }} />
    </div>
  );
}

function Strip({ id, label, hint, color, state, sounds, emitters, index }: StripProps) {
  const advanced = useDocs((s) => s.advanced);
  const ducked = useStore((s) => id === "music" && (s.engine?.ducked ?? false));
  const solo = state?.solo ?? false;
  const hasSet = useStore((s) => id === "music" && s.engine?.music_set != null);
  const stopChannel = useStore((s) => s.stopChannel);
  const play = useStore((s) => s.play);
  const dragging = useStore((s) => s.dragging !== null);
  const [over, setOver] = useState(false);
  const volume = state?.volume ?? 0.8;
  const muted = state?.muted ?? false;
  const fade = state?.fade_ms ?? 0;
  const queue = state?.queue ?? [];
  const loopQueue = state?.loop_queue ?? true;
  const fades = FADES.includes(fade) ? FADES : [...FADES, fade].sort((a, b) => a - b);
  const busy = sounds.some((s) => !s.stopping) || queue.length > 0 || emitters.length > 0 || hasSet;
  const isMusic = id === "music";

  return (
    <div
      className={`relative flex min-w-0 flex-col bg-panel px-4 pb-3 pt-3 ${dragging ? "transition-shadow" : ""}`}
      style={over ? { boxShadow: `inset 0 0 0 2px ${color}` } : undefined}
      onDragOver={(e) => {
        if (!e.dataTransfer.types.includes(DRAG_TYPE)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
        setOver(true);
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setOver(false);
      }}
      onDrop={(e) => {
        setOver(false);
        const assetId = Number(e.dataTransfer.getData(DRAG_TYPE));
        if (!assetId) return;
        e.preventDefault();
        void play(assetId, id);
      }}
    >
      <div className="flex items-center gap-1.5">
        <span className="size-2.5 shrink-0 rounded-full" style={{ background: color }} />
        <h3 className="truncate text-sm font-semibold" title={hint}>
          {label}
        </h3>
        <div className="ml-auto flex items-center gap-0.5">
          {isMusic && (
            <>
              <button
                onClick={() => void api.setLoopQueue(!loopQueue)}
                aria-pressed={loopQueue}
                title={
                  loopQueue
                    ? "Repeat is on: when the queue ends it starts again. A single track repeats."
                    : "Repeat is off: music stops when the queue ends."
                }
                className={`flex h-7 items-center gap-1.5 rounded-md px-2 text-xs font-medium ${
                  loopQueue ? "text-music" : "text-faint hover:bg-raised hover:text-ink"
                }`}
              >
                <LoopIcon width={13} height={13} /> Repeat
              </button>
              <button
                onClick={() => void api.nextTrack()}
                disabled={queue.length === 0}
                title="Crossfade to the next track in the queue"
                className="flex h-7 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-dim enabled:hover:bg-raised enabled:hover:text-ink disabled:opacity-30"
              >
                <NextIcon width={12} height={12} /> Next
              </button>
            </>
          )}
          <button
            onClick={() => void stopChannel(id)}
            disabled={!busy}
            title={isMusic ? "Fade the music out and empty the queue" : `Fade out everything on ${label.toLowerCase()}`}
            className="flex h-7 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-dim enabled:hover:bg-raised enabled:hover:text-ink disabled:opacity-30"
          >
            <StopIcon width={12} height={12} /> Stop
          </button>
        </div>
      </div>

      <div className="mt-1.5 flex items-center gap-2">
        <button
          onClick={() => void api.setChannel(id, { muted: !muted })}
          aria-pressed={muted}
          title={muted ? "Unmute" : "Mute"}
          className={`flex size-8 shrink-0 items-center justify-center rounded-md ${
            muted ? "bg-danger/20 text-danger" : "text-dim hover:bg-raised hover:text-ink"
          }`}
        >
          <VolumeIcon muted={muted} />
        </button>
        {advanced && (
          <button
            onClick={() => void api.setChannel(id, { solo: !solo })}
            aria-pressed={solo}
            title={solo ? "Solo is on: only soloed channels are heard. Click to turn off." : "Solo: hear only this channel"}
            className={`flex size-8 shrink-0 items-center justify-center rounded-md text-xs font-bold ${
              solo ? "bg-accent text-bg" : "text-dim hover:bg-raised hover:text-ink"
            }`}
          >
            S
          </button>
        )}
        <Fader
          value={volume}
          onChange={(v) => void api.setChannel(id, { volume: v })}
          color={color}
          muted={muted}
          label={`${label} volume`}
          className="min-w-0 flex-1"
        />
        <span className="w-9 shrink-0 text-right text-xs tabular-nums text-dim">
          {muted ? "Muted" : `${Math.round(volume * 100)}%`}
        </span>
        {advanced && <select
          value={fade}
          onChange={(e) => void api.setChannel(id, { fadeMs: Number(e.target.value) })}
          title="How long sounds on this channel take to fade in, fade out and crossfade"
          aria-label={`${label} fade time`}
          className="h-7 w-[6.5rem] shrink-0 rounded-md border border-line px-1.5 text-xs text-dim hover:text-ink focus:border-accent focus:outline-none"
        >
          {fades.map((ms) => (
            <option key={ms} value={ms}>
              {fadeLabel(ms)}
            </option>
          ))}
        </select>}
      </div>
      {advanced && <Meter index={index} color={color} />}
      {ducked && <p className="mt-1 text-[11px] text-faint">Lowered while a stinger plays</p>}

      <div className="mt-2 min-h-0 flex-1 space-y-1 overflow-y-auto">
        {hasSet && <SetPanel />}
        {hasSet && sounds.length === 0 ? null : sounds.length === 0 && queue.length === 0 && emitters.length === 0 ? (
          <p className="pt-1 text-xs text-faint">{over ? "Drop to play here" : "Nothing playing"}</p>
        ) : (
          <>
            {sounds.map((s) => (
              <Playing key={s.id} sound={s} color={color} fade={fade} />
            ))}
            {emitters.map((e) => (
              <Emitter key={e.id} emitter={e} />
            ))}
            {queue.length > 0 && <p className="pt-1 text-[11px] font-medium uppercase tracking-wider text-faint">Up next</p>}
            {queue.map((q, i) => (
              <Queued key={q.id} item={q} index={i} />
            ))}
          </>
        )}
      </div>
    </div>
  );
}

function LoopToggle({ id, on, name }: { id: number; on: boolean; name: string }) {
  return (
    <button
      onClick={() => void api.setSoundLooping(id, !on)}
      aria-pressed={on}
      aria-label={on ? `Stop repeating ${name}` : `Repeat ${name}`}
      title={on ? "Repeating until you stop it. Click to let it finish." : "Repeat this sound until you stop it"}
      className={`flex size-6 shrink-0 items-center justify-center rounded ${
        on ? "bg-ink/15 text-ink" : "text-faint hover:bg-line hover:text-ink"
      }`}
    >
      <LoopIcon width={12} height={12} />
    </button>
  );
}

const QUEUE_DRAG = "application/x-gmaudio-queue";

/** Pause or resume, at once or with a fade. Opens a two-line menu. */
function PauseMenu({ sound, fade }: { sound: SoundState; fade: number }) {
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  useEffect(() => {
    if (!at) return;
    const close = () => setAt(null);
    window.addEventListener("pointerdown", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("blur", close);
    };
  }, [at]);
  const paused = sound.paused;
  const go = (fadeMs: number) => {
    setAt(null);
    void api.pauseSound(sound.id, !paused, fadeMs);
  };
  const seconds = Math.max(0.5, fade / 1000);
  const item = "block w-full px-3 py-1.5 text-left text-xs hover:bg-line";
  return (
    <>
      <button
        onClick={(e) => {
          const box = e.currentTarget.getBoundingClientRect();
          setAt(at ? null : { x: box.right, y: box.top });
        }}
        onPointerDown={(e) => e.stopPropagation()}
        aria-haspopup="menu"
        aria-expanded={at !== null}
        aria-label={paused ? `Resume ${sound.name}` : `Pause ${sound.name}`}
        title={paused ? "Paused. Click to resume." : "Pause"}
        className={`flex size-6 shrink-0 items-center justify-center rounded ${
          paused ? "bg-ink/15 text-ink" : "text-dim hover:bg-line hover:text-ink"
        }`}
      >
        {paused ? <PlayIcon width={10} height={10} /> : <PauseIcon width={10} height={10} />}
      </button>
      {at && (
        <div
          role="menu"
          onPointerDown={(e) => e.stopPropagation()}
          style={{ right: window.innerWidth - at.x, bottom: window.innerHeight - at.y + 4 }}
          className="fixed z-50 w-44 overflow-hidden rounded-lg border border-line bg-raised py-1 shadow-xl"
        >
          <button role="menuitem" onClick={() => go(0)} className={item}>
            {paused ? "Resume now" : "Pause now"}
          </button>
          <button role="menuitem" onClick={() => go(Math.round(seconds * 1000))} className={item}>
            {paused ? `Fade back in (${seconds} s)` : `Fade out and pause (${seconds} s)`}
          </button>
        </div>
      )}
    </>
  );
}

function Playing({ sound, color, fade }: { sound: SoundState; color: string; fade: number }) {
  const stop = useStore((s) => s.stopSound);
  /** A queued track is being dragged over this one, to swap with it. */
  const [swap, setSwap] = useState(false);
  const takesSwap = sound.channel === "music" && !sound.stopping;
  const duration = sound.duration_ms;
  /** Where the pointer is while dragging along the bar, 0 to 1. */
  const [drag, setDrag] = useState<number | null>(null);
  const progress = drag ?? (duration ? Math.min(1, sound.position_ms / duration) : 0);
  // Worth a seek bar: long enough to want to jump around in.
  const seekable = !!duration && duration >= 5000 && !sound.stopping;
  const at = (e: React.PointerEvent<HTMLDivElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - box.left) / box.width));
  };

  return (
    <div
      onDragOver={(e) => {
        if (!takesSwap || !e.dataTransfer.types.includes(QUEUE_DRAG)) return;
        e.preventDefault();
        e.stopPropagation();
        e.dataTransfer.dropEffect = "move";
        setSwap(true);
      }}
      onDragLeave={() => setSwap(false)}
      onDrop={(e) => {
        const id = Number(e.dataTransfer.getData(QUEUE_DRAG));
        setSwap(false);
        if (!takesSwap || !id) return;
        e.preventDefault();
        e.stopPropagation();
        void api.swapWithPlaying(id);
      }}
      className={`relative overflow-hidden rounded-md bg-raised ${sound.stopping ? "opacity-50" : ""} ${swap ? "ring-1 ring-accent" : ""}`}
    >
      <span className="absolute inset-y-0 left-0 opacity-25" style={{ width: `${progress * 100}%`, background: color }} />
      {seekable && (
        <div
          role="slider"
          aria-label={`Position in ${sound.name}`}
          aria-valuemin={0}
          aria-valuemax={Math.round(duration / 1000)}
          aria-valuenow={Math.round((progress * duration) / 1000)}
          aria-valuetext={formatTime(progress * duration)}
          tabIndex={0}
          title="Click or drag to jump to another point. Arrow keys: 5 seconds."
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            setDrag(at(e));
          }}
          onPointerMove={(e) => {
            if (drag !== null) setDrag(at(e));
          }}
          onPointerUp={(e) => {
            if (drag === null) return;
            void api.seek(sound.id, Math.round(at(e) * duration));
            // Hold the bar where it was dropped until the engine reports the jump.
            setTimeout(() => setDrag(null), 150);
          }}
          onPointerCancel={() => setDrag(null)}
          onKeyDown={(e) => {
            const step = e.key === "ArrowRight" ? 5000 : e.key === "ArrowLeft" ? -5000 : 0;
            if (!step) return;
            e.preventDefault();
            e.stopPropagation();
            void api.seek(sound.id, Math.max(0, sound.position_ms + step));
          }}
          className="group absolute inset-x-0 bottom-0 z-10 h-2.5 cursor-pointer touch-none"
        >
          <span className="absolute inset-x-0 bottom-0 h-0.5 bg-line transition-[height] group-hover:h-1.5" />
          <span
            className="absolute bottom-0 left-0 h-0.5 transition-[height] group-hover:h-1.5"
            style={{ width: `${progress * 100}%`, background: color }}
          />
        </div>
      )}
      <div className="relative flex h-8 items-center gap-1.5 pl-2.5 pr-1">
        <span className="min-w-0 flex-1 truncate text-xs font-medium">{sound.name}</span>
        {sound.channel === "ambience" && sound.looping && !sound.stopping && (
          <input
            type="range"
            min={-30}
            max={6}
            value={Math.round(sound.trim_db)}
            onChange={(e) => void api.setSoundTrim(sound.id, Number(e.target.value))}
            aria-label={`${sound.name} volume`}
            title="This layer's own volume"
            className="fader w-16 shrink-0"
            style={{ "--c": color, "--v": `${((Math.round(sound.trim_db) + 30) / 36) * 100}%`, height: 20 } as React.CSSProperties}
          />
        )}
        <span className="shrink-0 text-[11px] tabular-nums text-dim">
          {sound.stopping ? "Fading out" : swap ? "Drop to swap" : `${sound.paused ? "Paused · " : ""}${formatTime(drag !== null && duration ? drag * duration : sound.position_ms)} / ${formatTime(duration)}`}
        </span>
        {seekable && <PauseMenu sound={sound} fade={fade} />}
        {!sound.stopping && <LoopToggle id={sound.id} on={sound.looping} name={sound.name} />}
        <button
          onClick={() => void stop(sound.id)}
          disabled={sound.stopping}
          aria-label={`Stop ${sound.name}`}
          title="Fade this sound out"
          className="flex size-6 shrink-0 items-center justify-center rounded text-dim enabled:hover:bg-line enabled:hover:text-ink disabled:opacity-40"
        >
          <StopIcon width={10} height={10} />
        </button>
      </div>
    </div>
  );
}

/** A layer that plays a sound every so often. */
function Emitter({ emitter }: { emitter: EmitterState }) {
  const stop = useStore((s) => s.stopSound);
  const seconds = (ms: number) => Math.round(ms / 1000);
  return (
    <div className="flex h-7 items-center gap-1.5 rounded-md border border-dashed border-line pl-2.5 pr-1">
      <span className="min-w-0 flex-1 truncate text-xs text-dim">{emitter.name}</span>
      <span className="shrink-0 text-[11px] tabular-nums text-faint">
        every {seconds(emitter.min_ms)}–{seconds(emitter.max_ms)} s
      </span>
      <button
        onClick={() => void stop(emitter.id)}
        aria-label={`Stop ${emitter.name}`}
        title="Stop this occasional sound"
        className="flex size-6 shrink-0 items-center justify-center rounded text-faint hover:bg-line hover:text-ink"
      >
        <StopIcon width={10} height={10} />
      </button>
    </div>
  );
}

function Queued({ item, index }: { item: QueuedState; index: number }) {
  const remove = useStore((s) => s.stopSound);
  const [over, setOver] = useState(false);
  return (
    <div
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData(QUEUE_DRAG, String(item.id));
        e.dataTransfer.effectAllowed = "move";
      }}
      onDragOver={(e) => {
        if (!e.dataTransfer.types.includes(QUEUE_DRAG)) return;
        e.preventDefault();
        e.stopPropagation();
        e.dataTransfer.dropEffect = "move";
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        const id = Number(e.dataTransfer.getData(QUEUE_DRAG));
        setOver(false);
        if (!id) return;
        e.preventDefault();
        e.stopPropagation();
        if (id !== item.id) void api.moveQueued(id, index);
      }}
      title="Drag to change the order"
      className={`flex h-7 cursor-grab items-center gap-1.5 rounded-md border pl-2.5 pr-1 active:cursor-grabbing ${over ? "border-accent bg-accent/10" : "border-line"}`}
    >
      <span className="min-w-0 flex-1 truncate text-xs text-dim">{item.name}</span>
      <span className="shrink-0 text-[11px] tabular-nums text-faint">{formatTime(item.duration_ms)}</span>
      <button
        onClick={() => void api.playQueuedNow(item.id)}
        aria-label={`Play ${item.name} now`}
        title="Crossfade to this track now"
        className="flex size-6 shrink-0 items-center justify-center rounded text-faint hover:bg-line hover:text-ink"
      >
        <PlayIcon width={10} height={10} />
      </button>
      <LoopToggle id={item.id} on={item.looping} name={item.name} />
      <button
        onClick={() => void remove(item.id)}
        aria-label={`Remove ${item.name} from the queue`}
        title="Take this track out of the queue"
        className="flex size-6 shrink-0 items-center justify-center rounded text-faint hover:bg-line hover:text-ink"
      >
        <CloseIcon width={10} height={10} />
      </button>
    </div>
  );
}
