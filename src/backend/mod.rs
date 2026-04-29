//! Async backend workers and command handlers.
//!
//! Future goMH migration targets:
//! - task queue and long-running operations;
//! - downloads and archive extraction;
//! - service orchestration and background monitoring.

pub mod downloader;

use std::time::Duration;

use crate::{
    CommandReceiver, EventSender,
    core::{
        AppCommand, AppEvent, AppTask,
        asset_mgr::{self, IikoDistribution},
    },
};

pub async fn backend_loop(mut rx_cmd: CommandReceiver, tx_event: EventSender) {
    while let Some(command) = rx_cmd.recv().await {
        match command {
            AppCommand::TestBackend => run_backend_test(&tx_event).await,
            AppCommand::RefreshIikoVersions => refresh_iiko_versions(tx_event.clone()),
            AppCommand::EnqueueTask(task) => enqueue_task(task, tx_event.clone()),
            AppCommand::Shutdown => {
                let _ = tx_event.send(AppEvent::BackendStopped);
                break;
            }
        }
    }
}

fn enqueue_task(task: AppTask, tx_event: EventSender) {
    let task_name = task.name();
    let _ = tx_event.send(AppEvent::TaskProgress {
        task_name: task_name.clone(),
        progress: 0.0,
        status_text: "Задача добавлена в очередь".to_owned(),
    });

    tokio::spawn(async move {
        match task {
            AppTask::DownloadFile { url, dest } => {
                if let Err(error) = downloader::download_file(url, dest, tx_event.clone()).await {
                    let _ = tx_event.send(AppEvent::Error(format!("{task_name}: {error}")));
                }
            }
            AppTask::DownloadIikoDistribution {
                component,
                version,
                dest_dir,
            } => {
                let distribution = IikoDistribution::new(component, version);
                if let Err(error) = asset_mgr::download_iiko_distribution(
                    distribution,
                    dest_dir.into(),
                    tx_event.clone(),
                )
                .await
                {
                    let _ = tx_event.send(AppEvent::Error(format!("{task_name}: {error}")));
                }
            }
        }
    });
}

fn refresh_iiko_versions(tx_event: EventSender) {
    tokio::spawn(async move {
        let _ = tx_event.send(AppEvent::StatusChanged(
            "Получение списка версий iiko с FTP...".to_owned(),
        ));

        match asset_mgr::fetch_iiko_versions().await {
            Ok(versions) => {
                let count = versions.len();
                let _ = tx_event.send(AppEvent::IikoVersionsLoaded(versions));
                let _ = tx_event.send(AppEvent::StatusChanged(format!(
                    "Загружено версий iiko: {count}"
                )));
            }
            Err(error) => {
                let _ = tx_event.send(AppEvent::Error(format!(
                    "Не удалось получить список версий iiko: {error}"
                )));
            }
        }
    });
}

async fn run_backend_test(tx_event: &EventSender) {
    let _ = tx_event.send(AppEvent::StatusChanged(
        "Backend: тестовая задача запущена".to_owned(),
    ));

    for step in 0..=10 {
        let progress = step as f32 / 10.0;
        let _ = tx_event.send(AppEvent::ProgressChanged(progress));
        let _ = tx_event.send(AppEvent::StatusChanged(format!(
            "Backend: обработка {step}/10"
        )));
        tokio::time::sleep(Duration::from_millis(180)).await;
    }

    let _ = tx_event.send(AppEvent::TaskFinished(
        "Backend: тестовая задача завершена".to_owned(),
    ));
}
