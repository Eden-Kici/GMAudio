import { useState } from "react";
import { duplicateSession } from "../actions";
import { importBundle } from "../bundle";
import { ExportDialog } from "./Backup";
import { useDocs } from "../docs";
import { ChevronIcon, PlusIcon } from "./icons";
import { buttonClass, ConfirmButton, inputClass, Modal, primaryClass, Segmented } from "./ui";

/** Top bar: which campaign and session is being prepared or run. */
export function CampaignPicker() {
  const campaign = useDocs((s) => s.campaigns.find((c) => c.id === s.campaignId));
  const session = useDocs((s) => s.sessions.find((x) => x.id === s.sessionId));
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        onClick={() => setOpen(true)}
        title="Choose the campaign and session you are working on"
        className="flex h-9 min-w-0 max-w-72 items-center gap-2 rounded-lg border border-line px-3 text-xs hover:border-dim"
      >
        <span className="truncate font-medium">{campaign ? campaign.data.name || "Untitled campaign" : "No campaign"}</span>
        {session && <span className="truncate text-dim">/ {session.data.name || "Untitled session"}</span>}
        <ChevronIcon width={12} height={12} className="shrink-0 text-dim" />
      </button>
      {open && <CampaignManager onClose={() => setOpen(false)} />}
    </>
  );
}

function CampaignManager({ onClose }: { onClose: () => void }) {
  const campaigns = useDocs((s) => s.campaigns);
  const sessions = useDocs((s) => s.sessions);
  const campaignId = useDocs((s) => s.campaignId);
  const sessionId = useDocs((s) => s.sessionId);
  const select = useDocs((s) => s.selectSession);
  const save = useDocs((s) => s.save);
  const remove = useDocs((s) => s.remove);
  const campaign = campaigns.find((c) => c.id === campaignId);
  const mine = sessions.filter((s) => s.data.campaign_id === campaignId);
  const [exporting, setExporting] = useState(false);

  const newCampaign = async () => {
    const id = await save("campaign", null, { name: `Campaign ${campaigns.length + 1}` });
    const session = await save("session", null, { name: "Session 1", campaign_id: id, notes: "", cues: [], position: 0 });
    select(id, session);
  };
  const newSession = async () => {
    if (campaignId === null) return;
    const id = await save("session", null, { name: `Session ${mine.length + 1}`, campaign_id: campaignId, notes: "", cues: [], position: 0 });
    select(campaignId, id);
  };

  return (
    <Modal title="Campaigns and sessions" onClose={onClose} wide>
      <div className="grid min-h-72 grid-cols-2 divide-x divide-line overflow-y-auto">
        <div className="space-y-1 p-3">
          <p className="px-2 pb-1 text-xs font-semibold uppercase tracking-wider text-dim">Campaigns</p>
          <button
            onClick={() => select(null, null)}
            aria-pressed={campaignId === null}
            className={`flex h-9 w-full items-center rounded-md px-2.5 text-left text-sm ${campaignId === null ? "bg-raised font-medium" : "text-dim hover:bg-raised/60"}`}
          >
            No campaign <span className="ml-2 text-xs text-faint">shared things only</span>
          </button>
          {campaigns.map((c) => (
            <button
              key={c.id}
              onClick={() => select(c.id, sessions.find((s) => s.data.campaign_id === c.id)?.id ?? null)}
              aria-pressed={c.id === campaignId}
              className={`flex h-9 w-full items-center rounded-md px-2.5 text-left text-sm ${c.id === campaignId ? "bg-raised font-medium" : "text-dim hover:bg-raised/60"}`}
            >
              <span className="truncate">{c.data.name || "Untitled campaign"}</span>
            </button>
          ))}
          <button onClick={() => void newCampaign()} className="flex h-9 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium text-dim hover:bg-raised hover:text-ink">
            <PlusIcon width={12} height={12} /> New campaign
          </button>
        </div>

        <div className="space-y-2 p-3">
          {!campaign ? (
            <p className="px-2 pt-6 text-center text-sm leading-relaxed text-dim">
              A campaign groups your game nights. Scenes, pads and music you make inside one stay out of the way of the
              others.
            </p>
          ) : (
            <>
              <input
                value={campaign.data.name}
                onChange={(e) => void save("campaign", campaign.id, { ...campaign.data, name: e.target.value })}
                aria-label="Campaign name"
                placeholder="Campaign name"
                className={`${inputClass} font-semibold`}
              />
              <p className="px-1 pt-1 text-xs font-semibold uppercase tracking-wider text-dim">Sessions</p>
              {mine.map((s) => (
                <div key={s.id} className={`rounded-md ${s.id === sessionId ? "bg-raised" : ""}`}>
                  {s.id === sessionId ? (
                    <div className="space-y-2 p-2">
                      <input
                        value={s.data.name}
                        onChange={(e) => void save("session", s.id, { ...s.data, name: e.target.value })}
                        aria-label="Session name"
                        placeholder="Session name"
                        className={inputClass}
                      />
                      <div className="flex items-center justify-between px-0.5">
                        <button
                          onClick={async () => select(campaign.id, await duplicateSession(s))}
                          title="Start the next session from a copy of this one"
                          className="text-xs font-medium text-dim hover:text-ink"
                        >
                          Duplicate as a starting point
                        </button>
                        <ConfirmButton
                          label="Delete"
                          confirm="Delete this session and its cue list?"
                          onConfirm={() => {
                            void remove("session", s.id);
                            select(campaign.id, mine.find((x) => x.id !== s.id)?.id ?? null);
                          }}
                        />
                      </div>
                    </div>
                  ) : (
                    <button onClick={() => select(campaign.id, s.id)} className="flex h-9 w-full items-center rounded-md px-2.5 text-left text-sm text-dim hover:bg-raised/60">
                      <span className="truncate">{s.data.name || "Untitled session"}</span>
                      <span className="ml-auto text-xs text-faint">{s.data.cues.length > 0 ? `${s.data.cues.length} cues` : ""}</span>
                    </button>
                  )}
                </div>
              ))}
              <button onClick={() => void newSession()} className="flex h-9 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium text-dim hover:bg-raised hover:text-ink">
                <PlusIcon width={12} height={12} /> New session
              </button>
              <div className="flex items-center justify-between pt-3">
                <button
                  onClick={() => setExporting(true)}
                  title="Save this campaign to one file, to back it up or move it to another computer"
                  className="text-xs font-medium text-dim hover:text-ink"
                >
                  Save to a file…
                </button>
                <ConfirmButton
                  label="Delete this campaign"
                  confirm="Delete the campaign and its sessions? Scenes and pads are kept and become shared."
                  onConfirm={() => {
                    for (const s of mine) void remove("session", s.id);
                    const docs = useDocs.getState();
                    // Its scenes, boards and music are kept, shared by all campaigns.
                    for (const scene of docs.scenes.filter((x) => x.data.campaign_id === campaign.id)) void docs.save("scene", scene.id, { ...scene.data, campaign_id: null, session_id: null });
                    for (const board of docs.boards.filter((x) => x.data.campaign_id === campaign.id)) void docs.save("board", board.id, { ...board.data, campaign_id: null, session_id: null });
                    for (const set of docs.sets.filter((x) => x.data.campaign_id === campaign.id)) void docs.save("set", set.id, { ...set.data, campaign_id: null, session_id: null });
                    void remove("campaign", campaign.id);
                    select(null, null);
                  }}
                />
              </div>
            </>
          )}
        </div>
      </div>
      <div className="flex items-center justify-between border-t border-line px-4 py-3">
        <button
          onClick={() => void importBundle()}
          title="Add a campaign from a file saved on this or another computer"
          className={buttonClass}
        >
          Open a campaign file…
        </button>
        <button onClick={onClose} className={primaryClass}>
          Done
        </button>
      </div>
      {exporting && campaign && <ExportDialog campaignId={campaign.id} name={campaign.data.name || "Untitled campaign"} onClose={() => setExporting(false)} />}
    </Modal>
  );
}

/** In an editor: who gets to see this scene, board or music set. */
export function ScopeField({
  value,
  onChange,
}: {
  value: { campaign_id: number | null; session_id?: number | null };
  onChange: (change: { campaign_id: number | null; session_id: number | null }) => void;
}) {
  const campaignId = useDocs((s) => s.campaignId);
  const sessionId = useDocs((s) => s.sessionId);
  if (campaignId === null) return null;
  const current = value.campaign_id === null ? "all" : value.session_id != null ? "session" : "campaign";
  return (
    <div className="space-y-1.5">
      <span className="text-xs font-semibold uppercase tracking-wider text-dim">Belongs to</span>
      <Segmented
        value={current}
        onChange={(scope) =>
          onChange(
            scope === "all"
              ? { campaign_id: null, session_id: null }
              : scope === "campaign"
                ? { campaign_id: campaignId, session_id: null }
                : { campaign_id: campaignId, session_id: sessionId },
          )
        }
        options={[
          ...(sessionId !== null ? [{ value: "session" as const, label: "This session", title: "Only shows up in this session" }] : []),
          { value: "campaign", label: "This campaign", title: "Shared by every session of this campaign" },
          { value: "all", label: "Everything", title: "Shows up in every campaign" },
        ]}
      />
    </div>
  );
}

/** At the foot of an editor: reveal or hide the less common options. */
export function MoreOptions() {
  const advanced = useDocs((s) => s.advanced);
  const setAdvanced = useDocs((s) => s.setAdvanced);
  return (
    <button onClick={() => setAdvanced(!advanced)} className="text-xs font-medium text-dim underline-offset-2 hover:text-ink hover:underline">
      {advanced ? "Fewer options" : "More options"}
    </button>
  );
}

