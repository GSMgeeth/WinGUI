//! Environment classification and the safety rules derived from it.
//!
//! This is the single source of truth for *"what is this host, and what may be
//! done to it"*. It lives in Rust and is enforced by the
//! [`super::feature::FeatureRegistry`] before any remote feature runs, so no
//! front end (GUI, CLI, agent skill) can bypass it. The frontend consumes the
//! computed [`EnvFeatures`] from the `list_connections` payload and merely
//! renders it.
//!
//! Ported from Remote Desktop's `core::env_class` (commit 76f65f0) — the
//! classification is unchanged — and extended with WinGUI's file, service and
//! process capabilities.
//!
//! Classes are derived from the standard computer-name suffix
//! (case-insensitive):
//!
//! * `-CL`        private client → deploy JARs only
//! * `-SD`        private server → all features
//! * `-CP`        private CCP (Tomcat) → **read-only** (logs + deploy status)
//! * `-CL-<num>`  public client  → nothing
//! * `-CP-<num>`  public CCP (Tomcat) → read-only
//! * `-EA-<num>` / `-SD-<num>` public server → logs + deployment status only
//!   (public environments must not be restarted or redeployed)
//!
//! CCP environments are read-only in **both** forms. A "private" one only means
//! it was self-provisioned (the `CCP=true` flag in Jenkins) rather than built by
//! IT; it is not a personal dev box, so it gets the same treatment as a public
//! server.
//!
//! A name that doesn't match the convention is **unclassified** and is limited
//! to read-only actions. This deliberately *fails closed*: deriving authority
//! from a user-editable name means an unrecognised spelling must not be a way
//! to obtain more permission than the environment it names would grant.

use serde::Serialize;

/// What kind of environment a computer name denotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EnvClass {
    PrivateClient,
    PrivateServer,
    /// Self-provisioned CCP (Tomcat) environment — read-only, see the module docs.
    PrivateCcp,
    PublicClient,
    PublicServer,
    /// IT-provisioned CCP (Tomcat) environment — read-only.
    PublicCcp,
    /// Doesn't match the naming convention — everything stays available.
    Unclassified,
}

impl EnvClass {
    /// Short human description used in guard errors.
    pub fn describe(self) -> &'static str {
        match self {
            EnvClass::PrivateClient => "private client",
            EnvClass::PrivateServer => "private server",
            EnvClass::PrivateCcp => "private CCP (Tomcat)",
            EnvClass::PublicClient => "public client",
            EnvClass::PublicServer => "public server",
            EnvClass::PublicCcp => "public CCP (Tomcat)",
            EnvClass::Unclassified => "unclassified",
        }
    }
}

/// Which host side a connection represents — used by auto-deploy to pre-select
/// the matching jars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    Server,
    Client,
    Both,
}

/// Classify a computer name by its suffix. Mirrors the retired frontend rule
/// `/-(CL|SD|EA)(?:-\d+)?$/i` exactly (verified by the truth-table test).
pub fn env_class(computer_name: &str) -> EnvClass {
    let segs: Vec<&str> = short_name(computer_name).split('-').collect();
    if segs.len() < 2 {
        return EnvClass::Unclassified;
    }
    let last = segs[segs.len() - 1];
    let is_public = !last.is_empty() && last.chars().all(|c| c.is_ascii_digit());
    let token = if is_public {
        if segs.len() < 3 {
            return EnvClass::Unclassified;
        }
        segs[segs.len() - 2]
    } else {
        last
    };
    match token.to_ascii_uppercase().as_str() {
        "CL" => {
            if is_public {
                EnvClass::PublicClient
            } else {
                EnvClass::PrivateClient
            }
        }
        // Both CCP forms are read-only, but they are kept distinct so the UI and
        // error messages can still say which one it is.
        "CP" => {
            if is_public {
                EnvClass::PublicCcp
            } else {
                EnvClass::PrivateCcp
            }
        }
        // SD and EA are both servers; EA normally only appears in public form.
        "SD" | "EA" => {
            if is_public {
                EnvClass::PublicServer
            } else {
                EnvClass::PrivateServer
            }
        }
        _ => EnvClass::Unclassified,
    }
}

/// Reduce a saved host to the short name the convention applies to.
///
/// Authorization is derived from the name's suffix, so any spelling that hides
/// that suffix from the parser silently grants **more** permission than the
/// environment should have. An FQDN does exactly that: splitting
/// `L-abc123-EA-7.corp.example` on `-` yields a final segment of
/// `7.corp.example`, which matches nothing — so a *public* server used to
/// classify as unclassified. Trailing dots (the absolute-form FQDN) and a UNC
/// prefix have the same effect.
///
/// Normalising first means the same machine classifies identically however it
/// was typed. An IP address reduces to its first octet, which matches no
/// convention and stays unclassified — correct, since an address carries no
/// environment identity.
fn short_name(computer_name: &str) -> &str {
    computer_name
        .trim()
        .trim_start_matches('\\')
        .trim_end_matches('.')
        .split('.')
        .next()
        .unwrap_or("")
}

/// One remote capability a feature can require. Every registered feature
/// declares exactly one; the registry checks it against the target's
/// [`EnvFeatures`] before running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    // --- Ported from Remote Desktop ---
    DeployJar,
    RestartJboss,
    Logs,
    DeployStatus,
    EditProperties,
    // --- WinGUI ---
    /// List drives and folders, stat, search, preview, hash. Also what merely
    /// opening a session to an environment requires.
    FsBrowse,
    /// Copy files from the environment to the laptop.
    FsDownload,
    /// Upload, create, rename, move, copy, delete, zip/unzip, edit.
    FsWrite,
    /// List Windows services and their details.
    ServiceView,
    /// Start / stop / restart a service.
    ServiceControl,
    /// List processes.
    ProcessView,
    /// End a process.
    ProcessKill,
}

impl Capability {
    /// Every capability, for exhaustive table tests and listings.
    pub const ALL: [Capability; 12] = [
        Capability::DeployJar,
        Capability::RestartJboss,
        Capability::Logs,
        Capability::DeployStatus,
        Capability::EditProperties,
        Capability::FsBrowse,
        Capability::FsDownload,
        Capability::FsWrite,
        Capability::ServiceView,
        Capability::ServiceControl,
        Capability::ProcessView,
        Capability::ProcessKill,
    ];

    /// Whether a capability mutates the remote host. Mutating capabilities are
    /// journaled, and gated by `--yes` on the CLI for non-private-dev targets.
    pub fn is_mutating(self) -> bool {
        matches!(
            self,
            Capability::DeployJar
                | Capability::RestartJboss
                | Capability::EditProperties
                | Capability::FsWrite
                | Capability::ServiceControl
                | Capability::ProcessKill
        )
    }
}

/// The set of features available on an environment class. Serialized into the
/// `list_connections` payload for the frontend to render.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvFeatures {
    pub deploy_jar: bool,
    pub restart_jboss: bool,
    pub logs: bool,
    /// Module-deployment monitoring — a read-only check, available on any server.
    pub deploy_status: bool,
    /// Appending property sets to environment.properties — a private-server edit.
    pub edit_properties: bool,
    pub fs_browse: bool,
    pub fs_download: bool,
    pub fs_write: bool,
    pub service_view: bool,
    pub service_control: bool,
    pub process_view: bool,
    pub process_kill: bool,
}

impl EnvFeatures {
    pub fn allows(&self, cap: Capability) -> bool {
        match cap {
            Capability::DeployJar => self.deploy_jar,
            Capability::RestartJboss => self.restart_jboss,
            Capability::Logs => self.logs,
            Capability::DeployStatus => self.deploy_status,
            Capability::EditProperties => self.edit_properties,
            Capability::FsBrowse => self.fs_browse,
            Capability::FsDownload => self.fs_download,
            Capability::FsWrite => self.fs_write,
            Capability::ServiceView => self.service_view,
            Capability::ServiceControl => self.service_control,
            Capability::ProcessView => self.process_view,
            Capability::ProcessKill => self.process_kill,
        }
    }

    /// True when nothing that changes the host is permitted — what the UI
    /// labels "read-only".
    pub fn is_read_only(&self) -> bool {
        !Capability::ALL
            .iter()
            .any(|c| c.is_mutating() && self.allows(*c))
    }

    /// True when the environment cannot even be opened.
    pub fn is_empty(&self) -> bool {
        !Capability::ALL.iter().any(|c| self.allows(*c))
    }
}

/// The features an environment class permits — the safety rule the registry
/// enforces centrally. The first five columns are Remote Desktop's table,
/// unchanged; the rest extend the same principle to WinGUI's capabilities
/// (docs/03-architecture.md §3.5): **only private environments may be
/// changed; everything else is observe-only; an unrecognised name fails
/// closed.**
pub fn features(class: EnvClass) -> EnvFeatures {
    const NOTHING: EnvFeatures = EnvFeatures {
        deploy_jar: false,
        restart_jboss: false,
        logs: false,
        deploy_status: false,
        edit_properties: false,
        fs_browse: false,
        fs_download: false,
        fs_write: false,
        service_view: false,
        service_control: false,
        process_view: false,
        process_kill: false,
    };
    // Observe-only: look, list, copy out — never change.
    const READ_ONLY: EnvFeatures = EnvFeatures {
        logs: true,
        deploy_status: true,
        fs_browse: true,
        fs_download: true,
        service_view: true,
        process_view: true,
        ..NOTHING
    };
    match class {
        EnvClass::PrivateServer => EnvFeatures {
            deploy_jar: true,
            restart_jboss: true,
            logs: true,
            deploy_status: true,
            edit_properties: true,
            fs_browse: true,
            fs_download: true,
            fs_write: true,
            service_view: true,
            service_control: true,
            process_view: true,
            process_kill: true,
        },
        // A personal client box: files, services and processes are the
        // owner's to change, but it runs no JBoss — so no restart, logs,
        // deployment status or property edits (Remote Desktop's rule).
        EnvClass::PrivateClient => EnvFeatures {
            deploy_jar: true,
            fs_browse: true,
            fs_download: true,
            fs_write: true,
            service_view: true,
            service_control: true,
            process_view: true,
            process_kill: true,
            ..NOTHING
        },
        // Fails closed: an unrecognised name must not out-rank the environment
        // it actually refers to.
        EnvClass::Unclassified => READ_ONLY,
        // CCP (Tomcat) — read-only whether self-provisioned or IT-provisioned.
        EnvClass::PrivateCcp | EnvClass::PublicCcp => READ_ONLY,
        EnvClass::PublicServer => READ_ONLY,
        // Shared client machines stay fully off-limits, as in Remote Desktop
        // (open question Q3 in the plan — relaxing this is a one-line change
        // here, guarded by the truth-table test below).
        EnvClass::PublicClient => NOTHING,
    }
}

/// Which side's jars auto-deploy should pre-select for this environment.
pub fn side(class: EnvClass) -> Side {
    match class {
        EnvClass::PrivateClient | EnvClass::PublicClient => Side::Client,
        EnvClass::PrivateServer | EnvClass::PublicServer => Side::Server,
        // Server-side app servers; only cosmetic, since CCP cannot be deployed to.
        EnvClass::PrivateCcp | EnvClass::PublicCcp => Side::Server,
        EnvClass::Unclassified => Side::Both,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shared truth table: computer name → allowed operations. This mirrors
    /// the retired TypeScript `envFeatures` exactly; if this table changes, the
    /// UI, CLI, and agent skills all change with it.
    #[test]
    fn capability_truth_table() {
        // (name, deploy, restart, logs, deploy_status, edit_properties)
        let table = [
            ("L-abc123-SD", true, true, true, true, true),
            ("l-abc123-sd", true, true, true, true, true), // case-insensitive
            ("L-abc123-CL", true, false, false, false, false),
            ("L-abc123-CL-12", false, false, false, false, false),
            ("L-abc123-EA-7", false, false, true, true, false),
            ("L-abc123-SD-3", false, false, true, true, false),
            ("L-abc123-EA", true, true, true, true, true), // private EA = server
            // CCP (Tomcat) is read-only in BOTH forms — a "private" one is only
            // self-provisioned, not a personal dev box.
            ("L-W8JB9DUBG-CP", false, false, true, true, false),
            ("S-15172-CP-01", false, false, true, true, false),
            // However the host was typed, it classifies as its short name does.
            ("L-abc123-SD.corp.example.se", true, true, true, true, true),
            ("L-abc123-EA-7.corp.example.se", false, false, true, true, false),
            ("L-abc123-EA-7.", false, false, true, true, false),
            (r"\\L-abc123-EA-7", false, false, true, true, false),
            // Unclassified now fails closed: read-only, never mutating.
            ("SOMEHOST", false, false, true, true, false),
            ("L-abc123", false, false, true, true, false),
            ("L-abc123-XX-9", false, false, true, true, false),
            ("10.20.30.40", false, false, true, true, false),
            ("  L-abc123-SD  ", true, true, true, true, true), // trimmed
        ];
        for (name, deploy, restart, logs, status, props) in table {
            let f = features(env_class(name));
            assert_eq!(f.deploy_jar, deploy, "deploy_jar for {name}");
            assert_eq!(f.restart_jboss, restart, "restart_jboss for {name}");
            assert_eq!(f.logs, logs, "logs for {name}");
            assert_eq!(f.deploy_status, status, "deploy_status for {name}");
            assert_eq!(f.edit_properties, props, "edit_properties for {name}");
        }
    }

    /// WinGUI's capabilities, per class. Columns:
    /// browse, download, write, svc view, svc control, proc view, proc kill.
    #[test]
    fn wingui_capability_truth_table() {
        let table = [
            // Private server: everything.
            ("L-abc123-SD", [true, true, true, true, true, true, true]),
            ("L-abc123-EA", [true, true, true, true, true, true, true]),
            // Private client: the owner's box — everything file/service/process.
            ("L-abc123-CL", [true, true, true, true, true, true, true]),
            // Public server: observe and copy out, never change.
            ("L-abc123-EA-7", [true, true, false, true, false, true, false]),
            ("L-abc123-SD-3", [true, true, false, true, false, true, false]),
            // CCP, both forms: observe-only.
            ("L-W8JB9DUBG-CP", [true, true, false, true, false, true, false]),
            ("S-15172-CP-01", [true, true, false, true, false, true, false]),
            // Public client: nothing at all.
            ("L-abc123-CL-12", [false; 7]),
            // Unclassified fails closed to observe-only.
            ("SOMEHOST", [true, true, false, true, false, true, false]),
            ("10.20.30.40", [true, true, false, true, false, true, false]),
            // Spelling cannot buy permission.
            ("L-abc123-EA-7.corp.example.se", [true, true, false, true, false, true, false]),
            (r"\\L-abc123-CL-12", [false; 7]),
        ];
        let caps = [
            Capability::FsBrowse,
            Capability::FsDownload,
            Capability::FsWrite,
            Capability::ServiceView,
            Capability::ServiceControl,
            Capability::ProcessView,
            Capability::ProcessKill,
        ];
        for (name, expected) in table {
            let f = features(env_class(name));
            for (cap, want) in caps.iter().zip(expected) {
                assert_eq!(f.allows(*cap), want, "{cap:?} for {name}");
            }
        }
    }

    /// The invariant behind the whole table, checked for every class rather
    /// than by example: a mutating capability exists only on a private
    /// server or private client.
    #[test]
    fn only_private_environments_can_be_changed() {
        for class in [
            EnvClass::PrivateClient,
            EnvClass::PrivateServer,
            EnvClass::PrivateCcp,
            EnvClass::PublicClient,
            EnvClass::PublicServer,
            EnvClass::PublicCcp,
            EnvClass::Unclassified,
        ] {
            let f = features(class);
            let private_dev = matches!(class, EnvClass::PrivateClient | EnvClass::PrivateServer);
            assert_eq!(f.is_read_only(), !private_dev, "read-only-ness of {class:?}");
            for cap in Capability::ALL {
                if cap.is_mutating() && !private_dev {
                    assert!(!f.allows(cap), "{cap:?} must be refused on {class:?}");
                }
            }
        }
        assert!(features(EnvClass::PublicClient).is_empty());
        assert!(!features(EnvClass::PublicServer).is_empty());
    }

    #[test]
    fn capability_all_lists_every_variant_once() {
        let mut seen = std::collections::HashSet::new();
        for cap in Capability::ALL {
            assert!(seen.insert(format!("{cap:?}")), "{cap:?} listed twice");
        }
        assert_eq!(seen.len(), 12);
    }

    #[test]
    fn classification() {
        assert_eq!(env_class("L-a-SD"), EnvClass::PrivateServer);
        assert_eq!(env_class("L-a-CL"), EnvClass::PrivateClient);
        assert_eq!(env_class("L-a-CL-1"), EnvClass::PublicClient);
        assert_eq!(env_class("L-a-EA-42"), EnvClass::PublicServer);
        assert_eq!(env_class("L-a-12"), EnvClass::Unclassified);
        assert_eq!(env_class("SD"), EnvClass::Unclassified);
        assert_eq!(env_class("-SD"), EnvClass::PrivateServer);
        assert_eq!(env_class("L-a-SD-"), EnvClass::Unclassified);
        // CCP, in the real-world shapes.
        assert_eq!(env_class("L-W8JB9DUBG-CP"), EnvClass::PrivateCcp);
        assert_eq!(env_class("S-15172-CP-01"), EnvClass::PublicCcp);
        assert_eq!(env_class("l-w8jb9dubg-cp"), EnvClass::PrivateCcp);
    }

    /// The bypass this closes: before normalisation, a *public* server saved by
    /// its FQDN fell through to `Unclassified`, which used to grant everything.
    #[test]
    fn host_spelling_cannot_change_the_class() {
        for name in [
            "L-a-EA-7",
            "L-a-EA-7.corp.example.se",
            "L-a-EA-7.",
            "L-A-EA-7.CORP.EXAMPLE.SE",
            r"\\L-a-EA-7",
            "  L-a-EA-7.corp.example.se  ",
        ] {
            assert_eq!(env_class(name), EnvClass::PublicServer, "for {name:?}");
        }
        assert_eq!(env_class("L-a-SD.corp.example.se"), EnvClass::PrivateServer);
        // An address carries no environment identity — stays unclassified.
        assert_eq!(env_class("10.20.30.40"), EnvClass::Unclassified);
        assert_eq!(env_class(""), EnvClass::Unclassified);
    }

    #[test]
    fn sides() {
        assert_eq!(side(env_class("L-a-CL")), Side::Client);
        assert_eq!(side(env_class("L-a-SD")), Side::Server);
        assert_eq!(side(env_class("L-a-EA-1")), Side::Server);
        assert_eq!(side(env_class("L-a-CP")), Side::Server);
        assert_eq!(side(env_class("whatever")), Side::Both);
    }
}
