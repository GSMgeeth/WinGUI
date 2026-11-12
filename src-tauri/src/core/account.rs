//! The global shared account used for every environment.
//!
//! All environments authenticate with one company Microsoft account, so the
//! username and password are configured once (in the GUI's Settings) rather
//! than per host. The secret lives in Windows Credential Manager (see
//! [`super::credentials`]); these functions expose it to front ends without
//! ever handing the password back.

use serde::Serialize;

/// What a front end needs to render account state: the username and whether a
/// password is currently stored. The password itself is never returned.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub username: Option<String>,
    pub has_password: bool,
}

/// Current global account state.
pub fn get_account() -> AccountInfo {
    match super::credentials::read_global() {
        Some((username, password)) => AccountInfo {
            username: Some(username).filter(|u| !u.is_empty()),
            has_password: !password.is_empty(),
        },
        None => AccountInfo {
            username: None,
            has_password: false,
        },
    }
}

/// Save the global account. The username may be blank — deploy/restart/logs and
/// RDP then fall back to the logged-in Windows account (`DOMAIN\user`), which is
/// what WinRM Negotiate expects. `password` is optional so the username can be
/// edited without re-entering it: when omitted, the stored password is kept.
pub fn set_account(username: String, password: Option<String>) -> Result<(), String> {
    let username = username.trim().to_string();
    let password = match password.filter(|p| !p.is_empty()) {
        Some(p) => p,
        None => super::credentials::read_global()
            .map(|(_, pw)| pw)
            .filter(|pw| !pw.is_empty())
            .ok_or_else(|| "Enter a password for the account.".to_string())?,
    };
    super::credentials::save_global(&username, &password)
}

/// Remove the global account entirely.
pub fn clear_account() -> Result<(), String> {
    super::credentials::delete_global()
}
