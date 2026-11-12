//! Persistent data models (connections, modules, property sets) and their
//! stores. Pure data + validation — no transport, no UI.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::storage::JsonStore;

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

/// A single saved remote host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    /// Stable unique id (UUID v4), generated on creation.
    pub id: String,
    /// Friendly name shown on the tile; falls back to the computer name in the UI.
    pub label: String,
    /// The host to connect to, e.g. `L-abc123-SD`.
    pub computer_name: String,
    /// Optional username override. `None`/blank uses the global account.
    /// Free-form: `user`, `DOMAIN\\user`, or `user@domain` are all valid.
    #[serde(default)]
    pub username: Option<String>,
    /// Windows service name of this environment's JBoss server (e.g.
    /// `Production_Cosmic`), used to restart it over WinRM. `None`/blank means no
    /// restart target is configured. This is the *service* name, not the display
    /// name shown in the Services app.
    #[serde(default)]
    pub service_name: Option<String>,
    /// Optional override of the server install root on this host (blank = the
    /// documented default, `D:\Cambio\Production`). See [`super::env_layout`].
    #[serde(default)]
    pub server_root: Option<String>,
    /// Optional override of the client application root on this host (blank =
    /// the documented default). See [`super::env_layout`].
    #[serde(default)]
    pub client_root: Option<String>,
    /// Whether this connection is pinned to the top of the list. Pinned
    /// connections are shown in a dedicated section above the label groups.
    /// Purely presentational — it changes ordering, not behavior.
    #[serde(default)]
    pub pinned: bool,
    /// Id of the [`Label`] group this connection belongs to, or `None` when it
    /// is ungrouped. A dangling id (its label was deleted) is treated as
    /// ungrouped by the front end.
    #[serde(default)]
    pub label_id: Option<String>,
    /// Id of the [`Category`] this connection belongs to, or `None` when it is
    /// uncategorized. Categories are a second, independent grouping axis from
    /// labels (see [`Category`]); a dangling id is treated as uncategorized by
    /// the front end.
    #[serde(default)]
    pub category_id: Option<String>,
}

impl Connection {
    /// The username to authenticate with, given the global account username.
    /// Precedence: this connection's explicit override, then the global account,
    /// then the logged-in Windows account (`DOMAIN\user` when a domain is
    /// present). Used for both the `.rdp` file and WinRM (deploy/restart) so the
    /// stored credential and the connection always agree.
    pub fn resolve_username(&self, global_username: Option<&str>) -> String {
        if let Some(u) = self.username.as_deref().filter(|u| !u.is_empty()) {
            return u.to_string();
        }
        if let Some(g) = global_username.filter(|g| !g.is_empty()) {
            return g.to_string();
        }
        let user = std::env::var("USERNAME").unwrap_or_default();
        match std::env::var("USERDOMAIN") {
            Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
            _ => user,
        }
    }
}

/// Fields supplied by a front end when creating or editing a connection.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInput {
    pub label: String,
    pub computer_name: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub server_root: Option<String>,
    #[serde(default)]
    pub client_root: Option<String>,
    /// Label group to assign. `None`/blank = ungrouped. Pinning is a separate,
    /// dedicated command (not part of the edit form), so it has no field here.
    #[serde(default)]
    pub label_id: Option<String>,
    /// Category to assign. `None`/blank = uncategorized.
    #[serde(default)]
    pub category_id: Option<String>,
}

/// Reject a value carrying control characters (newlines, NUL, …).
///
/// These fields end up in generated artifacts and OS calls where a line break
/// changes meaning — most importantly the `.rdp` file, whose settings are
/// newline-separated, so a computer name or username with a `\r\n` in it could
/// smuggle extra RDP settings (drive redirection, an alternate shell) into a
/// file the app signs and launches. They also become a Credential Manager
/// target name and a WinRM `-ComputerName`.
fn reject_control_chars(value: &str, field: &str) -> Result<(), String> {
    if value.chars().any(char::is_control) {
        return Err(format!("{field} must not contain line breaks or control characters."));
    }
    Ok(())
}

/// Trim and validate connection input; returns the normalized fields.
pub fn normalize_connection(input: ConnectionInput) -> Result<ConnectionInput, String> {
    let computer_name = input.computer_name.trim().to_string();
    if computer_name.is_empty() {
        return Err("Computer name is required.".into());
    }
    reject_control_chars(&computer_name, "Computer name")?;
    if let Some(u) = input.username.as_deref() {
        reject_control_chars(u, "Username")?;
    }
    if let Some(s) = input.service_name.as_deref() {
        reject_control_chars(s, "Service name")?;
    }
    let label = match input.label.trim() {
        "" => computer_name.clone(),
        l => l.to_string(),
    };
    let trim_opt = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let trim_root = |v: Option<String>| {
        v.map(|s| s.trim().trim_end_matches(['\\', '/']).to_string())
            .filter(|s| !s.is_empty())
    };
    Ok(ConnectionInput {
        label,
        computer_name,
        username: trim_opt(input.username),
        service_name: trim_opt(input.service_name),
        server_root: trim_root(input.server_root),
        client_root: trim_root(input.client_root),
        label_id: trim_opt(input.label_id),
        category_id: trim_opt(input.category_id),
    })
}

impl Connection {
    /// Build a new connection from validated input.
    pub fn from_input(n: ConnectionInput) -> Self {
        Connection {
            id: Uuid::new_v4().to_string(),
            label: n.label,
            computer_name: n.computer_name,
            username: n.username,
            service_name: n.service_name,
            server_root: n.server_root,
            client_root: n.client_root,
            pinned: false,
            label_id: n.label_id,
            category_id: n.category_id,
        }
    }

    /// Overwrite the editable fields from validated input. Leaves `pinned`
    /// untouched — pinning is toggled by its own command, not the edit form.
    pub fn apply_input(&mut self, n: ConnectionInput) {
        self.label = n.label;
        self.computer_name = n.computer_name;
        self.username = n.username;
        self.service_name = n.service_name;
        self.server_root = n.server_root;
        self.client_root = n.client_root;
        self.label_id = n.label_id;
        self.category_id = n.category_id;
    }
}

/// The `connections.json` store.
pub fn connections_store() -> Result<JsonStore<Vec<Connection>>, String> {
    JsonStore::in_config_dir("connections.json")
}

/// All saved connections; empty when the store is missing or unreadable.
pub fn load_connections() -> Vec<Connection> {
    connections_store().map(|s| s.load()).unwrap_or_default()
}

/// Find a connection by id, by computer name, or by label (case-insensitive,
/// in that precedence order). Name lookup is what the CLI uses.
pub fn find_connection<'a>(list: &'a [Connection], key: &str) -> Option<&'a Connection> {
    list.iter()
        .find(|c| c.id == key)
        .or_else(|| {
            list.iter()
                .find(|c| c.computer_name.eq_ignore_ascii_case(key))
        })
        .or_else(|| list.iter().find(|c| c.label.eq_ignore_ascii_case(key)))
}

// ---------------------------------------------------------------------------
// Labels (connection groups)
// ---------------------------------------------------------------------------

/// A user-defined group for connections, shown as a titled section with a
/// colored accent. Labels are stored as an ordered list — the vector order *is*
/// the display order, so reordering is just moving elements (no separate rank
/// field to keep in sync).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub id: String,
    /// Unique (case-insensitive), human-friendly group name, e.g. `Production`.
    pub name: String,
    /// Accent color as a `#rrggbb` hex string. Rendered as a dot/section accent.
    pub color: String,
}

/// Fields supplied by a front end when creating/editing a label.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelInput {
    pub name: String,
    pub color: String,
}

/// Fallback accent used when a front end sends a blank/invalid color.
pub const DEFAULT_LABEL_COLOR: &str = "#3b82f6";

/// Normalize a `#rgb`/`#rrggbb` hex color to lowercase `#rrggbb`; returns the
/// default when the input is blank or malformed (never an error — a bad color
/// should not block saving a label).
fn normalize_color(raw: &str) -> String {
    let s = raw.trim();
    let hex = s.strip_prefix('#').unwrap_or(s);
    let valid = |h: &str| h.chars().all(|c| c.is_ascii_hexdigit());
    match hex.len() {
        6 if valid(hex) => format!("#{}", hex.to_ascii_lowercase()),
        3 if valid(hex) => {
            // Expand shorthand `#abc` → `#aabbcc`.
            let mut out = String::from("#");
            for c in hex.chars() {
                out.push(c.to_ascii_lowercase());
                out.push(c.to_ascii_lowercase());
            }
            out
        }
        _ => DEFAULT_LABEL_COLOR.to_string(),
    }
}

/// Trim and validate label input; returns the normalized `(name, color)`.
pub fn normalize_label(input: LabelInput) -> Result<(String, String), String> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err("Label name is required.".into());
    }
    Ok((name, normalize_color(&input.color)))
}

/// Reject a name that collides (case-insensitively) with another label's.
pub fn ensure_unique_label_name(
    labels: &[Label],
    name: &str,
    skip_id: Option<&str>,
) -> Result<(), String> {
    let clash = labels
        .iter()
        .any(|l| Some(l.id.as_str()) != skip_id && l.name.eq_ignore_ascii_case(name));
    if clash {
        return Err(format!("A label named \"{name}\" already exists."));
    }
    Ok(())
}

/// The `labels.json` store.
pub fn labels_store() -> Result<JsonStore<Vec<Label>>, String> {
    JsonStore::in_config_dir("labels.json")
}

/// All configured labels in display order; empty when missing or unreadable.
pub fn load_labels() -> Vec<Label> {
    labels_store().map(|s| s.load()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Categories (a second, independent grouping axis)
// ---------------------------------------------------------------------------

/// A user-defined category for connections. Categories are a *second* grouping
/// axis, independent of [`Label`]s: a connection can carry both a category and a
/// label, and the front end lets the user navigate the two as a tree
/// (category → label). Stored as an ordered list — the vector order *is* the
/// display order, exactly like labels. Unlike labels, categories carry no
/// color — they are shown with a folder icon in the navigation tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    /// Unique (case-insensitive), human-friendly category name, e.g. `Cloud`.
    pub name: String,
}

/// Fields supplied by a front end when creating/editing a category.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInput {
    pub name: String,
}

/// Trim and validate category input; returns the normalized name.
pub fn normalize_category(input: CategoryInput) -> Result<String, String> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err("Category name is required.".into());
    }
    Ok(name)
}

/// Reject a name that collides (case-insensitively) with another category's.
pub fn ensure_unique_category_name(
    categories: &[Category],
    name: &str,
    skip_id: Option<&str>,
) -> Result<(), String> {
    let clash = categories
        .iter()
        .any(|c| Some(c.id.as_str()) != skip_id && c.name.eq_ignore_ascii_case(name));
    if clash {
        return Err(format!("A category named \"{name}\" already exists."));
    }
    Ok(())
}

/// The `categories.json` store.
pub fn categories_store() -> Result<JsonStore<Vec<Category>>, String> {
    JsonStore::in_config_dir("categories.json")
}

/// All configured categories in display order; empty when missing or unreadable.
pub fn load_categories() -> Vec<Category> {
    categories_store().map(|s| s.load()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Modules (manual JAR deployment targets)
// ---------------------------------------------------------------------------

/// A deployable module and where its JAR lives on the remote host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub id: String,
    /// Friendly name shown in the picker, e.g. `billing-core`.
    pub name: String,
    /// Full destination path of the JAR on the remote host, including filename,
    /// e.g. `C:\jboss\modules\com\acme\billing\main\billing-core.jar`.
    pub remote_path: String,
}

/// Fields supplied by a front end when creating/editing a module mapping.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleInput {
    pub name: String,
    pub remote_path: String,
}

/// Trim and validate module input.
pub fn normalize_module(input: ModuleInput) -> Result<(String, String), String> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err("Module name is required.".into());
    }
    let remote_path = input.remote_path.trim().to_string();
    if remote_path.is_empty() {
        return Err("Remote path is required.".into());
    }
    Ok((name, remote_path))
}

/// The `modules.json` store.
pub fn modules_store() -> Result<JsonStore<Vec<Module>>, String> {
    JsonStore::in_config_dir("modules.json")
}

/// All configured modules; empty when the store is missing or unreadable.
pub fn load_modules() -> Vec<Module> {
    modules_store().map(|s| s.load()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Property sets
// ---------------------------------------------------------------------------

/// A reusable, named block of properties to append to `environment.properties`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropertySet {
    pub id: String,
    /// Unique, human-friendly name shown in the picker, e.g. `Skip DB validation`.
    pub name: String,
    /// The raw property lines (one `key=value` per line) to add.
    pub content: String,
}

/// Fields supplied by a front end when creating/editing a property set.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropertySetInput {
    pub name: String,
    pub content: String,
}

/// Trim and validate property-set input.
pub fn normalize_property_set(input: PropertySetInput) -> Result<(String, String), String> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err("Set name is required.".into());
    }
    let content = input.content.trim().to_string();
    if content.is_empty() {
        return Err("Add at least one property line.".into());
    }
    Ok((name, content))
}

/// Reject a name that collides (case-insensitively) with another set's.
pub fn ensure_unique_set_name(
    sets: &[PropertySet],
    name: &str,
    skip_id: Option<&str>,
) -> Result<(), String> {
    let clash = sets
        .iter()
        .any(|s| Some(s.id.as_str()) != skip_id && s.name.eq_ignore_ascii_case(name));
    if clash {
        return Err(format!("A property set named \"{name}\" already exists."));
    }
    Ok(())
}

/// The `property_sets.json` store.
pub fn property_sets_store() -> Result<JsonStore<Vec<PropertySet>>, String> {
    JsonStore::in_config_dir("property_sets.json")
}

/// All configured property sets; empty when the store is missing or unreadable.
pub fn load_property_sets() -> Vec<PropertySet> {
    property_sets_store().map(|s| s.load()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(id: &str, name: &str, label: &str) -> Connection {
        Connection {
            id: id.into(),
            label: label.into(),
            computer_name: name.into(),
            username: None,
            service_name: None,
            server_root: None,
            client_root: None,
            pinned: false,
            label_id: None,
            category_id: None,
        }
    }

    #[test]
    fn find_connection_prefers_id_then_name_then_label() {
        let list = vec![
            conn("id-1", "L-AAA-SD", "alpha"),
            conn("id-2", "L-BBB-SD", "id-1-lookalike"),
            conn("id-3", "L-CCC-SD", "l-bbb-sd"),
        ];
        assert_eq!(find_connection(&list, "id-1").unwrap().id, "id-1");
        assert_eq!(find_connection(&list, "l-bbb-sd").unwrap().id, "id-2");
        assert_eq!(find_connection(&list, "ALPHA").unwrap().id, "id-1");
        assert!(find_connection(&list, "nope").is_none());
    }

    #[test]
    fn normalize_connection_defaults_label_and_trims_roots() {
        let n = normalize_connection(ConnectionInput {
            label: "  ".into(),
            computer_name: " L-abc-SD ".into(),
            username: Some(" ".into()),
            service_name: Some(" Production_Cosmic ".into()),
            server_root: Some(r"E:\Custom\Root\ ".into()),
            client_root: Some("".into()),
            label_id: Some("  lbl-1 ".into()),
            category_id: Some("  cat-1 ".into()),
        })
        .unwrap();
        assert_eq!(n.label, "L-abc-SD");
        assert_eq!(n.computer_name, "L-abc-SD");
        assert_eq!(n.username, None);
        assert_eq!(n.service_name.as_deref(), Some("Production_Cosmic"));
        assert_eq!(n.server_root.as_deref(), Some(r"E:\Custom\Root"));
        assert_eq!(n.client_root, None);
        assert_eq!(n.label_id.as_deref(), Some("lbl-1"));
        assert_eq!(n.category_id.as_deref(), Some("cat-1"));
    }

    #[test]
    fn normalize_connection_rejects_control_characters() {
        let with = |computer_name: &str, username: Option<&str>| {
            normalize_connection(ConnectionInput {
                label: "l".into(),
                computer_name: computer_name.into(),
                username: username.map(String::from),
                service_name: None,
                server_root: None,
                client_root: None,
                label_id: None,
                category_id: None,
            })
        };
        // A newline here would inject extra settings into the generated .rdp.
        assert!(with("L-abc-SD\r\nalternate shell:s:cmd.exe", None).is_err());
        assert!(with("L-abc-SD", Some("user\nusername:s:admin")).is_err());
        assert!(with("L-abc-SD", Some("DOMAIN\\user")).is_ok());
    }

    #[test]
    fn normalize_label_validates_name_and_color() {
        // Blank name is rejected.
        assert!(normalize_label(LabelInput {
            name: "  ".into(),
            color: "#fff".into(),
        })
        .is_err());

        // Shorthand hex expands; surrounding space is trimmed.
        let (name, color) = normalize_label(LabelInput {
            name: "  Production ".into(),
            color: " #ABC ".into(),
        })
        .unwrap();
        assert_eq!(name, "Production");
        assert_eq!(color, "#aabbcc");

        // A malformed color falls back to the default rather than erroring.
        let (_, color) = normalize_label(LabelInput {
            name: "Staging".into(),
            color: "not-a-color".into(),
        })
        .unwrap();
        assert_eq!(color, DEFAULT_LABEL_COLOR);
    }

    #[test]
    fn ensure_unique_label_name_is_case_insensitive_and_skips_self() {
        let labels = vec![
            Label {
                id: "l1".into(),
                name: "Production".into(),
                color: "#111111".into(),
            },
            Label {
                id: "l2".into(),
                name: "Staging".into(),
                color: "#222222".into(),
            },
        ];
        assert!(ensure_unique_label_name(&labels, "PRODUCTION", None).is_err());
        // Editing l1 keeping its own name is allowed.
        assert!(ensure_unique_label_name(&labels, "Production", Some("l1")).is_ok());
        assert!(ensure_unique_label_name(&labels, "QA", None).is_ok());
    }
}
