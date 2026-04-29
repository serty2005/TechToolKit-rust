mod backend;
mod cli;
mod core;
mod ui;
mod windows_utils;

use std::{
    path::PathBuf,
    thread::{self, JoinHandle},
    time::Duration,
};

use clap::Parser;
use eframe::egui;
use tokio::{runtime::Runtime, sync::mpsc};

use crate::{
    backend::backend_loop,
    cli::{Cli, CliCommand},
    core::{AppCommand, AppEvent, AppTask, IikoComponent, SystemStats},
    windows_utils::monitor::start_system_monitor,
};

type CommandSender = mpsc::UnboundedSender<AppCommand>;
type CommandReceiver = mpsc::UnboundedReceiver<AppCommand>;
type EventSender = mpsc::UnboundedSender<AppEvent>;
type EventReceiver = mpsc::UnboundedReceiver<AppEvent>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppTab {
    Dashboard,
    IikoInstall,
    FiscalDrivers,
    Logs,
}

impl AppTab {
    fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "Главная (Dashboard)",
            Self::IikoInstall => "Установка iiko",
            Self::FiscalDrivers => "ККТ и Драйверы",
            Self::Logs => "Логи",
        }
    }
}

fn main() -> eframe::Result {
    let cli = Cli::parse();

    if matches!(cli.command, Some(CliCommand::Automation { .. })) {
        run_headless_mode();
    }

    let (tx_cmd, rx_cmd) = mpsc::unbounded_channel::<AppCommand>();
    let (tx_event, rx_event) = mpsc::unbounded_channel::<AppEvent>();
    let backend_thread = start_backend_thread(rx_cmd, tx_event);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TechToolKit")
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "TechToolKit",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(RustMhApp::new(
                cc,
                tx_cmd,
                rx_event,
                backend_thread,
            )))
        }),
    )
}

fn run_headless_mode() -> ! {
    let runtime = Runtime::new().expect("failed to start Tokio runtime");
    runtime.block_on(async {
        println!("Запуск в headless режиме");
    });
    std::process::exit(0);
}

fn start_backend_thread(rx_cmd: CommandReceiver, tx_event: EventSender) -> JoinHandle<()> {
    thread::Builder::new()
        .name("tech-toolkit-tokio-backend".to_owned())
        .spawn(move || {
            let runtime = Runtime::new().expect("failed to start Tokio runtime");
            runtime.block_on(async move {
                tokio::spawn(start_system_monitor(tx_event.clone()));
                let _ = tx_event.send(AppEvent::BackendReady);
                backend_loop(rx_cmd, tx_event).await;
            });
        })
        .expect("failed to spawn backend thread")
}

pub struct RustMhApp {
    tx_cmd: CommandSender,
    rx_event: EventReceiver,
    backend_thread: Option<JoinHandle<()>>,
    status_text: String,
    progress: f32,
    backend_busy: bool,
    task_name: String,
    task_status_text: String,
    task_progress: f32,
    displayed_task_progress: f32,
    task_active: bool,
    current_stats: Option<SystemStats>,
    current_tab: AppTab,
    log_lines: Vec<String>,
    iiko_component: IikoComponent,
    iiko_versions: Vec<String>,
    iiko_selected_version: usize,
    iiko_manual_version: String,
    iiko_versions_loading: bool,
    iiko_versions_status: String,
}

impl RustMhApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        tx_cmd: CommandSender,
        rx_event: EventReceiver,
        backend_thread: JoinHandle<()>,
    ) -> Self {
        configure_touch_ui(&cc.egui_ctx);
        let _ = tx_cmd.send(AppCommand::RefreshIikoVersions);

        Self {
            tx_cmd,
            rx_event,
            backend_thread: Some(backend_thread),
            status_text: "UI готов. Ожидание backend...".to_owned(),
            progress: 0.0,
            backend_busy: false,
            task_name: "Нет активных задач".to_owned(),
            task_status_text: "Очередь задач пуста".to_owned(),
            task_progress: 0.0,
            displayed_task_progress: 0.0,
            task_active: false,
            current_stats: None,
            current_tab: AppTab::Dashboard,
            log_lines: Vec::new(),
            iiko_component: IikoComponent::Front,
            iiko_versions: Vec::new(),
            iiko_selected_version: 0,
            iiko_manual_version: String::new(),
            iiko_versions_loading: true,
            iiko_versions_status: "Загрузка списка версий iiko...".to_owned(),
        }
    }

    fn drain_backend_events(&mut self) -> bool {
        let mut received_any = false;
        while let Ok(event) = self.rx_event.try_recv() {
            received_any = true;
            self.apply_event(event);
        }
        received_any
    }

    fn apply_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::BackendReady => {
                self.status_text = "Backend готов к работе".to_owned();
                self.log_lines.push(self.status_text.clone());
            }
            AppEvent::StatusChanged(message) => {
                self.status_text = message.clone();
                self.log_lines.push(message);
            }
            AppEvent::ProgressChanged(progress) => {
                self.progress = progress.clamp(0.0, 1.0);
            }
            AppEvent::TaskProgress {
                task_name,
                progress,
                status_text,
            } => {
                self.task_name = task_name;
                self.task_progress = progress.clamp(0.0, 1.0);
                self.task_status_text = status_text.clone();
                self.status_text = status_text;
                self.task_active = self.task_progress < 1.0;
            }
            AppEvent::IikoVersionsLoaded(versions) => {
                self.iiko_versions = versions;
                self.iiko_selected_version = 0;
                self.iiko_versions_loading = false;
                self.iiko_versions_status = if let Some(version) = self.iiko_versions.first() {
                    self.iiko_manual_version = version.clone();
                    format!("Доступно версий: {}", self.iiko_versions.len())
                } else {
                    "Список версий пуст. Можно ввести версию вручную.".to_owned()
                };
                self.log_lines.push(self.iiko_versions_status.clone());
            }
            AppEvent::TaskFinished(message) => {
                self.backend_busy = false;
                self.progress = 1.0;
                if self.task_active || self.task_progress > 0.0 {
                    self.task_active = false;
                    self.task_progress = 1.0;
                    self.task_status_text = message.clone();
                }
                self.status_text = message.clone();
                self.log_lines.push(message);
            }
            AppEvent::ResourceUpdate(stats) => {
                self.current_stats = Some(stats);
            }
            AppEvent::BackendStopped => {
                self.status_text = "Backend остановлен".to_owned();
                self.log_lines.push(self.status_text.clone());
            }
            AppEvent::Error(message) => {
                self.backend_busy = false;
                self.task_active = false;
                self.iiko_versions_loading = false;
                self.status_text = format!("Ошибка: {message}");
                self.task_status_text = self.status_text.clone();
                self.log_lines.push(self.status_text.clone());
            }
        }

        const MAX_LOG_LINES: usize = 64;
        if self.log_lines.len() > MAX_LOG_LINES {
            let overflow = self.log_lines.len() - MAX_LOG_LINES;
            self.log_lines.drain(0..overflow);
        }
    }

    fn send_command(&mut self, command: AppCommand) {
        if let Err(error) = self.tx_cmd.send(command) {
            self.apply_event(AppEvent::Error(format!("backend channel closed: {error}")));
        }
    }

    fn show_status_panel(&self, ui: &mut egui::Ui) {
        egui::Panel::top("status_panel").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if let Some(stats) = &self.current_stats {
                    let ram_ratio = ratio(stats.ram_used, stats.ram_total);

                    ui.label(format!("CPU: {:.0}%", stats.cpu_usage));
                    ui.add(
                        egui::ProgressBar::new(stats.cpu_usage / 100.0)
                            .desired_width(110.0)
                            .show_percentage(),
                    );
                    ui.separator();
                    ui.label(format!(
                        "RAM: {:.1}/{:.1} GB",
                        bytes_to_gb(stats.ram_used),
                        bytes_to_gb(stats.ram_total)
                    ));
                    ui.add(
                        egui::ProgressBar::new(ram_ratio)
                            .desired_width(130.0)
                            .show_percentage(),
                    );
                    ui.separator();
                    ui.label(format!(
                        "Disk: R {} / W {} KB/s",
                        stats.disk_read_kb, stats.disk_write_kb
                    ));
                } else {
                    ui.label("CPU: -- | RAM: --/-- GB | Disk: R -- / W -- KB/s");
                    ui.add(egui::ProgressBar::new(0.0).desired_width(110.0));
                    ui.add(egui::ProgressBar::new(0.0).desired_width(130.0));
                }
            });
        });
    }

    fn show_navigation(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("navigation_panel")
            .resizable(false)
            .default_size(220.0)
            .show_inside(ui, |ui| {
                ui.heading("TechToolKit");
                ui.separator();
                ui.selectable_value(
                    &mut self.current_tab,
                    AppTab::Dashboard,
                    AppTab::Dashboard.title(),
                );
                ui.selectable_value(
                    &mut self.current_tab,
                    AppTab::IikoInstall,
                    AppTab::IikoInstall.title(),
                );
                ui.selectable_value(
                    &mut self.current_tab,
                    AppTab::FiscalDrivers,
                    AppTab::FiscalDrivers.title(),
                );
                ui.selectable_value(&mut self.current_tab, AppTab::Logs, AppTab::Logs.title());
            });
    }

    fn show_central_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| match self.current_tab {
            AppTab::Dashboard => self.show_dashboard(ui),
            AppTab::IikoInstall => self.show_iiko_install(ui),
            AppTab::FiscalDrivers => self.show_placeholder(ui, AppTab::FiscalDrivers.title()),
            AppTab::Logs => self.show_logs(ui),
        });
    }

    fn show_dashboard(&mut self, ui: &mut egui::Ui) {
        ui.heading("Главная");
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            let test_button = egui::Button::new("Тест Backend").min_size(egui::vec2(180.0, 48.0));

            if ui.add_enabled(!self.backend_busy, test_button).clicked() {
                self.backend_busy = true;
                self.progress = 0.0;
                self.send_command(AppCommand::TestBackend);
            }

            ui.label(&self.status_text);
        });

        ui.add_space(12.0);
        ui.add(
            egui::ProgressBar::new(self.progress)
                .desired_width(f32::INFINITY)
                .show_percentage()
                .text(format!("{:.0}%", self.progress * 100.0)),
        );
    }

    fn show_iiko_install(&mut self, ui: &mut egui::Ui) {
        ui.heading("Установка iiko");
        ui.add_space(12.0);

        ui.horizontal_wrapped(|ui| {
            ui.label("Компонент:");
            for component in IikoComponent::ALL {
                ui.radio_value(&mut self.iiko_component, component, component.title());
            }
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Версия:");

            egui::ComboBox::from_id_salt("iiko_version_select")
                .selected_text(self.selected_iiko_version_label())
                .width(220.0)
                .show_ui(ui, |ui| {
                    for (idx, version) in self.iiko_versions.iter().enumerate() {
                        if ui
                            .selectable_value(&mut self.iiko_selected_version, idx, version)
                            .clicked()
                        {
                            self.iiko_manual_version = version.clone();
                        }
                    }
                });

            if self.iiko_versions_loading {
                ui.spinner();
            }

            if ui
                .add_enabled(!self.iiko_versions_loading, egui::Button::new("Обновить"))
                .clicked()
            {
                self.iiko_versions_loading = true;
                self.iiko_versions_status = "Загрузка списка версий iiko...".to_owned();
                self.send_command(AppCommand::RefreshIikoVersions);
            }
        });

        ui.horizontal(|ui| {
            ui.label("Вручную:");
            ui.text_edit_singleline(&mut self.iiko_manual_version);
            ui.label(&self.iiko_versions_status);
        });

        ui.add_space(8.0);
        let selected_version_ready = !self.selected_iiko_version().is_empty();

        ui.horizontal(|ui| {
            let download_button = egui::Button::new("Скачать").min_size(egui::vec2(150.0, 48.0));

            if ui
                .add_enabled(!self.task_active && selected_version_ready, download_button)
                .clicked()
            {
                let version = self.selected_iiko_version();
                let dest_dir = default_download_dir();

                self.task_name = format!("Скачивание {} {version}", self.iiko_component.title());
                self.task_status_text = format!("Ожидание backend: {}", dest_dir.display());
                self.task_progress = 0.0;
                self.displayed_task_progress = 0.0;
                self.task_active = true;

                self.send_command(AppCommand::EnqueueTask(AppTask::DownloadIikoDistribution {
                    component: self.iiko_component,
                    version,
                    dest_dir: dest_dir.to_string_lossy().into_owned(),
                }));
            }

            ui.label(&self.task_status_text);
        });

        ui.add_space(12.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(&self.task_name);
            ui.add_space(6.0);
            ui.add(
                egui::ProgressBar::new(self.displayed_task_progress)
                    .desired_width(f32::INFINITY)
                    .show_percentage()
                    .text(format!("{:.0}%", self.displayed_task_progress * 100.0)),
            );
            ui.add_space(4.0);
            ui.label(&self.task_status_text);
        });
    }

    fn selected_iiko_version(&self) -> String {
        let manual = self.iiko_manual_version.trim();
        if !manual.is_empty() {
            return manual.to_owned();
        }

        self.iiko_versions
            .get(self.iiko_selected_version)
            .cloned()
            .unwrap_or_default()
    }

    fn selected_iiko_version_label(&self) -> String {
        self.iiko_versions
            .get(self.iiko_selected_version)
            .cloned()
            .unwrap_or_else(|| "Введите версию вручную".to_owned())
    }

    fn show_placeholder(&self, ui: &mut egui::Ui, title: &str) {
        ui.heading(title);
        ui.add_space(12.0);
        ui.label("Раздел в работе.");
    }

    fn show_logs(&self, ui: &mut egui::Ui) {
        ui.heading("Логи");
        ui.add_space(12.0);
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for line in &self.log_lines {
                    ui.label(line);
                }
            });
    }
}

impl eframe::App for RustMhApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let received_events = self.drain_backend_events();

        let delta = self.task_progress - self.displayed_task_progress;
        let mut progress_changed = false;
        if delta.abs() > 0.001 {
            self.displayed_task_progress += delta * 0.18;
            progress_changed = true;
        } else {
            self.displayed_task_progress = self.task_progress;
        }

        if received_events {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else {
            let _animating_progress = progress_changed;
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show_status_panel(ui);
        self.show_navigation(ui);
        self.show_central_panel(ui);
    }
}

impl Drop for RustMhApp {
    fn drop(&mut self) {
        let _ = self.tx_cmd.send(AppCommand::Shutdown);

        if let Some(thread) = self.backend_thread.take() {
            let _ = thread.join();
        }
    }
}

fn configure_touch_ui(ctx: &egui::Context) {
    ctx.set_pixels_per_point(1.15);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(12.0, 12.0);
        style.spacing.button_padding = egui::vec2(18.0, 12.0);
        style.spacing.interact_size = egui::vec2(64.0, 48.0);
    });
}

fn ratio(value: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (value as f32 / total as f32).clamp(0.0, 1.0)
    }
}

fn bytes_to_gb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

fn default_download_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|path| path.join("Downloads"))
        .unwrap_or_else(std::env::temp_dir)
}
