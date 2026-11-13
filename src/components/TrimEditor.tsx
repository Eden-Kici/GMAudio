import { useState } from "react";
import { api, assetChannels, type Asset, type AssetEdit } from "../api";
import { formatTime } from "../format";
import { message, useStore } from "../store";
import { CloseIcon, PlusIcon } from "./icons";
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

function TimeBox({ value, onChange, placeholder, label }: { value: string; onChange: (v: string) => void; placeholder?: string; label: string }) {
  const bad = value.trim() !== "" && parseTime(value) === null;
  return (
    <input
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      aria-label={label}
      aria-invalid={bad}
      spellCheck={false}
      className={`${inputBase} h-9 w-24 text-center tabular-nums ${bad ? "border-danger" : ""}`}
    />
  );
}

/** Chooses which parts of a sound play. The file itself is not changed. */
export function TrimEditor({ asset, onClose }: { asset: Asset; onClose: () => void }) {
  const duration = asset.duration_ms ?? 0;
  const [start, setStart] = useState(asset.edit?.start_ms ? formatExact(asset.edit.start_ms) : "");
  const [end, setEnd] = useState(asset.edit?.end_ms != null ? formatExact(asset.edit.end_ms) : "");
  const [cuts, setCuts] = useState<[string, string][]>(asset.edit?.cuts.map(([a, b]) => [formatExact(a), formatExact(b)]) ?? []);
  /** Where this sound is right now, if it is playing. */
  const live = useStore((s) => s.engine?.sounds.find((x) => x.asset_id === asset.id && !x.stopping)?.position_ms ?? null);
  const here = live === null ? "" : formatExact(live);

  const edit: AssetEdit = {
    start_ms: parseTime(start) ?? 0,
    end_ms: end.trim() ? parseTime(end) : null,
    cuts: cuts.flatMap(([a, b]) => {
      const from = parseTime(a);
      const to = parseTime(b);
      return from !== null && to !== null && to > from ? [[from, to] as [number, number]] : [];
    }),
  };
  const invalid =
    (start.trim() !== "" && parseTime(start) === null) ||
    (end.trim() !== "" && parseTime(end) === null) ||
    cuts.some(([a, b]) => parseTime(a) === null || parseTime(b) === null || parseTime(b)! <= parseTime(a)!) ||
    (edit.end_ms !== null && edit.end_ms <= edit.start_ms);

  const save = async (thenPlay: boolean) => {
    try {
      const whole = edit.start_ms === 0 && edit.end_ms === null && edit.cuts.length === 0;
      await api.setEdit(asset.id, whole ? null : edit);
      await useStore.getState().refresh();
      if (thenPlay) {
        // Restart it where it is playing; otherwise on the channel it is tagged for.
        const playing = useStore.getState().engine?.sounds.find((x) => x.asset_id === asset.id && !x.stopping);
        if (playing) await api.stopSound(playing.id);
        await api.play(asset.id, playing?.channel ?? assetChannels(asset)[0] ?? "sfx", { interrupt: true });
      } else {
        onClose();
      }
    } catch (e) {
      useStore.getState().notify(message(e));
    }
  };

  const useHere = "rounded px-1.5 py-0.5 text-[11px] font-medium text-accent hover:bg-accent/10";
  const row = "flex items-center gap-2 text-sm";

  return (
    <Modal title={`Trim “${asset.name}”`} onClose={onClose}>
      <div className="space-y-4 p-4">
        <p className="text-xs leading-relaxed text-faint">
          Choose which parts play. Times are minutes:seconds into the file, such as 1:23.5. The file on disk is not
          changed, and you can undo this at any time.
        </p>

        <div className={row}>
          <span className="w-24 text-dim">Start at</span>
          <TimeBox value={start} onChange={setStart} placeholder="0:00.0" label="Start time" />
          {here && (
            <button onClick={() => setStart(here)} className={useHere}>
              Use {here}
            </button>
          )}
        </div>
        <div className={row}>
          <span className="w-24 text-dim">End at</span>
          <TimeBox value={end} onChange={setEnd} placeholder={formatExact(duration)} label="End time" />
          {here && (
            <button onClick={() => setEnd(here)} className={useHere}>
              Use {here}
            </button>
          )}
        </div>

        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-sm text-dim">Skip these parts</span>
            <button onClick={() => setCuts([...cuts, [here, ""]])} className="flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-medium text-dim hover:bg-raised hover:text-ink">
              <PlusIcon width={12} height={12} /> Add a part to skip
            </button>
          </div>
          {cuts.length === 0 && <p className="text-xs text-faint">Nothing is skipped.</p>}
          {cuts.map(([from, to], i) => (
            <div key={i} className={row}>
              <span className="w-24 text-xs text-faint">from</span>
              <TimeBox value={from} onChange={(v) => setCuts(cuts.map((c, j) => (j === i ? [v, c[1]] : c)))} label={`Skip ${i + 1} from`} />
              <span className="text-xs text-faint">to</span>
              <TimeBox value={to} onChange={(v) => setCuts(cuts.map((c, j) => (j === i ? [c[0], v] : c)))} label={`Skip ${i + 1} to`} />
              {here && (
                <button onClick={() => setCuts(cuts.map((c, j) => (j === i ? [c[0], here] : c)))} className={useHere}>
                  to {here}
                </button>
              )}
              <button onClick={() => setCuts(cuts.filter((_, j) => j !== i))} aria-label={`Remove skip ${i + 1}`} className="ml-auto flex size-7 items-center justify-center rounded text-faint hover:bg-raised hover:text-ink">
                <CloseIcon width={10} height={10} />
              </button>
            </div>
          ))}
        </div>

        <p className="text-xs text-dim">
          {live !== null ? `Playing now, at ${here}. ` : "Tip: play the sound and the current time can be filled in with one click. "}
          {!invalid && duration > 0 && `Plays for ${formatTime(editedLength(duration, edit))} of ${formatTime(duration)}.`}
          {invalid && <span className="text-danger">Check the times: each must be a time, and every “to” later than its “from”.</span>}
        </p>

        <div className="flex items-center gap-2 border-t border-line pt-3">
          <button
            onClick={() => {
              setStart("");
              setEnd("");
              setCuts([]);
            }}
            className="mr-auto text-xs font-medium text-dim underline-offset-2 hover:text-ink hover:underline"
          >
            Play the whole file again
          </button>
          <button onClick={() => void save(true)} disabled={invalid} className={buttonClass} title="Save and play it from its new start">
            Save and listen
          </button>
          <button onClick={() => void save(false)} disabled={invalid} className={primaryClass}>
            Save
          </button>
        </div>
      </div>
    </Modal>
  );
}
