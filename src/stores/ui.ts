import { create } from "zustand";

export type View = { kind: "home" } | { kind: "settings" } | { kind: "env"; id: string };

export interface Toast {
  id: number;
  tone: "info" | "success" | "error";
  text: string;
}

interface UiState {
  view: View;
  /** Ids of the environments open as tabs, in tab order. */
  openIds: string[];
  toasts: Toast[];
  show: (view: View) => void;
  openEnv: (id: string) => void;
  /** Remove the tab; returns nothing — callers close the session themselves. */
  closeEnv: (id: string) => void;
  toast: (tone: Toast["tone"], text: string) => void;
  dismissToast: (id: number) => void;
}

let nextToast = 1;

export const useUi = create<UiState>((set, get) => ({
  view: { kind: "home" },
  openIds: [],
  toasts: [],
  show: (view) => set({ view }),
  openEnv: (id) =>
    set((s) => ({
      openIds: s.openIds.includes(id) ? s.openIds : [...s.openIds, id],
      view: { kind: "env", id },
    })),
  closeEnv: (id) =>
    set((s) => {
      const openIds = s.openIds.filter((x) => x !== id);
      const wasActive = s.view.kind === "env" && s.view.id === id;
      if (!wasActive) return { openIds };
      // Activate the neighbour, or go home when it was the last tab.
      const idx = s.openIds.indexOf(id);
      const next = openIds[Math.min(idx, openIds.length - 1)];
      return { openIds, view: next ? { kind: "env", id: next } : { kind: "home" } };
    }),
  toast: (tone, text) => {
    const id = nextToast++;
    set((s) => ({ toasts: [...s.toasts, { id, tone, text }] }));
    setTimeout(() => get().dismissToast(id), tone === "error" ? 8000 : 4000);
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));
