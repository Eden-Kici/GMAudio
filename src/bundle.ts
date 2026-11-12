// Moving campaigns between computers, and backing them up.
//
// The Rust side packs and unpacks the file and looks after the sounds.
// This side decides what belongs to a campaign, and re-creates it on
// import with fresh ids.

import { open, save } from "@tauri-apps/plugin-dialog";
import { api, type Doc } from "./api";
import { useDocs } from "./docs";
import { message, useStore } from "./store";
import type { Board, Campaign, Cue, MusicSet, Scene, Session } from "./types";

interface BundleData {
  campaigns: Doc<Campaign>[];
  sessions: Doc<Session>[];
  scenes: Doc<Scene>[];
  boards: Doc<Board>[];
  sets: Doc<MusicSet>[];
}

const FILTERS = [{ name: "GMAudio bundle", extensions: ["gmaudio"] }];

/** Everything one campaign needs: its own things, and shared ones it uses. */
function collect(campaignId: number | null): BundleData {
  const docs = useDocs.getState();
  if (campaignId === null) {
    return { campaigns: docs.campaigns, sessions: docs.sessions, scenes: docs.scenes, boards: docs.boards, sets: docs.sets };
  }
  const sessions = docs.sessions.filter((s) => s.data.campaign_id === campaignId);
  const scenes = new Map(docs.scenes.filter((s) => s.data.campaign_id === campaignId).map((s) => [s.id, s]));
  const boards = new Map(docs.boards.filter((b) => b.data.campaign_id === campaignId).map((b) => [b.id, b]));
  const sets = new Map(docs.sets.filter((s) => s.data.campaign_id === campaignId).map((s) => [s.id, s]));
  const addScene = (id: number) => {
    const scene = docs.scenes.find((s) => s.id === id);
    if (scene) scenes.set(id, scene);
  };
  const addBoard = (id: number | null) => {
    const board = docs.boards.find((b) => b.id === id);
    if (board) boards.set(board.id, board);
  };
  // Shared scenes and boards the cue lists fire.
  for (const session of sessions) {
    for (const cue of session.data.cues) {
      if (cue.action.type === "scene") addScene(cue.action.id);
      if (cue.action.type === "pad") addBoard(cue.action.board);
    }
  }
  // Shared boards and music the scenes use.
  for (const scene of scenes.values()) {
    addBoard(scene.data.board_id);
    const music = scene.data.music;
    if (music.kind === "set") {
      const set = docs.sets.find((s) => s.id === music.set_id);
      if (set) sets.set(set.id, set);
    }
  }
  return {
    campaigns: docs.campaigns.filter((c) => c.id === campaignId),
    sessions,
    scenes: [...scenes.values()],
    boards: [...boards.values()],
    sets: [...sets.values()],
  };
}

/** Every sound the bundle's contents refer to. */
function soundsOf(data: BundleData): number[] {
  const ids = new Set<number>();
  const add = (id: number | null | undefined) => {
    if (id != null) ids.add(id);
  };
  for (const { data: scene } of data.scenes) {
    if (scene.music.kind === "tracks") scene.music.asset_ids.forEach(add);
    scene.layers.forEach((l) => add(l.asset_id));
    scene.emitters.forEach((e) => e.asset_ids.forEach(add));
  }
  for (const { data: board } of data.boards) board.pads.forEach((p) => p.asset_ids.forEach(add));
  for (const { data: set } of data.sets) {
    set.tiers.forEach((t) => add(t.asset_id));
    set.stems.forEach((s) => add(s.asset_id));
    set.stingers.forEach((s) => add(s.asset_id));
  }
  for (const { data: session } of data.sessions) {
    for (const { action } of session.cues) {
      if (action.type === "stinger" || action.type === "sound") add(action.asset_id);
    }
  }
  return [...ids];
}

const megabytes = (bytes: number) => (bytes < 1_000_000 ? "under 1 MB" : `${Math.round(bytes / 1_000_000).toLocaleString()} MB`);

/** Asks where to save, then writes a campaign (or everything) to one file. */
export async function exportBundle(campaignId: number | null, includeAudio: boolean): Promise<void> {
  const notify = useStore.getState().notify;
  const data = collect(campaignId);
  const name = campaignId === null ? "GMAudio backup" : data.campaigns[0]?.data.name || "Campaign";
  const path = await save({ title: "Save the bundle as", defaultPath: `${name.replace(/[<>:"/\\|?*]/g, " ").trim()}.gmaudio`, filters: FILTERS });
  if (!path) return;
  try {
    notify(includeAudio ? "Saving, including the sound files…" : "Saving…", "info");
    const report = await api.bundleExport(path, data, soundsOf(data), includeAudio);
    const left = report.skipped.length > 0 ? ` ${report.skipped.length} sound file(s) could not be read and were left out.` : "";
    notify(`Saved (${megabytes(report.bytes)}, ${report.sounds} sounds).${left}`, report.skipped.length > 0 ? "error" : "info");
  } catch (e) {
    notify(message(e));
  }
}

/** Asks for a bundle file and adds what is inside. Nothing existing is changed. */
export async function importBundle(): Promise<void> {
  const notify = useStore.getState().notify;
  const path = await open({ title: "Choose a GMAudio bundle", multiple: false, filters: FILTERS });
  if (typeof path !== "string") return;
  try {
    notify("Opening the bundle…", "info");
    const report = await api.bundleImport(path);
    const data = report.data as Partial<BundleData> | null;
    if (!data || !Array.isArray(data.scenes)) throw new Error("That bundle has nothing GMAudio can use.");
    const counts = await recreate(
      {
        campaigns: data.campaigns ?? [],
        sessions: data.sessions ?? [],
        scenes: data.scenes ?? [],
        boards: data.boards ?? [],
        sets: data.sets ?? [],
      },
      new Map(Object.entries(report.asset_map).map(([from, to]) => [Number(from), to])),
    );
    await useStore.getState().refresh();
    const missing = report.missing.length > 0 ? ` ${report.missing.length} sound(s) were not in the bundle or your library and have been left out.` : "";
    notify(`Added ${counts}.${missing}`, report.missing.length > 0 ? "error" : "info");
  } catch (e) {
    notify(message(e));
  }
}

/** Creates the bundle's documents with new ids, pointing at this library's sounds. */
async function recreate(data: BundleData, sounds: Map<number, number>): Promise<string> {
  const docs = useDocs.getState();
  const sound = (id: number | null): number | null => (id == null ? null : (sounds.get(id) ?? null));
  const soundList = (ids: number[]) => ids.map((id) => sounds.get(id)).filter((id): id is number => id !== undefined);
  const campaigns = new Map<number, number>();
  const sessions = new Map<number, number>();
  const boards = new Map<number, number>();
  const sets = new Map<number, number>();
  const scenes = new Map<number, number>();
  const scope = <T extends { campaign_id: number | null; session_id?: number | null }>(item: T) => ({
    campaign_id: item.campaign_id == null ? null : (campaigns.get(item.campaign_id) ?? null),
    session_id: item.session_id == null ? null : (sessions.get(item.session_id) ?? null),
  });

  const taken = new Set(docs.campaigns.map((c) => c.data.name));
  for (const campaign of data.campaigns) {
    const name = taken.has(campaign.data.name) ? `${campaign.data.name} (imported)` : campaign.data.name;
    campaigns.set(campaign.id, await docs.save("campaign", null, { ...campaign.data, name }));
  }
  // Sessions first, without cues: scenes can belong to a session, and cues point at scenes.
  for (const session of data.sessions) {
    const campaign_id = campaigns.get(session.data.campaign_id);
    if (campaign_id === undefined) continue;
    sessions.set(session.id, await docs.save("session", null, { ...session.data, campaign_id, cues: [], position: 0 }));
  }
  for (const board of data.boards) {
    const pads = board.data.pads.map((p) => ({ ...p, asset_ids: soundList(p.asset_ids) }));
    boards.set(board.id, await docs.save("board", null, { ...board.data, ...scope(board.data), pads }));
  }
  for (const set of data.sets) {
    // A stem whose sound is missing goes, together with its column in every level.
    const keep = set.data.stems.map((s) => sounds.has(s.asset_id));
    sets.set(
      set.id,
      await docs.save("set", null, {
        ...set.data,
        ...scope(set.data),
        stems: set.data.stems.filter((_, i) => keep[i]).map((s) => ({ ...s, asset_id: sounds.get(s.asset_id)! })),
        tiers: set.data.tiers.map((t) => ({ ...t, asset_id: sound(t.asset_id), stems: t.stems.filter((_, i) => keep[i]) })),
        stingers: set.data.stingers.map((s) => ({ ...s, asset_id: sound(s.asset_id) })),
      }),
    );
  }
  for (const scene of data.scenes) {
    const music = scene.data.music;
    const setId = music.kind === "set" ? sets.get(music.set_id) : undefined;
    scenes.set(
      scene.id,
      await docs.save("scene", null, {
        ...scene.data,
        ...scope(scene.data),
        board_id: scene.data.board_id == null ? null : (boards.get(scene.data.board_id) ?? null),
        music:
          music.kind === "tracks"
            ? { kind: "tracks", asset_ids: soundList(music.asset_ids) }
            : music.kind === "set"
              ? setId !== undefined
                ? { ...music, set_id: setId }
                : { kind: "keep" }
              : music,
        layers: scene.data.layers.filter((l) => sounds.has(l.asset_id)).map((l) => ({ ...l, asset_id: sounds.get(l.asset_id)! })),
        emitters: scene.data.emitters.map((e) => ({ ...e, asset_ids: soundList(e.asset_ids) })).filter((e) => e.asset_ids.length > 0),
      }),
    );
  }
  for (const session of data.sessions) {
    const id = sessions.get(session.id);
    const campaign_id = campaigns.get(session.data.campaign_id);
    if (id === undefined || campaign_id === undefined) continue;
    const cues: Cue[] = [];
    for (const { state: _state, ...cue } of session.data.cues) {
      const a = cue.action;
      if (a.type === "scene") {
        const scene = scenes.get(a.id);
        if (scene !== undefined) cues.push({ ...cue, action: { ...a, id: scene } });
      } else if (a.type === "pad") {
        const board = boards.get(a.board);
        if (board !== undefined) cues.push({ ...cue, action: { ...a, board } });
      } else if (a.type === "stinger" || a.type === "sound") {
        const asset_id = sounds.get(a.asset_id);
        if (asset_id !== undefined) cues.push({ ...cue, action: { ...a, asset_id } });
      } else {
        cues.push(cue);
      }
    }
    await docs.save("session", id, { ...session.data, campaign_id, cues, position: 0 });
  }

  const first = data.campaigns[0];
  if (first) {
    const campaign = campaigns.get(first.id)!;
    const session = data.sessions.find((s) => s.data.campaign_id === first.id);
    useDocs.getState().selectSession(campaign, session ? (sessions.get(session.id) ?? null) : null);
  }
  const parts = [
    [campaigns.size, "campaign"],
    [sessions.size, "session"],
    [scenes.size, "scene"],
    [boards.size, "board"],
    [sets.size, "music set"],
  ] as const;
  const listed = parts.filter(([n]) => n > 0).map(([n, word]) => `${n} ${word}${n === 1 ? "" : "s"}`);
  return listed.length > 0 ? listed.join(", ") : "nothing new";
}
