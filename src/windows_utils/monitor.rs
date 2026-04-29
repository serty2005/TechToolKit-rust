use std::time::Duration;

use sysinfo::{Disks, Networks, System};
use tokio::sync::mpsc::UnboundedSender;

use crate::{AppEvent, core::SystemStats};

pub async fn start_system_monitor(tx: UnboundedSender<AppEvent>) {
    let mut sys = System::new();
    let mut disks = Disks::new_with_refreshed_list();
    let mut networks = Networks::new_with_refreshed_list();

    sys.refresh_cpu_usage();
    sys.refresh_memory();

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;

        sys.refresh_cpu_usage();
        sys.refresh_memory();
        disks.refresh(true);
        networks.refresh(true);

        let (disk_read_kb, disk_write_kb) = disks.list().iter().fold((0, 0), |acc, disk| {
            let usage = disk.usage();
            (
                acc.0 + usage.read_bytes / 1024,
                acc.1 + usage.written_bytes / 1024,
            )
        });

        let stats = SystemStats {
            cpu_usage: sys.global_cpu_usage().clamp(0.0, 100.0),
            ram_used: sys.used_memory(),
            ram_total: sys.total_memory(),
            disk_read_kb,
            disk_write_kb,
        };

        if tx.send(AppEvent::ResourceUpdate(stats)).is_err() {
            break;
        }
    }
}
