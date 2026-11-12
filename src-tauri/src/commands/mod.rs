//! The Tauri front end: thin `#[tauri::command]` wrappers over the core.
//!
//! Nothing feature-specific lives here — a command's job is to translate
//! Tauri arguments into a core call and hand the structured result back.
//! Anything that touches an environment goes through the
//! [`FeatureRegistry`], so the safety guard cannot be skipped from the UI.
//!
//! Commands that can block (anything remote, anything that shells out) are
//! `async` and run their work on the blocking pool via [`blocking`], so the
//! window stays responsive.

pub mod connections;
pub mod session;
pub mod settings;

use std::sync::Arc;

use crate::core::error::{codes, AppError, AppResult};
use crate::core::feature::{FeatureDeps, FeatureRegistry};
use crate::core::features::build_registry;

/// The shared runtime every command dispatches through: the feature registry
/// plus the transport/session dependencies. Built once at startup. Both are
/// `Arc`s so a command can hand clones to a blocking task.
pub struct AppRuntime {
    pub registry: Arc<FeatureRegistry>,
    pub deps: Arc<FeatureDeps>,
}

impl AppRuntime {
    pub fn new() -> Self {
        AppRuntime {
            registry: Arc::new(build_registry()),
            deps: Arc::new(FeatureDeps::real()),
        }
    }
}

/// Run blocking work off the async runtime's worker threads.
pub async fn blocking<T, F>(work: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| AppError::new(codes::ERROR, format!("background task failed: {e}")))?
}
