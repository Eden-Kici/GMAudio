import { useDocs, type View } from "../docs";
import { BoltIcon, GearIcon, GridIcon, LibraryIcon, ListIcon, SceneIcon, WaveIcon } from "./icons";

const ITEMS: { view: View; label: string; hint: string; icon: typeof GearIcon }[] = [
  { view: "perform", label: "Play", hint: "Run the game: cues, scenes and pads on one screen", icon: BoltIcon },
  { view: "library", label: "Library", hint: "All your music and sounds", icon: LibraryIcon },
  { view: "scenes", label: "Scenes", hint: "Whole soundscapes on one button", icon: SceneIcon },
  { view: "boards", label: "Pads", hint: "Soundboard: effects on pads", icon: GridIcon },
  { view: "sets", label: "Music", hint: "Dynamic music that changes with the action", icon: WaveIcon },
  { view: "cues", label: "Cues", hint: "A session's running order, fired with Space", icon: ListIcon },
  { view: "settings", label: "Settings", hint: "Keys and options", icon: GearIcon },
];

/** The main navigation, down the left edge. */
export function Rail() {
  const view = useDocs((s) => s.view);
  const setView = useDocs((s) => s.setView);
  return (
    <nav aria-label="Sections" className="flex w-[4.5rem] shrink-0 flex-col items-center gap-1 border-r border-line bg-panel py-2">
      {ITEMS.map(({ view: id, label, hint, icon: Icon }) => (
        <button
          key={id}
          onClick={() => setView(id)}
          aria-current={view === id ? "page" : undefined}
          title={hint}
          className={`flex w-14 flex-col items-center gap-1 rounded-lg py-2 text-[11px] font-medium ${
            view === id ? "bg-raised text-accent" : "text-dim hover:bg-raised/60 hover:text-ink"
          } ${id === "settings" ? "mt-auto" : ""}`}
        >
          <Icon width={20} height={20} />
          {label}
        </button>
      ))}
    </nav>
  );
}
