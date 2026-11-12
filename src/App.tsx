import { useEffect } from "react";
import { useStore } from "./store";
import { TopBar } from "./components/TopBar";
import { Sidebar } from "./components/Sidebar";
import { Browser } from "./components/Browser";
import { Mixer } from "./components/Mixer";
import { Notice } from "./components/Notice";
import { DeviceBanner } from "./components/DeviceBanner";

export default function App() {
  const init = useStore((s) => s.init);
  const panic = useStore((s) => s.panic);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    let cancelled = false;
    void init().then((off) => {
      if (cancelled) off();
      else dispose = off;
    });
    return () => {
      cancelled = true;
      dispose?.();
    };
  }, [init]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Esc is the panic key everywhere, even while typing in the search box.
      if (e.key === "Escape") {
        e.preventDefault();
        void panic();
        return;
      }
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement;
      if (!typing && (e.key === "/" || (e.ctrlKey && e.key.toLowerCase() === "f"))) {
        e.preventDefault();
        document.getElementById("library-search")?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [panic]);

  return (
    <div className="flex h-full flex-col">
      <TopBar />
      <DeviceBanner />
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        <Browser />
      </div>
      <Mixer />
      <Notice />
    </div>
  );
}
