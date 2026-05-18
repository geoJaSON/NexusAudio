//! Left sidebar: library nav, playlist list, modules.

use eframe::egui::{self, RichText};

use super::theme::{AMBER, CRT_DIM, CRT_GREEN, CRT_MID};
use super::views::ViewAction;
use super::View;

pub fn show(
    ui: &mut egui::Ui,
    current: &mut View,
    playlists: &[(uuid::Uuid, String)],
    selected: Option<uuid::Uuid>,
    resume_hint: Option<&str>,
) -> Option<ViewAction> {
    let mut action = None;

    ui.add_space(8.0);
    section_label(ui, "// LIBRARY");
    nav_item(ui, current, View::Tracks, "ALL TRACKS");
    nav_item(ui, current, View::Albums, "ALBUMS");
    nav_item(ui, current, View::Artists, "ARTISTS");
    nav_item(ui, current, View::Genres, "GENRES");

    ui.add_space(10.0);
    section_label(ui, "// PLAYLISTS");
    for (i, (id, name)) in playlists.iter().enumerate() {
        let active = *current == View::Playlists && selected == Some(*id);
        let resp = ui
            .horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(format!("[{:02}]", i + 1)).size(10.0).color(CRT_MID),
                );
                ui.label(
                    RichText::new(name)
                        .size(10.0)
                        .color(if active { CRT_GREEN } else { CRT_DIM }),
                )
            })
            .inner
            .interact(egui::Sense::click());
        if resp.clicked() {
            action = Some(ViewAction::PlaylistSelect(*id));
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    let new = ui
        .horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(RichText::new("[--] + NEW PLAYLIST").size(10.0).color(CRT_MID))
        })
        .inner
        .interact(egui::Sense::click());
    if new.clicked() {
        action = Some(ViewAction::PlaylistNew);
    }
    if new.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    ui.add_space(10.0);
    section_label(ui, "// MODULES");
    nav_item(ui, current, View::Audiobooks, "AUDIOBOOKS");
    nav_item(ui, current, View::Settings, "SETTINGS");

    if let Some(hint) = resume_hint {
        ui.add_space(10.0);
        section_label(ui, "// RESUME");
        let resp = ui
            .horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(RichText::new(">").size(10.0).color(CRT_GREEN));
                ui.add_sized(
                    [ui.available_width(), 30.0],
                    egui::Label::new(
                        RichText::new(hint).size(10.0).color(AMBER),
                    )
                    .truncate(),
                )
            })
            .inner
            .interact(egui::Sense::click());
        if resp.clicked() {
            action = Some(ViewAction::ResumeLastBook);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    action
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(text).size(9.0).color(CRT_MID));
    });
    ui.add_space(4.0);
}

fn nav_item(ui: &mut egui::Ui, current: &mut View, target: View, label: &str) {
    let active = *current == target;
    let prefix = if active { ">" } else { "_" };
    let color = if active { CRT_GREEN } else { CRT_DIM };

    let resp = ui
        .horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(RichText::new(prefix).size(10.0).color(CRT_MID));
            ui.label(RichText::new(label).size(11.0).color(color))
        })
        .inner;

    let row = resp.interact(egui::Sense::click());
    if row.clicked() {
        *current = target;
    }
    if row.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}
