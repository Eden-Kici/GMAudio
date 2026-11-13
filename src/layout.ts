// Sizes the user has dragged things to: panel widths and heights, and how
// space is shared between panes. Remembered between runs.

import { create } from "zustand";
import { api } from "./api";

interface LayoutState {
  sizes: Record<string, number[]>;
  load: () => Promise<void>;
  set: (key: string, value: number[]) => void;
}

let saving: ReturnType<typeof setTimeout> | undefined;

export const useLayout = create<LayoutState>((set, get) => ({
  sizes: {},
  load: async () => {
    try {
      const saved = await api.uiGet("layout");
      if (saved) set({ sizes: JSON.parse(saved) as Record<string, number[]> });
    } catch {
      // A broken or missing layout just means the default sizes.
    }
  },
  set: (key, value) => {
    set({ sizes: { ...get().sizes, [key]: value } });
    clearTimeout(saving);
    saving = setTimeout(() => void api.uiSet("layout", JSON.stringify(get().sizes)).catch(() => {}), 400);
  },
}));

/** A remembered list of sizes. Falls back when nothing fitting is saved. */
export function useSizes(key: string, fallback: number[]): [number[], (value: number[]) => void] {
  const saved = useLayout((s) => s.sizes[key]);
  const set = useLayout((s) => s.set);
  const fits = saved && saved.length === fallback.length && saved.every((n) => Number.isFinite(n) && n > 0);
  return [fits ? saved : fallback, (value) => set(key, value)];
}
