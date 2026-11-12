//! Per-connection environment paths — the one source of truth for where things
//! live on a remote host.
//!
//! Previously `D:\Cambio\Production\…` was hardcoded independently in the log,
//! deploy-status, and properties features (silently assuming Production).
//! Every remote feature now derives its paths from an [`EnvLayout`], which is
//! the documented default unless the connection carries an override.

use serde::Serialize;

use super::model::Connection;

/// Default server install root — the only place this path is written.
pub const DEFAULT_SERVER_ROOT: &str = r"D:\Cambio\Production";
/// Default client application root.
pub const DEFAULT_CLIENT_ROOT: &str = r"D:\CosmicClient\Client\Application";

/// The filesystem layout of one environment. Two roots are stored; everything
/// else is derived.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvLayout {
    pub server_root: String,
    pub client_root: String,
}

impl Default for EnvLayout {
    fn default() -> Self {
        EnvLayout {
            server_root: DEFAULT_SERVER_ROOT.to_string(),
            client_root: DEFAULT_CLIENT_ROOT.to_string(),
        }
    }
}

impl EnvLayout {
    /// The layout for a connection: its overrides where set, defaults otherwise.
    pub fn for_connection(conn: &Connection) -> Self {
        let default = EnvLayout::default();
        EnvLayout {
            server_root: conn
                .server_root
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or(default.server_root),
            client_root: conn
                .client_root
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or(default.client_root),
        }
    }

    /// The JBoss instance root, `<server_root>\instance\Cosmic`.
    pub fn instance_root(&self) -> String {
        format!(r"{}\instance\Cosmic", self.server_root)
    }

    /// The JBoss deployment-scanner folder watched by deploy-status.
    pub fn deployments_dir(&self) -> String {
        format!(r"{}\standalone\deployments", self.instance_root())
    }

    /// The active JBoss log file tailed by the log viewer.
    pub fn log_path(&self) -> String {
        format!(r"{}\var\Cosmic\log\ix3.log", self.server_root)
    }

    /// The environment property file the property-set feature appends to.
    pub fn env_properties(&self) -> String {
        format!(r"{}\env\environment.properties", self.server_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(server_root: Option<&str>, client_root: Option<&str>) -> Connection {
        Connection {
            id: "x".into(),
            label: "x".into(),
            computer_name: "L-x-SD".into(),
            username: None,
            service_name: None,
            server_root: server_root.map(String::from),
            client_root: client_root.map(String::from),
            pinned: false,
            label_id: None,
            category_id: None,
        }
    }

    #[test]
    fn default_layout_matches_the_previous_hardcoded_paths() {
        let l = EnvLayout::default();
        assert_eq!(l.log_path(), r"D:\Cambio\Production\var\Cosmic\log\ix3.log");
        assert_eq!(
            l.deployments_dir(),
            r"D:\Cambio\Production\instance\Cosmic\standalone\deployments"
        );
        assert_eq!(
            l.env_properties(),
            r"D:\Cambio\Production\env\environment.properties"
        );
    }

    #[test]
    fn connection_override_wins() {
        let l = EnvLayout::for_connection(&conn(Some(r"E:\Other"), None));
        assert_eq!(l.server_root, r"E:\Other");
        assert_eq!(l.client_root, DEFAULT_CLIENT_ROOT);
        assert_eq!(l.log_path(), r"E:\Other\var\Cosmic\log\ix3.log");
    }

    #[test]
    fn blank_override_falls_back_to_default() {
        let l = EnvLayout::for_connection(&conn(Some(""), Some("")));
        assert_eq!(l.server_root, DEFAULT_SERVER_ROOT);
        assert_eq!(l.client_root, DEFAULT_CLIENT_ROOT);
    }
}
