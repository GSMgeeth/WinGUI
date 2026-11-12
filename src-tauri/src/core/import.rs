//! One-time import of the Remote Desktop app's saved data.
//!
//! WinGUI keeps its own config folder and its own account entry, so the two
//! apps never write each other's files. To spare users re-typing everything,
//! this module **copies** Remote Desktop's connections, labels, categories,
//! modules, property sets and auto-deploy roots into WinGUI's stores, and —
//! only on explicit consent — its account.
//!
//! Guarantees:
//!
//! * Remote Desktop's files and credential are **read, never modified**.
//! * The import **merges**: nothing already in WinGUI is overwritten or
//!   removed, so running it twice adds nothing the second time.
//! * Every imported connection passes the same
//!   [`normalize_connection`](crate::core::model::normalize_connection)
//!   validation as one typed by hand — an edited `connections.json` cannot
//!   smuggle in a host name that manual entry would reject.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use uuid::Uuid;

use super::credentials;
use super::model::{
    categories_store, connections_store, labels_store, modules_store, normalize_category,
    normalize_connection, normalize_label, normalize_module, normalize_property_set,
    property_sets_store, Category, CategoryInput, Connection, ConnectionInput, Label, LabelInput,
    Module, ModuleInput, PropertySet, PropertySetInput,
};
use super::storage::{config_dir, remote_desktop_config_dir, JsonStore};

/// What an import would bring in, shown before the user confirms.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    /// Whether a Remote Desktop config folder exists on this machine.
    pub available: bool,
    pub source_dir: Option<String>,
    pub connections: usize,
    pub labels: usize,
    pub categories: usize,
    pub modules: usize,
    pub property_sets: usize,
    /// Whether Remote Desktop has an account stored.
    pub has_account: bool,
    /// Its username (never the password), so the user knows what they are importing.
    pub account_username: Option<String>,
}

/// What an import actually did.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub connections_added: usize,
    /// Already present (same computer name) or rejected by validation.
    pub connections_skipped: usize,
    pub labels_added: usize,
    pub categories_added: usize,
    pub modules_added: usize,
    pub property_sets_added: usize,
    pub auto_deploy_copied: bool,
    pub account_imported: bool,
}

fn source<T>(dir: &Path, file: &str) -> Vec<T>
where
    T: serde::de::DeserializeOwned + Serialize,
{
    JsonStore::<Vec<T>>::at(dir.join(file))
        .load_readonly()
        .unwrap_or_default()
}

/// Describe what is available to import.
pub fn preview() -> ImportPreview {
    let Some(dir) = remote_desktop_config_dir() else {
        return ImportPreview::default();
    };
    let account = credentials::read_remote_desktop_global().filter(|(_, pw)| !pw.is_empty());
    ImportPreview {
        available: true,
        connections: source::<Connection>(&dir, "connections.json").len(),
        labels: source::<Label>(&dir, "labels.json").len(),
        categories: source::<Category>(&dir, "categories.json").len(),
        modules: source::<Module>(&dir, "modules.json").len(),
        property_sets: source::<PropertySet>(&dir, "property_sets.json").len(),
        has_account: account.is_some(),
        account_username: account.map(|(u, _)| u).filter(|u| !u.is_empty()),
        source_dir: Some(dir.to_string_lossy().into_owned()),
    }
}

/// Run the import. `include_account` is the user's explicit consent to copy
/// the stored password; an account already configured in WinGUI is kept.
pub fn run(include_account: bool) -> Result<ImportSummary, String> {
    let dir: PathBuf = remote_desktop_config_dir()
        .ok_or_else(|| "Remote Desktop has no saved data on this machine.".to_string())?;
    let mut summary = ImportSummary::default();

    // Groups first: connections refer to them by id.
    let labels = labels_store()?;
    let (merged, added, label_ids) = merge_labels(labels.load(), source(&dir, "labels.json"));
    if added > 0 {
        labels.save(&merged)?;
    }
    summary.labels_added = added;

    let categories = categories_store()?;
    let (merged, added, category_ids) =
        merge_categories(categories.load(), source(&dir, "categories.json"));
    if added > 0 {
        categories.save(&merged)?;
    }
    summary.categories_added = added;

    let connections = connections_store()?;
    let (merged, added, skipped) = merge_connections(
        connections.load(),
        source(&dir, "connections.json"),
        &label_ids,
        &category_ids,
    );
    if added > 0 {
        connections.save(&merged)?;
    }
    summary.connections_added = added;
    summary.connections_skipped = skipped;

    let modules = modules_store()?;
    let (merged, added) = merge_modules(modules.load(), source(&dir, "modules.json"));
    if added > 0 {
        modules.save(&merged)?;
    }
    summary.modules_added = added;

    let sets = property_sets_store()?;
    let (merged, added) = merge_property_sets(sets.load(), source(&dir, "property_sets.json"));
    if added > 0 {
        sets.save(&merged)?;
    }
    summary.property_sets_added = added;

    // Auto-deploy roots: an opaque settings file, copied only if WinGUI has
    // none yet (its feature arrives in Step 5 and reads the same shape).
    let from = dir.join("auto_deploy.json");
    let to = config_dir()?.join("auto_deploy.json");
    if from.is_file() && !to.exists() {
        summary.auto_deploy_copied = std::fs::copy(&from, &to).is_ok();
    }

    if include_account && credentials::read_global().is_none() {
        if let Some((user, password)) =
            credentials::read_remote_desktop_global().filter(|(_, pw)| !pw.is_empty())
        {
            credentials::save_global(&user, &password)?;
            summary.account_imported = true;
        }
    }

    Ok(summary)
}

/// Merge labels. Returns the merged list, how many were added, and a map from
/// each incoming id to the id it has in the merged list (an incoming label
/// whose *name* already exists is folded into the existing one).
fn merge_labels(
    mut existing: Vec<Label>,
    incoming: Vec<Label>,
) -> (Vec<Label>, usize, HashMap<String, String>) {
    let mut ids = HashMap::new();
    let mut added = 0;
    for label in incoming {
        let Ok((name, color)) = normalize_label(LabelInput {
            name: label.name,
            color: label.color,
        }) else {
            continue;
        };
        if let Some(same) = existing.iter().find(|l| l.name.eq_ignore_ascii_case(&name)) {
            ids.insert(label.id, same.id.clone());
            continue;
        }
        let id = fresh_id(&label.id, existing.iter().map(|l| l.id.as_str()));
        ids.insert(label.id, id.clone());
        existing.push(Label { id, name, color });
        added += 1;
    }
    (existing, added, ids)
}

/// Merge categories; same contract as [`merge_labels`].
fn merge_categories(
    mut existing: Vec<Category>,
    incoming: Vec<Category>,
) -> (Vec<Category>, usize, HashMap<String, String>) {
    let mut ids = HashMap::new();
    let mut added = 0;
    for category in incoming {
        let Ok(name) = normalize_category(CategoryInput { name: category.name }) else {
            continue;
        };
        if let Some(same) = existing.iter().find(|c| c.name.eq_ignore_ascii_case(&name)) {
            ids.insert(category.id, same.id.clone());
            continue;
        }
        let id = fresh_id(&category.id, existing.iter().map(|c| c.id.as_str()));
        ids.insert(category.id, id.clone());
        existing.push(Category { id, name });
        added += 1;
    }
    (existing, added, ids)
}

/// Merge connections. One per computer name (case-insensitive): a host WinGUI
/// already has is skipped, as is any entry that fails validation. Group ids
/// are rewritten through the maps from the group merges.
fn merge_connections(
    mut existing: Vec<Connection>,
    incoming: Vec<Connection>,
    label_ids: &HashMap<String, String>,
    category_ids: &HashMap<String, String>,
) -> (Vec<Connection>, usize, usize) {
    let (mut added, mut skipped) = (0, 0);
    for conn in incoming {
        let remap = |id: Option<String>, map: &HashMap<String, String>| {
            id.and_then(|old| map.get(&old).cloned())
        };
        let Ok(input) = normalize_connection(ConnectionInput {
            label: conn.label,
            computer_name: conn.computer_name,
            username: conn.username,
            service_name: conn.service_name,
            server_root: conn.server_root,
            client_root: conn.client_root,
            label_id: remap(conn.label_id, label_ids),
            category_id: remap(conn.category_id, category_ids),
        }) else {
            skipped += 1;
            continue;
        };
        if existing
            .iter()
            .any(|c| c.computer_name.eq_ignore_ascii_case(&input.computer_name))
        {
            skipped += 1;
            continue;
        }
        let mut imported = Connection::from_input(input);
        imported.id = fresh_id(&conn.id, existing.iter().map(|c| c.id.as_str()));
        imported.pinned = conn.pinned;
        existing.push(imported);
        added += 1;
    }
    (existing, added, skipped)
}

/// Merge modules by name (case-insensitive).
fn merge_modules(mut existing: Vec<Module>, incoming: Vec<Module>) -> (Vec<Module>, usize) {
    let mut added = 0;
    for module in incoming {
        let Ok((name, remote_path)) = normalize_module(ModuleInput {
            name: module.name,
            remote_path: module.remote_path,
        }) else {
            continue;
        };
        if existing.iter().any(|m| m.name.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let id = fresh_id(&module.id, existing.iter().map(|m| m.id.as_str()));
        existing.push(Module {
            id,
            name,
            remote_path,
        });
        added += 1;
    }
    (existing, added)
}

/// Merge property sets by name (case-insensitive).
fn merge_property_sets(
    mut existing: Vec<PropertySet>,
    incoming: Vec<PropertySet>,
) -> (Vec<PropertySet>, usize) {
    let mut added = 0;
    for set in incoming {
        let Ok((name, content)) = normalize_property_set(PropertySetInput {
            name: set.name,
            content: set.content,
        }) else {
            continue;
        };
        if existing.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let id = fresh_id(&set.id, existing.iter().map(|s| s.id.as_str()));
        existing.push(PropertySet { id, name, content });
        added += 1;
    }
    (existing, added)
}

/// Keep the incoming id when it is usable and free, so references between the
/// two apps' data stay recognisable; otherwise mint a new one.
fn fresh_id<'a>(wanted: &str, mut taken: impl Iterator<Item = &'a str>) -> String {
    if wanted.trim().is_empty() || taken.any(|id| id == wanted) {
        Uuid::new_v4().to_string()
    } else {
        wanted.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(id: &str, name: &str) -> Label {
        Label {
            id: id.into(),
            name: name.into(),
            color: "#112233".into(),
        }
    }

    fn conn(id: &str, host: &str, label_id: Option<&str>) -> Connection {
        Connection {
            id: id.into(),
            label: host.into(),
            computer_name: host.into(),
            username: None,
            service_name: Some("Production_Cosmic".into()),
            server_root: None,
            client_root: None,
            pinned: true,
            label_id: label_id.map(String::from),
            category_id: None,
        }
    }

    #[test]
    fn labels_merge_by_name_and_map_ids() {
        let (merged, added, ids) = merge_labels(
            vec![label("mine", "Dev")],
            vec![label("theirs-1", "dev"), label("theirs-2", "QA"), label("x", "  ")],
        );
        assert_eq!(added, 1);
        assert_eq!(merged.len(), 2);
        // The duplicate name folds into the existing label…
        assert_eq!(ids["theirs-1"], "mine");
        // …the new one keeps its id; the blank one is dropped.
        assert_eq!(ids["theirs-2"], "theirs-2");
        assert!(!ids.contains_key("x"));
    }

    #[test]
    fn connections_merge_by_host_and_follow_remapped_groups() {
        let mut ids = HashMap::new();
        ids.insert("theirs-1".to_string(), "mine".to_string());
        let (merged, added, skipped) = merge_connections(
            vec![conn("a", "L-one-SD", None)],
            vec![
                conn("b", "l-ONE-sd", None),                 // same host, different case
                conn("c", "L-two-SD", Some("theirs-1")),     // new, label remapped
                conn("d", "L-three-SD", Some("gone")),       // new, dangling label dropped
                conn("e", "bad\r\nhost", None),              // rejected by validation
                conn("a", "L-four-SD", None),                // id collides with an existing one
            ],
            &ids,
            &HashMap::new(),
        );
        assert_eq!((added, skipped), (3, 2));
        assert_eq!(merged.len(), 4);

        let two = merged.iter().find(|c| c.computer_name == "L-two-SD").unwrap();
        assert_eq!(two.id, "c");
        assert_eq!(two.label_id.as_deref(), Some("mine"));
        assert!(two.pinned, "pinning is carried over");
        assert_eq!(two.service_name.as_deref(), Some("Production_Cosmic"));

        let three = merged.iter().find(|c| c.computer_name == "L-three-SD").unwrap();
        assert_eq!(three.label_id, None);

        let four = merged.iter().find(|c| c.computer_name == "L-four-SD").unwrap();
        assert_ne!(four.id, "a", "a colliding id must be replaced");
    }

    #[test]
    fn importing_twice_adds_nothing() {
        let incoming = || vec![conn("b", "L-two-SD", None), conn("c", "L-three-SD", None)];
        let none = HashMap::new();
        let (once, added, _) = merge_connections(Vec::new(), incoming(), &none, &none);
        assert_eq!(added, 2);
        let (twice, added, skipped) = merge_connections(once.clone(), incoming(), &none, &none);
        assert_eq!((added, skipped), (0, 2));
        assert_eq!(twice.len(), once.len());
    }

    #[test]
    fn modules_and_property_sets_merge_by_name() {
        let module = |id: &str, name: &str| Module {
            id: id.into(),
            name: name.into(),
            remote_path: r"D:\x\a.jar".into(),
        };
        let (merged, added) = merge_modules(
            vec![module("1", "billing")],
            vec![module("2", "BILLING"), module("3", "referral"), module("4", "")],
        );
        assert_eq!((merged.len(), added), (2, 1));

        let set = |id: &str, name: &str| PropertySet {
            id: id.into(),
            name: name.into(),
            content: "a=b".into(),
        };
        let (merged, added) =
            merge_property_sets(vec![set("1", "Skip DB")], vec![set("2", "skip db"), set("3", "Other")]);
        assert_eq!((merged.len(), added), (2, 1));
    }

    #[test]
    fn reads_remote_desktops_bare_json_files() {
        let dir = std::env::temp_dir().join(format!("wingui-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("connections.json"),
            r#"[{"id":"x","label":"Dev","computerName":"L-abc-SD","pinned":true,"serviceName":"Production_Cosmic"}]"#,
        )
        .unwrap();
        let conns: Vec<Connection> = source(&dir, "connections.json");
        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].computer_name, "L-abc-SD");
        assert!(conns[0].pinned);
        // A missing file is simply empty, and reading never alters the source.
        assert!(source::<Label>(&dir, "labels.json").is_empty());
        assert!(dir.join("connections.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
