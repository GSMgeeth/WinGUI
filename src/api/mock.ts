// In-memory stand-in for the backend, used ONLY when the Vite dev server is
// opened in a plain browser (no Tauri shell). It exists so the UI can be
// built and looked at without an environment; it is dynamically imported
// behind `import.meta.env.DEV` and never ships.
//
// It deliberately contains one environment per class and one per failure mode
// so every state of the UI can be reached.

import type {
  AppError,
  Category,
  Connection,
  ConnectionInput,
  DriveInfo,
  EnvClass,
  EnvFeatures,
  Label,
} from "./types";

const NOTHING: EnvFeatures = {
  deployJar: false,
  restartJboss: false,
  logs: false,
  deployStatus: false,
  editProperties: false,
  fsBrowse: false,
  fsDownload: false,
  fsWrite: false,
  serviceView: false,
  serviceControl: false,
  processView: false,
  processKill: false,
};
const READ_ONLY: EnvFeatures = {
  ...NOTHING,
  logs: true,
  deployStatus: true,
  fsBrowse: true,
  fsDownload: true,
  serviceView: true,
  processView: true,
};
const ALL: EnvFeatures = Object.fromEntries(
  Object.keys(NOTHING).map((k) => [k, true]),
) as unknown as EnvFeatures;

// A rough copy of the backend's classification, for the mock only.
function classify(host: string): { envClass: EnvClass; label: string; features: EnvFeatures } {
  const short = host.trim().replace(/^\\+/, "").split(".")[0];
  const m = /-(CL|SD|EA|CP)(-\d+)?$/i.exec(short);
  if (!m) return { envClass: "unclassified", label: "unclassified", features: READ_ONLY };
  const kind = m[1].toUpperCase();
  const pub = !!m[2];
  if (kind === "CL") {
    return pub
      ? { envClass: "publicClient", label: "public client", features: NOTHING }
      : {
          envClass: "privateClient",
          label: "private client",
          features: { ...ALL, restartJboss: false, logs: false, deployStatus: false, editProperties: false },
        };
  }
  if (kind === "CP") {
    return pub
      ? { envClass: "publicCcp", label: "public CCP (Tomcat)", features: READ_ONLY }
      : { envClass: "privateCcp", label: "private CCP (Tomcat)", features: READ_ONLY };
  }
  return pub
    ? { envClass: "publicServer", label: "public server", features: READ_ONLY }
    : { envClass: "privateServer", label: "private server", features: ALL };
}

function view(c: Omit<Connection, "envClass" | "envClassLabel" | "features" | "side" | "readOnly" | "openable">): Connection {
  const k = classify(c.computerName);
  const mutating: (keyof EnvFeatures)[] = ["deployJar", "restartJboss", "editProperties", "fsWrite", "serviceControl", "processKill"];
  return {
    ...c,
    envClass: k.envClass,
    envClassLabel: k.label,
    features: k.features,
    side: k.envClass.endsWith("Client") ? "client" : k.envClass === "unclassified" ? "both" : "server",
    readOnly: !mutating.some((f) => k.features[f]),
    openable: Object.values(k.features).some(Boolean),
  };
}

let labels: Label[] = [
  { id: "l1", name: "Billing", color: "#2563eb" },
  { id: "l2", name: "Referral", color: "#16a34a" },
];
let categories: Category[] = [
  { id: "c1", name: "My environments" },
  { id: "c2", name: "Shared" },
];
const seed = (id: string, label: string, computerName: string, extra: Partial<Connection> = {}) => ({
  id,
  label,
  computerName,
  username: null,
  serviceName: null,
  serverRoot: null,
  clientRoot: null,
  pinned: false,
  labelId: null,
  categoryId: null,
  ...extra,
});
let connections = [
  seed("1", "Billing dev server", "L-abc123-SD", { pinned: true, labelId: "l1", categoryId: "c1", serviceName: "Production_Cosmic" }),
  seed("2", "Billing dev client", "L-abc123-CL", { labelId: "l1", categoryId: "c1" }),
  seed("3", "Referral dev server", "L-def456-SD", { labelId: "l2", categoryId: "c1" }),
  seed("4", "Team test (public)", "S-15172-EA-7", { categoryId: "c2" }),
  seed("5", "CCP", "L-W8JB9DUBG-CP", { categoryId: "c2" }),
  seed("6", "Shared client", "S-15172-CL-12", { categoryId: "c2" }),
  seed("7", "Old box (bad password)", "L-authfail-SD"),
  seed("8", "Offline box", "L-offline-SD"),
  seed("9", "Unknown naming", "BUILD-AGENT-04"),
].map(view);

let account = { username: "cambio.se\\dev.user" as string | null, hasPassword: true };

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));
const fail = (code: string, message: string): AppError => ({ code, message });
const GB = 1024 ** 3;

const drives: DriveInfo[] = [
  { name: "C:", driveType: "Fixed", ready: true, label: "OS", fileSystem: "NTFS", totalBytes: 127 * GB, freeBytes: 41 * GB },
  { name: "D:", driveType: "Fixed", ready: true, label: "Data", fileSystem: "NTFS", totalBytes: 256 * GB, freeBytes: 12 * GB },
  { name: "E:", driveType: "CDRom", ready: false, label: "", fileSystem: "", totalBytes: 0, freeBytes: 0 },
];

function find(id: string) {
  const c = connections.find((x) => x.id === id);
  if (!c) throw fail("NoConnection", `No connection with id ${id}`);
  return c;
}

function fromInput(id: string, input: ConnectionInput, pinned: boolean) {
  if (!input.computerName.trim()) throw fail("Error", "Computer name is required.");
  return view({ id, ...input, label: input.label.trim() || input.computerName.trim(), computerName: input.computerName.trim(), pinned });
}

export async function mockInvoke<T>(cmd: string, a: Record<string, any>): Promise<T> {
  const out = await handle(cmd, a);
  return structuredClone(out) as T;
}

async function handle(cmd: string, a: Record<string, any>): Promise<unknown> {
  switch (cmd) {
    case "list_connections":
      return connections;
    case "add_connection": {
      const c = fromInput(crypto.randomUUID(), a.input, false);
      connections = [...connections, c];
      return c;
    }
    case "update_connection": {
      const old = find(a.id);
      const c = fromInput(a.id, a.input, old.pinned);
      connections = connections.map((x) => (x.id === a.id ? c : x));
      return c;
    }
    case "delete_connection":
      connections = connections.filter((x) => x.id !== a.id);
      return null;
    case "set_connection_pinned": {
      const c = { ...find(a.id), pinned: a.pinned };
      connections = connections.map((x) => (x.id === a.id ? c : x));
      return c;
    }

    case "list_labels":
      return labels;
    case "add_label": {
      const l = { id: crypto.randomUUID(), ...a.input };
      labels = [...labels, l];
      return l;
    }
    case "update_label":
      labels = labels.map((l) => (l.id === a.id ? { ...l, ...a.input } : l));
      return labels.find((l) => l.id === a.id);
    case "delete_label":
      labels = labels.filter((l) => l.id !== a.id);
      connections = connections.map((c) => (c.labelId === a.id ? { ...c, labelId: null } : c));
      return null;
    case "reorder_labels":
      labels = (a.orderedIds as string[]).map((id) => labels.find((l) => l.id === id)!);
      return labels;

    case "list_categories":
      return categories;
    case "add_category": {
      const c = { id: crypto.randomUUID(), ...a.input };
      categories = [...categories, c];
      return c;
    }
    case "update_category":
      categories = categories.map((c) => (c.id === a.id ? { ...c, ...a.input } : c));
      return categories.find((c) => c.id === a.id);
    case "delete_category":
      categories = categories.filter((c) => c.id !== a.id);
      connections = connections.map((c) => (c.categoryId === a.id ? { ...c, categoryId: null } : c));
      return null;
    case "reorder_categories":
      categories = (a.orderedIds as string[]).map((id) => categories.find((c) => c.id === id)!);
      return categories;

    case "get_account":
      return account;
    case "set_account":
      account = { username: a.username || null, hasPassword: true };
      return null;
    case "clear_account":
      account = { username: null, hasPassword: false };
      return null;
    case "winrm_status":
      await delay(300);
      return { serviceRunning: true, trustedHosts: "L-abc123-SD,L-abc123-CL", configured: true, wildcard: false };
    case "setup_winrm":
      await delay(800);
      return "WinRM configured (service running, TrustedHosts = L-abc123-SD,L-abc123-CL).";
    case "import_preview":
      return {
        available: true,
        sourceDir: "C:\\Users\\dev\\AppData\\Roaming\\com.curiouserlabs.remote-desktop",
        connections: 14,
        labels: 3,
        categories: 2,
        modules: 26,
        propertySets: 4,
        hasAccount: true,
        accountUsername: "cambio.se\\dev.user",
      };
    case "import_remote_desktop":
      await delay(500);
      return {
        connectionsAdded: 14,
        connectionsSkipped: 0,
        labelsAdded: 3,
        categoriesAdded: 2,
        modulesAdded: 26,
        propertySetsAdded: 4,
        autoDeployCopied: true,
        accountImported: !!a.includeAccount,
      };

    case "session_open": {
      const c = find(a.connectionId);
      if (!c.openable) {
        throw fail("NotAllowed", `'Open environment' is not allowed on ${c.computerName} — it is a ${c.envClassLabel} environment. This kind of environment is not available here.`);
      }
      if (!account.hasPassword) {
        throw fail("NoAccount", "No account password set. Add the account username and password in Settings.");
      }
      await delay(900);
      if (c.computerName.includes("authfail")) {
        throw fail("AuthFailed", `Connecting to remote server ${c.computerName} failed with the following error message : Access is denied.\n\nAttempted as: cambio.se\\dev.user  (host: ${c.computerName})`);
      }
      if (c.computerName.includes("offline")) {
        throw fail("Unreachable", `WinRM cannot complete the operation. Verify that the specified computer name is valid, that the computer is accessible over the network.\n\nAttempted as: cambio.se\\dev.user  (host: ${c.computerName})`);
      }
      return {
        computerName: c.computerName.toUpperCase(),
        user: "CAMBIO\\dev.user",
        os: "Microsoft Windows Server 2022 Standard",
        psVersion: "5.1.20348.2849",
        utcNow: new Date().toISOString(),
      };
    }
    case "session_close":
      return null;
    case "session_status":
      return null;
    case "fs_drives":
      await delay(250);
      return drives;
  }
  throw fail("Error", `mock: unknown command ${cmd}`);
}
