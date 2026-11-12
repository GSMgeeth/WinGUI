//! The feature-plugin contract: every remote-capable feature is one uniform,
//! nameable, capability-checked unit that any front end (GUI, CLI, AI-agent
//! skill) invokes identically through the [`FeatureRegistry`].
//!
//! The registry — not the caller — owns the steps every invocation must go
//! through, in order:
//!
//! 1. load the saved connections and resolve the target + credentials
//!    ([`super::remoting::resolve_target`]);
//! 2. enforce the environment-class safety rule ([`super::env_class`]) — a
//!    disallowed operation (e.g. deleting a file on a public server) fails
//!    here with the same error on every front end and cannot be bypassed;
//! 3. resolve the environment's path layout ([`super::env_layout::EnvLayout`]);
//! 4. hand the assembled [`FeatureCtx`] to the feature.
//!
//! Features return **structured results** (`serde_json::Value` of a typed
//! struct) — the UI or an agent formats them; Rust never bakes English
//! sentences into a success payload.
//!
//! Ported from Remote Desktop's `core::feature` (commit 76f65f0). Differences:
//! errors are [`AppError`] (code + message) instead of `String`; the context
//! carries the [`SessionPool`] so features can use the environment's held
//! session; and the connection-less `LocalFeature` kind is gone with the
//! workspace features that needed it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use super::env_class::{env_class, features as class_features, Capability, EnvClass};
use super::env_layout::EnvLayout;
use super::error::{codes, AppError, AppResult};
use super::model::{load_connections, Connection};
use super::remoting::pool::{PoolConfig, Retry, SessionPool};
use super::remoting::worker::{PsWorkerFactory, SessionInfo};
use super::remoting::{resolve_target, PsRunner, RemoteTarget, SessionRegistry};

/// Progress callback for long-running one-shot features (e.g. restart phases).
/// Front ends adapt it: the GUI forwards to a Tauri channel, the CLI prints.
pub type Progress<'a> = &'a (dyn Fn(&str) + Send + Sync);

/// Event sink for streaming features. Receives one JSON message per event
/// (each feature documents its schema); returns `false` when the receiver is
/// gone so the pump can stop.
pub type EventSink = Box<dyn Fn(Value) -> bool + Send + 'static>;

/// Everything a feature invocation may need, assembled by the registry.
pub struct FeatureCtx {
    pub connection: Connection,
    pub target: RemoteTarget,
    pub layout: EnvLayout,
    /// The app config dir, for features that read their own stores.
    pub config_dir: PathBuf,
    /// One-shot transport (a `powershell.exe` per call) — the ported features.
    pub runner: Arc<dyn PsRunner>,
    /// Streaming child processes (log tail, deployment watch).
    pub sessions: Arc<SessionRegistry>,
    /// Held sessions — what interactive features use.
    pub pool: Arc<SessionPool>,
}

impl FeatureCtx {
    /// The pool key for this environment's interactive session.
    pub fn session_key(&self) -> &str {
        &self.connection.id
    }

    /// Open (or reuse) the environment's held session.
    pub fn open_session(&self) -> AppResult<SessionInfo> {
        self.pool.open(self.session_key(), &self.target)
    }

    /// Run one worker operation on the environment's held session.
    pub fn request(&self, op: &str, args: Value, timeout: Duration, retry: Retry) -> AppResult<Value> {
        self.pool
            .request(self.session_key(), &self.target, op, args, timeout, retry)
    }
}

/// A one-shot remote feature (list drives, deploy, restart, …).
pub trait RemoteFeature: Send + Sync {
    /// Stable machine id, e.g. `"fs.drives"` — the registry key and CLI name.
    fn id(&self) -> &'static str;
    /// Human name used in guard errors and CLI listings.
    fn name(&self) -> &'static str;
    /// The capability this feature requires on the target environment.
    fn capability(&self) -> Capability;
    /// Run to completion. `args` is the feature's documented JSON argument
    /// object; the result is the feature's documented JSON result object.
    fn run(&self, ctx: &FeatureCtx, args: Value, progress: Progress) -> AppResult<Value>;
}

/// A streaming remote feature (log tail, deploy watch). Same identity shape,
/// but starts a background stream and returns a session id to stop it with.
pub trait StreamingRemoteFeature: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn capability(&self) -> Capability;
    /// Start streaming events into `sink`; returns the session id for
    /// [`FeatureRegistry::stop`].
    fn stream(&self, ctx: &FeatureCtx, args: Value, sink: EventSink) -> AppResult<String>;
}

/// Shared runtime dependencies a front end constructs once: the transports
/// and the streaming-session registry.
#[derive(Clone)]
pub struct FeatureDeps {
    pub runner: Arc<dyn PsRunner>,
    pub sessions: Arc<SessionRegistry>,
    pub pool: Arc<SessionPool>,
}

impl FeatureDeps {
    /// The production dependencies (real `powershell.exe` transports).
    pub fn real() -> Self {
        FeatureDeps {
            runner: Arc::new(super::remoting::RealPsRunner),
            sessions: Arc::new(SessionRegistry::default()),
            pool: Arc::new(SessionPool::new(
                Arc::new(PsWorkerFactory),
                PoolConfig::default(),
            )),
        }
    }
}

/// Basic metadata about a registered feature, for CLI/agent listings.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub streaming: bool,
    pub mutating: bool,
}

/// All registered features, keyed by id. Built once at startup by
/// [`super::features::build_registry`]; every front end dispatches through it.
#[derive(Default)]
pub struct FeatureRegistry {
    run_features: HashMap<&'static str, Box<dyn RemoteFeature>>,
    stream_features: HashMap<&'static str, Box<dyn StreamingRemoteFeature>>,
}

impl FeatureRegistry {
    pub fn register(&mut self, feature: Box<dyn RemoteFeature>) {
        self.run_features.insert(feature.id(), feature);
    }

    pub fn register_streaming(&mut self, feature: Box<dyn StreamingRemoteFeature>) {
        self.stream_features.insert(feature.id(), feature);
    }

    /// Metadata for every registered feature (one-shot and streaming).
    pub fn list(&self) -> Vec<FeatureInfo> {
        let mut infos: Vec<FeatureInfo> = self
            .run_features
            .values()
            .map(|f| FeatureInfo {
                id: f.id(),
                name: f.name(),
                streaming: false,
                mutating: f.capability().is_mutating(),
            })
            .chain(self.stream_features.values().map(|f| FeatureInfo {
                id: f.id(),
                name: f.name(),
                streaming: true,
                mutating: f.capability().is_mutating(),
            }))
            .collect();
        infos.sort_by_key(|i| i.id);
        infos
    }

    /// The capability a feature id requires, if registered (used by the CLI's
    /// confirmation gate).
    pub fn capability_of(&self, id: &str) -> Option<Capability> {
        self.run_features
            .get(id)
            .map(|f| f.capability())
            .or_else(|| self.stream_features.get(id).map(|f| f.capability()))
    }

    /// Run a one-shot feature against a connection. This is the single front
    /// door: resolution, the safety guard, and layout all happen here.
    pub fn run(
        &self,
        deps: &FeatureDeps,
        feature_id: &str,
        connection_id: &str,
        args: Value,
        progress: Progress,
    ) -> AppResult<Value> {
        let feature = self.run_features.get(feature_id).ok_or_else(|| {
            AppError::new(codes::UNKNOWN_FEATURE, format!("No feature with id {feature_id}"))
        })?;
        let ctx = self.ctx_for(deps, connection_id, feature.capability(), feature.name())?;
        feature.run(&ctx, args, progress)
    }

    /// Start a streaming feature against a connection; same front door.
    pub fn stream(
        &self,
        deps: &FeatureDeps,
        feature_id: &str,
        connection_id: &str,
        args: Value,
        sink: EventSink,
    ) -> AppResult<String> {
        let feature = self.stream_features.get(feature_id).ok_or_else(|| {
            AppError::new(codes::UNKNOWN_FEATURE, format!("No feature with id {feature_id}"))
        })?;
        let ctx = self.ctx_for(deps, connection_id, feature.capability(), feature.name())?;
        feature.stream(&ctx, args, sink)
    }

    /// Stop a streaming session started by [`Self::stream`].
    pub fn stop(&self, deps: &FeatureDeps, session_id: &str) {
        deps.sessions.stop(session_id);
    }

    /// Assemble the invocation context: find, guard, resolve, layout.
    ///
    /// The guard runs **before** credentials are read: whether an action is
    /// allowed depends only on the environment's name, and neither a refusal
    /// nor an unknown id should first ask the user to configure an account.
    fn ctx_for(
        &self,
        deps: &FeatureDeps,
        connection_id: &str,
        capability: Capability,
        feature_name: &str,
    ) -> AppResult<FeatureCtx> {
        let connections = load_connections();
        precheck(&connections, connection_id, capability, feature_name)?;
        let (conn, target) = resolve_target(&connections, connection_id)?;
        Ok(FeatureCtx {
            layout: EnvLayout::for_connection(conn),
            connection: conn.clone(),
            target,
            config_dir: super::storage::config_dir()?,
            runner: deps.runner.clone(),
            sessions: deps.sessions.clone(),
            pool: deps.pool.clone(),
        })
    }
}

/// The checks that need no credential: the connection exists, and its
/// environment class allows the capability.
fn precheck(
    connections: &[Connection],
    connection_id: &str,
    capability: Capability,
    feature_name: &str,
) -> AppResult<EnvClass> {
    let conn = connections
        .iter()
        .find(|c| c.id == connection_id)
        .ok_or_else(|| {
            AppError::new(
                codes::NO_CONNECTION,
                format!("No connection with id {connection_id}"),
            )
        })?;
    guard(&conn.computer_name, capability, feature_name)
}

/// The backend-enforced environment-class safety rule: returns an error when
/// `capability` is not allowed on `computer_name`.
pub fn guard(
    computer_name: &str,
    capability: Capability,
    feature_name: &str,
) -> AppResult<EnvClass> {
    let class = env_class(computer_name);
    if !class_features(class).allows(capability) {
        // Unclassified is the case worth explaining: the user sees a refusal on a
        // host they may believe is a normal dev box, and the fix is a naming one.
        let why = if class == EnvClass::Unclassified {
            " — its name does not match the environment convention \
             (…-SD, -CL, -CP, -EA), so only read-only actions are allowed"
        } else {
            ""
        };
        let rule = if capability.is_mutating() {
            "Changes are limited to private client and server environments."
        } else {
            "This kind of environment is not available here."
        };
        return Err(AppError::new(
            codes::NOT_ALLOWED,
            format!(
                "'{feature_name}' is not allowed on {computer_name} — it is a {} environment{why}. {rule}",
                class.describe()
            ),
        ));
    }
    Ok(class)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_blocks_mutations_on_public_environments() {
        assert!(guard("L-x-EA-7", Capability::RestartJboss, "Restart JBoss").is_err());
        assert!(guard("L-x-EA-7", Capability::DeployJar, "Deploy JAR").is_err());
        assert!(guard("L-x-CL-2", Capability::DeployJar, "Deploy JAR").is_err());
        assert!(guard("L-x-EA-7", Capability::Logs, "Logs").is_ok());
        assert!(guard("L-x-EA-7", Capability::DeployStatus, "Deployment status").is_ok());
        assert!(guard("L-x-SD", Capability::RestartJboss, "Restart JBoss").is_ok());
        assert!(guard("L-x-CL", Capability::DeployJar, "Deploy JAR").is_ok());
        assert!(guard("L-x-CL", Capability::EditProperties, "Add properties").is_err());

        // Unclassified fails closed: read-only only.
        assert!(guard("UNKNOWN-HOST", Capability::RestartJboss, "Restart JBoss").is_err());
        assert!(guard("UNKNOWN-HOST", Capability::DeployJar, "Deploy JAR").is_err());
        assert!(guard("UNKNOWN-HOST", Capability::Logs, "Logs").is_ok());

        // CCP (Tomcat) is read-only however it was provisioned.
        assert!(guard("L-W8JB9DUBG-CP", Capability::DeployJar, "Deploy JAR").is_err());
        assert!(guard("L-W8JB9DUBG-CP", Capability::RestartJboss, "Restart JBoss").is_err());
        assert!(guard("S-15172-CP-01", Capability::DeployJar, "Deploy JAR").is_err());
        assert!(guard("L-W8JB9DUBG-CP", Capability::Logs, "Logs").is_ok());

        // The FQDN bypass: a public server must stay blocked however it is typed.
        assert!(guard("L-x-EA-7.corp.example.se", Capability::DeployJar, "Deploy JAR").is_err());
        assert!(guard(r"\\L-x-EA-7", Capability::RestartJboss, "Restart JBoss").is_err());
    }

    #[test]
    fn guard_covers_the_file_and_service_capabilities() {
        // Looking is allowed on a public server; changing is not.
        assert!(guard("L-x-EA-7", Capability::FsBrowse, "Browse files").is_ok());
        assert!(guard("L-x-EA-7", Capability::FsDownload, "Download").is_ok());
        assert!(guard("L-x-EA-7", Capability::ServiceView, "Services").is_ok());
        assert!(guard("L-x-EA-7", Capability::FsWrite, "Delete").is_err());
        assert!(guard("L-x-EA-7", Capability::ServiceControl, "Stop service").is_err());
        assert!(guard("L-x-EA-7", Capability::ProcessKill, "End process").is_err());

        // A private server allows all of it.
        assert!(guard("L-x-SD", Capability::FsWrite, "Delete").is_ok());
        assert!(guard("L-x-SD", Capability::ServiceControl, "Stop service").is_ok());

        // A public client cannot even be opened.
        assert!(guard("L-x-CL-2", Capability::FsBrowse, "Browse files").is_err());

        // Spelling cannot buy write access.
        assert!(guard("L-x-EA-7.corp.example.se", Capability::FsWrite, "Upload").is_err());
        assert!(guard("SOMEHOST", Capability::FsWrite, "Upload").is_err());
        assert!(guard("SOMEHOST", Capability::FsBrowse, "Browse files").is_ok());
    }

    #[test]
    fn precheck_needs_no_credential_and_fails_in_the_right_order() {
        let conn = |id: &str, host: &str| Connection {
            id: id.into(),
            label: host.into(),
            computer_name: host.into(),
            username: None,
            service_name: None,
            server_root: None,
            client_root: None,
            pinned: false,
            label_id: None,
            category_id: None,
        };
        let list = vec![conn("dev", "L-x-SD"), conn("pub", "L-x-EA-7")];

        // Unknown id: reported as such, whatever the account state.
        let err = precheck(&list, "nope", Capability::FsBrowse, "Browse files").unwrap_err();
        assert!(err.is(codes::NO_CONNECTION), "{err:?}");

        // Known id: the class decides.
        assert_eq!(
            precheck(&list, "dev", Capability::FsWrite, "Delete").unwrap(),
            EnvClass::PrivateServer
        );
        let err = precheck(&list, "pub", Capability::FsWrite, "Delete").unwrap_err();
        assert!(err.is(codes::NOT_ALLOWED), "{err:?}");
        assert!(precheck(&list, "pub", Capability::FsBrowse, "Browse files").is_ok());
    }

    #[test]
    fn unclassified_refusal_explains_the_naming_convention() {
        let err = guard("SOMEHOST", Capability::DeployJar, "Deploy JAR").unwrap_err();
        assert!(err.is(codes::NOT_ALLOWED));
        assert!(err.message.contains("does not match the environment convention"), "{err}");
        assert!(err.message.contains("-CP"), "{err}");
    }

    #[test]
    fn guard_error_names_the_feature_and_host() {
        let err = guard("L-x-EA-7", Capability::RestartJboss, "Restart JBoss").unwrap_err();
        assert!(err.message.contains("Restart JBoss"));
        assert!(err.message.contains("L-x-EA-7"));
        assert!(err.message.contains("public server"));
        assert!(err.message.contains("Changes are limited"));
    }

    #[test]
    fn a_refused_read_does_not_talk_about_changes() {
        let err = guard("L-x-CL-2", Capability::FsBrowse, "Browse files").unwrap_err();
        assert!(err.message.contains("public client"));
        assert!(!err.message.contains("Changes are limited"));
    }
}
