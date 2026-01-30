//! Native egui app skeleton for the LocalPaste rewrite.

use crate::backend::{spawn_backend, BackendHandle, CoreCmd, CoreEvent, PasteSummary};
use eframe::egui;
use localpaste_core::models::paste::Paste;
use localpaste_core::{Config, Database};
use tracing::{info, warn};

pub struct LocalPasteApp {
    backend: BackendHandle,
    pastes: Vec<PasteSummary>,
    selected_id: Option<String>,
    selected_paste: Option<Paste>,
    selected_content: String,
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
            selected_paste: None,
            selected_content: String::new(),
            db_path,
            status: None,
        })
    }

    fn apply_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::PasteList { items } => {
                self.pastes = items;
                let selection_valid = self
                    .selected_id
                    .as_ref()
                    .map(|id| self.pastes.iter().any(|p| p.id == *id))
                    .unwrap_or(false);
                if !selection_valid {
                    if let Some(first) = self.pastes.first() {
                        self.select_paste(first.id.clone());
                    } else {
                        self.selected_id = None;
                        self.selected_paste = None;
                        self.selected_content.clear();
                    }
                }
            }
            CoreEvent::PasteLoaded { paste } => {
                if self.selected_id.as_deref() == Some(paste.id.as_str()) {
                    self.selected_content = paste.content.clone();
                    self.selected_paste = Some(paste);
                }
            }
            CoreEvent::PasteMissing { id } => {
                self.pastes.retain(|paste| paste.id != id);
                if self.selected_id.as_deref() == Some(id.as_str()) {
                    self.selected_id = None;
                    self.selected_paste = None;
                    self.selected_content.clear();
                    self.status = Some("Selected paste was deleted; list refreshed.".to_string());
                } else {
                    self.status = Some("Paste was deleted; list refreshed.".to_string());
                }
                self.request_refresh();
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

    fn select_paste(&mut self, id: String) {
        self.selected_id = Some(id.clone());
        self.selected_paste = None;
        self.selected_content.clear();
        let _ = self.backend.cmd_tx.send(CoreCmd::GetPaste { id });
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
                let mut pending_select: Option<String> = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for paste in &self.pastes {
                        let selected = self.selected_id.as_deref() == Some(paste.id.as_str());
                        let label = match &paste.language {
                            Some(lang) => format!("{}  ({})", paste.name, lang),
                            None => paste.name.clone(),
                        };
                        if ui.selectable_label(selected, label).clicked() {
                            pending_select = Some(paste.id.clone());
                        }
                    }
                });
                if let Some(id) = pending_select {
                    self.select_paste(id);
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Editor");
            ui.add_space(12.0);

            let selected_meta = self
                .selected_paste
                .as_ref()
                .map(|paste| (paste.name.clone(), paste.language.clone(), paste.id.clone()));

            if let Some((name, language, id)) = selected_meta {
                ui.horizontal(|ui| {
                    ui.heading(name);
                    ui.add_space(8.0);
                    if let Some(lang) = language {
                        ui.label(format!("({})", lang));
                    }
                });
                ui.label(egui::RichText::new(id).small().monospace());
                ui.add_space(8.0);
                ui.add_enabled(
                    false,
                    egui::TextEdit::multiline(&mut self.selected_content)
                        .desired_width(f32::INFINITY)
                        .desired_rows(18),
                );
            } else if self.selected_id.is_some() {
                ui.label("Loading paste...");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    fn make_app() -> LocalPasteApp {
        let (cmd_tx, _cmd_rx) = unbounded();
        let (_evt_tx, evt_rx) = unbounded();
        LocalPasteApp {
            backend: BackendHandle { cmd_tx, evt_rx },
            pastes: vec![PasteSummary {
                id: "alpha".to_string(),
                name: "Alpha".to_string(),
                language: None,
            }],
            selected_id: Some("alpha".to_string()),
            selected_paste: Some(Paste::new("content".to_string(), "Alpha".to_string())),
            selected_content: "content".to_string(),
            db_path: "test".to_string(),
            status: None,
        }
    }

    #[test]
    fn paste_missing_clears_selection_and_removes_list_entry() {
        let mut app = make_app();
        app.apply_event(CoreEvent::PasteMissing {
            id: "alpha".to_string(),
        });

        assert!(app.pastes.is_empty());
        assert!(app.selected_id.is_none());
        assert!(app.selected_paste.is_none());
        assert!(app.selected_content.is_empty());
        assert!(app.status.is_some());
    }
}
