//! Genres — grouped list; click a genre to drill into its tracks. Mirrors the
//! Artists view and reuses the shared drill-down `track_list`.

use eframe::egui::{self, RichText};

use super::{LibraryUi, ViewAction};
use crate::library::db::Db;
use crate::ui::theme::{CRT_DIM, CRT_GREEN, CRT_MID};

pub fn show(
    ui: &mut egui::Ui,
    db: &Db,
    state: &mut LibraryUi,
    playlists: super::Playlists,
) -> Option<ViewAction> {
    if let Some(genre) = state.genre_filter.clone() {
        let mut bulk: Option<bool> = None;
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            if ui
                .selectable_label(false, RichText::new("< GENRES").size(10.0).color(CRT_GREEN))
                .clicked()
            {
                state.genre_filter = None;
            }
            ui.label(RichText::new(format!("/ {genre}")).size(11.0).color(CRT_DIM));
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
        let list = db.tracks_where("genre", &genre).unwrap_or_default();
        if let Some(shuffle) = bulk {
            return Some(ViewAction::Play { list, index: 0, shuffle });
        }
        let pick = super::artists::track_list(ui, &list, playlists);
        return super::list_action(list, pick);
    }

    let genres = db.genres().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("{} GENRES", genres.len()))
                .size(9.0)
                .color(CRT_MID),
        );
    });
    ui.separator();

    let mut action = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (g, count) in &genres {
                let resp = super::list_row(ui, 28.0, |ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new(g).size(11.0).color(CRT_DIM));
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(format!("{count} TRK"))
                                    .size(9.0)
                                    .color(CRT_MID),
                            );
                        },
                    );
                });
                if resp.clicked() {
                    state.genre_filter = Some(g.clone());
                }
                ui.separator();
            }
        });
    let _ = &mut action;
    action
}
