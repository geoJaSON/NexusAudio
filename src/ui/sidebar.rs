//! Left sidebar: library nav, playlist list, modules.

use eframe::egui::{self, RichText};

use super::theme::{CRT_DIM, CRT_GREEN, CRT_MID};
use super::View;

pub fn show(ui: &mut egui::Ui, current: &mut View) {
    ui.add_space(8.0);
    section_label(ui, "// LIBRARY");
    nav_item(ui, current, View::Tracks, "ALL TRACKS");
    nav_item(ui, current, View::Albums, "ALBUMS");
    nav_item(ui, current, View::Artists, "ARTISTS");
    nav_item(ui, current, View::Queue, "QUEUE");
    nav_item(ui, current, View::Folders, "FOLDERS");

    ui.add_space(10.0);
    section_label(ui, "// PLAYLISTS");
    // Placeholder until Phase 5 wires real playlists from the JSON store.
    for (tag, name) in [
        ("[01]", "SYNTHWAVE MIX"),
        ("[02]", "LATE NIGHT CODE"),
        ("[03]", "MORNING SECTOR"),
    ] {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(RichText::new(tag).size(10.0).color(CRT_MID));
            ui.label(RichText::new(name).size(10.0).color(CRT_DIM));
        });
    }
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new("[--] + NEW PLAYLIST").size(10.0).color(CRT_MID));
    });

    ui.add_space(10.0);
    section_label(ui, "// MODULES");
    nav_item(ui, current, View::Audiobooks, "AUDIOBOOKS");
    nav_item(ui, current, View::Settings, "SETTINGS");
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
