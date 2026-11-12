//! File-system features. Step 1 has only the drive list; listing, search,
//! preview and the rest arrive with the explorer (Step 2).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::env_class::Capability;
use crate::core::error::{codes, AppError, AppResult};
use crate::core::feature::{FeatureCtx, Progress, RemoteFeature};
use crate::core::remoting::pool::Retry;

/// Deadline for quick, bounded reads.
const QUICK: Duration = Duration::from_secs(30);

/// One drive on the environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    /// Drive letter with colon, e.g. `D:`.
    pub name: String,
    /// `Fixed`, `Removable`, `CDRom`, `Network`, `Ram`, `Unknown`, `NoRootDirectory`.
    pub drive_type: String,
    /// False for an empty optical drive or an offline volume — the size
    /// fields are then zero and the drive cannot be browsed.
    pub ready: bool,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub file_system: String,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub free_bytes: u64,
}

/// Parse and validate the worker's `fs.drives` reply. Anything that is not a
/// plain drive letter is dropped rather than shown: the name becomes the root
/// of paths sent back to the server.
pub fn parse_drives(reply: Value) -> AppResult<Vec<DriveInfo>> {
    let mut drives: Vec<DriveInfo> = serde_json::from_value(reply).map_err(|e| {
        AppError::new(codes::WORKER_FAILED, format!("unexpected drive list from the environment: {e}"))
    })?;
    drives.retain(|d| is_drive_name(&d.name));
    drives.sort_by(|a, b| a.name.to_ascii_uppercase().cmp(&b.name.to_ascii_uppercase()));
    Ok(drives)
}

/// `X:` — exactly one ASCII letter and a colon.
fn is_drive_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

pub struct DrivesFeature;

impl RemoteFeature for DrivesFeature {
    fn id(&self) -> &'static str {
        super::ids::FS_DRIVES
    }

    fn name(&self) -> &'static str {
        "List drives"
    }

    fn capability(&self) -> Capability {
        Capability::FsBrowse
    }

    /// Result: `DriveInfo[]`, sorted by letter.
    fn run(&self, ctx: &FeatureCtx, _args: Value, _progress: Progress) -> AppResult<Value> {
        let reply = ctx.request("fs.drives", json!({}), QUICK, Retry::Idempotent)?;
        let drives = parse_drives(reply)?;
        serde_json::to_value(drives).map_err(|e| format!("serialize result failed: {e}").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sorts_and_keeps_only_drive_letters() {
        let reply = json!([
            { "name": "D:", "driveType": "Fixed", "ready": true, "label": "Data",
              "fileSystem": "NTFS", "totalBytes": 200, "freeBytes": 150 },
            { "name": "c:", "driveType": "Fixed", "ready": true, "label": "",
              "fileSystem": "NTFS", "totalBytes": 100, "freeBytes": 40 },
            { "name": "E:", "driveType": "CDRom", "ready": false },
            { "name": r"\\server\share", "driveType": "Network", "ready": true },
            { "name": "..", "driveType": "Fixed", "ready": true }
        ]);
        let drives = parse_drives(reply).unwrap();
        let names: Vec<&str> = drives.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["c:", "D:", "E:"]);
        assert_eq!(drives[1].free_bytes, 150);
        assert!(!drives[2].ready);
        assert_eq!(drives[2].total_bytes, 0);
    }

    #[test]
    fn a_malformed_reply_is_a_worker_error() {
        let err = parse_drives(json!({ "not": "a list" })).unwrap_err();
        assert!(err.is(codes::WORKER_FAILED));
    }

    #[test]
    fn serializes_in_camel_case_for_the_ui() {
        let d = DriveInfo {
            name: "C:".into(),
            drive_type: "Fixed".into(),
            ready: true,
            label: "OS".into(),
            file_system: "NTFS".into(),
            total_bytes: 10,
            free_bytes: 4,
        };
        let v = serde_json::to_value(d).unwrap();
        assert_eq!(v["driveType"], "Fixed");
        assert_eq!(v["fileSystem"], "NTFS");
        assert_eq!(v["totalBytes"], 10);
        assert_eq!(v["freeBytes"], 4);
    }
}
