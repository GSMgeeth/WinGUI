import { invoke, isTauri } from "@tauri-apps/api/core";
import type { AppError } from "./types";

/** Normalise whatever a command rejected with into an `AppError`. */
export function toAppError(e: unknown): AppError {
  if (e && typeof e === "object" && "message" in e) {
    const o = e as { code?: unknown; message?: unknown };
    return {
      code: typeof o.code === "string" ? o.code : "Error",
      message: String(o.message),
    };
  }
  return { code: "Error", message: typeof e === "string" ? e : String(e) };
}

/**
 * Call a backend command. Always rejects with an `AppError`.
 *
 * Outside the desktop shell (the Vite dev server opened in a plain browser)
 * the call is served by an in-memory mock so the UI can be developed and
 * checked without a backend. The mock is only reachable in dev builds and is
 * not part of the production bundle.
 */
export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    if (isTauri()) {
      return await invoke<T>(cmd, args);
    }
    if (import.meta.env.DEV) {
      const { mockInvoke } = await import("./mock");
      return await mockInvoke<T>(cmd, args ?? {});
    }
    throw { code: "Error", message: "WinGUI must run inside its desktop window." };
  } catch (e) {
    throw toAppError(e);
  }
}
