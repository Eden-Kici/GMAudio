import { useEffect, useState } from "react";
import { CHANNELS, type Channel, type Doc, type Quantize } from "../api";
import { describeCue, fireCue, go, jumpToCue, resetCues, skipCue } from "../actions";
import { useDocs, useScoped } from "../docs";
import type { Cue, CueAction, Session } from "../types";
import { ArrowIcon, CloseIcon, PlayIcon, PlusIcon } from "./icons";
import { AssetPicker, buttonClass, inputBase, inputClass, Modal, primaryClass } from "./ui";

/** The running order across the top of the Play view: what is next, and GO. */
export function CueStrip() {
  const session = useDocs((s) => s.sessions.find((x) => x.id === s.sessionId));
  const setView = useDocs((s) => s.setView);
  useCueAssets(session);
  if (!session || session.data.cues.length === 0) return null;
  const { cues, position } = session.data;
  const next = cues[position];
  const after = cues[position + 1];
  const last = position > 0 ? cues[position - 1] : undefined;

  return (
    <div className="flex items-center gap-4 border-b border-line bg-panel px-5 py-2.5">
      <button onClick={() => setView("cues")} title="Open the full cue list" className="shrink-0 text-left text-xs text-dim hover:text-ink">
        <span className="block font-semibold uppercase tracking-wider">Cue</span>
        <span className="tabular-nums">
          {Math.min(position + 1, cues.length)} of {cues.length}
        </span>
      </button>
      <div className="min-w-0 flex-1">
        {next ? (
          <>
            <p className="truncate text-[15px] font-semibold">{next.note || describeCue(next)}</p>
            <p className="truncate text-xs text-dim">
              {next.note ? describeCue(next) : ""}
              {after && <span className="text-faint">{next.note ? " · " : ""}then: {after.note || describeCue(after)}</span>}
            </p>
          </>
        ) : (
          <>
            <p className="text-[15px] font-semibold text-dim">End of the cue list</p>
            {last && <p className="truncate text-xs text-faint">Last: {last.note || describeCue(last)}</p>}
          </>
        )}
      </div>
      {next ? (
        <>
          <button onClick={skipCue} title="Pass over this cue without firing it" className={buttonClass}>
            Skip
          </button>
          <button onClick={go} title="Fire this cue and move to the next (Space)" className="flex h-11 items-center gap-2.5 rounded-lg bg-accent px-6 text-base font-bold text-bg hover:brightness-110">
            GO <kbd className="rounded bg-black/20 px-1.5 py-0.5 text-[11px] font-semibold">Space</kbd>
          </button>
        </>
      ) : (
        <button onClick={resetCues} className={buttonClass}>
          Start over
        </button>
      )}
    </div>
  );
}

/** Makes sure the names of sounds used in cues are loaded. */
function useCueAssets(session: Doc<Session> | undefined) {
  const needAssets = useDocs((s) => s.needAssets);
  useEffect(() => {
    const ids = (session?.data.cues ?? []).flatMap((c) => ("asset_id" in c.action ? [c.action.asset_id] : []));
    if (ids.length > 0) needAssets(ids);
  }, [session, needAssets]);
}

export function CuesView() {
  const session = useDocs((s) => s.sessions.find((x) => x.id === s.sessionId));
  const campaigns = useDocs((s) => s.campaigns);
  const save = useDocs((s) => s.save);
  const select = useDocs((s) => s.selectSession);
  // Subscribe, so cue descriptions refresh when names change.
  useDocs((s) => s.assets);
  useDocs((s) => s.scenes);
  const [adding, setAdding] = useState(false);
  useCueAssets(session);

  if (!session) {
    const start = async () => {
      const docs = useDocs.getState();
      const campaignId = docs.campaignId ?? (await save("campaign", null, { name: campaigns.length === 0 ? "My campaign" : `Campaign ${campaigns.length + 1}` }));
      const sessionId = await save("session", null, { name: "Session 1", campaign_id: campaignId, notes: "", cues: [], position: 0 });
      select(campaignId, sessionId);
    };
    return (
      <main className="flex min-w-0 flex-1 items-center justify-center p-8">
        <div className="max-w-md text-center text-dim">
          <p className="text-[15px] text-ink">Plan a session step by step</p>
          <p className="mt-2 text-sm leading-relaxed">
            A cue list is your running order for a game night: "party enters the crypt", "the lich appears". At the
            table, one key fires the next cue. Cue lists belong to a session.
          </p>
          <button onClick={() => void start()} className={`${primaryClass} mt-5`}>
            <PlusIcon /> Start a session
          </button>
        </div>
      </main>
    );
  }

  const { cues, position } = session.data;
  const update = (change: Partial<Session>) => void save("session", session.id, { ...session.data, ...change });
  const setCue = (at: number, change: Partial<Cue>) => update({ cues: cues.map((c, i) => (i === at ? { ...c, ...change } : c)) });
  const move = (at: number, by: number) => {
    const next = [...cues];
    const [cue] = next.splice(at, 1);
    next.splice(at + by, 0, cue);
    // Keep "next up" on the same cue.
    const position = session.data.position === at ? at + by : session.data.position === at + by ? at : session.data.position;
    update({ cues: next, position });
  };

  return (
    <main className="flex min-h-0 min-w-0 flex-1">
      <div className="min-w-0 flex-1 overflow-y-auto p-5">
        <div className="mb-4 flex flex-wrap items-center gap-2">
          <h1 className="mr-auto text-lg font-semibold">
            Cue list <span className="font-normal text-dim">for {session.data.name || "this session"}</span>
          </h1>
          {cues.some((c) => c.state) && (
            <button onClick={resetCues} title="Unmark every cue and go back to the top" className={buttonClass}>
              Start over
            </button>
          )}
          <button onClick={() => setAdding(true)} className={primaryClass}>
            <PlusIcon /> Add a cue
          </button>
        </div>

        {cues.length === 0 ? (
          <p className="mx-auto mt-12 max-w-md text-center text-sm leading-relaxed text-dim">
            No cues yet. Add the moments you expect, in order: a scene change, a stinger, a fade to silence. At the table,
            press Space to fire the next one.
          </p>
        ) : (
          <ol className="space-y-1">
            {cues.map((cue, at) => {
              const isNext = at === position;
              return (
                <li
                  key={at}
                  className={`flex items-center gap-2 rounded-lg border px-2 py-1.5 ${
                    isNext ? "border-accent bg-accent/10" : "border-transparent bg-panel"
                  } ${cue.state ? "opacity-55" : ""}`}
                >
                  <button
                    onClick={(e) => (isNext ? go() : e.shiftKey ? fireCue(at) : jumpToCue(at))}
                    title={isNext ? "Fire this cue and move to the next (Space)" : "Click: make this the next cue. Shift+click: fire it now without moving."}
                    className={`flex size-8 shrink-0 items-center justify-center rounded-md text-xs font-semibold tabular-nums ${
                      isNext ? "bg-accent text-bg" : "bg-raised text-dim hover:text-ink"
                    }`}
                  >
                    {isNext ? <PlayIcon width={11} height={11} /> : at + 1}
                  </button>
                  <div className="min-w-0 flex-1">
                    <input
                      value={cue.note}
                      onChange={(e) => setCue(at, { note: e.target.value })}
                      placeholder="Note to self, e.g. Party enters the crypt"
                      className="h-6 w-full select-text bg-transparent text-sm font-medium placeholder:font-normal placeholder:text-faint focus:outline-none"
                    />
                    <p className="truncate text-xs text-dim">
                      {describeCue(cue)}
                      {cue.state === "skipped" && <span className="ml-2 rounded bg-raised px-1.5 py-px text-[11px]">Skipped</span>}
                      {cue.state === "done" && <span className="ml-2 text-[11px] text-faint">Done</span>}
                    </p>
                  </div>
                  <button onClick={() => fireCue(at)} title="Fire this cue now, without moving the list" className="rounded p-1.5 text-dim hover:bg-raised hover:text-ink">
                    <PlayIcon width={12} height={12} />
                  </button>
                  <button disabled={at === 0} onClick={() => move(at, -1)} aria-label="Move up" className="rounded p-1.5 text-dim enabled:hover:bg-raised disabled:opacity-25">
                    <ArrowIcon up width={12} height={12} />
                  </button>
                  <button disabled={at === cues.length - 1} onClick={() => move(at, 1)} aria-label="Move down" className="rounded p-1.5 text-dim enabled:hover:bg-raised disabled:opacity-25">
                    <ArrowIcon width={12} height={12} />
                  </button>
                  <button
                    onClick={() => update({ cues: cues.filter((_, i) => i !== at), position: position > at ? position - 1 : position })}
                    aria-label="Remove cue"
                    className="rounded p-1.5 text-dim hover:bg-raised hover:text-ink"
                  >
                    <CloseIcon width={12} height={12} />
                  </button>
                </li>
              );
            })}
          </ol>
        )}
      </div>

      <aside className="flex w-80 shrink-0 flex-col border-l border-line bg-panel p-4">
        <span className="pb-1.5 text-xs font-semibold uppercase tracking-wider text-dim">Session notes</span>
        <textarea
          value={session.data.notes}
          onChange={(e) => update({ notes: e.target.value })}
          placeholder="Anything to remember for this session."
          className={`${inputBase} min-h-0 w-full flex-1 resize-none py-2 leading-relaxed`}
        />
      </aside>

      {adding && <AddCue onAdd={(action) => update({ cues: [...cues, { action, note: "" }] })} onClose={() => setAdding(false)} />}
    </main>
  );
}

function AddCue({ onAdd, onClose }: { onAdd: (action: CueAction) => void; onClose: () => void }) {
  const scenes = useScoped("scenes");
  const boards = useScoped("boards");
  const sets = useScoped("sets");
  const advanced = useDocs((s) => s.advanced);
  const [picking, setPicking] = useState<"stinger" | "sound" | null>(null);
  const [channel, setChannel] = useState<Channel>("sfx");
  const [quantize, setQuantize] = useState<Quantize>("beat");
  const [fade, setFade] = useState(3);
  const add = (action: CueAction) => {
    onAdd(action);
    onClose();
  };
  const row = "flex h-9 w-full items-center rounded-md px-2.5 text-left text-sm hover:bg-raised";
  const heading = "px-2.5 pb-1 pt-3 text-xs font-semibold uppercase tracking-wider text-dim";

  if (picking) {
    return (
      <AssetPicker
        title={picking === "stinger" ? "Stinger to play" : "Sound to play"}
        channel={picking === "stinger" ? "stingers" : undefined}
        chosen={[]}
        onPick={(asset) => {
          useDocs.setState((s) => ({ assets: { ...s.assets, [asset.id]: asset } }));
          add(picking === "stinger" ? { type: "stinger", asset_id: asset.id, quantize, then_tier: null } : { type: "sound", asset_id: asset.id, channel });
        }}
        onClose={onClose}
      />
    );
  }

  return (
    <Modal title="Add a cue" onClose={onClose}>
      <div className="overflow-y-auto px-2 pb-3">
        <p className={heading}>Switch to a scene</p>
        {scenes.length === 0 && <p className="px-2.5 text-xs text-faint">No scenes yet. Make some under Scenes.</p>}
        {scenes.map((s) => (
          <button key={s.id} className={row} onClick={() => add({ type: "scene", id: s.id })}>
            <span className="mr-2.5 size-2 rounded-full" style={{ background: s.data.color }} />
            {s.data.name}
          </button>
        ))}

        {sets.length > 0 && (
          <>
            <p className={heading}>Change the music level</p>
            {[...new Map(sets.flatMap((set) => set.data.tiers.map((t, i) => [`${i}:${t.name}`, { tier: i, label: t.name }] as const))).values()].map((t) => (
              <button key={`${t.tier}:${t.label}`} className={row} onClick={() => add({ type: "tier", ...t })}>
                {t.label || `Level ${t.tier + 1}`} <span className="ml-auto text-xs text-faint">level {t.tier + 1}</span>
              </button>
            ))}
          </>
        )}

        <p className={heading}>Play something</p>
        <div className="flex items-center gap-2 px-2.5 py-1">
          <button className={buttonClass} onClick={() => setPicking("stinger")}>
            A stinger…
          </button>
          {advanced && (
            <select value={quantize} onChange={(e) => setQuantize(e.target.value as Quantize)} aria-label="Stinger timing" className={`${inputBase} h-9 w-32`}>
              <option value="now">at once</option>
              <option value="beat">on the beat</option>
              <option value="bar">on the bar</option>
            </select>
          )}
        </div>
        <div className="flex items-center gap-2 px-2.5 py-1">
          <button className={buttonClass} onClick={() => setPicking("sound")}>
            A sound…
          </button>
          <span className="text-xs text-dim">on</span>
          <select value={channel} onChange={(e) => setChannel(e.target.value as Channel)} aria-label="Channel" className={`${inputBase} h-9 w-40`}>
            {CHANNELS.map((c) => (
              <option key={c.id} value={c.id}>
                {c.label}
              </option>
            ))}
          </select>
        </div>
        {boards.flatMap((b) =>
          b.data.pads.map((pad, i) =>
            pad.asset_ids.length === 0 ? null : (
              <button key={`${b.id}-${i}`} className={row} onClick={() => add({ type: "pad", board: b.id, pad: i })}>
                Pad: {pad.label} <span className="ml-auto text-xs text-faint">{b.data.name}</span>
              </button>
            ),
          ),
        )}

        <p className={heading}>Stop or fade out</p>
        <div className="flex items-center gap-2 px-2.5 pb-1 text-xs text-dim">
          Over
          <input type="number" min={0} max={60} value={fade} onChange={(e) => setFade(Math.max(0, Number(e.target.value)))} aria-label="Fade seconds" className={`${inputBase} h-8 w-16 text-center`} />
          seconds
        </div>
        {(["all", ...CHANNELS.map((c) => c.id)] as const).map((id) => (
          <button key={id} className={row} onClick={() => add({ type: "stop", channel: id, fade_ms: fade * 1000 })}>
            {id === "all" ? "Everything" : CHANNELS.find((c) => c.id === id)!.label}
          </button>
        ))}
      </div>
    </Modal>
  );
}

void inputClass;
