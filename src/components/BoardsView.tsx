import { useEffect, useState } from "react";
import { CHANNELS, type Doc } from "../api";
import { DEFAULT_PAD_KEYS, padTag, preloadBoards, setHotkey, setVisibleBoard, triggerPad } from "../actions";
import { useDocs, useScoped } from "../docs";
import { useLayout } from "../layout";
import { MoreOptions, ScopeField } from "./Campaigns";
import { useStore } from "../store";
import { newBoard, newPad, type Board, type Pad } from "../types";
import { CloseIcon, LoopIcon, PencilIcon, PlusIcon, SplitIcon } from "./icons";
import { Split } from "./Resize";
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

const MAX_BOARD_PANES = 4;

/**
 * One or more boards side by side, each pane showing a board of its own
 * choosing. Which boards are open is remembered under `id`.
 */
export function BoardPanes({
  id,
  boards,
  editing,
  onEditPad,
  onCreate,
  front,
}: {
  id: string;
  boards: Doc<Board>[];
  editing?: boolean;
  onEditPad?: (board: number, pad: number | null) => void;
  /** Makes a new board and returns its id. Shown as "+" when given. */
  onCreate?: () => Promise<number>;
  /** A board to bring to the first pane, e.g. the playing scene's own. */
  front?: number | null;
}) {
  const save = useDocs((s) => s.save);
  const remove = useDocs((s) => s.remove);
  const saved = useLayout((s) => s.sizes[`pads:${id}`]);
  const setSaved = useLayout((s) => s.set);
  const open = saved && saved.length > 0 ? saved.slice(0, MAX_BOARD_PANES) : [boards[0]?.id ?? 0];
  const setOpen = (next: number[]) => setSaved(`pads:${id}`, next);
  const [active, setActive] = useState(0);
  const shown = open.map((boardId) => boards.find((b) => b.id === boardId) ?? boards[0]);

  useEffect(() => {
    if (front != null && open[0] !== front) setOpen([front, ...open.slice(1)]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [front]);
  // The number keys play the pads of the pane last touched.
  const keyed = shown[Math.min(active, shown.length - 1)];
  useEffect(() => {
    setVisibleBoard(keyed ?? null);
    return () => setVisibleBoard(null);
  }, [keyed]);

  const choose = (pane: number, boardId: number) => {
    setOpen(open.map((b, i) => (i === pane ? boardId : b)));
    onEditPad?.(boardId, null);
  };

  return (
    <Split id={`pads-${id}`} min={220} className="min-h-0 flex-1">
      {shown.map((board, pane) => (
        <section
          key={pane}
          aria-label="Soundboard"
          onPointerDownCapture={() => setActive(pane)}
          className={`flex min-h-0 flex-1 flex-col ${shown.length > 1 && pane === Math.min(active, shown.length - 1) ? "shadow-[inset_0_2px_0_var(--color-accent)]" : ""}`}
        >
          <div className="flex flex-wrap items-center gap-1 px-4 pb-2 pt-3">
            {boards.map((b) => (
              <button
                key={b.id}
                onClick={() => choose(pane, b.id)}
                className={`h-7 rounded-md px-2.5 text-xs font-medium ${b.id === board?.id ? "bg-raised text-ink" : "text-dim hover:text-ink"}`}
              >
                {b.data.name || "Untitled"}
              </button>
            ))}
            {onCreate && (
              <button
                onClick={() => void onCreate().then((made) => choose(pane, made))}
                title="New board"
                aria-label="New board"
                className="flex size-7 items-center justify-center rounded-md text-dim hover:bg-raised hover:text-ink"
              >
                <PlusIcon />
              </button>
            )}
            <span className="ml-auto flex items-center gap-0.5">
              {open.length < MAX_BOARD_PANES && boards.length > 0 && (
                <button
                  onClick={() => setOpen([...open.slice(0, pane + 1), boards.find((b) => !open.includes(b.id))?.id ?? board?.id ?? 0, ...open.slice(pane + 1)])}
                  title="Show another board beside this one"
                  aria-label="Split"
                  className="flex size-7 items-center justify-center rounded-md text-dim hover:bg-raised hover:text-ink"
                >
                  <SplitIcon />
                </button>
              )}
              {open.length > 1 && (
                <button
                  onClick={() => setOpen(open.filter((_, i) => i !== pane))}
                  title="Close this pane"
                  aria-label="Close pane"
                  className="flex size-7 items-center justify-center rounded-md text-dim hover:bg-raised hover:text-ink"
                >
                  <CloseIcon width={12} height={12} />
                </button>
              )}
            </span>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
            {!board ? (
              <p className="text-sm text-dim">No boards yet.</p>
            ) : (
              <>
                {editing && (
                  <div className="mb-3 flex flex-wrap items-center gap-3 rounded-lg bg-panel p-3">
                    <input
                      value={board.data.name}
                      onChange={(e) => void save("board", board.id, { ...board.data, name: e.target.value })}
                      placeholder="Board name"
                      aria-label="Board name"
                      className={`${inputBase} h-9 w-48`}
                    />
                    <label className="flex items-center gap-2 text-xs text-dim">
                      Pads per row
                      <select
                        value={board.data.columns}
                        onChange={(e) => void save("board", board.id, { ...board.data, columns: Number(e.target.value) })}
                        className={`${inputBase} h-9 w-16`}
                      >
                        {[2, 3, 4, 5, 6, 8].map((n) => (
                          <option key={n}>{n}</option>
                        ))}
                      </select>
                    </label>
                    <div className="w-64">
                      <ScopeField value={board.data} onChange={(scope) => void save("board", board.id, { ...board.data, ...scope })} />
                    </div>
                    <span className="ml-auto">
                      <ConfirmButton
                        label="Delete board"
                        confirm="Delete this board and its pads?"
                        onConfirm={() => {
                          onEditPad?.(board.id, null);
                          void remove("board", board.id);
                        }}
                      />
                    </span>
                  </div>
                )}
                <PadGrid board={board} editing={editing} onEdit={onEditPad ? (index) => onEditPad(board.id, index) : undefined} />
                {!editing && board.data.pads.length === 0 && (
                  <p className="mt-6 text-center text-sm text-dim">This board is empty.{onEditPad ? " Choose “Edit pads” to add some." : ""}</p>
                )}
              </>
            )}
          </div>
        </section>
      ))}
    </Split>
  );
}

export function BoardsView() {
  const boards = useScoped("boards");
  const campaignId = useDocs((s) => s.campaignId);
  const save = useDocs((s) => s.save);
  const [editing, setEditing] = useState(false);
  /** The pad open in the editor: which board, and which pad on it. */
  const [pad, setPad] = useState<{ board: number; index: number } | null>(null);
  const edited = pad ? boards.find((b) => b.id === pad.board) : undefined;

  useEffect(() => preloadBoards(boards), [boards]);

  const create = async () => {
    const id = await save("board", null, { ...newBoard(`Board ${boards.length + 1}`), campaign_id: campaignId });
    setEditing(true);
    return id;
  };

  return (
    <main className="flex min-h-0 min-w-0 flex-1">
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <div className="flex flex-wrap items-center gap-2 px-5 pt-5">
          <h1 className="mr-auto text-lg font-semibold">Soundboard</h1>
          {boards.length > 0 && (
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

        {boards.length === 0 ? (
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
          <BoardPanes
            id="prep"
            boards={boards}
            editing={editing}
            onCreate={create}
            onEditPad={(board, index) => {
              if (index === null) return setPad(null);
              setEditing(true);
              setPad({ board, index });
            }}
          />
        )}
      </div>
      {edited && editing && pad && edited.data.pads[pad.index] && (
        <PadEditor key={`${edited.id}-${pad.index}`} board={edited} index={pad.index} onClose={() => setPad(null)} />
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

        <Field label="Plays on" hint={pad.channel === "music" ? "Replaces the music that is playing" : "Which mixer channel carries it"}>
          <Segmented value={pad.channel} onChange={(channel) => set({ channel })} options={CHANNELS.map((c) => ({ value: c.id, label: c.label }))} />
        </Field>

        {advanced && (
          <>
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
        <button onClick={onClose} className={primaryClass} title="Changes are kept as you make them; this closes the editor">
          Save
        </button>
      </div>
    </aside>
  );
}

void padTag;
