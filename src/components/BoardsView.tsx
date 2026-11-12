import { useEffect, useState } from "react";
import { CHANNELS, type Doc } from "../api";
import { DEFAULT_PAD_KEYS, padTag, preloadBoards, setHotkey, setVisibleBoard, triggerPad } from "../actions";
import { useDocs, useScoped } from "../docs";
import { MoreOptions, ScopeField } from "./Campaigns";
import { useStore } from "../store";
import { newBoard, newPad, type Board, type Pad } from "../types";
import { CloseIcon, LoopIcon, PencilIcon, PlusIcon } from "./icons";
import {
  AssetList,
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

/** A grid of pads. Used in the Soundboard view and while performing. */
export function PadGrid({ board, editing, onEdit }: { board: Doc<Board>; editing?: boolean; onEdit?: (pad: number) => void }) {
  const save = useDocs((s) => s.save);
  // Which of this board's pads are sounding, as "0,3,7".
  const live = useStore((s) => {
    const prefix = `pad:${board.id}:`;
    const on = new Set<string>();
    for (const sound of s.engine?.sounds ?? []) {
      if (!sound.stopping && sound.tag?.startsWith(prefix)) on.add(sound.tag.slice(prefix.length));
    }
    return [...on].sort().join(",");
  });
  const hotkeys = useDocs((s) => s.hotkeys);
  const livePads = new Set(live ? live.split(",").map(Number) : []);

  return (
    <div className="grid gap-2.5" style={{ gridTemplateColumns: `repeat(${board.data.columns}, minmax(0, 1fr))` }}>
      {board.data.pads.map((pad, index) => {
        const custom = hotkeys.find((h) => h.action.type === "pad" && h.action.board === board.id && h.action.pad === index)?.combo;
        const key = custom ?? DEFAULT_PAD_KEYS[index];
        const on = livePads.has(index);
        const empty = pad.asset_ids.length === 0;
        return (
          <button
            key={index}
            style={{ "--c": pad.color } as React.CSSProperties}
            onPointerDown={(e) => {
              if (editing) return onEdit?.(index);
              if (e.button !== 0) return;
              e.currentTarget.setPointerCapture(e.pointerId);
              void triggerPad(board.id, index, pad, "down");
            }}
            onPointerUp={() => !editing && void triggerPad(board.id, index, pad, "up")}
            onPointerCancel={() => !editing && void triggerPad(board.id, index, pad, "up")}
            onContextMenu={(e) => {
              e.preventDefault();
              onEdit?.(index);
            }}
            title={editing ? "Edit this pad" : empty ? "No sound yet. Right-click to edit." : pad.mode === "hold" ? "Hold to play" : pad.mode === "loop" ? "Press to start, press again to stop" : "Press to play"}
            className={`relative flex h-20 flex-col justify-between rounded-xl border-2 p-2.5 text-left transition-[background-color,transform] active:scale-[0.97] ${
              on ? "border-(--c) bg-(--c)/30" : empty ? "border-dashed border-line bg-panel" : "border-(--c)/40 bg-(--c)/10 hover:bg-(--c)/20"
            }`}
          >
            <span className="line-clamp-2 text-sm font-semibold leading-tight">{pad.label || (empty ? "Empty pad" : "Pad")}</span>
            <span className="flex items-center gap-1.5 text-[11px] text-dim">
              {pad.mode === "loop" && <LoopIcon width={11} height={11} />}
              {pad.mode === "hold" && <span>Hold</span>}
              {pad.asset_ids.length > 1 && <span>×{pad.asset_ids.length}</span>}
              {key && <kbd className="ml-auto rounded bg-black/30 px-1.5 py-px font-sans font-medium">{key}</kbd>}
            </span>
            {editing && <PencilIcon width={12} height={12} className="absolute right-2 top-2 text-dim" />}
          </button>
        );
      })}
      {editing && (
        <button
          onClick={() => {
            void save("board", board.id, { ...board.data, pads: [...board.data.pads, newPad()] });
            onEdit?.(board.data.pads.length);
          }}
          className="flex h-20 items-center justify-center gap-2 rounded-xl border-2 border-dashed border-line text-sm font-medium text-dim hover:border-dim hover:text-ink"
        >
          <PlusIcon /> Add pad
        </button>
      )}
    </div>
  );
}

export function BoardsView() {
  const boards = useScoped("boards");
  const campaignId = useDocs((s) => s.campaignId);
  const save = useDocs((s) => s.save);
  const remove = useDocs((s) => s.remove);
  const [selected, setSelected] = useState<number | null>(null);
  const [editing, setEditing] = useState(false);
  const [pad, setPad] = useState<number | null>(null);
  const board = boards.find((b) => b.id === selected) ?? boards[0];

  useEffect(() => {
    setVisibleBoard(board ?? null);
    return () => setVisibleBoard(null);
  }, [board]);
  useEffect(() => preloadBoards(boards), [boards]);

  const create = async () => {
    const id = await save("board", null, { ...newBoard(`Board ${boards.length + 1}`), campaign_id: campaignId });
    setSelected(id);
    setEditing(true);
  };

  return (
    <main className="flex min-h-0 min-w-0 flex-1">
      <div className="min-w-0 flex-1 overflow-y-auto p-5">
        <div className="mb-4 flex flex-wrap items-center gap-2">
          <h1 className="mr-2 text-lg font-semibold">Soundboard</h1>
          <div className="mr-auto flex flex-wrap gap-1">
            {boards.map((b) => (
              <button
                key={b.id}
                onClick={() => {
                  setSelected(b.id);
                  setPad(null);
                }}
                className={`h-8 rounded-md px-3 text-sm font-medium ${b.id === board?.id ? "bg-raised text-ink" : "text-dim hover:text-ink"}`}
              >
                {b.data.name || "Untitled"}
              </button>
            ))}
            <button onClick={() => void create()} title="New board" aria-label="New board" className="flex size-8 items-center justify-center rounded-md text-dim hover:bg-raised hover:text-ink">
              <PlusIcon />
            </button>
          </div>
          {board && (
            <button
              onClick={() => {
                setEditing(!editing);
                setPad(null);
              }}
              aria-pressed={editing}
              className={editing ? primaryClass : buttonClass}
            >
              {editing ? "Done editing" : "Edit pads"}
            </button>
          )}
        </div>

        {!board ? (
          <div className="mx-auto mt-16 max-w-md text-center text-dim">
            <p className="text-[15px] text-ink">Sound effects at your fingertips</p>
            <p className="mt-2 text-sm leading-relaxed">
              A board is a grid of pads. Each pad plays a sound when you press it: a door slam, a sword clash, a
              victory fanfare. The first ten pads also answer to the number keys.
            </p>
            <button onClick={() => void create()} className={`${primaryClass} mt-5`}>
              <PlusIcon /> Make a board
            </button>
          </div>
        ) : (
          <>
            {editing && (
              <div className="mb-4 flex flex-wrap items-center gap-3 rounded-lg bg-panel p-3">
                <input
                  value={board.data.name}
                  onChange={(e) => void save("board", board.id, { ...board.data, name: e.target.value })}
                  placeholder="Board name"
                  aria-label="Board name"
                  className={`${inputBase} h-9 w-56`}
                />
                <label className="flex items-center gap-2 text-xs text-dim">
                  Pads per row
                  <select
                    value={board.data.columns}
                    onChange={(e) => void save("board", board.id, { ...board.data, columns: Number(e.target.value) })}
                    className={`${inputBase} h-9 w-16`}
                  >
                    {[3, 4, 5, 6, 8].map((n) => (
                      <option key={n}>{n}</option>
                    ))}
                  </select>
                </label>
                <div className="w-72">
                  <ScopeField value={board.data} onChange={(scope) => void save("board", board.id, { ...board.data, ...scope })} />
                </div>
                <span className="ml-auto">
                  <ConfirmButton
                    label="Delete board"
                    confirm="Delete this board and its pads?"
                    onConfirm={() => {
                      void remove("board", board.id);
                      setSelected(null);
                      setEditing(false);
                    }}
                  />
                </span>
              </div>
            )}
            <PadGrid
              board={board}
              editing={editing}
              onEdit={(index) => {
                setEditing(true);
                setPad(index);
              }}
            />
            {!editing && board.data.pads.length === 0 && (
              <p className="mt-6 text-center text-sm text-dim">This board is empty. Choose “Edit pads” to add some.</p>
            )}
          </>
        )}
      </div>
      {board && editing && pad !== null && board.data.pads[pad] && (
        <PadEditor key={`${board.id}-${pad}`} board={board} index={pad} onClose={() => setPad(null)} />
      )}
    </main>
  );
}

function PadEditor({ board, index, onClose }: { board: Doc<Board>; index: number; onClose: () => void }) {
  const save = useDocs((s) => s.save);
  const advanced = useDocs((s) => s.advanced);
  const assets = useDocs((s) => s.assets);
  const hotkey = useDocs((s) => s.hotkeys.find((h) => h.action.type === "pad" && h.action.board === board.id && h.action.pad === index));
  const pad = board.data.pads[index];
  const set = (change: Partial<Pad>) =>
    void save("board", board.id, { ...board.data, pads: board.data.pads.map((p, i) => (i === index ? { ...p, ...change } : p)) });

  return (
    <aside className="flex w-[24rem] shrink-0 flex-col border-l border-line bg-panel">
      <div className="flex items-center gap-2 border-b border-line px-4 py-3">
        <input value={pad.label} onChange={(e) => set({ label: e.target.value })} placeholder="Pad label" aria-label="Pad label" className={`${inputClass} font-semibold`} />
        <button onClick={onClose} aria-label="Close editor" className="rounded p-2 text-dim hover:bg-raised hover:text-ink">
          <CloseIcon />
        </button>
      </div>
      <div className="min-h-0 flex-1 space-y-5 overflow-y-auto p-4">
        <Field label="Sound" hint={pad.asset_ids.length > 1 ? "One is picked at random each press" : undefined}>
          <AssetList
            ids={pad.asset_ids}
            onChange={(asset_ids) => {
              // A new pad takes its first sound's name.
              const first = asset_ids[0];
              set({ asset_ids, label: pad.label || (first !== undefined ? (assets[first]?.name ?? "") : "") });
            }}
            addLabel={pad.asset_ids.length === 0 ? "Choose a sound" : "Add another to pick from at random"}
            pickerTitle="Sounds for this pad"
            channel={pad.channel}
          />
        </Field>

        <Field label="When pressed">
          <Segmented
            value={pad.mode}
            onChange={(mode) => set({ mode })}
            options={[
              { value: "oneshot", label: "Play once" },
              { value: "loop", label: "Repeat until pressed again" },
              ...(advanced || pad.mode === "hold" ? [{ value: "hold" as const, label: "Play while held" }] : []),
            ]}
          />
        </Field>

        <Field label="Volume">
          <Slider value={pad.volume_db} min={-30} max={6} onChange={(volume_db) => set({ volume_db })} format={formatDb} label="Pad volume" />
        </Field>

        {advanced && (
          <>
            <Field label="Plays on" hint="Which mixer channel carries it">
              <Segmented
                value={pad.channel}
                onChange={(channel) => set({ channel })}
                options={CHANNELS.filter((c) => c.id !== "music").map((c) => ({ value: c.id, label: c.label }))}
              />
            </Field>
            <Toggle on={pad.duck} onChange={(duck) => set({ duck })} label="Lower the music while this plays" />
            <Field label="Variation" hint="So repeated presses do not sound identical">
              <Slider
                value={pad.pitch_var}
                min={0}
                max={3}
                step={0.5}
                onChange={(pitch_var) => set({ pitch_var })}
                format={(v) => (v === 0 ? "No pitch" : `±${v} st`)}
                label="Random pitch change"
              />
              <Slider
                value={pad.volume_var}
                min={0}
                max={9}
                onChange={(volume_var) => set({ volume_var })}
                format={(v) => (v === 0 ? "No volume" : `−${v} dB`)}
                label="Random volume drop"
              />
            </Field>
          </>
        )}

        <Field label="Colour">
          <ColorPicker value={pad.color} onChange={(color) => set({ color })} />
        </Field>

        <Field label="Key">
          <HotkeyInput
            combo={hotkey?.combo ?? null}
            global={hotkey?.global ?? false}
            onChange={(combo, global) => setHotkey({ type: "pad", board: board.id, pad: index }, combo, global)}
            fallback={DEFAULT_PAD_KEYS[index]}
          />
        </Field>
        <MoreOptions />
      </div>
      <div className="flex items-center justify-between border-t border-line px-4 py-3">
        <ConfirmButton
          label="Delete pad"
          confirm="Delete this pad?"
          onConfirm={() => {
            // Hotkeys point at pads by position, so shift the ones after it.
            const docs = useDocs.getState();
            docs.setHotkeys(
              docs.hotkeys
                .filter((h) => !(h.action.type === "pad" && h.action.board === board.id && h.action.pad === index))
                .map((h) =>
                  h.action.type === "pad" && h.action.board === board.id && h.action.pad > index
                    ? { ...h, action: { ...h.action, pad: h.action.pad - 1 } }
                    : h,
                ),
            );
            void save("board", board.id, { ...board.data, pads: board.data.pads.filter((_, i) => i !== index) });
            onClose();
          }}
        />
        <button onPointerDown={() => void triggerPad(board.id, index, pad, "down")} onPointerUp={() => void triggerPad(board.id, index, pad, "up")} className={primaryClass}>
          Try it
        </button>
      </div>
    </aside>
  );
}

void padTag;
