//! Domain models and pure business logic.
//!
//! Keep this module independent from egui and Windows APIs where possible.

#[derive(Debug, Clone, Default)]
pub struct SystemStats {
    pub cpu_usage: f32,
    pub ram_used: u64,
    pub ram_total: u64,
    pub disk_read_kb: u64,
    pub disk_write_kb: u64,
}
