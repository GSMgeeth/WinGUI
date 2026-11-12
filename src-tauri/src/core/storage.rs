//! App-config storage shared by every front end.
//!
//! Ported from Remote Desktop's `core::storage` (commit 76f65f0) with three
//! changes the plan calls for (docs/03-architecture.md §3.8.3):
//!
//! * **Atomic writes** — a store is written to a sibling temp file and renamed
//!   over the target, so a crash or power loss mid-save can't leave a
//!   half-written `connections.json`.
//! * **Versioned files** — stores are wrapped as `{ "schemaVersion": N,
//!   "data": … }`. Bare (unwrapped) content still loads, which is what makes
//!   Remote Desktop's files directly importable.
//! * **Corruption is preserved, not overwritten** — an unreadable store loads
//!   as the default and the bad file is renamed to `*.corrupt-<epoch>` so the
//!   user's data is still recoverable by hand.
//!
//! Disk is the single source of truth: there is no in-memory mirror, so a CLI
//! process sees exactly what the GUI wrote.

use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

/// Must match `identifier` in `tauri.conf.json` so the core and the Tauri
/// path API resolve the same directory.
pub const APP_IDENTIFIER: &str = "com.curiouserlabs.wingui";

/// The config dir of the Remote Desktop app, read (never written) by the
/// one-time import.
pub const REMOTE_DESKTOP_IDENTIFIER: &str = "com.curiouserlabs.remote-desktop";

/// Version written into every store. Bump when a store's shape changes and
/// teach [`JsonStore::load`] to migrate the older shape forward.
pub const SCHEMA_VERSION: u32 = 1;

/// Overrides the config dir (development and tests): lets a dev build run
/// against a scratch folder instead of the real `%APPDATA%` data.
pub const CONFIG_DIR_ENV: &str = "WINGUI_CONFIG_DIR";

#[cfg(windows)]
fn base_config_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(not(windows))]
fn base_config_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
}

/// The app config dir (created if missing): `%APPDATA%\com.curiouserlabs.wingui`.
pub fn config_dir() -> Result<PathBuf, String> {
    let dir = match std::env::var_os(CONFIG_DIR_ENV).filter(|v| !v.is_empty()) {
        Some(over) => PathBuf::from(over),
        None => base_config_dir()
            .ok_or_else(|| "could not resolve config dir: APPDATA is not set".to_string())?
            .join(APP_IDENTIFIER),
    };
    fs::create_dir_all(&dir).map_err(|e| format!("could not create config dir: {e}"))?;
    Ok(dir)
}

/// Where Remote Desktop keeps its stores, if that app has ever run here.
pub fn remote_desktop_config_dir() -> Option<PathBuf> {
    let dir = base_config_dir()?.join(REMOTE_DESKTOP_IDENTIFIER);
    dir.is_dir().then_some(dir)
}

/// A JSON-file-backed store for one value (typically a `Vec<T>` or a config
/// struct). Load never fails — a missing or unreadable file yields the
/// default, so a fresh install starts clean.
pub struct JsonStore<T> {
    path: PathBuf,
    _marker: PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned + Default> JsonStore<T> {
    /// Store backed by `<config dir>\<file_name>`.
    pub fn in_config_dir(file_name: &str) -> Result<Self, String> {
        Ok(Self::at(config_dir()?.join(file_name)))
    }

    /// Store backed by an explicit path (the import reads Remote Desktop's
    /// files this way; tests use it too).
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            _marker: PhantomData,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the backing file exists.
    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    /// Read the stored value; default when the file is missing. A file that
    /// exists but cannot be parsed is set aside as `*.corrupt-<epoch>` and the
    /// default is returned.
    pub fn load(&self) -> T {
        let Ok(data) = fs::read_to_string(&self.path) else {
            return T::default();
        };
        match parse::<T>(&data) {
            Some(value) => value,
            None => {
                self.quarantine();
                T::default()
            }
        }
    }

    /// Read the stored value without touching the file on failure. Used for
    /// files this app does not own (Remote Desktop's stores).
    pub fn load_readonly(&self) -> Option<T> {
        parse::<T>(&fs::read_to_string(&self.path).ok()?)
    }

    /// Write the value atomically as versioned, pretty JSON.
    pub fn save(&self, value: &T) -> Result<(), String> {
        let envelope = serde_json::json!({
            "schemaVersion": SCHEMA_VERSION,
            "data": value,
        });
        let data = serde_json::to_string_pretty(&envelope)
            .map_err(|e| format!("serialize failed: {e}"))?;

        // Same directory => same volume, so the rename is atomic.
        let tmp = self.sibling(&format!("tmp-{}", std::process::id()));
        fs::write(&tmp, data).map_err(|e| format!("could not write store: {e}"))?;
        fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("could not replace store: {e}")
        })
    }

    /// `<file name>.<suffix>` next to the store.
    fn sibling(&self, suffix: &str) -> PathBuf {
        let mut name = self
            .path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(".");
        name.push(suffix);
        self.path.with_file_name(name)
    }

    /// Move an unparseable file out of the way so the next save doesn't
    /// destroy whatever is recoverable in it.
    fn quarantine(&self) {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = fs::rename(&self.path, self.sibling(&format!("corrupt-{epoch}")));
    }
}

/// Parse store content: the versioned envelope, or a bare value (Remote
/// Desktop's format, and anything written before versioning).
fn parse<T: DeserializeOwned>(data: &str) -> Option<T> {
    // A UTF-8 BOM (files touched by Notepad/PowerShell) is not valid JSON.
    let value: Value = serde_json::from_str(data.trim_start_matches('\u{feff}')).ok()?;
    let payload = match value {
        Value::Object(mut map) if map.contains_key("schemaVersion") && map.contains_key("data") => {
            map.remove("data").unwrap_or(Value::Null)
        }
        other => other,
    };
    serde_json::from_value(payload).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wingui-store-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("store.json")
    }

    fn cleanup(path: &Path) {
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn load_missing_file_yields_default() {
        let store: JsonStore<Vec<String>> = JsonStore::at(temp("missing"));
        assert!(store.load().is_empty());
        assert!(!store.exists());
        cleanup(store.path());
    }

    #[test]
    fn save_then_load_round_trips_through_the_envelope() {
        let store: JsonStore<Vec<String>> = JsonStore::at(temp("roundtrip"));
        store.save(&vec!["a".to_string(), "b".to_string()]).unwrap();
        assert_eq!(store.load(), vec!["a".to_string(), "b".to_string()]);

        let raw: Value = serde_json::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
        assert_eq!(raw["schemaVersion"], SCHEMA_VERSION);
        assert!(raw["data"].is_array());
        // No temp file is left behind by the atomic write.
        let leftovers: Vec<_> = fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty());
        cleanup(store.path());
    }

    #[test]
    fn bare_remote_desktop_format_still_loads() {
        let path = temp("bare");
        fs::write(&path, r#"["x","y"]"#).unwrap();
        let store: JsonStore<Vec<String>> = JsonStore::at(path);
        assert_eq!(store.load(), vec!["x".to_string(), "y".to_string()]);
        cleanup(store.path());
    }

    #[test]
    fn a_bom_prefixed_file_loads() {
        let path = temp("bom");
        fs::write(&path, "\u{feff}[\"x\"]").unwrap();
        let store: JsonStore<Vec<String>> = JsonStore::at(path);
        assert_eq!(store.load(), vec!["x".to_string()]);
        cleanup(store.path());
    }

    #[test]
    fn corrupt_file_yields_default_and_is_preserved() {
        let path = temp("corrupt");
        fs::write(&path, "{not json").unwrap();
        let store: JsonStore<Vec<String>> = JsonStore::at(path.clone());
        assert!(store.load().is_empty());
        assert!(!path.exists(), "the corrupt file should have been moved aside");
        let kept: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(fs::read_to_string(kept[0].path()).unwrap(), "{not json");
        cleanup(&path);
    }

    #[test]
    fn readonly_load_never_moves_a_foreign_file() {
        let path = temp("readonly");
        fs::write(&path, "{not json").unwrap();
        let store: JsonStore<Vec<String>> = JsonStore::at(path.clone());
        assert!(store.load_readonly().is_none());
        assert!(path.exists());
        cleanup(&path);
    }
}
