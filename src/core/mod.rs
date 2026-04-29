//! Domain models and pure business logic.
//!
//! Keep this module independent from egui and Windows APIs where possible.

pub mod models;

pub use models::{AppCommand, AppEvent, AppTask, SystemStats};
