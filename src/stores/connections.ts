import { create } from "zustand";
import * as api from "../api";
import type { AppError, Category, Connection, Label } from "../api";

interface ConnectionsState {
  connections: Connection[];
  labels: Label[];
  categories: Category[];
  loaded: boolean;
  error: AppError | null;
  /** Reload everything from disk (the backend is the source of truth). */
  refresh: () => Promise<void>;
}

export const useConnections = create<ConnectionsState>((set) => ({
  connections: [],
  labels: [],
  categories: [],
  loaded: false,
  error: null,
  refresh: async () => {
    try {
      const [connections, labels, categories] = await Promise.all([
        api.listConnections(),
        api.listLabels(),
        api.listCategories(),
      ]);
      set({ connections, labels, categories, loaded: true, error: null });
    } catch (e) {
      set({ loaded: true, error: api.toAppError(e) });
    }
  },
}));
