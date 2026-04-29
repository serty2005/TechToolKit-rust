//! Domain models and pure business logic.
//!
//! Keep this module independent from egui and Windows APIs where possible.

pub mod asset_mgr;
pub mod models;

pub use asset_mgr::IikoComponent;
pub use models::{AppCommand, AppEvent, AppTask, SystemStats};
