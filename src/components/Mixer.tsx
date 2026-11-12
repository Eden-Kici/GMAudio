import { api, CHANNELS, type Channel, type ChannelState, type SoundState } from "../api";
import { formatTime } from "../format";
import { useStore } from "../store";
import { Fader } from "./Fader";
import { LoopIcon, StopIcon, VolumeIcon } from "./icons";

const color: Record<Channel, string> = {
  music: "var(--color-music)",
  ambience: "var(--color-ambience)",
  sfx: "var(--color-sfx)",
  stingers: "var(--color-stingers)",
};

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
  state: ChannelState | undefined;
  sounds: SoundState[];
}

function Strip({ id, label, hint, state, sounds }: StripProps) {
  const stopChannel = useStore((s) => s.stopChannel);
  const volume = state?.volume ?? 0.8;
  const muted = state?.muted ?? false;
  const fade = state?.fade_ms ?? 0;
  const fades = FADES.includes(fade) ? FADES : [...FADES, fade].sort((a, b) => a - b);
  const busy = sounds.some((s) => !s.stopping);

  return (
    <div className="flex min-w-0 flex-col bg-panel px-4 pb-3 pt-3">
      <div className="flex items-center gap-2">
        <span className="size-2.5 shrink-0 rounded-full" style={{ background: color[id] }} />
        <h3 className="truncate text-sm font-semibold" title={hint}>
          {label}
        </h3>
        <button
          onClick={() => void stopChannel(id)}
          disabled={!busy}
          title={`Fade out everything on ${label.toLowerCase()}`}
          className="ml-auto flex h-7 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-dim enabled:hover:bg-raised enabled:hover:text-ink disabled:opacity-30"
        >
          <StopIcon width={12} height={12} /> Stop
        </button>
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
          color={color[id]}
          muted={muted}
          label={`${label} volume`}
          className="min-w-0 flex-1"
        />
        <span className="w-9 shrink-0 text-right text-xs tabular-nums text-dim">
          {muted ? "Muted" : `${Math.round(volume * 100)}%`}
        </span>
      </div>

      <select
        value={fade}
        onChange={(e) => void api.setChannel(id, { fadeMs: Number(e.target.value) })}
        title="How long sounds on this channel take to fade in, fade out and crossfade"
        aria-label={`${label} fade time`}
        className="mt-1 h-7 w-28 rounded-md border border-line bg-bg px-1.5 text-xs text-dim hover:text-ink focus:border-accent focus:outline-none"
      >
        {fades.map((ms) => (
          <option key={ms} value={ms}>
            {fadeLabel(ms)}
          </option>
        ))}
      </select>

      <div className="mt-2 min-h-0 flex-1 space-y-1 overflow-y-auto">
        {sounds.length === 0 ? (
          <p className="pt-1 text-xs text-faint">Nothing playing</p>
        ) : (
          sounds.map((s) => <Playing key={s.id} sound={s} color={color[id]} />)
        )}
      </div>
    </div>
  );
}

function Playing({ sound, color }: { sound: SoundState; color: string }) {
  const stop = useStore((s) => s.stopSound);
  const duration = sound.duration_ms;
  const progress = duration ? Math.min(1, sound.position_ms / duration) : 0;

  return (
    <div className={`relative overflow-hidden rounded-md bg-raised ${sound.stopping ? "opacity-50" : ""}`}>
      <span
        className="absolute inset-y-0 left-0 opacity-25"
        style={{ width: `${progress * 100}%`, background: color }}
      />
      <div className="relative flex h-8 items-center gap-2 pl-2.5 pr-1">
        {sound.looping && <LoopIcon width={12} height={12} className="shrink-0 text-dim" aria-label="Looping" />}
        <span className="min-w-0 flex-1 truncate text-xs font-medium">{sound.name}</span>
        <span className="shrink-0 text-[11px] tabular-nums text-dim">
          {sound.stopping ? "Fading out" : `${formatTime(sound.position_ms)} / ${formatTime(duration)}`}
        </span>
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
