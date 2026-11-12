//! The feature plugins. Each module owns one capability: its worker operation
//! (or script body), its argument/result schema, and its result parsing —
//! nothing about sessions, credentials or safety, which the registry owns.
//!
//! Adding a feature: new module here implementing
//! [`RemoteFeature`](crate::core::feature::RemoteFeature) → one line in
//! [`build_registry`] → a thin command in `commands/` → UI.

pub mod fs;
pub mod session;

use crate::core::feature::FeatureRegistry;

/// Stable feature ids — the registry keys, and later the CLI subcommand names.
pub mod ids {
    pub const SESSION_OPEN: &str = "session.open";
    pub const FS_DRIVES: &str = "fs.drives";
}

/// Every feature the app knows, registered once at start-up.
pub fn build_registry() -> FeatureRegistry {
    let mut registry = FeatureRegistry::default();
    registry.register(Box::new(session::SessionOpenFeature));
    registry.register(Box::new(fs::DrivesFeature));
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env_class::Capability;

    #[test]
    fn registry_lists_the_step_one_features() {
        let registry = build_registry();
        let listed: Vec<&str> = registry.list().iter().map(|f| f.id).collect();
        assert_eq!(listed, vec![ids::FS_DRIVES, ids::SESSION_OPEN]);
        assert!(registry.list().iter().all(|f| !f.mutating && !f.streaming));
        assert_eq!(registry.capability_of(ids::FS_DRIVES), Some(Capability::FsBrowse));
        assert_eq!(registry.capability_of("nope"), None);
    }
}
