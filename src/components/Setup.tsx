import { useEffect, useState } from "react";
import { api } from "../api";
import { useDocs } from "../docs";
import { message, useStore } from "../store";
import { COLORS, newPad, type Board, type MusicSet, type Scene } from "../types";
import { ProfilePicker } from "./Profile";
import { pickFolder } from "./Sidebar";
import { buttonClass, inputClass, primaryClass } from "./ui";

/** Chooses which speakers or headphones the sound comes out of. */
export function OutputDevice() {
  const [devices, setDevices] = useState<string[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  useEffect(() => {
    void api.outputDevices().then((found) => {
      setDevices(found.devices);
      setSelected(found.selected);
    });
  }, []);
  return (
    <select
      value={selected ?? ""}
      onChange={(e) => {
        const name = e.target.value || null;
        setSelected(name);
        void api.setOutputDevice(name).catch((err) => useStore.getState().notify(message(err)));
      }}
      aria-label="Sound output"
      className={inputClass}
    >
      <option value="">Whatever the computer is using (recommended)</option>
      {devices.map((d) => (
        <option key={d} value={d}>
          {d}
        </option>
      ))}
    </select>
  );
}

/** Waits until the library has finished looking through its folders. */
async function libraryIdle(timeoutMs = 60_000) {
  const until = Date.now() + timeoutMs;
  while (Date.now() < until) {
    const status = await api.scanStatus();
    if (status.scanning === null && status.remaining === 0) return;
    await new Promise((r) => setTimeout(r, 300));
  }
}

/**
 * Builds a scene, a soundboard and a piece of dynamic music from the
 * built-in sounds, so there is something to press straight away.
 */
export async function buildExample(): Promise<boolean> {
  await libraryIdle();
  const find = async (name: string) => (await api.search(name, null, [], 5)).assets.find((a) => a.name === name)?.id;
  const names = [
    "Tavern Evening", "Campfire", "Rain", "Wind", "Thunder", "Quiet Road", "Dungeon Depths", "Cave Drips",
    "Sword Clash", "Arrow Shot", "Fireball", "Door Knock", "Coin Drop", "Bell Toll", "Heartbeat", "Magic Chime", "Heavy Impact",
    "Victory Fanfare", "Critical Hit", "Ominous Reveal", "Defeat",
    "Battle March - Drums", "Battle March - Strings", "Battle March - Brass",
  ];
  const ids = new Map<string, number>();
  for (const name of names) {
    const id = await find(name);
    if (id !== undefined) ids.set(name, id);
  }
  if (ids.size < names.length) return false;
  const id = (name: string) => ids.get(name)!;
  const docs = useDocs.getState();
  const shared = { campaign_id: null, session_id: null };

  const pad = (label: string, sounds: string[], color: string, extra: object = {}) => ({ ...newPad(), label, asset_ids: sounds.map(id), color, ...extra });
  const board: Board = {
    name: "Example pads",
    columns: 5,
    ...shared,
    pads: [
      pad("Sword clash", ["Sword Clash"], COLORS[7], { pitch_var: 1.5, volume_var: 3 }),
      pad("Arrow", ["Arrow Shot"], COLORS[7]),
      pad("Fireball", ["Fireball"], COLORS[1]),
      pad("Thunder", ["Thunder"], COLORS[4]),
      pad("Heavy impact", ["Heavy Impact"], COLORS[1]),
      pad("Door knock", ["Door Knock"], COLORS[6]),
      pad("Coins", ["Coin Drop"], COLORS[0]),
      pad("Bell", ["Bell Toll"], COLORS[5]),
      pad("Magic", ["Magic Chime"], COLORS[3]),
      pad("Heartbeat", ["Heartbeat"], COLORS[2], { mode: "loop" }),
    ],
  };
  const boardId = await docs.save("board", null, board);

  const set: MusicSet = {
    name: "Battle March (example)",
    color: COLORS[1],
    kind: "stems",
    bpm: 140,
    beats_per_bar: 4,
    bars_per_phrase: 4,
    offset_ms: 0,
    crossfade_ms: 600,
    quantize: "bar",
    stems: ["Drums", "Strings", "Brass"].map((s) => ({ name: s, asset_id: id(`Battle March - ${s}`) })),
    tiers: [
      { name: "Calm", asset_id: null, stems: [null, 0, null] },
      { name: "Tense", asset_id: null, stems: [0, 0, null] },
      { name: "Combat", asset_id: null, stems: [0, 0, 0] },
    ],
    stingers: [
      { name: "Victory", asset_id: id("Victory Fanfare"), quantize: "bar", then_tier: 0, duck: true },
      { name: "Critical hit", asset_id: id("Critical Hit"), quantize: "beat", then_tier: null, duck: true },
      { name: "Defeat", asset_id: id("Defeat"), quantize: "bar", then_tier: 0, duck: true },
    ],
    ...shared,
  };
  const setId = await docs.save("set", null, set);

  const scene = (name: string, color: string, music: Scene["music"], layers: [string, number][], emitters: Scene["emitters"] = []): Scene => ({
    name,
    color,
    transition_ms: 3000,
    music,
    layers: layers.map(([sound, volume_db]) => ({ asset_id: id(sound), volume_db })),
    emitters,
    board_id: boardId,
    ...shared,
  });
  const tracks = (...sounds: string[]): Scene["music"] => ({ kind: "tracks", asset_ids: sounds.map(id) });
  await docs.save("scene", null, scene("Tavern", COLORS[0], tracks("Tavern Evening"), [["Campfire", -3]]));
  await docs.save(
    "scene",
    null,
    scene("Storm on the road", COLORS[4], tracks("Quiet Road"), [["Rain", 0], ["Wind", -6]], [
      { name: "Distant thunder", asset_ids: [id("Thunder")], min_s: 20, max_s: 60, volume_db: -3, random_pan: true },
    ]),
  );
  await docs.save("scene", null, scene("Crypt", COLORS[3], tracks("Dungeon Depths"), [["Cave Drips", 0], ["Wind", -12]]));
  await docs.save("scene", null, scene("Battle!", COLORS[1], { kind: "set", set_id: setId, tier: 1 }, [["Rain", -6]]));
  return true;
}

/** First launch: three short steps to sound coming out of the speakers. */
export function SetupWizard() {
  const setSetupDone = useDocs((s) => s.setSetupDone);
  const setView = useDocs((s) => s.setView);
  const addFolder = useStore((s) => s.addFolder);
  const addStarterSounds = useStore((s) => s.addStarterSounds);
  const folders = useStore((s) => s.folders);
  const [step, setStep] = useState(0);
  const [busy, setBusy] = useState(false);
  const hasStarter = folders.some((f) => /[\\/]Starter Sounds$/i.test(f.path));

  const finish = (view: "perform" | "library") => {
    setSetupDone(true);
    setView(view);
  };
  const withExample = async () => {
    setBusy(true);
    if (!hasStarter) await addStarterSounds();
    const built = await buildExample();
    setBusy(false);
    if (!built) useStore.getState().notify("The built-in sounds are still being read. Try again in a moment.", "info");
    else finish("perform");
  };

  const steps = [
    {
      title: "Where should the sound come out?",
      body: (
        <>
          <p className="text-sm leading-relaxed text-dim">Pick the speakers or headphones your players will hear. You can change this later in Settings.</p>
          <OutputDevice />
          <p className="pt-1 text-sm leading-relaxed text-dim">What kind are they? GMAudio tunes the sound to suit.</p>
          <ProfilePicker />
        </>
      ),
    },
    {
      title: "Add your music and sounds",
      body: (
        <>
          <p className="text-sm leading-relaxed text-dim">
            Point GMAudio at the folder where you keep them. Files stay where they are. No collection yet? Start with
            the built-in sounds and add your own later.
          </p>
          <div className="flex flex-wrap gap-2">
            <button
              className={buttonClass}
              onClick={async () => {
                const path = await pickFolder();
                if (path) await addFolder(path);
              }}
            >
              Choose a folder…
            </button>
            <button className={buttonClass} disabled={hasStarter} onClick={() => void addStarterSounds()}>
              {hasStarter ? "Built-in sounds added" : "Add the built-in sounds"}
            </button>
          </div>
          {folders.length > 0 && <p className="text-xs text-faint">{folders.length === 1 ? "1 folder added." : `${folders.length} folders added.`} You can add more at any time.</p>}
        </>
      ),
    },
    {
      title: "Want an example to play with?",
      body: (
        <p className="text-sm leading-relaxed text-dim">
          GMAudio can build four scenes (a tavern, a storm, a crypt and a battle), a board of sound-effect pads and a
          piece of music that changes with the action, all from the built-in sounds. Press things to see how they
          work, then change or delete them.
        </p>
      ),
    },
  ];
  const last = step === steps.length - 1;

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/70 p-6">
      <div role="dialog" aria-label="Set up GMAudio" className="w-full max-w-lg rounded-2xl border border-line bg-panel p-6 shadow-2xl shadow-black/70">
        <p className="text-xs font-semibold uppercase tracking-wider text-accent">
          Step {step + 1} of {steps.length}
        </p>
        <h1 className="mt-1.5 text-xl font-semibold">{steps[step].title}</h1>
        <div className="mt-4 space-y-4">{steps[step].body}</div>
        <div className="mt-7 flex items-center gap-2">
          <button onClick={() => finish("library")} className="mr-auto text-xs font-medium text-dim hover:text-ink">
            Skip setup
          </button>
          {step > 0 && (
            <button onClick={() => setStep(step - 1)} className={buttonClass}>
              Back
            </button>
          )}
          {last ? (
            <>
              <button onClick={() => finish("library")} className={buttonClass} disabled={busy}>
                No thanks
              </button>
              <button onClick={() => void withExample()} className={primaryClass} disabled={busy}>
                {busy ? "Building…" : "Build the example"}
              </button>
            </>
          ) : (
            <button onClick={() => setStep(step + 1)} className={primaryClass}>
              Next
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
