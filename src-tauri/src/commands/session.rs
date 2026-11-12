//! Opening environments and talking to their held sessions.
//!
//! Everything that reaches a server goes through `registry.run`, which applies
//! the environment-class guard. Closing and status only touch local state.

use serde_json::{json, Value};
use tauri::State;

use super::{blocking, AppRuntime};
use crate::core::error::AppResult;
use crate::core::features::ids;
use crate::core::remoting::worker::SessionInfo;

/// Run a registry feature on the blocking pool.
async fn run_feature(
    rt: &State<'_, AppRuntime>,
    feature_id: &'static str,
    connection_id: String,
    args: Value,
) -> AppResult<Value> {
    let registry = rt.registry.clone();
    let deps = rt.deps.clone();
    blocking(move || registry.run(&deps, feature_id, &connection_id, args, &|_| {})).await
}

/// Connect to an environment (or confirm the existing session). This is the
/// user-initiated path: it clears a latched authentication failure and tries
/// again. Returns what the remote end reported.
#[tauri::command]
pub async fn session_open(rt: State<'_, AppRuntime>, connection_id: String) -> AppResult<Value> {
    run_feature(&rt, ids::SESSION_OPEN, connection_id, json!({})).await
}

/// Close an environment's session (its tab was closed).
#[tauri::command]
pub async fn session_close(rt: State<'_, AppRuntime>, connection_id: String) -> AppResult<()> {
    let pool = rt.deps.pool.clone();
    blocking(move || {
        pool.close(&connection_id);
        Ok(())
    })
    .await
}

/// The live session's details, or `null` when the environment is not connected.
#[tauri::command]
pub fn session_status(rt: State<AppRuntime>, connection_id: String) -> Option<SessionInfo> {
    rt.deps.pool.status(&connection_id)
}

/// The environment's drives with size and free space.
#[tauri::command]
pub async fn fs_drives(rt: State<'_, AppRuntime>, connection_id: String) -> AppResult<Value> {
    run_feature(&rt, ids::FS_DRIVES, connection_id, json!({})).await
}
