//! Artists — grouped list, click an artist to drill into their tracks.
//! Also hosts the shared `track_list` helper used by the drill-down views
//! (a plain list — these collections are small, no virtualization needed).

use eframe::egui::{self, RichText};

use super::{LibraryUi, ViewAction};
use crate::library::db::Db;
use crate::library::models::Track;
use crate::ui::theme::{CRT_DIM, CRT_GREEN, CRT_MID};

pub fn show(
    ui: &mut egui::Ui,
    db: &Db,
    state: &mut LibraryUi,
    playlists: super::Playlists,
) -> Option<ViewAction> {
    if let Some(artist) = state.artist_filter.clone() {
        let mut bulk: Option<bool> = None;
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            if ui
                .selectable_label(false, RichText::new("< ARTISTS").size(10.0).color(CRT_GREEN))
                .clicked()
            {
                state.artist_filter = None;
            }
            ui.label(RichText::new(format!("/ {artist}")).size(11.0).color(CRT_DIM));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(RichText::new("SHUFFLE").size(9.0).color(CRT_GREEN)).clicked() {
                    bulk = Some(true);
                }
                if ui.button(RichText::new("> PLAY ALL").size(9.0).color(CRT_GREEN)).clicked() {
                    bulk = Some(false);
                }
            });
        });
        ui.separator();
        let list = db.tracks_where("artist", &artist).unwrap_or_default();
        if let Some(shuffle) = bulk {
            return Some(ViewAction::Play { list, index: 0, shuffle });
        }
        let pick = track_list(ui, &list, playlists);
        return super::list_action(list, pick);
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

    let mut action = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for a in &artists {
                let mut play: Option<bool> = None;
                let resp = super::list_row_actions(
                    ui,
                    28.0,
                    74.0,
                    |ui| {
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
                    },
                    |ui| {
                        if ui
                            .button(RichText::new("~").size(11.0).color(CRT_GREEN))
                            .on_hover_text("Shuffle all")
                            .clicked()
                        {
                            play = Some(true);
                        }
                        if ui
                            .button(RichText::new(">").size(11.0).color(CRT_GREEN))
                            .on_hover_text("Play all")
                            .clicked()
                        {
                            play = Some(false);
                        }
                    },
                );
                if let Some(shuffle) = play {
                    let list = db.tracks_where("artist", &a.artist).unwrap_or_default();
                    action = Some(ViewAction::Play { list, index: 0, shuffle });
                } else if resp.clicked() {
                    state.artist_filter = Some(a.artist.clone());
                }
                ui.separator();
            }
        });
    action
}

/// Plain (non-virtualized) track listing for album/artist drill-downs.
/// Returns the row index and its action (double-click or context menu).
pub fn track_list(
    ui: &mut egui::Ui,
    tracks: &[Track],
    playlists: super::Playlists,
) -> Option<(usize, super::RowAction)> {
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, t) in tracks.iter().enumerate() {
                let mut add_q = false;
                let row = super::list_row_actions(
                    ui,
                    26.0,
                    36.0,
                    |ui| {
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
                            egui::Label::new(
                                RichText::new(&t.title).size(11.0).color(CRT_DIM),
                            )
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
                if add_q {
                    clicked = Some((i, super::RowAction::AddToQueue));
                } else if let Some(a) = super::row_actions(&row, playlists) {
                    clicked = Some((i, a));
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
