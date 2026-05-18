//! All Tracks — a virtualized, sortable, FTS-searchable list. Only the visible
//! rows are ever pulled from SQLite, so 50k tracks scroll without strain.

use eframe::egui::{self, RichText};

use super::{LibraryUi, ViewAction};
use crate::library::db::{Db, SortKey};
use crate::ui::theme::{CRT_DIM, CRT_GREEN, CRT_MID};

const ROW_H: f32 = 36.0;
/// Cap on the queue built from a double-click in this view (Phase 3).
const PLAY_CAP: i64 = 10_000;

pub fn show(ui: &mut egui::Ui, db: &Db, state: &mut LibraryUi) -> Option<ViewAction> {
    let mut action = None;
    // ---- toolbar: search + sort ----
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("> SEARCH TRACKS...")
                .desired_width(220.0),
        );
        if resp.changed() {
            state.invalidate();
        }
        ui.add_space(8.0);
        for (label, key) in [
            ("TITLE", SortKey::Title),
            ("ARTIST", SortKey::Artist),
            ("ALBUM", SortKey::Album),
            ("DATE", SortKey::DateAdded),
        ] {
            let active = state.sort == key;
            if ui
                .selectable_label(active, RichText::new(label).size(10.0))
                .clicked()
            {
                state.sort = key;
                state.invalidate();
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} TRACKS", state.total))
                    .size(9.0)
                    .color(CRT_MID),
            );
        });
    });

    ui.add_space(2.0);
    header_row(ui);
    ui.separator();

    state.total = db.count(&state.search).unwrap_or(0);
    let total = state.total.max(0) as usize;

    if total == 0 {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(if state.search.is_empty() {
                    "NO TRACKS — ADD A FOLDER IN [ FOLDERS ] AND SCAN"
                } else {
                    "NO MATCHES"
                })
                .size(11.0)
                .color(CRT_MID),
            );
        });
        return action;
    }

    let mut clicked_global: Option<usize> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, ROW_H, total, |ui, range| {
            let offset = range.start as i64;
            let limit = (range.end - range.start) as i64;
            let page = state.page(db, offset, limit).to_vec();
            for (i, t) in page.iter().enumerate() {
                let num = range.start + i + 1;
                if track_row(ui, num, t) {
                    clicked_global = Some(range.start + i);
                }
            }
        });

    if let Some(global) = clicked_global {
        // Build the play queue from the current filtered/sorted view so
        // next/prev are meaningful, then start at the clicked row.
        let list = db
            .tracks_page(&state.search, state.sort, PLAY_CAP, 0)
            .unwrap_or_default();
        if global < list.len() {
            action = Some(ViewAction::Play { list, index: global });
        }
    }
    action
}

fn header_row(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        cell(ui, 30.0, "#", CRT_MID, 9.0);
        cell(ui, flex_width(ui), "TITLE / ARTIST", CRT_MID, 9.0);
        cell(ui, 160.0, "ALBUM", CRT_MID, 9.0);
        cell(ui, 90.0, "GENRE", CRT_MID, 9.0);
        cell(ui, 56.0, "TIME", CRT_MID, 9.0);
    });
}

/// Returns true on double-click (play this track).
fn track_row(ui: &mut egui::Ui, num: usize, t: &crate::library::models::Track) -> bool {
    let row = super::list_row(ui, ROW_H, |ui| {
        ui.add_space(8.0);
        cell(ui, 30.0, &num.to_string(), CRT_MID, 10.0);

        ui.vertical(|ui| {
            let w = flex_width(ui);
            ui.add_sized(
                [w, 16.0],
                egui::Label::new(RichText::new(&t.title).size(11.0).color(CRT_DIM))
                    .truncate(),
            );
            ui.add_sized(
                [w, 12.0],
                egui::Label::new(RichText::new(&t.artist).size(10.0).color(CRT_MID))
                    .truncate(),
            );
        });

        cell(ui, 160.0, &t.album, CRT_MID, 10.0);
        cell(ui, 90.0, &t.genre, CRT_MID, 10.0);
        cell(ui, 56.0, &fmt_dur(t.duration_secs), CRT_MID, 10.0);
    });

    let _ = CRT_GREEN;
    row.double_clicked()
}

fn cell(ui: &mut egui::Ui, w: f32, text: &str, color: egui::Color32, size: f32) {
    ui.add_sized(
        [w, ROW_H],
        egui::Label::new(RichText::new(text).size(size).color(color)).truncate(),
    );
}

fn flex_width(ui: &egui::Ui) -> f32 {
    // Total width minus the fixed columns and paddings.
    (ui.available_width() - (30.0 + 160.0 + 90.0 + 56.0 + 48.0)).max(120.0)
}

fn fmt_dur(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
