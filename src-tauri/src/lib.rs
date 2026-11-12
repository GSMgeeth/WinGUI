//! Crate layout:
//!
//! * [`core`] — every piece of business logic, with **no Tauri dependency**:
//!   models + stores, credentials, the WinRM transports (one-shot and held
//!   sessions), environment class/layout rules, and the feature plugins behind
//!   the [`core::feature::FeatureRegistry`].
//! * [`commands`] — the Tauri front end: thin command wrappers over the core.

mod commands;
pub mod core;

use std::time::Duration;

use tauri::Manager;

/// How often idle sessions are looked for.
const REAP_INTERVAL: Duration = Duration::from_secs(30);

/// How long app exit waits for workers to remove their remote sessions.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

pub fn run() {
    tauri::Builder::default()
        // A second launch focuses the running window instead of starting a
        // second set of sessions against the same environments.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            let runtime = commands::AppRuntime::new();

            // Close sessions nobody has used for a while: each one is a local
            // powershell.exe and a slot in the server's per-user WinRM quota.
            let pool = runtime.deps.pool.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(REAP_INTERVAL);
                pool.reap_idle();
            });

            app.manage(runtime);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::connections::list_connections,
            commands::connections::add_connection,
            commands::connections::update_connection,
            commands::connections::delete_connection,
            commands::connections::set_connection_pinned,
            commands::connections::list_labels,
            commands::connections::add_label,
            commands::connections::update_label,
            commands::connections::delete_label,
            commands::connections::reorder_labels,
            commands::connections::list_categories,
            commands::connections::add_category,
            commands::connections::update_category,
            commands::connections::delete_category,
            commands::connections::reorder_categories,
            commands::settings::get_account,
            commands::settings::set_account,
            commands::settings::clear_account,
            commands::settings::winrm_status,
            commands::settings::setup_winrm,
            commands::settings::import_preview,
            commands::settings::import_remote_desktop,
            commands::session::session_open,
            commands::session::session_close,
            commands::session::session_status,
            commands::session::fs_drives,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Remove the remote sessions before the process goes away;
                // abandoned ones keep counting against the server's quota
                // until WinRM's idle timeout reclaims them.
                app.state::<commands::AppRuntime>()
                    .deps
                    .pool
                    .shutdown(SHUTDOWN_GRACE);
            }
        });
}
