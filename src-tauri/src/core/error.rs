//! The one error type that crosses the core boundary.
//!
//! Remote Desktop returned plain `String` errors, which the UI could only show
//! verbatim. An explorer has to *react* to failures — offer the account
//! settings on an auth failure, a retry when the host is unreachable, the
//! naming-convention hint on a refused action — so every error carries a
//! stable machine [`code`](AppError::code) next to its human message.
//!
//! `String` still converts in (`?` works on the ported code unchanged) and
//! lands on the generic [`codes::ERROR`].

use serde::Serialize;

/// Stable error codes the front ends may branch on. Adding one is fine;
/// renaming one is a breaking change for the UI and the CLI's `--json`.
pub mod codes {
    /// Anything without a more specific code.
    pub const ERROR: &str = "Error";
    /// No account password is stored yet.
    pub const NO_ACCOUNT: &str = "NoAccount";
    /// The connection id/name does not exist.
    pub const NO_CONNECTION: &str = "NoConnection";
    /// The environment class forbids the requested capability.
    pub const NOT_ALLOWED: &str = "NotAllowed";
    /// No feature is registered under the requested id.
    pub const UNKNOWN_FEATURE: &str = "UnknownFeature";

    // --- Session / transport (produced by the worker, see `remoting::worker`) ---
    /// The server rejected the credentials. **Never retried** — repeated
    /// attempts with a stale password lock the shared account out.
    pub const AUTH_FAILED: &str = "AuthFailed";
    /// Name did not resolve, port closed, VPN down, or the open timed out.
    pub const UNREACHABLE: &str = "Unreachable";
    /// Negotiate fell back to NTLM and the host is not in `TrustedHosts`.
    pub const NOT_TRUSTED: &str = "NotTrusted";
    /// The server's per-user WinRM shell quota is exhausted.
    pub const QUOTA_EXCEEDED: &str = "QuotaExceeded";
    /// Any other failure to establish the session.
    pub const CONNECT_FAILED: &str = "ConnectFailed";
    /// An established session died (VPN drop, sleep, host restart).
    pub const SESSION_LOST: &str = "SessionLost";
    /// The operation did not answer within its deadline.
    pub const TIMEOUT: &str = "Timeout";
    /// This client already holds its maximum number of sessions to the host.
    pub const TOO_MANY_SESSIONS: &str = "TooManySessions";
    /// The worker process could not be started or spoke nonsense.
    pub const WORKER_FAILED: &str = "WorkerFailed";

    // --- Remote operation results ---
    pub const NOT_FOUND: &str = "NotFound";
    pub const ACCESS_DENIED: &str = "AccessDenied";
    pub const IN_USE: &str = "InUse";
    pub const PATH_TOO_LONG: &str = "PathTooLong";
    pub const UNKNOWN_OP: &str = "UnknownOp";
}

/// An error with a stable code and a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        AppError {
            code: code.to_string(),
            message: message.into(),
        }
    }

    pub fn is(&self, code: &str) -> bool {
        self.code == code
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

impl From<String> for AppError {
    fn from(message: String) -> Self {
        AppError::new(codes::ERROR, message)
    }
}

impl From<&str> for AppError {
    fn from(message: &str) -> Self {
        AppError::new(codes::ERROR, message)
    }
}

/// Shorthand used throughout the core.
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_convert_to_the_generic_code() {
        let e: AppError = "boom".to_string().into();
        assert!(e.is(codes::ERROR));
        assert_eq!(e.to_string(), "boom");
    }

    #[test]
    fn serializes_as_code_and_message() {
        let e = AppError::new(codes::AUTH_FAILED, "bad password");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "AuthFailed");
        assert_eq!(v["message"], "bad password");
    }
}
