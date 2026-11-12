//! Environments (saved connections) and their two grouping axes, labels and
//! categories: CRUD plus the backend-computed class and feature set.
//!
//! Ported from Remote Desktop's `commands.rs` (commit 76f65f0).

use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::AppRuntime;
use crate::core::env_class::{self, EnvClass, EnvFeatures, Side};
use crate::core::error::{codes, AppError, AppResult};
use crate::core::model::{
    categories_store, connections_store, ensure_unique_category_name, ensure_unique_label_name,
    labels_store, load_categories, load_connections, load_labels, normalize_category,
    normalize_connection, normalize_label, Category, CategoryInput, Connection, ConnectionInput,
    Label, LabelInput,
};

fn no_connection(id: &str) -> AppError {
    AppError::new(codes::NO_CONNECTION, format!("No connection with id {id}"))
}

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

/// A connection plus its backend-computed environment class and feature set.
/// The frontend renders these verbatim instead of re-deriving the safety
/// rules in TypeScript.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    #[serde(flatten)]
    pub connection: Connection,
    pub env_class: EnvClass,
    /// Short human description of the class, e.g. `public server`.
    pub env_class_label: &'static str,
    pub features: EnvFeatures,
    pub side: Side,
    /// Nothing that changes the host is allowed.
    pub read_only: bool,
    /// The environment may be opened at all.
    pub openable: bool,
}

impl From<Connection> for ConnectionView {
    fn from(connection: Connection) -> Self {
        let class = env_class::env_class(&connection.computer_name);
        let features = env_class::features(class);
        ConnectionView {
            env_class: class,
            env_class_label: class.describe(),
            side: env_class::side(class),
            read_only: features.is_read_only(),
            openable: !features.is_empty(),
            features,
            connection,
        }
    }
}

#[tauri::command]
pub fn list_connections() -> Vec<ConnectionView> {
    load_connections().into_iter().map(Into::into).collect()
}

#[tauri::command]
pub fn add_connection(input: ConnectionInput) -> AppResult<ConnectionView> {
    let n = normalize_connection(input)?;
    let store = connections_store()?;
    let mut list = store.load();
    if list
        .iter()
        .any(|c| c.computer_name.eq_ignore_ascii_case(&n.computer_name))
    {
        return Err(format!("{} is already saved.", n.computer_name).into());
    }
    let conn = Connection::from_input(n);
    list.push(conn.clone());
    store.save(&list)?;
    Ok(conn.into())
}

#[tauri::command]
pub fn update_connection(
    rt: State<AppRuntime>,
    id: String,
    input: ConnectionInput,
) -> AppResult<ConnectionView> {
    let n = normalize_connection(input)?;
    let store = connections_store()?;
    let mut list = store.load();
    if list
        .iter()
        .any(|c| c.id != id && c.computer_name.eq_ignore_ascii_case(&n.computer_name))
    {
        return Err(format!("{} is already saved.", n.computer_name).into());
    }
    let conn = list
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| no_connection(&id))?;
    let retargeted = !conn.computer_name.eq_ignore_ascii_case(&n.computer_name)
        || conn.username != n.username;
    conn.apply_input(n);
    let updated = conn.clone();
    store.save(&list)?;
    // The held session belongs to the old host/identity.
    if retargeted {
        rt.deps.pool.close(&id);
    }
    Ok(updated.into())
}

#[tauri::command]
pub fn delete_connection(rt: State<AppRuntime>, id: String) -> AppResult<()> {
    let store = connections_store()?;
    let mut list = store.load();
    let Some(conn) = list.iter().find(|c| c.id == id) else {
        return Err(no_connection(&id));
    };
    // Remove any derived per-host RDP credential cached for silent login (the
    // global account in Settings is left untouched).
    let _ = crate::core::credentials::delete(&conn.computer_name);
    list.retain(|c| c.id != id);
    store.save(&list)?;
    rt.deps.pool.close(&id);
    Ok(())
}

/// Toggle a connection's pinned state. A dedicated command (rather than going
/// through the edit form) so pinning stays a one-click tile action.
#[tauri::command]
pub fn set_connection_pinned(id: String, pinned: bool) -> AppResult<ConnectionView> {
    let store = connections_store()?;
    let mut list = store.load();
    let conn = list
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| no_connection(&id))?;
    conn.pinned = pinned;
    let updated = conn.clone();
    store.save(&list)?;
    Ok(updated.into())
}

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_labels() -> Vec<Label> {
    load_labels()
}

#[tauri::command]
pub fn add_label(input: LabelInput) -> AppResult<Label> {
    let (name, color) = normalize_label(input)?;
    let store = labels_store()?;
    let mut list = store.load();
    ensure_unique_label_name(&list, &name, None)?;
    let label = Label {
        id: Uuid::new_v4().to_string(),
        name,
        color,
    };
    list.push(label.clone());
    store.save(&list)?;
    Ok(label)
}

#[tauri::command]
pub fn update_label(id: String, input: LabelInput) -> AppResult<Label> {
    let (name, color) = normalize_label(input)?;
    let store = labels_store()?;
    let mut list = store.load();
    ensure_unique_label_name(&list, &name, Some(&id))?;
    let label = list
        .iter_mut()
        .find(|l| l.id == id)
        .ok_or_else(|| format!("No label with id {id}"))?;
    label.name = name;
    label.color = color;
    let updated = label.clone();
    store.save(&list)?;
    Ok(updated)
}

/// Delete a label and unassign every connection that referenced it, so no tile
/// is left pointing at a group that no longer exists.
#[tauri::command]
pub fn delete_label(id: String) -> AppResult<()> {
    let store = labels_store()?;
    let mut list = store.load();
    let before = list.len();
    list.retain(|l| l.id != id);
    if list.len() == before {
        return Err(format!("No label with id {id}").into());
    }
    store.save(&list)?;

    let conn_store = connections_store()?;
    let mut conns = conn_store.load();
    let mut touched = false;
    for c in conns.iter_mut() {
        if c.label_id.as_deref() == Some(id.as_str()) {
            c.label_id = None;
            touched = true;
        }
    }
    if touched {
        conn_store.save(&conns)?;
    }
    Ok(())
}

/// Persist a new display order. `ordered_ids` must be exactly the current set
/// of ids (a permutation); anything else is rejected so a stale front end
/// can't silently drop or duplicate an entry.
fn reorder<T: Clone>(
    current: &[T],
    ordered_ids: &[String],
    id_of: impl Fn(&T) -> &str,
    what: &str,
) -> AppResult<Vec<T>> {
    if ordered_ids.len() != current.len() {
        return Err(format!("The {what} order is out of date; reload and retry.").into());
    }
    let mut reordered: Vec<T> = Vec::with_capacity(current.len());
    for id in ordered_ids {
        let item = current
            .iter()
            .find(|x| id_of(x) == id)
            .ok_or_else(|| format!("Unknown {what} id in new order: {id}"))?;
        if reordered.iter().any(|x| id_of(x) == id) {
            return Err(format!("Duplicate {what} id in new order: {id}").into());
        }
        reordered.push(item.clone());
    }
    Ok(reordered)
}

#[tauri::command]
pub fn reorder_labels(ordered_ids: Vec<String>) -> AppResult<Vec<Label>> {
    let store = labels_store()?;
    let reordered = reorder(&store.load(), &ordered_ids, |l| &l.id, "label")?;
    store.save(&reordered)?;
    Ok(reordered)
}

// ---------------------------------------------------------------------------
// Categories — deliberately shaped like labels: a parallel grouping axis.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_categories() -> Vec<Category> {
    load_categories()
}

#[tauri::command]
pub fn add_category(input: CategoryInput) -> AppResult<Category> {
    let name = normalize_category(input)?;
    let store = categories_store()?;
    let mut list = store.load();
    ensure_unique_category_name(&list, &name, None)?;
    let category = Category {
        id: Uuid::new_v4().to_string(),
        name,
    };
    list.push(category.clone());
    store.save(&list)?;
    Ok(category)
}

#[tauri::command]
pub fn update_category(id: String, input: CategoryInput) -> AppResult<Category> {
    let name = normalize_category(input)?;
    let store = categories_store()?;
    let mut list = store.load();
    ensure_unique_category_name(&list, &name, Some(&id))?;
    let category = list
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("No category with id {id}"))?;
    category.name = name;
    let updated = category.clone();
    store.save(&list)?;
    Ok(updated)
}

/// Delete a category and unassign every connection that referenced it.
#[tauri::command]
pub fn delete_category(id: String) -> AppResult<()> {
    let store = categories_store()?;
    let mut list = store.load();
    let before = list.len();
    list.retain(|c| c.id != id);
    if list.len() == before {
        return Err(format!("No category with id {id}").into());
    }
    store.save(&list)?;

    let conn_store = connections_store()?;
    let mut conns = conn_store.load();
    let mut touched = false;
    for c in conns.iter_mut() {
        if c.category_id.as_deref() == Some(id.as_str()) {
            c.category_id = None;
            touched = true;
        }
    }
    if touched {
        conn_store.save(&conns)?;
    }
    Ok(())
}

#[tauri::command]
pub fn reorder_categories(ordered_ids: Vec<String>) -> AppResult<Vec<Category>> {
    let store = categories_store()?;
    let reordered = reorder(&store.load(), &ordered_ids, |c| &c.id, "category")?;
    store.save(&reordered)?;
    Ok(reordered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(host: &str) -> ConnectionView {
        Connection {
            id: "x".into(),
            label: host.into(),
            computer_name: host.into(),
            username: None,
            service_name: None,
            server_root: None,
            client_root: None,
            pinned: false,
            label_id: None,
            category_id: None,
        }
        .into()
    }

    #[test]
    fn the_view_carries_the_backend_computed_rules() {
        let dev = view("L-abc-SD");
        assert!(dev.openable && !dev.read_only);
        assert_eq!(dev.env_class_label, "private server");

        let public = view("L-abc-EA-7");
        assert!(public.openable && public.read_only);

        let off_limits = view("L-abc-CL-3");
        assert!(!off_limits.openable && off_limits.read_only);

        // The payload the UI receives is flat and camelCased.
        let v = serde_json::to_value(view("L-abc-EA-7")).unwrap();
        assert_eq!(v["computerName"], "L-abc-EA-7");
        assert_eq!(v["envClass"], "publicServer");
        assert_eq!(v["readOnly"], true);
        assert_eq!(v["features"]["fsBrowse"], true);
        assert_eq!(v["features"]["fsWrite"], false);
    }

    #[test]
    fn reorder_requires_an_exact_permutation() {
        fn id(s: &String) -> &str {
            s.as_str()
        }
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let ok = reorder(&items, &["c".into(), "a".into(), "b".into()], id, "label").unwrap();
        assert_eq!(ok, vec!["c", "a", "b"]);
        assert!(reorder(&items, &["a".into(), "b".into()], id, "label").is_err());
        assert!(reorder(&items, &["a".into(), "a".into(), "b".into()], id, "label").is_err());
        assert!(reorder(&items, &["a".into(), "b".into(), "z".into()], id, "label").is_err());
    }
}
