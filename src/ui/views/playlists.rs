//! Playlists view: pick from the sidebar, then manage the selected list —
//! play/shuffle all, reorder/remove tracks, rename/delete/duplicate, M3U I/O.

use eframe::egui::{self, RichText};

use super::{LibraryUi, ViewAction};
use crate::library::db::Db;
use crate::playlists::PlaylistStore;
use crate::ui::theme::{AMBER, CRT_DIM, CRT_GREEN, CRT_MID};

pub fn show(
    ui: &mut egui::Ui,
    db: &Db,
    store: &PlaylistStore,
    state: &mut LibraryUi,
) -> Option<ViewAction> {
    let mut action = None;

    // Resolve selection (it may have been deleted).
    let pl = state
        .selected_playlist
        .and_then(|id| store.get(id))
        .cloned();
    let Some(pl) = pl else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("SELECT A PLAYLIST FROM THE SIDEBAR")
                    .size(12.0)
                    .color(CRT_MID),
            );
            ui.add_space(8.0);
            if ui
                .button(RichText::new("[ + NEW PLAYLIST ]").size(10.0).color(CRT_GREEN))
                .clicked()
            {
                action = Some(ViewAction::PlaylistNew);
            }
            if ui
                .button(RichText::new("[ IMPORT M3U ]").size(10.0).color(AMBER))
                .clicked()
            {
                action = Some(ViewAction::PlaylistImport);
            }
        });
        return action;
    };

    // ---- header / toolbar ----
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        match &mut state.rename_buf {
            Some(buf) => {
                let r = ui.add(
                    egui::TextEdit::singleline(buf)
                        .desired_width(220.0)
                        .font(egui::TextStyle::Monospace),
                );
                let commit = ui.button(RichText::new("OK").size(10.0).color(CRT_GREEN));
                let cancel = ui.button(RichText::new("CANCEL").size(10.0).color(CRT_MID));
                if commit.clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    action = Some(ViewAction::PlaylistRename(pl.id, buf.clone()));
                }
                if cancel.clicked() {
                    state.rename_buf = None;
                }
            }
            None => {
                ui.label(RichText::new(&pl.name).size(14.0).color(CRT_GREEN));
                ui.label(
                    RichText::new(format!("· {} TRACKS", pl.track_ids.len()))
                        .size(9.0)
                        .color(CRT_MID),
                );
            }
        }
    });

    let tracks = db.tracks_by_ids(&pl.track_ids);

    ui.horizontal(|ui| {
        ui.add_space(8.0);
        if ui.button(RichText::new("> PLAY ALL").size(9.0).color(CRT_GREEN)).clicked()
            && !tracks.is_empty()
        {
            action = Some(ViewAction::Play { list: tracks.clone(), index: 0, shuffle: false });
        }
        if ui.button(RichText::new("SHUFFLE").size(9.0).color(CRT_GREEN)).clicked()
            && !tracks.is_empty()
        {
            action = Some(ViewAction::Play { list: tracks.clone(), index: 0, shuffle: true });
        }
        ui.separator();
        if ui.button(RichText::new("RENAME").size(9.0).color(CRT_DIM)).clicked() {
            state.rename_buf = Some(pl.name.clone());
        }
        if ui.button(RichText::new("DUPLICATE").size(9.0).color(CRT_DIM)).clicked() {
            action = Some(ViewAction::PlaylistDuplicate(pl.id));
        }
        if ui.button(RichText::new("DELETE").size(9.0).color(AMBER)).clicked() {
            action = Some(ViewAction::PlaylistDelete(pl.id));
        }
        ui.separator();
        if ui.button(RichText::new("EXPORT M3U").size(9.0).color(CRT_DIM)).clicked() {
            action = Some(ViewAction::PlaylistExport(pl.id));
        }
        if ui.button(RichText::new("IMPORT M3U").size(9.0).color(CRT_DIM)).clicked() {
            action = Some(ViewAction::PlaylistImport);
        }
    });
    ui.separator();

    if tracks.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("EMPTY — RIGHT-CLICK TRACKS → ADD TO PLAYLIST")
                    .size(11.0)
                    .color(CRT_MID),
            );
        });
        return action;
    }

    let names: Vec<(uuid::Uuid, String)> =
        store.lists.iter().map(|p| (p.id, p.name.clone())).collect();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, t) in tracks.iter().enumerate() {
                let mut rm = false;
                let mut mv: Option<bool> = None;
                let row = super::list_row_actions(
                    ui,
                    28.0,
                    96.0,
                    |ui| {
                        ui.add_space(10.0);
                        ui.add_sized(
                            [26.0, 18.0],
                            egui::Label::new(
                                RichText::new(format!("{:>2}", i + 1))
                                    .size(10.0)
                                    .color(CRT_MID),
                            ),
                        );
                        ui.add_sized(
                            [ui.available_width() - 70.0, 18.0],
                            egui::Label::new(
                                RichText::new(format!("{}  —  {}", t.title, t.artist))
                                    .size(11.0)
                                    .color(CRT_DIM),
                            )
                            .truncate(),
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(fmt(t.duration_secs))
                                        .size(9.0)
                                        .color(CRT_MID),
                                );
                            },
                        );
                    },
                    |ui| {
                        if ui.button(RichText::new("x").size(10.0).color(CRT_MID)).clicked() {
                            rm = true;
                        }
                        if ui.button(RichText::new("v").size(10.0).color(CRT_MID)).clicked() {
                            mv = Some(false);
                        }
                        if ui.button(RichText::new("^").size(10.0).color(CRT_MID)).clicked() {
                            mv = Some(true);
                        }
                    },
                );
                if rm {
                    action = Some(ViewAction::PlaylistRemoveAt(pl.id, i));
                } else if let Some(up) = mv {
                    action = Some(ViewAction::PlaylistMoveAt { id: pl.id, i, up });
                } else if let Some(a) = super::row_actions(&row, &names) {
                    action = super::list_action(tracks.clone(), Some((i, a)));
                }
                ui.separator();
            }
        });

    action
}

fn fmt(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
