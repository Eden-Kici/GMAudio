import { useEffect, useRef, useState } from "react";
import { api, type Doc, type Quantize } from "../api";
import { playSet, playStinger } from "../actions";
import { assetName, useDocs, useScoped } from "../docs";
import { MoreOptions, ScopeField } from "./Campaigns";
import { useStore } from "../store";
import { newSet, type MusicSet, type Stinger, type Tier } from "../types";
import { ArrowIcon, CloseIcon, PencilIcon, PlusIcon } from "./icons";
import {
  AssetList,
  AssetPicker,
  buttonClass,
  ColorPicker,
  ConfirmButton,
  Field,
  formatDb,
  inputBase,
  inputClass,
  primaryClass,
  Segmented,
  Slider,
  Toggle,
} from "./ui";

const TIMING: { value: Quantize; label: string; title: string }[] = [
  { value: "now", label: "At once", title: "Change straight away" },
  { value: "beat", label: "Next beat", title: "Wait for the next beat" },
  { value: "bar", label: "Next bar", title: "Wait for the start of the next bar" },
  { value: "phrase", label: "Next phrase", title: "Wait for the start of the next phrase (several bars)" },
  { value: "loop", label: "End of loop", title: "Wait until the music reaches its end" },
];

/** A slot holding one sound: shows its name, lets the GM pick or clear it. */
function AssetSlot({ id, onChange, title, placeholder }: { id: number | null; onChange: (id: number | null) => void; title: string; placeholder: string }) {
  const assets = useDocs((s) => s.assets);
  const needAssets = useDocs((s) => s.needAssets);
  const [picking, setPicking] = useState(false);
  useEffect(() => {
    if (id !== null) needAssets([id]);
  }, [id, needAssets]);
  return (
    <>
      <div className="flex h-8 min-w-0 flex-1 items-center gap-1 rounded-md bg-bg pl-2.5 pr-1">
        <button onClick={() => setPicking(true)} className={`min-w-0 flex-1 truncate text-left text-sm ${id === null ? "text-faint" : ""}`}>
          {id === null ? placeholder : assetName(assets, id)}
        </button>
        {id !== null && (
          <button onClick={() => onChange(null)} aria-label="Clear" className="rounded p-1 text-dim hover:bg-line hover:text-ink">
            <CloseIcon width={12} height={12} />
          </button>
        )}
      </div>
      {picking && (
        <AssetPicker
          title={title}
          channel="music"
          chosen={id === null ? [] : [id]}
          onPick={(asset) => {
            useDocs.setState((s) => ({ assets: { ...s.assets, [asset.id]: asset } }));
            onChange(asset.id);
            setPicking(false);
          }}
          onClose={() => setPicking(false)}
        />
      )}
    </>
  );
}

/** Tap along to the music to set its tempo. */
function TapTempo({ onTempo }: { onTempo: (bpm: number) => void }) {
  const taps = useRef<number[]>([]);
  const [count, setCount] = useState(0);
  const tap = () => {
    const now = performance.now();
    const recent = taps.current.filter((t) => now - t < 2500).slice(-7);
    recent.push(now);
    taps.current = recent;
    setCount(recent.length);
    if (recent.length >= 4) {
      const beat = (recent[recent.length - 1] - recent[0]) / (recent.length - 1);
      onTempo(Math.round(600000 / beat) / 10);
    }
  };
  return (
    <button onClick={tap} className={buttonClass} title="Tap in time with the music, four times or more">
      {count > 0 && count < 4 ? `Tap ${4 - count} more` : "Tap tempo"}
    </button>
  );
}

export function SetsView() {
  const sets = useScoped("sets");
  const campaignId = useDocs((s) => s.campaignId);
  const save = useDocs((s) => s.save);
  const playing = useStore((s) => s.engine?.music_set?.id ?? null);
  const [editing, setEditing] = useState<number | null>(null);
  const edited = sets.find((s) => s.id === editing);

  return (
    <main className="flex min-h-0 min-w-0 flex-1">
      <div className="min-w-0 flex-1 overflow-y-auto p-5">
        <div className="mb-4 flex items-center gap-2">
          <h1 className="mr-auto text-lg font-semibold">Dynamic music</h1>
          <button onClick={async () => setEditing(await save("set", null, { ...newSet(`Music ${sets.length + 1}`), campaign_id: campaignId }))} className={primaryClass}>
            <PlusIcon /> New music set
          </button>
        </div>
        {sets.length === 0 ? (
          <div className="mx-auto mt-16 max-w-md text-center text-dim">
            <p className="text-[15px] text-ink">Music that follows the action</p>
            <p className="mt-2 text-sm leading-relaxed">
              A music set has levels, such as Calm, Tense and Combat. Switch level as the scene heats up and the music
              changes in time with its beat, without a jarring cut.
            </p>
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(230px,1fr))] gap-3">
            {sets.map((set) => (
              <div
                key={set.id}
                className={`group relative rounded-xl border bg-panel ${playing === set.id ? "border-(--c)" : "border-line hover:border-dim"}`}
                style={{ "--c": set.data.color } as React.CSSProperties}
              >
                <button onClick={() => void playSet(set)} title="Play from the first level" className="flex h-24 w-full flex-col justify-between rounded-xl p-3.5 text-left">
                  <span className="flex items-center gap-2">
                    <span className={`size-2.5 shrink-0 rounded-full bg-(--c) ${playing === set.id ? "ring-4 ring-(--c)/25" : ""}`} />
                    <span className="min-w-0 flex-1 truncate text-[15px] font-semibold">{set.data.name || "Untitled"}</span>
                  </span>
                  <span className="truncate text-xs text-dim">
                    {playing === set.id ? "Playing now" : `${set.data.tiers.map((t) => t.name).join(" → ")} · ${Math.round(set.data.bpm)} BPM`}
                  </span>
                </button>
                <button
                  onClick={() => setEditing(set.id)}
                  aria-label={`Edit ${set.data.name}`}
                  className="absolute bottom-2 right-2 hidden rounded-md bg-raised p-1.5 text-dim hover:text-ink group-hover:block group-focus-within:block"
                >
                  <PencilIcon width={14} height={14} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
      {edited && <SetEditor key={edited.id} set={edited} onClose={() => setEditing(null)} />}
    </main>
  );
}

function SetEditor({ set, onClose }: { set: Doc<MusicSet>; onClose: () => void }) {
  const save = useDocs((s) => s.save);
  const remove = useDocs((s) => s.remove);
  const advanced = useDocs((s) => s.advanced);
  const assets = useDocs((s) => s.assets);
  const data = set.data;
  const update = (change: Partial<MusicSet>) => void save("set", set.id, { ...data, ...change });
  const setTier = (at: number, change: Partial<Tier>) => update({ tiers: data.tiers.map((t, i) => (i === at ? { ...t, ...change } : t)) });
  const setStinger = (at: number, change: Partial<Stinger>) =>
    update({ stingers: data.stingers.map((s, i) => (i === at ? { ...s, ...change } : s)) });

  /** The first sound's measured tempo, if it has one. */
  const firstAsset = data.kind === "stems" ? data.stems[0]?.asset_id : data.tiers.find((t) => t.asset_id !== null)?.asset_id;
  const detected = firstAsset != null ? assets[firstAsset] : undefined;

  /** Fills in the tempo the first time a sound with a known tempo is added. */
  const adopt = (assetId: number, change: Partial<MusicSet>) => {
    const asset = useDocs.getState().assets[assetId];
    const fresh = data.bpm === 120 && data.offset_ms === 0;
    if (fresh && asset?.bpm) update({ ...change, bpm: asset.bpm, offset_ms: asset.beat_offset_ms ?? 0 });
    else update(change);
  };

  const moveTier = (at: number, by: number) => {
    const tiers = [...data.tiers];
    const [tier] = tiers.splice(at, 1);
    tiers.splice(at + by, 0, tier);
    update({ tiers });
  };

  return (
    <aside className="flex w-[28rem] shrink-0 flex-col border-l border-line bg-panel">
      <div className="flex items-center gap-2 border-b border-line px-4 py-3">
        <input value={data.name} onChange={(e) => update({ name: e.target.value })} placeholder="Name" aria-label="Music set name" className={`${inputClass} font-semibold`} />
        <button onClick={onClose} aria-label="Close editor" className="rounded p-2 text-dim hover:bg-raised hover:text-ink">
          <CloseIcon />
        </button>
      </div>

      <div className="min-h-0 flex-1 space-y-5 overflow-y-auto p-4">
        {(advanced || data.kind === "stems") && (
          <Field label="Built from">
            <Segmented
              value={data.kind}
              onChange={(kind) => update({ kind })}
              options={[
                { value: "tracks", label: "A track per level", title: "Each level is its own recording" },
                { value: "stems", label: "Layers of one piece", title: "Stems of equal length (drums, strings...) that play in sync; each level is a mix of them" },
              ]}
            />
          </Field>
        )}

        {data.kind === "stems" && (
          <Field label="Layers" hint="Same length, exported from one piece">
            <AssetList
              ids={data.stems.map((s) => s.asset_id)}
              onChange={(ids) => {
                const stems = ids.map((asset_id) => data.stems.find((s) => s.asset_id === asset_id) ?? { name: "", asset_id });
                // Keep each level's per-layer volumes lined up with the layers.
                const tiers = data.tiers.map((t) => ({
                  ...t,
                  stems: stems.map((stem) => {
                    const was = data.stems.findIndex((s) => s.asset_id === stem.asset_id);
                    return was >= 0 ? (t.stems[was] ?? null) : 0;
                  }),
                }));
                const added = ids.find((id) => !data.stems.some((s) => s.asset_id === id));
                if (added !== undefined) adopt(added, { stems, tiers });
                else update({ stems, tiers });
              }}
              addLabel="Add a layer"
              pickerTitle="Add layers (stems)"
              channel="music"
              ordered
            />
          </Field>
        )}

        <Field label="Levels" hint="From quietest to most intense">
          <div className="space-y-2">
            {data.tiers.map((tier, at) => (
              <div key={at} className="space-y-1.5 rounded-md bg-raised/60 p-2">
                <div className="flex items-center gap-1.5">
                  <span className="w-4 text-center text-xs font-semibold text-faint">{at + 1}</span>
                  <input value={tier.name} onChange={(e) => setTier(at, { name: e.target.value })} placeholder="Level name" className={`${inputBase} h-8 w-28 shrink-0`} />
                  {data.kind === "tracks" && (
                    <AssetSlot
                      id={tier.asset_id}
                      onChange={(asset_id) => (asset_id === null ? setTier(at, { asset_id }) : adopt(asset_id, { tiers: data.tiers.map((t, i) => (i === at ? { ...t, asset_id } : t)) }))}
                      title={`Track for ${tier.name || "this level"}`}
                      placeholder="Choose a track"
                    />
                  )}
                  <span className="ml-auto flex shrink-0">
                    <button disabled={at === 0} onClick={() => moveTier(at, -1)} aria-label="Move up" className="rounded p-1 text-dim enabled:hover:bg-line disabled:opacity-25">
                      <ArrowIcon up width={12} height={12} />
                    </button>
                    <button disabled={at === data.tiers.length - 1} onClick={() => moveTier(at, 1)} aria-label="Move down" className="rounded p-1 text-dim enabled:hover:bg-line disabled:opacity-25">
                      <ArrowIcon width={12} height={12} />
                    </button>
                    <button
                      disabled={data.tiers.length <= 1}
                      onClick={() => update({ tiers: data.tiers.filter((_, i) => i !== at) })}
                      aria-label="Remove level"
                      className="rounded p-1 text-dim enabled:hover:bg-line disabled:opacity-25"
                    >
                      <CloseIcon width={12} height={12} />
                    </button>
                  </span>
                </div>
                {data.kind === "stems" &&
                  data.stems.map((stem, s) => {
                    const level = tier.stems[s] ?? null;
                    const setLevel = (value: number | null) => setTier(at, { stems: data.stems.map((_, i) => (i === s ? value : (tier.stems[i] ?? null))) });
                    return (
                      <div key={stem.asset_id} className="flex items-center gap-2 pl-5">
                        <button
                          onClick={() => setLevel(level === null ? 0 : null)}
                          aria-pressed={level !== null}
                          className={`h-6 w-36 shrink-0 truncate rounded px-2 text-left text-xs font-medium ${level !== null ? "bg-accent/20 text-accent" : "bg-bg text-faint"}`}
                          title={level !== null ? "Playing in this level. Click to leave it out." : "Left out of this level. Click to add it."}
                        >
                          {assetName(assets, stem.asset_id)}
                        </button>
                        {level !== null && <Slider value={level} min={-24} max={6} onChange={setLevel} format={formatDb} label="Layer volume" className="flex-1" />}
                      </div>
                    );
                  })}
              </div>
            ))}
            <button
              onClick={() => update({ tiers: [...data.tiers, { name: `Level ${data.tiers.length + 1}`, asset_id: null, stems: data.stems.map(() => 0) }] })}
              className="flex h-8 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-dim hover:bg-raised hover:text-ink"
            >
              <PlusIcon width={12} height={12} /> Add a level
            </button>
          </div>
        </Field>

        <Field label="Tempo" hint={detected?.bpm ? `Measured: ${detected.bpm} BPM` : "Used to change level in time"}>
          <div className="flex flex-wrap items-center gap-2">
            <span className="flex h-9 items-center rounded-md bg-bg px-3 text-sm font-semibold tabular-nums">{Math.round(data.bpm * 10) / 10} BPM</span>
            <button onClick={() => update({ bpm: Math.round(data.bpm * 5) / 10 })} className={buttonClass} title="The beat is half as fast as this">
              Half
            </button>
            <button onClick={() => update({ bpm: Math.round(data.bpm * 20) / 10 })} className={buttonClass} title="The beat is twice as fast as this">
              Double
            </button>
            <TapTempo onTempo={(bpm) => update({ bpm })} />
            {detected?.bpm && detected.bpm !== data.bpm && (
              <button onClick={() => update({ bpm: detected.bpm!, offset_ms: detected.beat_offset_ms ?? 0 })} className="text-xs text-dim underline hover:text-ink">
                Use measured
              </button>
            )}
          </div>
          {advanced && (
            <div className="grid grid-cols-2 gap-x-3 gap-y-2 pt-1 text-xs text-dim">
              {(
                [
                  ["Beats per minute", "bpm", 20, 400, 0.1],
                  ["First beat at (ms)", "offset_ms", 0, 60000, 1],
                  ["Beats in a bar", "beats_per_bar", 1, 16, 1],
                  ["Bars in a phrase", "bars_per_phrase", 1, 64, 1],
                ] as const
              ).map(([label, key, min, max, step]) => (
                <label key={key} className="space-y-1">
                  {label}
                  <input
                    type="number"
                    min={min}
                    max={max}
                    step={step}
                    value={data[key]}
                    onChange={(e) => update({ [key]: Math.min(max, Math.max(min, Number(e.target.value) || min)) })}
                    className={`${inputBase} w-full h-8`}
                  />
                </label>
              ))}
            </div>
          )}
        </Field>

        {advanced && (
          <>
            <Field label="Level changes happen">
              <Segmented value={data.quantize} onChange={(quantize) => update({ quantize })} options={TIMING} />
            </Field>
            <Field label="Blend time" hint="How long the change itself takes">
              <Slider value={data.crossfade_ms} min={0} max={4000} step={100} onChange={(crossfade_ms) => update({ crossfade_ms })} format={(ms) => (ms === 0 ? "Cut" : `${ms / 1000} s`)} label="Blend time" />
            </Field>
          </>
        )}

        <Field label="Stingers" hint="Short cues played in time with the music">
          <div className="space-y-2">
            {data.stingers.map((stinger, at) => (
              <div key={at} className="space-y-1.5 rounded-md bg-raised/60 p-2">
                <div className="flex items-center gap-1.5">
                  <input value={stinger.name} onChange={(e) => setStinger(at, { name: e.target.value })} placeholder="Name, e.g. Victory" className={`${inputBase} h-8 w-28 shrink-0`} />
                  <AssetSlot
                    id={stinger.asset_id}
                    onChange={(asset_id) => setStinger(at, { asset_id, name: stinger.name || (asset_id !== null ? (useDocs.getState().assets[asset_id]?.name ?? "") : "") })}
                    title="Sound for this stinger"
                    placeholder="Choose a sound"
                  />
                  <button onClick={() => update({ stingers: data.stingers.filter((_, i) => i !== at) })} aria-label="Remove stinger" className="rounded p-1 text-dim hover:bg-line">
                    <CloseIcon width={12} height={12} />
                  </button>
                </div>
                <label className="flex items-center gap-2 pl-1 text-xs text-dim">
                  Then go to
                  <select
                    value={stinger.then_tier ?? ""}
                    onChange={(e) => setStinger(at, { then_tier: e.target.value === "" ? null : Number(e.target.value) })}
                    className={`${inputBase} h-7 flex-1`}
                  >
                    <option value="">the same level</option>
                    {data.tiers.map((t, i) => (
                      <option key={i} value={i}>
                        {t.name || `Level ${i + 1}`}
                      </option>
                    ))}
                  </select>
                </label>
                {advanced && (
                  <>
                    <Segmented value={stinger.quantize} onChange={(quantize) => setStinger(at, { quantize })} options={TIMING.slice(0, 3)} />
                    <Toggle on={stinger.duck} onChange={(duck) => setStinger(at, { duck })} label="Lower the music while it plays" />
                  </>
                )}
              </div>
            ))}
            <button
              onClick={() => update({ stingers: [...data.stingers, { name: "", asset_id: null, quantize: "beat", then_tier: null, duck: true }] })}
              className="flex h-8 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-dim hover:bg-raised hover:text-ink"
            >
              <PlusIcon width={12} height={12} /> Add a stinger
            </button>
          </div>
        </Field>

        <Field label="Colour">
          <ColorPicker value={data.color} onChange={(color) => update({ color })} />
        </Field>

        <ScopeField value={data} onChange={update} />
        <MoreOptions />
      </div>

      <div className="flex items-center justify-between border-t border-line px-4 py-3">
        <ConfirmButton
          label="Delete music set"
          confirm="Delete this music set?"
          onConfirm={() => {
            void remove("set", set.id);
            onClose();
          }}
        />
        <button onClick={() => void playSet({ id: set.id, data })} className={primaryClass}>
          Try it
        </button>
      </div>
    </aside>
  );
}

/** The dynamic music controls shown in the mixer while a set plays. */
export function SetPanel() {
  const state = useStore((s) => s.engine?.music_set ?? null);
  const set = useDocs((s) => s.sets.find((x) => x.id === state?.id));
  if (!state) return null;
  const color = set?.data.color ?? "var(--color-music)";
  return (
    <div className="space-y-2 rounded-md bg-raised p-2" style={{ "--c": color } as React.CSSProperties}>
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate text-xs font-semibold">{state.name}</span>
        <span className="flex gap-1" aria-label={`Beat ${state.beat + 1} of ${state.beats_per_bar}`}>
          {Array.from({ length: state.beats_per_bar }, (_, i) => (
            <span key={i} className={`size-2 rounded-full ${i === state.beat ? "bg-(--c)" : "bg-line"}`} />
          ))}
        </span>
        <span className="text-[11px] tabular-nums text-faint">{Math.round(state.bpm)} BPM</span>
      </div>
      <div className="flex flex-wrap gap-1" role="radiogroup" aria-label="Music level">
        {state.tiers.map((name, i) => (
          <button
            key={i}
            role="radio"
            aria-checked={state.tier === i}
            onClick={() => void api.setTier(i)}
            title={state.pending === i ? "Changing on the next bar…" : `Change the music to ${name}`}
            className={`h-7 flex-1 whitespace-nowrap rounded px-2 text-xs font-semibold ${
              state.tier === i ? "bg-(--c) text-bg" : state.pending === i ? "panic-armed bg-(--c)/30 text-ink" : "bg-bg text-dim hover:text-ink"
            }`}
          >
            {name || i + 1}
          </button>
        ))}
      </div>
      {set && set.data.stingers.some((s) => s.asset_id !== null) && (
        <div className="flex flex-wrap gap-1">
          {set.data.stingers
            .filter((s) => s.asset_id !== null)
            .map((stinger, i) => (
              <button
                key={i}
                onClick={() => void playStinger(stinger)}
                title={stinger.then_tier !== null ? `Play, then go to ${state.tiers[stinger.then_tier] ?? "another level"}` : "Play this stinger in time with the music"}
                className="h-6 rounded border border-stingers/50 px-2 text-[11px] font-medium text-stingers hover:bg-stingers/15"
              >
                {stinger.name || "Stinger"}
              </button>
            ))}
        </div>
      )}
    </div>
  );
}

