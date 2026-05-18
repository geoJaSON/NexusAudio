//! Settings view: folder management, appearance, playback prefs, audiobooks.
//! Mutates `Settings` in place for toggles/colors/sliders; folder ops and
//! clear-resume come back as `ViewAction`s. Returns `SettingsChanged` when an
//! in-place widget changed so the App persists + re-applies visuals.

use eframe::egui::{self, RichText};

use super::folders::{folder_section, SUPPORTED};
use super::ViewAction;
use crate::settings::{Settings, DEFAULT_ACCENT, DEFAULT_TEXT};
use crate::ui::theme::{AMBER, CRT_DIM, CRT_GREEN, CRT_MID};

pub fn show(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    scan_status: Option<&str>,
    ab_status: Option<&str>,
) -> Option<ViewAction> {
    let mut action = None;
    let mut dirty = false;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // ---- FOLDERS ----
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
            ui.add_space(16.0);
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

            // ---- APPEARANCE ----
            heading(ui, "// APPEARANCE");
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(RichText::new("ACCENT").size(10.0).color(CRT_DIM));
                if ui.color_edit_button_srgb(&mut settings.accent_color).changed() {
                    dirty = true;
                }
                ui.add_space(16.0);
                ui.label(RichText::new("TEXT").size(10.0).color(CRT_DIM));
                if ui.color_edit_button_srgb(&mut settings.text_color).changed() {
                    dirty = true;
                }
                ui.add_space(16.0);
                if ui
                    .button(RichText::new("RESET TO PHOSPHOR").size(9.0).color(CRT_MID))
                    .clicked()
                {
                    settings.accent_color = DEFAULT_ACCENT;
                    settings.text_color = DEFAULT_TEXT;
                    dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(
                        "note: explicitly-styled text stays green until a full theme pass",
                    )
                    .size(9.0)
                    .color(CRT_MID),
                );
            });

            // ---- PLAYBACK ----
            heading(ui, "// PLAYBACK");
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                if ui
                    .checkbox(
                        &mut settings.auto_scan_on_startup,
                        RichText::new("Auto-scan on startup").size(10.0).color(CRT_DIM),
                    )
                    .changed()
                {
                    dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                if ui
                    .checkbox(
                        &mut settings.eq_enabled,
                        RichText::new("Titlebar EQ bars").size(10.0).color(CRT_DIM),
                    )
                    .changed()
                {
                    dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    RichText::new("Resume save interval (s)")
                        .size(10.0)
                        .color(CRT_DIM),
                );
                if ui
                    .add(egui::Slider::new(
                        &mut settings.resume_save_interval_secs,
                        5..=120,
                    ))
                    .changed()
                {
                    dirty = true;
                }
            });

            // ---- AUDIOBOOKS ----
            heading(ui, "// AUDIOBOOKS");
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                if ui
                    .button(
                        RichText::new("CLEAR QUICK-RESUME").size(9.0).color(AMBER),
                    )
                    .on_hover_text("Forget all saved audiobook positions")
                    .clicked()
                {
                    action = Some(ViewAction::ClearQuickResume);
                }
            });

            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(RichText::new("SUPPORTED FORMATS").size(10.0).color(CRT_MID));
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.add_space(12.0);
                for fmt in SUPPORTED {
                    ui.label(RichText::new(format!(" {fmt} ")).size(9.0).color(CRT_DIM));
                }
            });
            ui.add_space(12.0);
        });

    // Explicit folder/clear actions take priority; otherwise signal a save.
    if action.is_none() && dirty {
        action = Some(ViewAction::SettingsChanged);
    }
    action
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(text).size(10.0).color(CRT_GREEN));
    });
    ui.separator();
}
