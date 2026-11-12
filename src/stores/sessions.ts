import { create } from "zustand";
import * as api from "../api";
import type { AppError, DriveInfo, SessionInfo } from "../api";

export type SessionState =
  | { status: "idle" }
  | { status: "connecting" }
  | { status: "connected"; info: SessionInfo; drives: DriveInfo[] | null; drivesError: AppError | null }
  | { status: "error"; error: AppError };

interface SessionsState {
  byId: Record<string, SessionState>;
  /** Connect (or reconnect) an environment — always a user-initiated attempt. */
  connect: (id: string) => Promise<void>;
  /** Re-read the drive list on the existing session. */
  loadDrives: (id: string) => Promise<void>;
  /** Close the session and forget its state. */
  disconnect: (id: string) => Promise<void>;
}

// Guards against a slow, superseded attempt overwriting a newer one's result.
const attempt: Record<string, number> = {};

export const useSessions = create<SessionsState>((set, get) => {
  const put = (id: string, state: SessionState) =>
    set((s) => ({ byId: { ...s.byId, [id]: state } }));

  return {
    byId: {},

    connect: async (id) => {
      // One attempt at a time: two overlapping attempts would send the
      // credential twice, and a wrong one must only ever be sent once.
      if (get().byId[id]?.status === "connecting") return;
      const mine = (attempt[id] = (attempt[id] ?? 0) + 1);
      put(id, { status: "connecting" });
      try {
        const info = await api.sessionOpen(id);
        if (attempt[id] !== mine) return;
        put(id, { status: "connected", info, drives: null, drivesError: null });
        await get().loadDrives(id);
      } catch (e) {
        if (attempt[id] !== mine) return;
        put(id, { status: "error", error: api.toAppError(e) });
      }
    },

    loadDrives: async (id) => {
      const mine = attempt[id];
      const patch = (p: { drives: DriveInfo[] | null; drivesError: AppError | null }) => {
        const cur = get().byId[id];
        if (attempt[id] === mine && cur?.status === "connected") put(id, { ...cur, ...p });
      };
      try {
        patch({ drives: await api.fsDrives(id), drivesError: null });
      } catch (e) {
        const error = api.toAppError(e);
        // A session that could not be re-established is a connection problem,
        // not a drive-listing one.
        if (["SessionLost", "AuthFailed", "Unreachable", "NotTrusted", "NoAccount"].includes(error.code)) {
          if (attempt[id] === mine) put(id, { status: "error", error });
        } else {
          patch({ drives: null, drivesError: error });
        }
      }
    },

    disconnect: async (id) => {
      attempt[id] = (attempt[id] ?? 0) + 1;
      set((s) => {
        const { [id]: _gone, ...rest } = s.byId;
        return { byId: rest };
      });
      try {
        await api.sessionClose(id);
      } catch {
        // Closing is best-effort; the backend also reaps idle sessions.
      }
    },
  };
});
