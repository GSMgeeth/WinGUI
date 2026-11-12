//! Opening an environment: establish (or reuse) its held session and report
//! what answered.
//!
//! This is a registry feature rather than a direct pool call so that merely
//! *connecting* goes through the same guard as everything else — a public
//! client machine is refused before any credential is sent to it.

use serde_json::Value;

use crate::core::env_class::Capability;
use crate::core::error::AppResult;
use crate::core::feature::{FeatureCtx, Progress, RemoteFeature};

pub struct SessionOpenFeature;

impl RemoteFeature for SessionOpenFeature {
    fn id(&self) -> &'static str {
        super::ids::SESSION_OPEN
    }

    fn name(&self) -> &'static str {
        "Open environment"
    }

    fn capability(&self) -> Capability {
        // The least any openable environment grants.
        Capability::FsBrowse
    }

    /// Result: [`SessionInfo`](crate::core::remoting::worker::SessionInfo).
    fn run(&self, ctx: &FeatureCtx, _args: Value, _progress: Progress) -> AppResult<Value> {
        let info = ctx.open_session()?;
        serde_json::to_value(info).map_err(|e| format!("serialize result failed: {e}").into())
    }
}
