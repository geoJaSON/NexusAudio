//! Artists — grouped list, click an artist to drill into their tracks.
//! Also hosts the shared `track_list` helper used by the drill-down views
//! (a plain list — these collections are small, no virtualization needed).

use eframe::egui::{self, RichText};

use super::{LibraryUi, ViewAction};
use crate::library::db::Db;
use crate::library::models::Track;
use crate::ui::theme::{CRT_DIM, CRT_GREEN, CRT_MID};

pub fn show(ui: &mut egui::Ui, db: &Db, state: &mut LibraryUi) -> Option<ViewAction> {
    if let Some(artist) = state.artist_filter.clone() {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            if ui
                .selectable_label(false, RichText::new("< ARTISTS").size(10.0).color(CRT_GREEN))
                .clicked()
            {
                state.artist_filter = None;
            }
            ui.label(RichText::new(format!("/ {artist}")).size(11.0).color(CRT_DIM));
        });
        ui.separator();
        let list = db.tracks_where("artist", &artist).unwrap_or_default();
        return track_list(ui, &list).map(|index| ViewAction::Play { list, index });
    }

    let artists = db.artists().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("{} ARTISTS", artists.len()))
                .size(9.0)
                .color(CRT_MID),
        );
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for a in &artists {
                let resp = super::list_row(ui, 28.0, |ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new(&a.artist).size(11.0).color(CRT_DIM));
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} ALB · {} TRK",
                                    a.album_count, a.track_count
                                ))
                                .size(9.0)
                                .color(CRT_MID),
                            );
                        },
                    );
                });
                if resp.clicked() {
                    state.artist_filter = Some(a.artist.clone());
                }
                ui.separator();
            }
        });
    None
}

/// Plain (non-virtualized) track listing for album/artist drill-downs.
/// Returns the index of a double-clicked track.
pub fn track_list(ui: &mut egui::Ui, tracks: &[Track]) -> Option<usize> {
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, t) in tracks.iter().enumerate() {
                let row = super::list_row(ui, 26.0, |ui| {
                    ui.add_space(10.0);
                    ui.add_sized(
                        [28.0, 20.0],
                        egui::Label::new(
                            RichText::new(
                                t.track_number
                                    .map(|n| n.to_string())
                                    .unwrap_or_else(|| (i + 1).to_string()),
                            )
                            .size(10.0)
                            .color(CRT_MID),
                        ),
                    );
                    ui.add_sized(
                        [ui.available_width() - 64.0, 20.0],
                        egui::Label::new(RichText::new(&t.title).size(11.0).color(CRT_DIM))
                            .truncate(),
                    );
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(fmt_dur(t.duration_secs))
                                    .size(10.0)
                                    .color(CRT_MID),
                            );
                        },
                    );
                });
                if row.double_clicked() {
                    clicked = Some(i);
                }
                ui.separator();
            }
        });
    clicked
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
