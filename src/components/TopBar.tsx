import { api } from "../api";
import { plural } from "../format";
import { useStore } from "../store";
import { Fader } from "./Fader";
import { SearchIcon, StopIcon } from "./icons";
import { CampaignPicker } from "./Campaigns";

export function TopBar({ onPalette }: { onPalette: () => void }) {
  const master = useStore((s) => s.engine?.master_volume ?? 0.8);
  const armed = useStore((s) => s.engine?.panic_armed ?? false);
  const playing = useStore((s) => (s.engine?.sounds.length ?? 0) > 0);
  const panic = useStore((s) => s.panic);

  return (
    <header className="flex h-14 shrink-0 items-center gap-6 border-b border-line bg-panel px-4">
      <div className="flex items-baseline gap-2">
        <span className="text-lg font-semibold tracking-tight">
          GM<span className="text-accent">Audio</span>
        </span>
      </div>

      <ScanStatusPill />
      <CampaignPicker />

      <button
        onClick={onPalette}
        title="Find a scene, pad or sound by name and fire it"
        className="ml-auto flex h-9 items-center gap-2 rounded-lg border border-line px-3 text-xs text-dim hover:border-dim hover:text-ink"
      >
        <SearchIcon width={14} height={14} /> Find and play
        <kbd className="rounded bg-raised px-1.5 py-0.5 text-[11px] font-medium">Ctrl K</kbd>
      </button>

      <div className="flex items-center gap-3">
        <span className="text-xs font-medium uppercase tracking-wider text-dim">Master</span>
        <Fader
          value={master}
          onChange={(v) => void api.setMaster(v)}
          color="var(--color-ink)"
          label="Master volume"
          className="w-44"
        />
        <span className="w-9 text-right text-xs tabular-nums text-dim">{Math.round(master * 100)}%</span>
      </div>

      <button
        onClick={() => void panic()}
        title={
          armed
            ? "Press again to cut the sound instantly (Esc)"
            : "Fade everything out over one second. Press twice to cut instantly. (Esc)"
        }
        className={`flex h-10 items-center gap-2 rounded-lg px-4 text-sm font-semibold transition-colors ${
          armed
            ? "panic-armed bg-danger text-white"
            : playing
              ? "bg-danger/90 text-white hover:bg-danger"
              : "bg-raised text-dim hover:bg-danger hover:text-white"
        }`}
      >
        <StopIcon />
        {armed ? "Cut now" : "Stop all"}
        <kbd className="rounded bg-black/25 px-1.5 py-0.5 text-[11px] font-medium">Esc</kbd>
      </button>
    </header>
  );
}

/** What the library is doing in the background, in plain words. */
function ScanStatusPill() {
  const scan = useStore((s) => s.scan);
  const librarySize = useStore((s) => s.librarySize);

  if (scan.scanning) {
    return (
      <Status busy>
        Looking through your folder… {plural(scan.files_found, "sound")} found
      </Status>
    );
  }
  if (scan.remaining > 0) {
    const done = scan.analyzed;
    const all = scan.analyzed + scan.remaining;
    return (
      <Status busy progress={all > 0 ? done / all : 0}>
        Measuring loudness: {done.toLocaleString()} of {all.toLocaleString()}. You can already play
        everything.
      </Status>
    );
  }
  if (librarySize === 0) return null;
  return <Status>{plural(librarySize, "sound")} in your library</Status>;
}

function Status({ children, busy, progress }: { children: React.ReactNode; busy?: boolean; progress?: number }) {
  return (
    <div className="flex min-w-0 items-center gap-2.5 text-xs text-dim" role="status">
      <span className={`size-2 shrink-0 rounded-full ${busy ? "animate-pulse bg-accent" : "bg-faint"}`} />
      <span className="truncate">{children}</span>
      {progress != null && (
        <span className="h-1 w-24 shrink-0 overflow-hidden rounded-full bg-line">
          <span className="block h-full bg-accent transition-[width]" style={{ width: `${progress * 100}%` }} />
        </span>
      )}
    </div>
  );
}
