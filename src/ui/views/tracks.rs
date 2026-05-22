//! All Tracks — a virtualized, sortable, FTS-searchable list. Only the visible
//! rows are ever pulled from SQLite, so 50k tracks scroll without strain.

use eframe::egui::{self, RichText};

use super::{LibraryUi, ViewAction};
use crate::library::db::{Db, SortKey};
use crate::library::models::Track;
use crate::ui::theme::{CRT_DIM, CRT_GREEN, CRT_MID};

const ROW_H: f32 = 36.0;
/// Cap on the queue built from a double-click in this view (Phase 3).
const PLAY_CAP: i64 = 10_000;

pub fn show(
    ui: &mut egui::Ui,
    db: &Db,
    state: &mut LibraryUi,
    playlists: super::Playlists,
) -> Option<ViewAction> {
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
        ui.add_space(8.0);
        if ui
            .button(RichText::new("> PLAY ALL").size(9.0).color(CRT_GREEN))
            .on_hover_text("Play all tracks matching search/filter")
            .clicked()
        {
            let all = db
                .tracks_page(&state.search, state.sort, PLAY_CAP, 0)
                .unwrap_or_default();
            action = Some(ViewAction::Play {
                list: all,
                index: 0,
                shuffle: false,
            });
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} TRACKS", state.total))
                    .size(9.0)
                    .color(CRT_MID),
            );
        });
    });

    // Selection toolbar (visible whenever any tracks are selected).
    if let Some(sa) = super::selection_toolbar(ui, state.selected.len(), playlists) {
        let ids: Vec<uuid::Uuid> = state.selected.iter().cloned().collect();
        let tracks = db.tracks_by_ids(&ids);
        action = super::selection_to_view_action(sa, tracks);
    }

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

    // (RowAction, global index, the track) captured during the row pass.
    let mut hit: Option<(super::RowAction, usize, Track)> = None;
    let mut toggle_select: Option<uuid::Uuid> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, ROW_H, total, |ui, range| {
            let offset = range.start as i64;
            let limit = (range.end - range.start) as i64;
            let page = state.page(db, offset, limit).to_vec();
            let selection_mode = !state.selected.is_empty();
            for (i, t) in page.iter().enumerate() {
                let num = range.start + i + 1;
                let mut is_selected = state.selected.contains(&t.id);
                let (resp, add_q) = track_row(ui, num, t);

                // Click-and-hold to enter / extend multi-select.
                let lp_id = ui.make_persistent_id(("track_lp", t.id));
                let lp = super::check_long_press(ui, &resp, lp_id);
                if lp.just_fired {
                    toggle_select = Some(t.id);
                    // Reflect the toggle visually NOW — the mutation only
                    // lands after the scroll-area closure returns.
                    is_selected = !is_selected;
                }
                if is_selected {
                    super::paint_selected_outline(ui, resp.rect);
                }

                if resp.dragged() {
                    if is_selected {
                        let ids: Vec<uuid::Uuid> = state.selected.iter().cloned().collect();
                        let bundle = db.tracks_by_ids(&ids);
                        resp.dnd_set_drag_payload(bundle);
                    } else {
                        resp.dnd_set_drag_payload(t.clone());
                    }
                }
                if add_q {
                    hit = Some((super::RowAction::AddToQueue, range.start + i, t.clone()));
                } else if lp.suppress_click {
                    // Either the long-press just fired, or the user is still
                    // holding — either way, don't fire click/double-click.
                } else if selection_mode {
                    // While anything is selected, a plain click toggles
                    // membership instead of drilling/playing.
                    if resp.clicked() {
                        toggle_select = Some(t.id);
                    } else if let Some(a) = super::row_actions(&resp, playlists) {
                        // Right-click context menu still works on a single row.
                        hit = Some((a, range.start + i, t.clone()));
                    }
                } else if let Some(a) = super::row_actions(&resp, playlists) {
                    hit = Some((a, range.start + i, t.clone()));
                }
            }
        });

    if let Some(id) = toggle_select {
        if !state.selected.insert(id) {
            state.selected.remove(&id);
        }
    }

    if let Some((a, _global, track)) = hit {
        action = Some(match a {
            super::RowAction::Play => {
                ViewAction::Play {
                    list: vec![track],
                    index: 0,
                    shuffle: false,
                }
            }
            super::RowAction::PlayNext => ViewAction::Enqueue { track, next: true },
            super::RowAction::AddToQueue => ViewAction::Enqueue { track, next: false },
            super::RowAction::AddToPlaylist(playlist) => {
                ViewAction::PlaylistAddTrack { playlist, track }
            }
            super::RowAction::EditTags => ViewAction::EditTags(track),
        });
    }
    action
}

fn header_row(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        cell(ui, 30.0, "#", CRT_MID, 9.0);
        cell(ui, flex_width(ui), "TITLE", CRT_MID, 9.0);
        cell(ui, 160.0, "ARTIST", CRT_MID, 9.0);
        cell(ui, 150.0, "ALBUM", CRT_MID, 9.0);
        cell(ui, 90.0, "GENRE", CRT_MID, 9.0);
        cell(ui, 52.0, "TIME", CRT_MID, 9.0);
        ui.add_space(40.0); // aligns with the rows' +Q action strip
    });
}

/// Renders one row. Returns the row response (double-click / context menu)
/// and whether the inline `+Q` add-to-queue button was clicked.
fn track_row(
    ui: &mut egui::Ui,
    num: usize,
    t: &crate::library::models::Track,
) -> (egui::Response, bool) {
    let mut add_q = false;
    let row = super::list_row_actions(
        ui,
        ROW_H,
        40.0,
        |ui| {
            ui.add_space(8.0);
            cell(ui, 30.0, &num.to_string(), CRT_MID, 10.0);
            cell(ui, flex_width(ui), &t.title, CRT_DIM, 11.0);
            cell(ui, 160.0, &t.artist, CRT_MID, 10.0);
            cell(ui, 150.0, &t.album, CRT_MID, 10.0);
            cell(ui, 90.0, &t.genre, CRT_MID, 10.0);
            cell(ui, 52.0, &fmt_dur(t.duration_secs), CRT_MID, 10.0);
        },
        |ui| {
            if ui
                .button(RichText::new("+Q").size(10.0).color(CRT_GREEN))
                .on_hover_text("Add to queue")
                .clicked()
            {
                add_q = true;
            }
        },
    );
    (row, add_q)
}

fn cell(ui: &mut egui::Ui, w: f32, text: &str, color: egui::Color32, size: f32) {
    ui.add_sized(
        [w, ROW_H],
        egui::Label::new(RichText::new(text).size(size).color(color)).truncate(),
    );
}

fn flex_width(ui: &egui::Ui) -> f32 {
    // Content width (already excludes the +Q strip) minus the fixed columns
    // (#, artist, album, genre, time) and inter-cell padding.
    (ui.available_width() - (30.0 + 160.0 + 150.0 + 90.0 + 52.0 + 40.0)).max(120.0)
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
