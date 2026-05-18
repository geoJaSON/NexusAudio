//! Folders — watched-directory manager. Read-only over Settings; mutations go
//! back to the App as `ViewAction`s (the App owns rfd dialogs + the scanner).

use eframe::egui::{self, RichText};

use super::ViewAction;
use crate::settings::Settings;
use crate::ui::theme::{AMBER, CRT_DIM, CRT_GREEN, CRT_MID};

const SUPPORTED: &[&str] = &[
    "MP3", "FLAC", "AAC", "OGG", "WAV", "AIFF", "M4A", "OPUS", "M4B",
];

pub fn show(
    ui: &mut egui::Ui,
    settings: &Settings,
    scan_status: Option<&str>,
) -> Option<ViewAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("> WATCHED MUSIC DIRECTORIES").size(10.0).color(CRT_MID));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(RichText::new("⟳ SCAN ALL").size(9.0).color(AMBER))
                .clicked()
            {
                action = Some(ViewAction::ScanAll);
            }
            if ui
                .button(RichText::new("+ ADD FOLDER").size(9.0).color(AMBER))
                .clicked()
            {
                action = Some(ViewAction::AddMusicFolder);
            }
        });
    });

    if let Some(status) = scan_status {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(status).size(10.0).color(AMBER));
        });
    }
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);
            if settings.music_folders.is_empty() {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("NO FOLDERS WATCHED — CLICK [ + ADD FOLDER ]")
                            .size(11.0)
                            .color(CRT_MID),
                    );
                });
            }

            for folder in &settings.music_folders {
                let key = folder.display().to_string();
                let exists = folder.exists();
                let stat = settings.folder_stats.get(&key);

                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    let (mark, mc) = if exists {
                        ("▸", CRT_GREEN)
                    } else {
                        ("!", AMBER)
                    };
                    ui.label(RichText::new(mark).size(11.0).color(mc));
                    ui.add_sized(
                        [ui.available_width() - 230.0, 18.0],
                        egui::Label::new(RichText::new(&key).size(11.0).color(CRT_DIM))
                            .truncate(),
                    );

                    if exists {
                        let files = stat.map(|s| s.file_count).unwrap_or(0);
                        ui.label(
                            RichText::new(format!("{files} FILES")).size(9.0).color(CRT_MID),
                        );
                        let when = stat
                            .and_then(|s| s.last_scan)
                            .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_else(|| "NEVER".into());
                        ui.label(RichText::new(when).size(9.0).color(CRT_MID));
                    } else {
                        ui.label(RichText::new("PATH NOT FOUND").size(9.0).color(AMBER));
                    }

                    if ui
                        .button(RichText::new("✕").size(9.0).color(CRT_MID))
                        .clicked()
                    {
                        action = Some(ViewAction::RemoveFolder(folder.clone()));
                    }
                });
                ui.separator();
            }

            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(RichText::new("SUPPORTED FORMATS").size(10.0).color(CRT_MID));
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.add_space(10.0);
                for fmt in SUPPORTED {
                    ui.label(
                        RichText::new(format!(" {fmt} ")).size(9.0).color(CRT_DIM),
                    );
                }
            });
        });

    action
}
