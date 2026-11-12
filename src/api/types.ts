// Shapes exchanged with the Rust core. Field names are the camelCase the
// backend serialises; nothing here is derived in TypeScript — in particular the
// environment class and what it allows always come from the backend.

export type EnvClass =
  | "privateClient"
  | "privateServer"
  | "privateCcp"
  | "publicClient"
  | "publicServer"
  | "publicCcp"
  | "unclassified";

export interface EnvFeatures {
  deployJar: boolean;
  restartJboss: boolean;
  logs: boolean;
  deployStatus: boolean;
  editProperties: boolean;
  fsBrowse: boolean;
  fsDownload: boolean;
  fsWrite: boolean;
  serviceView: boolean;
  serviceControl: boolean;
  processView: boolean;
  processKill: boolean;
}

export interface Connection {
  id: string;
  label: string;
  computerName: string;
  username: string | null;
  serviceName: string | null;
  serverRoot: string | null;
  clientRoot: string | null;
  pinned: boolean;
  labelId: string | null;
  categoryId: string | null;
  envClass: EnvClass;
  envClassLabel: string;
  features: EnvFeatures;
  side: "server" | "client" | "both";
  readOnly: boolean;
  openable: boolean;
}

export interface ConnectionInput {
  label: string;
  computerName: string;
  username: string | null;
  serviceName: string | null;
  serverRoot: string | null;
  clientRoot: string | null;
  labelId: string | null;
  categoryId: string | null;
}

export interface Label {
  id: string;
  name: string;
  color: string;
}

export interface Category {
  id: string;
  name: string;
}

export interface AccountInfo {
  username: string | null;
  hasPassword: boolean;
}

export interface WinrmStatus {
  serviceRunning: boolean;
  trustedHosts: string;
  configured: boolean;
  wildcard: boolean;
}

export interface ImportPreview {
  available: boolean;
  sourceDir: string | null;
  connections: number;
  labels: number;
  categories: number;
  modules: number;
  propertySets: number;
  hasAccount: boolean;
  accountUsername: string | null;
}

export interface ImportSummary {
  connectionsAdded: number;
  connectionsSkipped: number;
  labelsAdded: number;
  categoriesAdded: number;
  modulesAdded: number;
  propertySetsAdded: number;
  autoDeployCopied: boolean;
  accountImported: boolean;
}

export interface SessionInfo {
  computerName: string;
  user: string;
  os: string;
  psVersion: string;
  utcNow: string;
}

export interface DriveInfo {
  name: string;
  driveType: string;
  ready: boolean;
  label: string;
  fileSystem: string;
  totalBytes: number;
  freeBytes: number;
}

/** Every backend failure: a stable code to branch on and a message to show. */
export interface AppError {
  code: string;
  message: string;
}
