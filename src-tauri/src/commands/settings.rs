//! Machine-level settings: the global account, the WinRM client setup, and
//! the one-time import from Remote Desktop. None of these talk to an
//! environment, so none go through the feature registry.

use tauri::State;

use super::{blocking, AppRuntime};
use crate::core::error::AppResult;
use crate::core::import::{self, ImportPreview, ImportSummary};
use crate::core::{account, winrm};

#[tauri::command]
pub fn get_account() -> account::AccountInfo {
    account::get_account()
}

#[tauri::command]
pub fn set_account(
    rt: State<AppRuntime>,
    username: String,
    password: Option<String>,
) -> AppResult<()> {
    account::set_account(username, password)?;
    // A new credential deserves a fresh attempt on environments that
    // rejected the old one.
    rt.deps.pool.clear_auth_latches();
    Ok(())
}

#[tauri::command]
pub fn clear_account(rt: State<AppRuntime>) -> AppResult<()> {
    account::clear_account()?;
    // Sessions opened with the removed credential should not linger.
    rt.deps.pool.close_all();
    Ok(())
}

/// Current WinRM client state (spawns PowerShell, so it runs off the UI thread).
#[tauri::command]
pub async fn winrm_status() -> AppResult<winrm::WinrmStatus> {
    blocking(|| Ok(winrm::read_status())).await
}

/// Configure the WinRM client for the saved environments (UAC prompt).
#[tauri::command]
pub async fn setup_winrm() -> AppResult<String> {
    blocking(|| Ok(winrm::run_setup()?)).await
}

#[tauri::command]
pub async fn import_preview() -> AppResult<ImportPreview> {
    blocking(|| Ok(import::preview())).await
}

#[tauri::command]
pub async fn import_remote_desktop(
    rt: State<'_, AppRuntime>,
    include_account: bool,
) -> AppResult<ImportSummary> {
    let pool = rt.deps.pool.clone();
    blocking(move || {
        let summary = import::run(include_account)?;
        if summary.account_imported {
            pool.clear_auth_latches();
        }
        Ok(summary)
    })
    .await
}
