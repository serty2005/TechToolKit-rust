use std::{
    error::Error,
    ffi::OsStr,
    net::ToSocketAddrs,
    path::{Path, PathBuf},
    time::Duration,
};

use suppaftp::FtpStream;
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
    sync::mpsc::UnboundedSender,
    time::Instant,
};

use crate::core::AppEvent;

type AssetResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const IIKO_MIN_VERSION: &str = "8.7.6032.0";
const IIKO_RELEASES_PATH: &str = "/release_iiko";
const IIKO_FTP_HOSTS: &[&str] = &["ftp.iiko.ru:21", "ftp2.iiko.ru:21"];
const IIKO_FTP_USER: &str = "partners";
const IIKO_FTP_PASS: &str = "partners#iiko";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IikoComponent {
    RmsBackOffice,
    Front,
}

impl IikoComponent {
    pub const ALL: [Self; 2] = [Self::RmsBackOffice, Self::Front];

    pub fn id(self) -> &'static str {
        match self {
            Self::RmsBackOffice => "iiko_rms_back",
            Self::Front => "iiko_front",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::RmsBackOffice => "iikoRMS BackOffice",
            Self::Front => "iikoFront",
        }
    }

    pub fn installer_file_name(self) -> &'static str {
        match self {
            Self::RmsBackOffice => "Setup.RMS.BackOffice.exe",
            Self::Front => "Setup.Front.exe",
        }
    }

    fn url_template(self) -> &'static str {
        match self {
            Self::RmsBackOffice => {
                "https://downloads.iiko.online/{{VERSION}}/iiko/RMS/BackOffice/Setup.RMS.BackOffice.exe"
            }
            Self::Front => {
                "https://downloads.iiko.online/{{VERSION}}/iiko/RMS/Front/Setup.Front.exe"
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct IikoDistribution {
    pub component: IikoComponent,
    pub version: String,
    pub url: String,
    pub file_name: String,
}

impl IikoDistribution {
    pub fn new(component: IikoComponent, version: impl Into<String>) -> Self {
        let version = version.into();
        let url = component.url_template().replace("{{VERSION}}", &version);
        let file_name = format!(
            "{}-{}-{}",
            component.id(),
            version,
            component.installer_file_name()
        );

        Self {
            component,
            version,
            url,
            file_name,
        }
    }
}

pub async fn fetch_iiko_versions() -> AssetResult<Vec<String>> {
    tokio::task::spawn_blocking(fetch_iiko_versions_blocking).await?
}

fn fetch_iiko_versions_blocking() -> AssetResult<Vec<String>> {
    let mut errors = Vec::new();

    for host in IIKO_FTP_HOSTS {
        match list_iiko_release_names(host) {
            Ok(names) => {
                let mut versions: Vec<String> = names
                    .into_iter()
                    .filter_map(|name| extract_version_name(&name))
                    .filter(|version| !compare_versions(version, IIKO_MIN_VERSION).is_lt())
                    .collect();

                versions.sort_by(|a, b| compare_versions(b, a));
                versions.dedup();

                if versions.is_empty() {
                    errors.push(format!(
                        "{host}: release directory does not contain versions"
                    ));
                    continue;
                }

                return Ok(versions);
            }
            Err(error) => errors.push(format!("{host}: {error}")),
        }
    }

    Err(format!("failed to read iiko FTP releases: {}", errors.join("; ")).into())
}

fn list_iiko_release_names(host: &str) -> AssetResult<Vec<String>> {
    let mut last_error: Option<Box<dyn Error + Send + Sync>> = None;

    for addr in host.to_socket_addrs()? {
        match FtpStream::connect_timeout(addr, Duration::from_secs(10)) {
            Ok(mut ftp) => {
                if let Err(error) = ftp.login(IIKO_FTP_USER, IIKO_FTP_PASS) {
                    let _ = ftp.quit();
                    last_error = Some(Box::new(error));
                    continue;
                }

                let names = ftp.nlst(Some(IIKO_RELEASES_PATH));
                let _ = ftp.quit();
                return names.map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>);
            }
            Err(error) => last_error = Some(Box::new(error)),
        }
    }

    Err(last_error.unwrap_or_else(|| format!("cannot resolve {host}").into()))
}

fn extract_version_name(raw_name: &str) -> Option<String> {
    let name = raw_name
        .trim()
        .trim_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(raw_name)
        .trim();

    if is_four_part_version(name) {
        Some(name.to_owned())
    } else {
        None
    }
}

fn is_four_part_version(value: &str) -> bool {
    let mut count = 0;
    for part in value.split('.') {
        count += 1;
        if part.is_empty() || part.parse::<u32>().is_err() {
            return false;
        }
    }
    count == 4
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts: Vec<u32> = left
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect();
    let right_parts: Vec<u32> = right
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect();
    let max_len = left_parts.len().max(right_parts.len());

    for idx in 0..max_len {
        let left = left_parts.get(idx).copied().unwrap_or(0);
        let right = right_parts.get(idx).copied().unwrap_or(0);
        match left.cmp(&right) {
            std::cmp::Ordering::Equal => {}
            ordering => return ordering,
        }
    }

    std::cmp::Ordering::Equal
}

pub async fn download_iiko_distribution(
    distribution: IikoDistribution,
    dest_dir: PathBuf,
    tx_event: UnboundedSender<AppEvent>,
) -> AssetResult<PathBuf> {
    let dest_path = dest_dir.join(&distribution.file_name);
    download_http_with_progress(
        distribution.url,
        dest_path,
        format!(
            "Скачивание {} {}",
            distribution.component.title(),
            distribution.version
        ),
        tx_event,
    )
    .await
}

pub async fn download_http_with_progress(
    url: String,
    dest_path: PathBuf,
    task_name: String,
    tx_event: UnboundedSender<AppEvent>,
) -> AssetResult<PathBuf> {
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let partial_path = partial_download_path(&dest_path);
    let client = reqwest::Client::new();
    let mut response = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, "rustMH/TechToolKit")
        .send()
        .await?
        .error_for_status()?;
    let total_size = response.content_length();
    let mut downloaded: u64 = 0;
    let mut file = File::create(&partial_path).await?;
    let mut last_progress_event = Instant::now() - Duration::from_secs(1);

    send_progress(&tx_event, &task_name, 0.0, format!("Подключение к {url}"));

    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        if last_progress_event.elapsed() >= Duration::from_millis(200) {
            send_download_progress(&tx_event, &task_name, downloaded, total_size);
            last_progress_event = Instant::now();
        }
    }

    file.flush().await?;
    drop(file);

    if fs::try_exists(&dest_path).await.unwrap_or(false) {
        fs::remove_file(&dest_path).await?;
    }
    fs::rename(&partial_path, &dest_path).await?;

    send_progress(
        &tx_event,
        &task_name,
        1.0,
        format!("Файл сохранен: {}", dest_path.display()),
    );
    let _ = tx_event.send(AppEvent::TaskFinished(format!("{task_name}: завершено")));

    Ok(dest_path)
}

fn send_download_progress(
    tx_event: &UnboundedSender<AppEvent>,
    task_name: &str,
    downloaded: u64,
    total_size: Option<u64>,
) {
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

    send_progress(tx_event, task_name, progress, status_text);
}

fn send_progress(
    tx_event: &UnboundedSender<AppEvent>,
    task_name: &str,
    progress: f32,
    status_text: String,
) {
    let _ = tx_event.send(AppEvent::TaskProgress {
        task_name: task_name.to_owned(),
        progress,
        status_text,
    });
}

fn partial_download_path(dest_path: &Path) -> PathBuf {
    let partial_name = dest_path
        .file_name()
        .and_then(OsStr::to_str)
        .map(|name| format!("{name}.part"))
        .unwrap_or_else(|| "download.part".to_owned());

    dest_path.with_file_name(partial_name)
}

fn bytes_to_mb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_and_compares_iiko_versions() {
        assert_eq!(
            extract_version_name("/release_iiko/8.7.6032.0").as_deref(),
            Some("8.7.6032.0")
        );
        assert_eq!(extract_version_name("not-a-version"), None);
        assert!(compare_versions("8.8.1.0", "8.7.9999.0").is_gt());
        assert!(compare_versions("8.7.6031.9", IIKO_MIN_VERSION).is_lt());
    }

    #[test]
    fn builds_official_distribution_urls() {
        let distro = IikoDistribution::new(IikoComponent::Front, "8.9.1.2");
        assert_eq!(
            distro.url,
            "https://downloads.iiko.online/8.9.1.2/iiko/RMS/Front/Setup.Front.exe"
        );
    }
}
