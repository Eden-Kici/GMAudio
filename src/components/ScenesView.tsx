import { useEffect, useState } from "react";
import type { Doc } from "../api";
import { activateScene, DEFAULT_PAD_KEYS, hotkeyFor, sceneFromCurrentMix, setHotkey } from "../actions";
import { assetName, useDocs, useScoped } from "../docs";
import { MoreOptions, ScopeField } from "./Campaigns";
import { useStore } from "../store";
import { newScene, type RandomLayer, type Scene } from "../types";
import { CloseIcon, PencilIcon, PlusIcon } from "./icons";
import {
  AssetList,
  AssetPicker,
  buttonClass,
  ColorPicker,
  ConfirmButton,
  Field,
  formatDb,
  HotkeyInput,
  inputBase,
  inputClass,
  primaryClass,
  Segmented,
  Slider,
  Toggle,
} from "./ui";

void DEFAULT_PAD_KEYS;

/** One line saying what a scene plays. */
export function sceneSummary(scene: Scene): string {
  const parts: string[] = [];
  if (scene.music.kind === "tracks") parts.push(scene.music.asset_ids.length === 1 ? "1 track" : `${scene.music.asset_ids.length} tracks`);
  else if (scene.music.kind === "set") parts.push("Dynamic music");
  else if (scene.music.kind === "silence") parts.push("No music");
  const layers = scene.layers.length + scene.emitters.length;
  if (layers > 0) parts.push(layers === 1 ? "1 ambience layer" : `${layers} ambience layers`);
  return parts.join(" · ") || "Empty";
}

export function SceneTile({ scene, index, onEdit }: { scene: Doc<Scene>; index: number; onEdit?: () => void }) {
  const active = useStore((s) => s.engine?.scene?.id === scene.id);
  const custom = useDocs((s) => s.hotkeys.find((h) => h.action.type === "scene" && h.action.id === scene.id)?.combo);
  const key = custom ?? (index < 12 ? `F${index + 1}` : null);
  return (
    <div
      className={`group relative rounded-xl border bg-panel transition-colors ${active ? "border-(--c)" : "border-line hover:border-dim"}`}
      style={{ "--c": scene.data.color } as React.CSSProperties}
    >
      <button onClick={() => void activateScene(scene)} className="flex h-24 w-full flex-col justify-between rounded-xl p-3.5 text-left" title="Switch to this scene">
        <span className="flex items-start gap-2">
          <span className={`mt-1.5 size-2.5 shrink-0 rounded-full bg-(--c) ${active ? "ring-4 ring-(--c)/25" : ""}`} />
          <span className="min-w-0 flex-1 truncate text-[15px] font-semibold">{scene.data.name || "Untitled scene"}</span>
          {key && <kbd className="rounded bg-raised px-1.5 py-0.5 text-[11px] font-medium text-dim">{key}</kbd>}
        </span>
        <span className="truncate text-xs text-dim">{active ? "Playing now" : sceneSummary(scene.data)}</span>
      </button>
      {onEdit && (
        <button
          onClick={onEdit}
          aria-label={`Edit ${scene.data.name}`}
          title="Edit this scene"
          className="absolute bottom-2 right-2 hidden rounded-md bg-raised p-1.5 text-dim hover:text-ink group-hover:block group-focus-within:block"
        >
          <PencilIcon width={14} height={14} />
        </button>
      )}
    </div>
  );
}

export function ScenesView() {
  const scenes = useScoped("scenes");
  const campaignId = useDocs((s) => s.campaignId);
  const save = useDocs((s) => s.save);
  const anythingPlaying = useStore((s) => (s.engine?.sounds.length ?? 0) > 0);
  const [editing, setEditing] = useState<number | null>(null);
  const edited = scenes.find((s) => s.id === editing);

  const create = async (scene: Scene) => setEditing(await save("scene", null, { ...scene, campaign_id: campaignId, session_id: null }));

  return (
    <main className="flex min-h-0 min-w-0 flex-1">
      <div className="min-w-0 flex-1 overflow-y-auto p-5">
        <div className="mb-4 flex flex-wrap items-center gap-2">
          <h1 className="mr-auto text-lg font-semibold">Scenes</h1>
          <button
            onClick={() => void create(sceneFromCurrentMix(`Scene ${scenes.length + 1}`))}
            disabled={!anythingPlaying}
            title="Make a scene from the music and ambience playing right now"
            className={buttonClass}
          >
            Save what's playing as a scene
          </button>
          <button onClick={() => void create(newScene(`Scene ${scenes.length + 1}`))} className={primaryClass}>
            <PlusIcon /> New scene
          </button>
        </div>
        {scenes.length === 0 ? (
          <div className="mx-auto mt-16 max-w-md text-center text-dim">
            <p className="text-[15px] text-ink">A scene is a whole soundscape on one button</p>
            <p className="mt-2 text-sm leading-relaxed">
              Music plus layers of ambience, for a place or a mood: the tavern, the crypt, a storm at sea. One click
              crossfades everything to it.
            </p>
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(210px,1fr))] gap-3">
            {scenes.map((scene, index) => (
              <SceneTile key={scene.id} scene={scene} index={index} onEdit={() => setEditing(scene.id)} />
            ))}
          </div>
        )}
      </div>
      {edited && <SceneEditor key={edited.id} scene={edited} onClose={() => setEditing(null)} />}
    </main>
  );
}

function SceneEditor({ scene, onClose }: { scene: Doc<Scene>; onClose: () => void }) {
  const save = useDocs((s) => s.save);
  const remove = useDocs((s) => s.remove);
  const advanced = useDocs((s) => s.advanced);
  const assets = useDocs((s) => s.assets);
  const needAssets = useDocs((s) => s.needAssets);
  const boards = useScoped("boards");
  const sets = useScoped("sets");
  const hotkey = useDocs((s) => s.hotkeys.find((h) => h.action.type === "scene" && h.action.id === scene.id));
  const index = useScoped("scenes").findIndex((x) => x.id === scene.id);
  const [addingLayer, setAddingLayer] = useState(false);
  const data = scene.data;
  const set = (change: Partial<Scene>) => void save("scene", scene.id, { ...data, ...change });
  void hotkeyFor;

  useEffect(() => needAssets(data.layers.map((l) => l.asset_id)), [data.layers, needAssets]);

  const setEmitter = (at: number, change: Partial<RandomLayer>) =>
    set({ emitters: data.emitters.map((e, i) => (i === at ? { ...e, ...change } : e)) });

  return (
    <aside className="flex w-[26rem] shrink-0 flex-col border-l border-line bg-panel">
      <div className="flex items-center gap-2 border-b border-line px-4 py-3">
        <input
          value={data.name}
          onChange={(e) => set({ name: e.target.value })}
          placeholder="Scene name"
          aria-label="Scene name"
          className={`${inputClass} font-semibold`}
        />
        <button onClick={onClose} aria-label="Close editor" className="rounded p-2 text-dim hover:bg-raised hover:text-ink">
          <CloseIcon />
        </button>
      </div>

      <div className="min-h-0 flex-1 space-y-5 overflow-y-auto p-4">
        <Field label="Music">
          <Segmented
            value={data.music.kind}
            onChange={(kind) =>
              set({
                music:
                  kind === "tracks"
                    ? { kind, asset_ids: data.music.kind === "tracks" ? data.music.asset_ids : [] }
                    : kind === "set"
                      ? { kind, set_id: sets[0]?.id ?? 0, tier: 0 }
                      : { kind },
              })
            }
            options={[
              { value: "keep", label: "Leave as is", title: "Whatever music is playing carries on" },
              { value: "tracks", label: "Tracks" },
              ...(sets.length > 0 || data.music.kind === "set" ? [{ value: "set" as const, label: "Dynamic", title: "A music set, at a level you choose" }] : []),
              { value: "silence", label: "None", title: "Fade the music out" },
            ]}
          />
          {data.music.kind === "set" && (
            <div className="flex gap-2">
              <select
                value={data.music.set_id}
                onChange={(e) => set({ music: { kind: "set", set_id: Number(e.target.value), tier: 0 } })}
                aria-label="Music set"
                className={inputClass}
              >
                {sets.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.data.name}
                  </option>
                ))}
              </select>
              <select
                value={data.music.tier}
                onChange={(e) => data.music.kind === "set" && set({ music: { ...data.music, tier: Number(e.target.value) } })}
                aria-label="Starting level"
                className={`${inputBase} h-9 w-36`}
              >
                {(sets.find((s) => data.music.kind === "set" && s.id === data.music.set_id)?.data.tiers ?? []).map((t, i) => (
                  <option key={i} value={i}>
                    {t.name || `Level ${i + 1}`}
                  </option>
                ))}
              </select>
            </div>
          )}
          {data.music.kind === "tracks" && (
            <AssetList
              ids={data.music.asset_ids}
              onChange={(asset_ids) => set({ music: { kind: "tracks", asset_ids } })}
              addLabel="Add a track"
              pickerTitle="Add music to this scene"
              channel="music"
              ordered
              empty="No tracks yet. With several, they play in order and repeat."
            />
          )}
        </Field>

        <Field label="Ambience" hint="Layers that play together, on repeat">
          <div className="space-y-1.5">
            {data.layers.length === 0 && <p className="text-xs text-faint">No layers yet. Try rain plus a crackling fire.</p>}
            {data.layers.map((layer, at) => (
              <div key={`${layer.asset_id}-${at}`} className="rounded-md bg-bg px-2.5 py-1.5">
                <div className="flex items-center gap-2">
                  <span className="min-w-0 flex-1 truncate text-sm">{assetName(assets, layer.asset_id)}</span>
                  <button
                    onClick={() => set({ layers: data.layers.filter((_, i) => i !== at) })}
                    aria-label={`Remove ${assetName(assets, layer.asset_id)}`}
                    className="rounded p-1 text-dim hover:bg-line hover:text-ink"
                  >
                    <CloseIcon width={12} height={12} />
                  </button>
                </div>
                <Slider
                  value={layer.volume_db}
                  min={-30}
                  max={6}
                  onChange={(volume_db) => set({ layers: data.layers.map((l, i) => (i === at ? { ...l, volume_db } : l)) })}
                  format={formatDb}
                  label="Layer volume"
                />
              </div>
            ))}
            <button onClick={() => setAddingLayer(true)} className="flex h-8 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-dim hover:bg-raised hover:text-ink">
              <PlusIcon width={12} height={12} /> Add a layer
            </button>
          </div>
        </Field>

        <Field label="Now and then" hint="Sounds that happen every so often">
          <div className="space-y-2">
            {data.emitters.map((emitter, at) => (
              <div key={at} className="space-y-2 rounded-md bg-bg p-2.5">
                <div className="flex items-center gap-2">
                  <input
                    value={emitter.name}
                    onChange={(e) => setEmitter(at, { name: e.target.value })}
                    placeholder="Name, e.g. Distant thunder"
                    className={`${inputBase} w-full h-8`}
                  />
                  <button onClick={() => set({ emitters: data.emitters.filter((_, i) => i !== at) })} aria-label="Remove" className="rounded p-1.5 text-dim hover:bg-line hover:text-ink">
                    <CloseIcon width={12} height={12} />
                  </button>
                </div>
                <AssetList
                  ids={emitter.asset_ids}
                  onChange={(asset_ids) => setEmitter(at, { asset_ids })}
                  addLabel="Add a sound"
                  pickerTitle="Sounds to pick from at random"
                  empty="Add one or more sounds. One is picked at random each time."
                />
                <div className="flex items-center gap-2 text-xs text-dim">
                  Every
                  <input
                    type="number"
                    min={1}
                    value={emitter.min_s}
                    onChange={(e) => setEmitter(at, { min_s: Math.max(1, Number(e.target.value)) })}
                    aria-label="Shortest gap in seconds"
                    className={`${inputBase} h-7 w-16 px-1.5 text-center`}
                  />
                  to
                  <input
                    type="number"
                    min={1}
                    value={emitter.max_s}
                    onChange={(e) => setEmitter(at, { max_s: Math.max(1, Number(e.target.value)) })}
                    aria-label="Longest gap in seconds"
                    className={`${inputBase} h-7 w-16 px-1.5 text-center`}
                  />
                  seconds
                </div>
                <Slider value={emitter.volume_db} min={-30} max={6} onChange={(volume_db) => setEmitter(at, { volume_db })} format={formatDb} label="Volume" />
                {advanced && (
                  <Toggle on={emitter.random_pan} onChange={(random_pan) => setEmitter(at, { random_pan })} label="Come from a different side each time" />
                )}
              </div>
            ))}
            <button
              onClick={() => set({ emitters: [...data.emitters, { name: "", asset_ids: [], min_s: 20, max_s: 90, volume_db: 0, random_pan: true }] })}
              className="flex h-8 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-dim hover:bg-raised hover:text-ink"
            >
              <PlusIcon width={12} height={12} /> Add an occasional sound
            </button>
          </div>
        </Field>

        {advanced && (
          <Field label="Crossfade time" hint="How long switching to this scene takes">
            <Segmented
              value={data.transition_ms}
              onChange={(transition_ms) => set({ transition_ms })}
              options={[0, 1000, 2000, 3000, 5000, 8000].map((ms) => ({ value: ms, label: ms === 0 ? "Instant" : `${ms / 1000} s` }))}
            />
          </Field>
        )}

        {boards.length > 0 && (
          <Field label="Soundboard" hint="Shown while this scene is playing">
            <select
              value={data.board_id ?? ""}
              onChange={(e) => set({ board_id: e.target.value ? Number(e.target.value) : null })}
              className={inputClass}
            >
              <option value="">None in particular</option>
              {boards.map((b) => (
                <option key={b.id} value={b.id}>
                  {b.data.name}
                </option>
              ))}
            </select>
          </Field>
        )}

        <Field label="Colour">
          <ColorPicker value={data.color} onChange={(color) => set({ color })} />
        </Field>

        <Field label="Key">
          <HotkeyInput
            combo={hotkey?.combo ?? null}
            global={hotkey?.global ?? false}
            onChange={(combo, global) => setHotkey({ type: "scene", id: scene.id }, combo, global)}
            fallback={index >= 0 && index < 12 ? `F${index + 1}` : undefined}
          />
        </Field>

        <ScopeField value={data} onChange={set} />
        <MoreOptions />
      </div>

      <div className="flex items-center justify-between border-t border-line px-4 py-3">
        <ConfirmButton
          label="Delete scene"
          confirm="Delete this scene?"
          onConfirm={() => {
            setHotkey({ type: "scene", id: scene.id }, null);
            void remove("scene", scene.id);
            onClose();
          }}
        />
        <button onClick={() => void activateScene({ id: scene.id, data })} className={primaryClass}>
          Try it
        </button>
      </div>

      {addingLayer && (
        <AssetPicker
          title="Add ambience layers"
          channel="ambience"
          chosen={data.layers.map((l) => l.asset_id)}
          onPick={(asset) => {
            useDocs.setState((s) => ({ assets: { ...s.assets, [asset.id]: asset } }));
            if (!data.layers.some((l) => l.asset_id === asset.id)) set({ layers: [...data.layers, { asset_id: asset.id, volume_db: 0 }] });
          }}
          onClose={() => setAddingLayer(false)}
        />
      )}
    </aside>
  );
}
