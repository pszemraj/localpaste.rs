//! Native egui app skeleton for the LocalPaste rewrite.

use crate::backend::{spawn_backend, BackendHandle, CoreCmd, CoreEvent, PasteSummary};
use eframe::egui;
use localpaste_core::{Config, Database};
use tracing::{info, warn};

pub struct LocalPasteApp {
    backend: BackendHandle,
    pastes: Vec<PasteSummary>,
    selected_id: Option<String>,
    db_path: String,
    status: Option<String>,
}

impl LocalPasteApp {
    pub fn new() -> Result<Self, localpaste_core::AppError> {
        let config = Config::from_env();
        let db_path = config.db_path.clone();
        let db = Database::new(&config.db_path)?;
        info!("native GUI opened database at {}", config.db_path);

        let backend = spawn_backend(db);
        let _ = backend.cmd_tx.send(CoreCmd::ListAll { limit: 512 });

        Ok(Self {
            backend,
            pastes: Vec::new(),
            selected_id: None,
            db_path,
            status: None,
        })
    }

    fn apply_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::PasteList { items } => {
                self.pastes = items;
                if self.selected_id.is_none() {
                    self.selected_id = self.pastes.first().map(|p| p.id.clone());
                }
            }
            CoreEvent::Error { message } => {
                warn!("backend error: {}", message);
                self.status = Some(message);
            }
        }
    }

    fn request_refresh(&self) {
        let _ = self.backend.cmd_tx.send(CoreCmd::ListAll { limit: 512 });
    }
}

impl eframe::App for LocalPasteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.backend.evt_rx.try_recv() {
            self.apply_event(event);
        }

        egui::TopBottomPanel::top("top")
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("LocalPaste Native");
                    ui.add_space(12.0);
                    if ui.button("Refresh").clicked() {
                        self.request_refresh();
                    }
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new(&self.db_path).monospace());
                });
            });

        egui::SidePanel::left("sidebar")
            .default_width(260.0)
            .show(ctx, |ui| {
                ui.heading(format!("Pastes ({})", self.pastes.len()));
                ui.add_space(8.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for paste in &self.pastes {
                        let selected = self.selected_id.as_deref() == Some(paste.id.as_str());
                        let label = match &paste.language {
                            Some(lang) => format!("{}  ({})", paste.name, lang),
                            None => paste.name.clone(),
                        };
                        if ui.selectable_label(selected, label).clicked() {
                            self.selected_id = Some(paste.id.clone());
                        }
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Editor");
            ui.add_space(12.0);
            if let Some(selected) = &self.selected_id {
                ui.label(format!("Selected paste id: {}", selected));
                ui.add_space(8.0);
                ui.label("Content view will be wired in Phase 3.");
            } else {
                ui.label("Select a paste from the sidebar.");
            }
        });

        egui::TopBottomPanel::bottom("status")
            .resizable(false)
            .show(ctx, |ui| {
                if let Some(status) = &self.status {
                    ui.label(egui::RichText::new(status).color(egui::Color32::YELLOW));
                }
            });
    }
}
