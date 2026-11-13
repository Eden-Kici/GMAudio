import { useEffect } from "react";
import { playSet, preloadBoards } from "../actions";
import { useDocs, useScoped } from "../docs";
import { useSizes } from "../layout";
import { useStore } from "../store";
import { BoardPanes } from "./BoardsView";
import { CueStrip } from "./CuesView";
import { Split } from "./Resize";
import { SceneTile } from "./ScenesView";
import { SetPanel } from "./SetsView";

const SECTIONS = [
  { id: "scenes", label: "Scenes" },
  { id: "pads", label: "Pads" },
  { id: "music", label: "Music" },
] as const;

/** The view for running a session: cues, scenes, pads and dynamic music on one screen. */
export function PerformView() {
  const scenes = useScoped("scenes");
  const boards = useScoped("boards");
  const sets = useScoped("sets");
  const setView = useDocs((s) => s.setView);
  const activeScene = useStore((s) => s.engine?.scene?.id ?? null);
  const playingSet = useStore((s) => s.engine?.music_set?.id ?? null);
  // Which sections are showing: 1 is on, 2 is off.
  const [show, setShow] = useSizes("play-show", [1, 1, 1]);
  const on = (id: (typeof SECTIONS)[number]["id"]) => show[SECTIONS.findIndex((s) => s.id === id)] === 1;

  // A scene can bring its own board to the front.
  const sceneBoard = scenes.find((s) => s.id === activeScene)?.data.board_id ?? null;
  useEffect(() => preloadBoards(boards), [boards]);

  const empty = scenes.length === 0 && boards.length === 0 && sets.length === 0;
  const link = "font-medium text-accent underline-offset-2 hover:underline";
  const heading = "px-4 pb-1 pt-3 text-xs font-semibold uppercase tracking-wider text-dim";

  const parts = [
    on("scenes") && (
      <section key="scenes" className="flex min-h-0 flex-1 flex-col" aria-label="Scenes">
        <h2 className={heading}>Scenes</h2>
        <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-4 pt-2">
          {scenes.length === 0 ? (
            <p className="text-sm text-dim">No scenes yet.</p>
          ) : (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-2.5">
              {scenes.map((scene, index) => (
                <SceneTile key={scene.id} scene={scene} index={index} />
              ))}
            </div>
          )}
        </div>
      </section>
    ),
    on("pads") && (
      <section key="pads" className="flex min-h-0 flex-1 flex-col" aria-label="Pads">
        {boards.length === 0 ? (
          <>
            <h2 className={heading}>Pads</h2>
            <p className="px-4 pt-2 text-sm text-dim">No boards yet.</p>
          </>
        ) : (
          <BoardPanes id="play" boards={boards} front={sceneBoard} />
        )}
      </section>
    ),
    on("music") && (
      <section key="music" className="flex min-h-0 flex-1 flex-col" aria-label="Dynamic music">
        <h2 className={heading}>Music</h2>
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-4 pb-4 pt-2">
          {sets.length === 0 ? (
            <p className="text-sm text-dim">
              No dynamic music yet. Make some under{" "}
              <button className={link} onClick={() => setView("sets")}>
                Music
              </button>
              .
            </p>
          ) : (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-2.5">
              {sets.map((set) => (
                <button
                  key={set.id}
                  onClick={() => void playSet(set)}
                  style={{ "--c": set.data.color } as React.CSSProperties}
                  title={playingSet === set.id ? "Playing. Press to start again from the first level." : "Play from the first level"}
                  className={`flex h-16 flex-col justify-between rounded-xl border-2 p-2.5 text-left transition-colors ${
                    playingSet === set.id ? "border-(--c) bg-(--c)/25" : "border-(--c)/40 bg-(--c)/10 hover:bg-(--c)/20"
                  }`}
                >
                  <span className="truncate text-sm font-semibold">{set.data.name || "Untitled"}</span>
                  <span className="text-[11px] text-dim">
                    {playingSet === set.id ? "Playing" : `${set.data.tiers.length} level${set.data.tiers.length === 1 ? "" : "s"}`}
                  </span>
                </button>
              ))}
            </div>
          )}
          {/* Levels and stingers of whatever is playing. */}
          <SetPanel />
        </div>
      </section>
    ),
  ].filter(Boolean);

  return (
    <main className="flex min-h-0 min-w-0 flex-1 flex-col">
      <CueStrip />
      {empty ? (
        <div className="flex flex-1 items-center justify-center p-8">
          <div className="max-w-md text-center text-dim">
            <p className="text-[15px] text-ink">This is where you run the game</p>
            <p className="mt-2 text-sm leading-relaxed">
              Your scenes and sound pads show up here, ready to press. Make some first under{" "}
              <button className={link} onClick={() => setView("scenes")}>
                Scenes
              </button>{" "}
              and{" "}
              <button className={link} onClick={() => setView("boards")}>
                Pads
              </button>
              , or just play sounds straight from the{" "}
              <button className={link} onClick={() => setView("library")}>
                Library
              </button>
              .
            </p>
          </div>
        </div>
      ) : (
        <>
          <div className="flex items-center gap-1.5 border-b border-line px-4 py-1.5 text-xs">
            <span className="mr-1 text-faint">Show</span>
            {SECTIONS.map((section, i) => (
              <button
                key={section.id}
                onClick={() => setShow(show.map((v, j) => (j === i ? (v === 1 ? 2 : 1) : v)))}
                aria-pressed={show[i] === 1}
                className={`h-6 rounded-full border px-2.5 font-medium ${
                  show[i] === 1 ? "border-ink bg-ink text-bg" : "border-line text-dim hover:text-ink"
                }`}
              >
                {section.label}
              </button>
            ))}
          </div>
          {parts.length === 0 ? (
            <p className="p-8 text-center text-sm text-dim">Everything is hidden. Choose what to show above.</p>
          ) : (
            // Each combination of sections keeps its own sizes.
            <Split id={`play-${show.join("")}`} min={200} className="min-h-0 flex-1">
              {parts}
            </Split>
          )}
        </>
      )}
    </main>
  );
}
