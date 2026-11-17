import { api } from "../api";
import { describeAction } from "../actions";
import { useDocs } from "../docs";
import { useStore } from "../store";
import { useEffect, useState } from "react";
import { BackupSection } from "./Backup";
import { CloseIcon } from "./icons";
import { OnlineSounds } from "./Online";
import { ProfilePicker } from "./Profile";
import { OutputDevice } from "./Setup";
import { buttonClass, Field, Slider, Toggle } from "./ui";

export function SettingsView() {
  const advanced = useDocs((s) => s.advanced);
  const setAdvanced = useDocs((s) => s.setAdvanced);
  const showFileName = useDocs((s) => s.showFileName);
  const setShowFileName = useDocs((s) => s.setShowFileName);
  const searchFileNames = useDocs((s) => s.searchFileNames);
  const setSearchFileNames = useDocs((s) => s.setSearchFileNames);
  const hotkeys = useDocs((s) => s.hotkeys);
  const setHotkeys = useDocs((s) => s.setHotkeys);
  const duckDb = useStore((s) => s.engine?.duck_db ?? -8);
  const setSetupDone = useDocs((s) => s.setSetupDone);
  const [online, setOnline] = useState(false);
  const [version, setVersion] = useState("");
  useEffect(() => {
    void api.appVersion().then(setVersion).catch(() => {});
  }, []);
  const copyCredits = async () => {
    const credits = await api.credits();
    if (credits.length === 0) return useStore.getState().notify("No downloaded sounds need a credit yet.", "info");
    await navigator.clipboard.writeText(credits.join("\n"));
    useStore.getState().notify(`${credits.length} credit line${credits.length === 1 ? "" : "s"} copied.`, "info");
  };

  return (
    <main className="min-w-0 flex-1 overflow-y-auto p-5">
      <div className="mx-auto max-w-2xl space-y-8">
        <h1 className="text-lg font-semibold">Settings</h1>

        <section className="space-y-3 rounded-xl bg-panel p-4">
          <Toggle
            on={advanced}
            onChange={setAdvanced}
            label="Show advanced controls"
            hint="Fade times, solo, level meters, pad variation, system-wide keys and more. Off keeps things simple."
          />
        </section>

        <section className="space-y-3 rounded-xl bg-panel p-4">
          <Toggle
            on={showFileName}
            onChange={setShowFileName}
            label="Show file names of renamed sounds"
            hint="In the library, a renamed sound shows its file's name in small print next to the new name."
          />
          <Toggle
            on={searchFileNames}
            onChange={setSearchFileNames}
            label="Search file names too"
            hint="Searching finds a renamed sound by its file's name as well as by its new name."
          />
        </section>

        <section className="space-y-4 rounded-xl bg-panel p-4">
          <Field label="Sound comes out of" hint="Changing this stops whatever is playing">
            <OutputDevice />
          </Field>
          <Field label="Tune the sound for" hint="Shapes everything that plays">
            <ProfilePicker />
          </Field>
          <Field label="Lower music during stingers" hint="Also for pads set to lower the music">
            <Slider
              value={duckDb}
              min={-24}
              max={0}
              onChange={(db) => void api.setDuckDb(db)}
              format={(db) => (db === 0 ? "Not at all" : db >= -6 ? "A little" : db >= -12 ? "Clearly" : "A lot")}
              label="How much the music drops"
            />
          </Field>
        </section>

        <section className="space-y-3 rounded-xl bg-panel p-4">
          <Field label="Keys" hint="Set a key on any scene or pad from its editor">
            <ul className="divide-y divide-line text-sm">
              {[
                ["Esc", "Stop all: fade out. Press twice to cut."],
                ["1 to 0", "Press the first ten pads of the board on screen"],
                ["F1 to F12", "Switch to the first twelve scenes"],
                ["Space", "GO: fire the next cue of the session"],
                ["Page Up / Down", "Dynamic music: one level up or down"],
                ["Ctrl+K", "Find and fire anything by name"],
                ["/", "Search the library"],
              ].map(([key, what]) => (
                <li key={key} className="flex items-center gap-3 py-2">
                  <kbd className="w-24 shrink-0 rounded bg-raised px-2 py-1 text-center text-xs font-semibold">{key}</kbd>
                  <span className="flex-1 text-dim">{what}</span>
                  <span className="text-xs text-faint">built in</span>
                </li>
              ))}
              {hotkeys.map((h) => (
                <li key={h.combo} className="flex items-center gap-3 py-2">
                  <kbd className="w-24 shrink-0 rounded bg-raised px-2 py-1 text-center text-xs font-semibold">{h.combo}</kbd>
                  <span className="min-w-0 flex-1 truncate">{describeAction(h.action)}</span>
                  {advanced && (
                    <button
                      onClick={() => setHotkeys(hotkeys.map((x) => (x.combo === h.combo ? { ...x, global: !x.global } : x)))}
                      aria-pressed={h.global}
                      title={h.global ? "Works in other programs too. Click to limit it to GMAudio." : "Only while GMAudio is in front. Click to make it work everywhere."}
                      className={`rounded px-2 py-1 text-xs font-medium ${h.global ? "bg-accent/20 text-accent" : "text-dim hover:bg-raised"}`}
                    >
                      {h.global ? "Everywhere" : "This app"}
                    </button>
                  )}
                  <button onClick={() => setHotkeys(hotkeys.filter((x) => x.combo !== h.combo))} aria-label={`Remove ${h.combo}`} className="rounded p-1.5 text-dim hover:bg-raised hover:text-ink">
                    <CloseIcon width={12} height={12} />
                  </button>
                </li>
              ))}
            </ul>
          </Field>
        </section>
        <section className="space-y-4 rounded-xl bg-panel p-4">
          <Field label="Online sounds" hint="Freesound: free effects and ambiences">
            <div className="flex flex-wrap items-center gap-2">
              <button onClick={() => setOnline(true)} className={buttonClass}>
                Freesound key and sign-in…
              </button>
              <button onClick={() => void copyCredits()} className={buttonClass} title="The credit lines of every sound downloaded from Freesound, ready to paste">
                Copy credits
              </button>
            </div>
          </Field>
          <Field label="Backup and moving" hint="One file with your campaigns, scenes, pads and music">
            <BackupSection />
          </Field>
        </section>
        {online && <OnlineSounds onClose={() => setOnline(false)} />}

        <section className="flex items-center justify-between rounded-xl bg-panel p-4">
          <span className="text-sm text-dim">Walk through first-time setup again, including the example scenes.</span>
          <button onClick={() => setSetupDone(false)} className={buttonClass}>
            Run setup
          </button>
        </section>
        <p className="pb-2 text-center text-xs text-faint">GMAudio version {version || "…"}</p>
      </div>
    </main>
  );
}
