use std::{error::Error, path::Path};

use tokio::{fs::File, io::AsyncWriteExt, sync::mpsc::UnboundedSender};

use crate::core::AppEvent;

type DownloadResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub async fn download_file(
    url: String,
    dest: String,
    tx_event: UnboundedSender<AppEvent>,
) -> DownloadResult<()> {
    let task_name = task_name_from_dest(&dest);
    let client = reqwest::Client::new();
    let mut response = client.get(&url).send().await?.error_for_status()?;
    let total_size = response.content_length();
    let mut downloaded: u64 = 0;
    let mut file = File::create(&dest).await?;

    let _ = tx_event.send(AppEvent::TaskProgress {
        task_name: task_name.clone(),
        progress: 0.0,
        status_text: format!("Подключение к {url}"),
    });

    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        let progress = total_size
            .map(|total| {
                if total == 0 {
                    0.0
                } else {
                    downloaded as f32 / total as f32
                }
            })
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let status_text = match total_size {
            Some(total) => format!(
                "Загружено {:.1}/{:.1} МБ",
                bytes_to_mb(downloaded),
                bytes_to_mb(total)
            ),
            None => format!("Загружено {:.1} МБ", bytes_to_mb(downloaded)),
        };

        let _ = tx_event.send(AppEvent::TaskProgress {
            task_name: task_name.clone(),
            progress,
            status_text,
        });
    }

    file.flush().await?;

    let _ = tx_event.send(AppEvent::TaskProgress {
        task_name: task_name.clone(),
        progress: 1.0,
        status_text: format!("Файл сохранён: {dest}"),
    });
    let _ = tx_event.send(AppEvent::TaskFinished(format!("{task_name}: завершено")));

    Ok(())
}

fn task_name_from_dest(dest: &str) -> String {
    Path::new(dest)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("Скачивание {name}"))
        .unwrap_or_else(|| "Скачивание файла".to_owned())
}

fn bytes_to_mb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}
