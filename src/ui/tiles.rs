use eframe::egui;

use crate::RustMhApp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    SystemMonitor,
    AssetManager,
    ServiceControl,
    Logs,
}

pub struct TreeBehavior<'a> {
    app: &'a mut RustMhApp,
}

impl<'a> TreeBehavior<'a> {
    pub fn new(app: &'a mut RustMhApp) -> Self {
        Self { app }
    }
}

impl egui_tiles::Behavior<Pane> for TreeBehavior<'_> {
    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| match pane {
                Pane::SystemMonitor => self.app.show_system_monitor(ui),
                Pane::AssetManager => self.app.show_iiko_install(ui),
                Pane::ServiceControl => self.app.show_service_control(ui),
                Pane::Logs => self.app.show_logs(ui),
            });

        egui_tiles::UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        match pane {
            Pane::SystemMonitor => "🖥 Мониторинг",
            Pane::AssetManager => "📥 Установка iiko",
            Pane::ServiceControl => "🧰 Сервисы",
            Pane::Logs => "📋 Логи",
        }
        .into()
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        34.0
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        8.0
    }

    fn min_size(&self) -> f32 {
        180.0
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }
}

pub fn default_tree() -> egui_tiles::Tree<Pane> {
    let mut tree = egui_tiles::Tree::empty("tech_toolkit_dashboard_tree");

    let panes = [
        Pane::SystemMonitor,
        Pane::AssetManager,
        Pane::ServiceControl,
        Pane::Logs,
    ]
    .into_iter()
    .map(|pane| tree.tiles.insert_pane(pane))
    .collect();

    let root = tree.tiles.insert_grid_tile(panes);
    tree.root = Some(root);
    tree
}
