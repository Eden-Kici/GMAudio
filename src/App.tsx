import { useEffect, useState } from "react";
import { handleKey, preloadBoards, syncGlobalHotkeys } from "./actions";
import { useDocs } from "./docs";
import { useStore } from "./store";
import { TopBar } from "./components/TopBar";
import { Sidebar } from "./components/Sidebar";
import { Browser } from "./components/Browser";
import { Mixer } from "./components/Mixer";
import { Notice } from "./components/Notice";
import { DeviceBanner } from "./components/DeviceBanner";
import { ContextMenu } from "./components/ContextMenu";
import { Rail } from "./components/Rail";
import { ScenesView } from "./components/ScenesView";
import { BoardsView } from "./components/BoardsView";
import { SettingsView } from "./components/SettingsView";
import { Palette } from "./components/Palette";

export default function App() {
  const init = useStore((s) => s.init);
  const panic = useStore((s) => s.panic);
  const loadDocs = useDocs((s) => s.load);
  const view = useDocs((s) => s.view);
  const hotkeys = useDocs((s) => s.hotkeys);
  const docsLoaded = useDocs((s) => s.loaded);
  const [palette, setPalette] = useState(false);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    let cancelled = false;
    void init().then((off) => {
      if (cancelled) off();
      else dispose = off;
    });
    void loadDocs().then(() => preloadBoards(useDocs.getState().boards));
    return () => {
      cancelled = true;
      dispose?.();
    };
  }, [init, loadDocs]);

  // Keys that should work while another program is in front.
  useEffect(() => {
    if (docsLoaded) void syncGlobalHotkeys(hotkeys);
  }, [docsLoaded, hotkeys]);

  useEffect(() => {
    const isTyping = (e: KeyboardEvent) =>
      e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement || e.target instanceof HTMLTextAreaElement;

    const onDown = (e: KeyboardEvent) => {
      // Esc is the panic key everywhere, even while typing in the search box.
      // The one exception: an open menu, which Esc closes instead.
      if (e.key === "Escape") {
        e.preventDefault();
        const { menu, openMenu } = useStore.getState();
        if (menu) openMenu(null);
        else void panic();
        return;
      }
      if (e.ctrlKey && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((open) => !open);
        return;
      }
      const typing = isTyping(e);
      if (handleKey(e, "down", typing)) {
        e.preventDefault();
        return;
      }
      if (!typing && (e.key === "/" || (e.ctrlKey && e.key.toLowerCase() === "f"))) {
        e.preventDefault();
        if (useDocs.getState().view !== "library") useDocs.getState().setView("library");
        const pane = useStore.getState().activePane;
        requestAnimationFrame(() => document.querySelector<HTMLInputElement>(`[data-pane-search="${pane}"]`)?.focus());
      }
    };
    const onUp = (e: KeyboardEvent) => {
      handleKey(e, "up", isTyping(e));
    };
    window.addEventListener("keydown", onDown);
    window.addEventListener("keyup", onUp);
    return () => {
      window.removeEventListener("keydown", onDown);
      window.removeEventListener("keyup", onUp);
    };
  }, [panic]);

  return (
    <div className="flex h-full flex-col">
      <TopBar onPalette={() => setPalette(true)} />
      <DeviceBanner />
      <div className="flex min-h-0 flex-1">
        <Rail />
        {view === "library" && (
          <>
            <Sidebar />
            <Browser />
          </>
        )}
        {view === "scenes" && <ScenesView />}
        {view === "boards" && <BoardsView />}
        {view === "settings" && <SettingsView />}
      </div>
      <Mixer />
      <Notice />
      <ContextMenu />
      {palette && <Palette onClose={() => setPalette(false)} />}
    </div>
  );
}
