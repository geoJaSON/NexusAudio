//! Folders — watched-directory manager for music *and* audiobooks. Read-only
//! over Settings; mutations go back to the App as `ViewAction`s.

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
    ab_status: Option<&str>,
) -> Option<ViewAction> {
    let mut action = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // ---- MUSIC ----
            if let Some(a) = folder_section(
                ui,
                "MUSIC DIRECTORIES",
                &settings.music_folders,
                settings,
                scan_status,
                ViewAction::AddMusicFolder,
                ViewAction::ScanAll,
                &ViewAction::RemoveFolder,
            ) {
                action = Some(a);
            }

            ui.add_space(18.0);

            // ---- AUDIOBOOKS ----
            if let Some(a) = folder_section(
                ui,
                "AUDIOBOOK DIRECTORIES",
                &settings.audiobook_folders,
                settings,
                ab_status,
                ViewAction::AddAudiobookFolder,
                ViewAction::ScanAudiobooks,
                &ViewAction::RemoveAudiobookFolder,
            ) {
                action = Some(a);
            }

            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(RichText::new("SUPPORTED FORMATS").size(10.0).color(CRT_MID));
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.add_space(10.0);
                for fmt in SUPPORTED {
                    ui.label(RichText::new(format!(" {fmt} ")).size(9.0).color(CRT_DIM));
                }
            });
        });

    action
}

#[allow(clippy::too_many_arguments)]
fn folder_section(
    ui: &mut egui::Ui,
    title: &str,
    folders: &[std::path::PathBuf],
    settings: &Settings,
    status: Option<&str>,
    add: ViewAction,
    scan: ViewAction,
    remove: &dyn Fn(std::path::PathBuf) -> ViewAction,
) -> Option<ViewAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(format!("> {title}")).size(10.0).color(CRT_MID));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(RichText::new("SCAN").size(9.0).color(AMBER)).clicked() {
                action = Some(scan);
            }
            if ui.button(RichText::new("+ ADD FOLDER").size(9.0).color(AMBER)).clicked() {
                action = Some(add);
            }
        });
    });
    if let Some(s) = status {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(s).size(10.0).color(AMBER));
        });
    }
    ui.separator();

    if folders.is_empty() {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(
                RichText::new("none watched — click [ + ADD FOLDER ]")
                    .size(10.0)
                    .color(CRT_MID),
            );
        });
    }
    for folder in folders {
        let key = folder.display().to_string();
        let exists = folder.exists();
        let stat = settings.folder_stats.get(&key);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            let (mark, mc) = if exists { (">", CRT_GREEN) } else { ("!", AMBER) };
            ui.label(RichText::new(mark).size(11.0).color(mc));
            ui.add_sized(
                [ui.available_width() - 230.0, 18.0],
                egui::Label::new(RichText::new(&key).size(11.0).color(CRT_DIM)).truncate(),
            );
            if exists {
                let files = stat.map(|s| s.file_count).unwrap_or(0);
                ui.label(RichText::new(format!("{files} FILES")).size(9.0).color(CRT_MID));
                let when = stat
                    .and_then(|s| s.last_scan)
                    .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "NEVER".into());
                ui.label(RichText::new(when).size(9.0).color(CRT_MID));
            } else {
                ui.label(RichText::new("PATH NOT FOUND").size(9.0).color(AMBER));
            }
            if ui.button(RichText::new("x").size(9.0).color(CRT_MID)).clicked() {
                action = Some(remove(folder.clone()));
            }
        });
        ui.separator();
    }
    action
}
