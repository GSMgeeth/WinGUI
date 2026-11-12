//! The application core: every piece of business logic, with **no Tauri
//! dependency**.
//!
//! Everything under `core::` can be driven by any front end — the Tauri GUI,
//! a CLI, an AI-agent skill shelling out to that CLI — without change. The
//! rules (inherited from the Remote Desktop app, whose core this is ported
//! from):
//!
//! * **No `tauri::` imports anywhere in this tree.** Front ends adapt at their
//!   edge.
//! * Remote features are **plugins**: one [`feature::RemoteFeature`] (or
//!   [`feature::StreamingRemoteFeature`]) implementation each, registered in
//!   the [`feature::FeatureRegistry`] built by [`features::build_registry`].
//!   Every front end invokes them identically through the registry, which owns
//!   target/credential resolution and the environment-class safety guard —
//!   callers cannot bypass either.
//! * Transport is owned by [`remoting`]; feature modules own only their
//!   operation and message schema.
//! * Environment paths are owned by [`env_layout::EnvLayout`]; the
//!   environment-class safety rules by [`env_class`].
//! * **Every external program is launched through [`exec`]**, which resolves
//!   system tools to their absolute `System32` path.
//! * Errors that cross the boundary are [`error::AppError`]: a stable code the
//!   front end can branch on, plus a message it can show.

pub mod account;
pub mod credentials;
pub mod env_class;
pub mod env_layout;
pub mod error;
pub mod exec;
pub mod feature;
pub mod features;
pub mod import;
pub mod model;
pub mod remoting;
pub mod storage;
pub mod winrm;
