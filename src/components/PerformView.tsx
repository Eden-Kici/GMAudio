import { useEffect, useState } from "react";
import { preloadBoards, setVisibleBoard } from "../actions";
import { useDocs, useScoped } from "../docs";
import { useStore } from "../store";
import { PadGrid } from "./BoardsView";
import { CueStrip } from "./CuesView";
import { SceneTile } from "./ScenesView";

/** The view for running a session: cues, scenes and pads on one screen. */
export function PerformView() {
  const scenes = useScoped("scenes");
  const boards = useScoped("boards");
  const setView = useDocs((s) => s.setView);
  const activeScene = useStore((s) => s.engine?.scene?.id ?? null);
  const [chosen, setChosen] = useState<number | null>(null);

  // A scene can bring its own board to the front.
  const sceneBoard = scenes.find((s) => s.id === activeScene)?.data.board_id ?? null;
  useEffect(() => {
    if (sceneBoard !== null) setChosen(sceneBoard);
  }, [sceneBoard]);

  const board = boards.find((b) => b.id === chosen) ?? boards[0];
  useEffect(() => {
    setVisibleBoard(board ?? null);
    return () => setVisibleBoard(null);
  }, [board]);
  useEffect(() => preloadBoards(boards), [boards]);

  const empty = scenes.length === 0 && boards.length === 0;
  const link = "font-medium text-accent underline-offset-2 hover:underline";

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
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,5fr)_minmax(0,6fr)] divide-x divide-line">
          <section className="min-h-0 overflow-y-auto p-4" aria-label="Scenes">
            <h2 className="pb-3 text-xs font-semibold uppercase tracking-wider text-dim">Scenes</h2>
            {scenes.length === 0 ? (
              <p className="text-sm text-dim">No scenes yet.</p>
            ) : (
              <div className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-2.5">
                {scenes.map((scene, index) => (
                  <SceneTile key={scene.id} scene={scene} index={index} />
                ))}
              </div>
            )}
          </section>
          <section className="min-h-0 overflow-y-auto p-4" aria-label="Soundboard">
            <div className="flex flex-wrap items-center gap-1 pb-3">
              <h2 className="mr-2 text-xs font-semibold uppercase tracking-wider text-dim">Pads</h2>
              {boards.map((b) => (
                <button
                  key={b.id}
                  onClick={() => setChosen(b.id)}
                  className={`h-7 rounded-md px-2.5 text-xs font-medium ${b.id === board?.id ? "bg-raised text-ink" : "text-dim hover:text-ink"}`}
                >
                  {b.data.name || "Untitled"}
                </button>
              ))}
            </div>
            {board ? <PadGrid board={board} /> : <p className="text-sm text-dim">No boards yet.</p>}
          </section>
        </div>
      )}
    </main>
  );
}
