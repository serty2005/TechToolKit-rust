#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;
mod core;
mod ui;
mod windows_utils;

use std::{
    thread::{self, JoinHandle},
    time::Duration,
};

use eframe::egui;
use tokio::{runtime::Runtime, sync::mpsc};

type CommandSender = mpsc::UnboundedSender<AppCommand>;
type CommandReceiver = mpsc::UnboundedReceiver<AppCommand>;
type EventSender = mpsc::UnboundedSender<AppEvent>;
type EventReceiver = mpsc::UnboundedReceiver<AppEvent>;

#[derive(Debug, Clone)]
pub enum AppCommand {
    TestBackend,
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    BackendReady,
    StatusChanged(String),
    ProgressChanged(f32),
    TaskFinished(String),
    BackendStopped,
    Error(String),
}

fn main() -> eframe::Result {
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

fn start_backend_thread(rx_cmd: CommandReceiver, tx_event: EventSender) -> JoinHandle<()> {
    thread::Builder::new()
        .name("tech-toolkit-tokio-backend".to_owned())
        .spawn(move || {
            let runtime = Runtime::new().expect("failed to start Tokio runtime");
            runtime.block_on(async move {
                let _ = tx_event.send(AppEvent::BackendReady);
                backend_loop(rx_cmd, tx_event).await;
            });
        })
        .expect("failed to spawn backend thread")
}

async fn backend_loop(mut rx_cmd: CommandReceiver, tx_event: EventSender) {
    while let Some(command) = rx_cmd.recv().await {
        match command {
            AppCommand::TestBackend => run_backend_test(&tx_event).await,
            AppCommand::Shutdown => {
                let _ = tx_event.send(AppEvent::BackendStopped);
                break;
            }
        }
    }
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

pub struct RustMhApp {
    tx_cmd: CommandSender,
    rx_event: EventReceiver,
    backend_thread: Option<JoinHandle<()>>,
    status_text: String,
    progress: f32,
    backend_busy: bool,
    log_lines: Vec<String>,
}

impl RustMhApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        tx_cmd: CommandSender,
        rx_event: EventReceiver,
        backend_thread: JoinHandle<()>,
    ) -> Self {
        configure_touch_ui(&cc.egui_ctx);

        Self {
            tx_cmd,
            rx_event,
            backend_thread: Some(backend_thread),
            status_text: "UI готов. Ожидание backend...".to_owned(),
            progress: 0.0,
            backend_busy: false,
            log_lines: Vec::new(),
        }
    }

    fn drain_backend_events(&mut self) {
        while let Ok(event) = self.rx_event.try_recv() {
            self.apply_event(event);
        }
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
            AppEvent::TaskFinished(message) => {
                self.backend_busy = false;
                self.progress = 1.0;
                self.status_text = message.clone();
                self.log_lines.push(message);
            }
            AppEvent::BackendStopped => {
                self.status_text = "Backend остановлен".to_owned();
                self.log_lines.push(self.status_text.clone());
            }
            AppEvent::Error(message) => {
                self.backend_busy = false;
                self.status_text = format!("Ошибка: {message}");
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
}

impl eframe::App for RustMhApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_backend_events();
        ctx.request_repaint();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.vertical_centered(|ui| {
            ui.heading("TechToolKit");
            ui.label("Windows-native toolkit для обслуживания iiko-касс");
        });

        ui.add_space(24.0);

        ui.horizontal(|ui| {
            let test_button = egui::Button::new("Тест Backend").min_size(egui::vec2(180.0, 56.0));

            if ui.add_enabled(!self.backend_busy, test_button).clicked() {
                self.backend_busy = true;
                self.progress = 0.0;
                self.send_command(AppCommand::TestBackend);
            }

            ui.label(&self.status_text);
        });

        ui.add_space(16.0);
        ui.add(
            egui::ProgressBar::new(self.progress)
                .desired_width(f32::INFINITY)
                .show_percentage()
                .text(format!("{:.0}%", self.progress * 100.0)),
        );

        ui.add_space(24.0);
        ui.separator();
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
