import { useState } from "react";
import { api, CHANNELS, type Channel, type ChannelState, type QueuedState, type SoundState } from "../api";
import { formatTime } from "../format";
import { useStore } from "../store";
import { DRAG_TYPE } from "./Browser";
import { Fader } from "./Fader";
import { CloseIcon, LoopIcon, NextIcon, PlayIcon, StopIcon, VolumeIcon } from "./icons";

const FADES = [0, 150, 500, 1000, 2000, 3000, 5000, 8000, 12000];

function fadeLabel(ms: number): string {
  if (ms === 0) return "No fade";
  return `${(ms / 1000).toLocaleString(undefined, { maximumFractionDigits: 2 })} s fade`;
}

export function Mixer() {
  const channels = useStore((s) => s.engine?.channels);
  const sounds = useStore((s) => s.engine?.sounds);

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
}

function Strip({ id, label, hint, color, state, sounds }: StripProps) {
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
  const busy = sounds.some((s) => !s.stopping) || queue.length > 0;
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
        <select
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
        </select>
      </div>

      <div className="mt-2 min-h-0 flex-1 space-y-1 overflow-y-auto">
        {sounds.length === 0 && queue.length === 0 ? (
          <p className="pt-1 text-xs text-faint">{over ? "Drop to play here" : "Nothing playing"}</p>
        ) : (
          <>
            {sounds.map((s) => (
              <Playing key={s.id} sound={s} color={color} />
            ))}
            {queue.length > 0 && <p className="pt-1 text-[11px] font-medium uppercase tracking-wider text-faint">Up next</p>}
            {queue.map((q) => (
              <Queued key={q.id} item={q} />
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

function Playing({ sound, color }: { sound: SoundState; color: string }) {
  const stop = useStore((s) => s.stopSound);
  const duration = sound.duration_ms;
  const progress = duration ? Math.min(1, sound.position_ms / duration) : 0;

  return (
    <div className={`relative overflow-hidden rounded-md bg-raised ${sound.stopping ? "opacity-50" : ""}`}>
      <span className="absolute inset-y-0 left-0 opacity-25" style={{ width: `${progress * 100}%`, background: color }} />
      <div className="relative flex h-8 items-center gap-1.5 pl-2.5 pr-1">
        <span className="min-w-0 flex-1 truncate text-xs font-medium">{sound.name}</span>
        <span className="shrink-0 text-[11px] tabular-nums text-dim">
          {sound.stopping ? "Fading out" : `${formatTime(sound.position_ms)} / ${formatTime(duration)}`}
        </span>
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

function Queued({ item }: { item: QueuedState }) {
  const remove = useStore((s) => s.stopSound);
  return (
    <div className="flex h-7 items-center gap-1.5 rounded-md border border-line pl-2.5 pr-1">
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
