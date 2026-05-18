//! Albums — grouped list, click an album to drill into its tracks.

use eframe::egui::{self, RichText};

use super::{LibraryUi, ViewAction};
use crate::library::db::Db;
use crate::ui::theme::{CRT_DIM, CRT_GREEN, CRT_MID};

pub fn show(ui: &mut egui::Ui, db: &Db, state: &mut LibraryUi) -> Option<ViewAction> {
    if let Some(album) = state.album_filter.clone() {
        return drill(ui, db, state, &album);
    }

    let albums = db.albums().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(format!("{} ALBUMS", albums.len())).size(9.0).color(CRT_MID));
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for a in &albums {
                let resp = super::list_row(ui, 40.0, |ui| {
                    ui.add_space(10.0);
                    ui.add_sized(
                        [16.0, 20.0],
                        egui::Label::new(RichText::new("▤").size(11.0).color(CRT_MID)),
                    );
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&a.album).size(11.0).color(CRT_DIM));
                        ui.label(
                            RichText::new(&a.album_artist).size(10.0).color(CRT_MID),
                        );
                    });
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(format!("{} TRK", a.track_count))
                                    .size(9.0)
                                    .color(CRT_MID),
                            );
                            if let Some(y) = a.year {
                                ui.label(
                                    RichText::new(y.to_string()).size(9.0).color(CRT_MID),
                                );
                            }
                        },
                    );
                });
                if resp.clicked() {
                    state.album_filter = Some(a.album.clone());
                }
                ui.separator();
            }
        });
    None
}

fn drill(
    ui: &mut egui::Ui,
    db: &Db,
    state: &mut LibraryUi,
    album: &str,
) -> Option<ViewAction> {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        if ui
            .selectable_label(false, RichText::new("< ALBUMS").size(10.0).color(CRT_GREEN))
            .clicked()
        {
            state.album_filter = None;
        }
        ui.label(RichText::new(format!("/ {album}")).size(11.0).color(CRT_DIM));
    });
    ui.separator();
    let list = db.tracks_where("album", album).unwrap_or_default();
    super::artists::track_list(ui, &list).map(|index| ViewAction::Play { list, index })
}
