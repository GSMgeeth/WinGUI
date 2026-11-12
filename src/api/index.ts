// Typed wrappers over the backend commands — the only place command names live.

import { call } from "./invoke";
import type {
  AccountInfo,
  Category,
  Connection,
  ConnectionInput,
  DriveInfo,
  ImportPreview,
  ImportSummary,
  Label,
  SessionInfo,
  WinrmStatus,
} from "./types";

export * from "./types";
export { toAppError } from "./invoke";

// --- Environments ---
export const listConnections = () => call<Connection[]>("list_connections");
export const addConnection = (input: ConnectionInput) =>
  call<Connection>("add_connection", { input });
export const updateConnection = (id: string, input: ConnectionInput) =>
  call<Connection>("update_connection", { id, input });
export const deleteConnection = (id: string) => call<void>("delete_connection", { id });
export const setConnectionPinned = (id: string, pinned: boolean) =>
  call<Connection>("set_connection_pinned", { id, pinned });

// --- Labels ---
export const listLabels = () => call<Label[]>("list_labels");
export const addLabel = (input: { name: string; color: string }) =>
  call<Label>("add_label", { input });
export const updateLabel = (id: string, input: { name: string; color: string }) =>
  call<Label>("update_label", { id, input });
export const deleteLabel = (id: string) => call<void>("delete_label", { id });
export const reorderLabels = (orderedIds: string[]) =>
  call<Label[]>("reorder_labels", { orderedIds });

// --- Categories ---
export const listCategories = () => call<Category[]>("list_categories");
export const addCategory = (input: { name: string }) => call<Category>("add_category", { input });
export const updateCategory = (id: string, input: { name: string }) =>
  call<Category>("update_category", { id, input });
export const deleteCategory = (id: string) => call<void>("delete_category", { id });
export const reorderCategories = (orderedIds: string[]) =>
  call<Category[]>("reorder_categories", { orderedIds });

// --- Settings ---
export const getAccount = () => call<AccountInfo>("get_account");
export const setAccount = (username: string, password: string | null) =>
  call<void>("set_account", { username, password });
export const clearAccount = () => call<void>("clear_account");
export const winrmStatus = () => call<WinrmStatus>("winrm_status");
export const setupWinrm = () => call<string>("setup_winrm");
export const importPreview = () => call<ImportPreview>("import_preview");
export const importRemoteDesktop = (includeAccount: boolean) =>
  call<ImportSummary>("import_remote_desktop", { includeAccount });

// --- Sessions ---
export const sessionOpen = (connectionId: string) =>
  call<SessionInfo>("session_open", { connectionId });
export const sessionClose = (connectionId: string) => call<void>("session_close", { connectionId });
export const sessionStatus = (connectionId: string) =>
  call<SessionInfo | null>("session_status", { connectionId });
export const fsDrives = (connectionId: string) => call<DriveInfo[]>("fs_drives", { connectionId });
