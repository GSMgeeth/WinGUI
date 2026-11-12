//! Windows Credential Manager integration.
//!
//! Two kinds of secrets are stored here, both as GENERIC credentials protected
//! at rest by Windows (DPAPI) and never written to our own config files:
//!
//! * **The global account** (`WinGUI/global`) — the shared company Microsoft
//!   account (username + password) used for every environment. This is the
//!   single source of truth set from Settings.
//! * **Per-host RDP entries** (`TERMSRV/<ComputerName>`) — exactly where
//!   `mstsc.exe` looks for saved Remote Desktop credentials. `mstsc` reads
//!   nowhere else, so the RDP launcher (Step 5) writes a derived copy of the
//!   global password here just before launching so login is silent.
//!
//! Ported from Remote Desktop's `core::credentials` (commit 76f65f0). WinGUI
//! keeps its **own** global target so the two apps never overwrite each
//! other's account; Remote Desktop's entry is only ever *read*, by the
//! one-time import.

/// Credential Manager target name for the global shared account.
#[cfg(windows)]
const GLOBAL_TARGET: &str = "WinGUI/global";

/// Where the Remote Desktop app keeps the same account (read-only, for import).
#[cfg(windows)]
const REMOTE_DESKTOP_GLOBAL_TARGET: &str = "RemoteDesktopApp/global";

#[cfg(windows)]
mod imp {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{ERROR_NOT_FOUND, FILETIME};
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS,
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    /// `TERMSRV/<computer>` as a NUL-terminated UTF-16 buffer.
    fn target_name(computer: &str) -> Vec<u16> {
        format!("TERMSRV/{computer}\0").encode_utf16().collect()
    }

    /// Any string as a NUL-terminated UTF-16 buffer.
    fn wide(s: &str) -> Vec<u16> {
        format!("{s}\0").encode_utf16().collect()
    }

    /// Write a generic credential at `target` holding `username` and `password`.
    /// The password blob is UTF-16LE bytes (no NUL) — the encoding `mstsc`
    /// expects for `TERMSRV/*` entries, and what [`read_credential`] reads back.
    ///
    /// Persistence is **`CRED_PERSIST_LOCAL_MACHINE`**, not
    /// `CRED_PERSIST_ENTERPRISE`: the enterprise scope roams the credential with
    /// the user's profile, so the shared company account's password (and a
    /// derived copy for every host ever connected to) would be replicated to the
    /// domain profile store and to every machine the user logs into. Local-machine
    /// persistence keeps the secret on the box that needs it while still
    /// surviving logoff, which is all `mstsc` and the WinRM features require.
    fn write_credential(target: &mut [u16], username: &str, password: &str) -> Result<(), String> {
        let mut user = wide(username);
        let mut blob: Vec<u8> = password
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();

        let cred = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_mut_ptr()),
            Comment: PWSTR::null(),
            LastWritten: FILETIME::default(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: PWSTR::null(),
            UserName: PWSTR(user.as_mut_ptr()),
        };

        let result = unsafe { CredWriteW(&cred, 0) }.map_err(|e| format!("CredWriteW failed: {e}"));
        // Don't leave the cleartext password sitting in this process's heap for
        // the rest of the session; `write_volatile` isn't optimised away.
        for b in blob.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        result
    }

    /// Read back the stored `(username, password)` at `target`, if present.
    fn read_credential(target: &[u16]) -> Option<(String, String)> {
        unsafe {
            let mut pcred: *mut CREDENTIALW = std::ptr::null_mut();
            if CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut pcred).is_err()
                || pcred.is_null()
            {
                if !pcred.is_null() {
                    CredFree(pcred as *const core::ffi::c_void);
                }
                return None;
            }
            let cred = &*pcred;
            let size = cred.CredentialBlobSize as usize;
            let password = if size >= 2 && !cred.CredentialBlob.is_null() {
                let bytes = std::slice::from_raw_parts(cred.CredentialBlob, size);
                let units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&units)
            } else {
                String::new()
            };
            // UserName comes back null when the credential was stored without
            // one (e.g. the global account with a blank username); calling
            // `to_string()` on a null PWSTR would read from a null pointer.
            let username = if cred.UserName.is_null() {
                String::new()
            } else {
                cred.UserName.to_string().unwrap_or_default()
            };
            CredFree(pcred as *const core::ffi::c_void);
            Some((username, password))
        }
    }

    fn delete_credential(target: &[u16]) -> Result<(), String> {
        match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
            Ok(()) => Ok(()),
            // Deleting a credential that isn't there is a no-op, not an error.
            Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
            Err(e) => Err(format!("CredDeleteW failed: {e}")),
        }
    }

    // --- Per-host RDP entries (`TERMSRV/<computer>`) ---

    pub fn save(computer: &str, username: &str, password: &str) -> Result<(), String> {
        write_credential(&mut target_name(computer), username, password)
    }

    pub fn delete(computer: &str) -> Result<(), String> {
        delete_credential(&target_name(computer))
    }

    // --- Global shared account (`RemoteDesktopApp/global`) ---

    fn global_target() -> Vec<u16> {
        wide(super::GLOBAL_TARGET)
    }

    pub fn save_global(username: &str, password: &str) -> Result<(), String> {
        write_credential(&mut global_target(), username, password)
    }

    pub fn read_global() -> Option<(String, String)> {
        read_credential(&global_target())
    }

    pub fn delete_global() -> Result<(), String> {
        delete_credential(&global_target())
    }

    /// The account the Remote Desktop app has stored, if any. Read-only: the
    /// import copies it into WinGUI's own target on explicit consent.
    pub fn read_remote_desktop_global() -> Option<(String, String)> {
        read_credential(&wide(super::REMOTE_DESKTOP_GLOBAL_TARGET))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn credential_password_round_trips() {
            // A throwaway target so we never touch real credentials.
            let target_str = "WinGUI/__roundtrip_test__";
            let user = "TESTDOMAIN\\tester";
            let pass = "P@ss w0rd!-ünïcödé-€";

            let mut target = wide(target_str);
            write_credential(&mut target, user, pass).expect("write");
            let (got_user, got_pass) = read_credential(&wide(target_str)).expect("read");
            let _ = delete_credential(&wide(target_str));

            assert_eq!(got_user, user, "username corrupted");
            assert_eq!(got_pass, pass, "password corrupted");
        }
    }
}

#[cfg(not(windows))]
mod imp {
    const MSG: &str = "Credential storage is only supported on Windows.";

    pub fn save(_computer: &str, _username: &str, _password: &str) -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn delete(_computer: &str) -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn save_global(_username: &str, _password: &str) -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn read_global() -> Option<(String, String)> {
        None
    }
    pub fn delete_global() -> Result<(), String> {
        Err(MSG.into())
    }
    pub fn read_remote_desktop_global() -> Option<(String, String)> {
        None
    }
}

pub use imp::{delete, delete_global, read_global, read_remote_desktop_global, save, save_global};
