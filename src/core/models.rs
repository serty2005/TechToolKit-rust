use std::path::Path;

use crate::core::asset_mgr::IikoComponent;

#[derive(Debug, Clone)]
pub enum AppTask {
    DownloadFile {
        url: String,
        dest: String,
    },
    DownloadIikoDistribution {
        component: IikoComponent,
        version: String,
        dest_dir: String,
    },
}

impl AppTask {
    pub fn name(&self) -> String {
        match self {
            Self::DownloadFile { dest, .. } => Path::new(dest)
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| format!("Скачивание {name}"))
                .unwrap_or_else(|| "Скачивание файла".to_owned()),
            Self::DownloadIikoDistribution {
                component, version, ..
            } => format!("Скачивание {} {}", component.title(), version),
        }
    }
}

#[derive(Debug, Clone)]
pub enum AppCommand {
    TestBackend,
    RefreshIikoVersions,
    EnqueueTask(AppTask),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    BackendReady,
    StatusChanged(String),
    ProgressChanged(f32),
    TaskProgress {
        task_name: String,
        progress: f32,
        status_text: String,
    },
    IikoVersionsLoaded(Vec<String>),
    TaskFinished(String),
    ResourceUpdate(SystemStats),
    BackendStopped,
    Error(String),
}

#[derive(Debug, Clone, Default)]
pub struct SystemStats {
    pub cpu_usage: f32,
    pub ram_used: u64,
    pub ram_total: u64,
    pub disk_read_kb: u64,
    pub disk_write_kb: u64,
}
