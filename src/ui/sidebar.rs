//! Left sidebar: library nav, playlist list, modules.

use eframe::egui::{self, RichText};

use super::theme::{AMBER, CRT_DIM, CRT_GREEN, CRT_MID, ROW_HOVER};
use super::views::ViewAction;
use super::View;

pub fn show(
    ui: &mut egui::Ui,
    current: &mut View,
    playlists: &[(uuid::Uuid, String)],
    selected: Option<uuid::Uuid>,
    resume_hint: Option<&str>,
    show_queue: bool,
    queue_len: usize,
) -> Option<ViewAction> {
    let mut action = None;

    // ---- Queue toggle (top-of-sidebar feature button) ----
    ui.add_space(6.0);
    let (prefix, color) = if show_queue {
        ("v", CRT_GREEN)
    } else {
        (">", AMBER)
    };
    // Claim an exact row rect so the hit area matches what's painted.
    let avail_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, 22.0), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        if ui.rect_contains_pointer(rect) {
            ui.painter().rect_filled(rect, 0.0, ROW_HOVER);
        }
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        row.set_clip_rect(rect.intersect(ui.clip_rect()));
        row.add_space(8.0);
        row.label(RichText::new(prefix).size(11.0).color(color));
        row.label(RichText::new("QUEUE").size(12.0).color(color));
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(10.0);
            ui.label(
                RichText::new(format!("{queue_len:>3}"))
                    .size(10.0)
                    .color(CRT_MID),
            );
        });
    }
    // Claim the hit area AFTER painting the labels so egui's occlusion
    // ordering doesn't let the labels steal the click.
    let q_id = ui.make_persistent_id("queue_toggle_btn");
    let q_resp = ui.interact(rect, q_id, egui::Sense::click());
    if q_resp.clicked() {
        action = Some(ViewAction::ToggleQueuePanel);
    }
    if q_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // Drag-drop target: drop a track or selection bundle here to enqueue.
    let q_hover_single = q_resp
        .dnd_hover_payload::<crate::library::models::Track>()
        .is_some();
    let q_hover_bundle = q_resp
        .dnd_hover_payload::<Vec<crate::library::models::Track>>()
        .is_some();
    if q_hover_single || q_hover_bundle {
        ui.painter().rect_filled(rect, 0.0, ROW_HOVER);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
    }
    if let Some(dropped) = q_resp.dnd_release_payload::<crate::library::models::Track>() {
        action = Some(ViewAction::Enqueue {
            track: (*dropped).clone(),
            next: false,
        });
    } else if let Some(dropped) =
        q_resp.dnd_release_payload::<Vec<crate::library::models::Track>>()
    {
        action = Some(ViewAction::BulkEnqueue((*dropped).clone()));
    }
    ui.add_space(4.0);
    ui.separator();

    ui.add_space(4.0);
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

        // Handle active Drag & Drop hover and drop action — supports both a
        // single Track payload and a `Vec<Track>` (multi-select bundle).
        let hovered_single = resp
            .dnd_hover_payload::<crate::library::models::Track>()
            .is_some();
        let hovered_bundle = resp
            .dnd_hover_payload::<Vec<crate::library::models::Track>>()
            .is_some();
        if hovered_single || hovered_bundle {
            ui.painter().rect_filled(resp.rect, 2.0, ROW_HOVER);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
        }
        if let Some(dropped_track) =
            resp.dnd_release_payload::<crate::library::models::Track>()
        {
            action = Some(ViewAction::PlaylistAddTrack {
                playlist: Some(*id),
                track: (*dropped_track).clone(),
            });
        } else if let Some(dropped_bundle) =
            resp.dnd_release_payload::<Vec<crate::library::models::Track>>()
        {
            action = Some(ViewAction::BulkAddToPlaylist {
                playlist: Some(*id),
                tracks: (*dropped_bundle).clone(),
            });
        }

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
    // Drop onto "+ NEW PLAYLIST" creates a fresh list seeded with the payload.
    let new_hover_single = new
        .dnd_hover_payload::<crate::library::models::Track>()
        .is_some();
    let new_hover_bundle = new
        .dnd_hover_payload::<Vec<crate::library::models::Track>>()
        .is_some();
    if new_hover_single || new_hover_bundle {
        ui.painter().rect_filled(new.rect, 2.0, ROW_HOVER);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
    }
    if let Some(dropped) = new.dnd_release_payload::<crate::library::models::Track>() {
        action = Some(ViewAction::PlaylistAddTrack {
            playlist: None,
            track: (*dropped).clone(),
        });
    } else if let Some(dropped) =
        new.dnd_release_payload::<Vec<crate::library::models::Track>>()
    {
        action = Some(ViewAction::BulkAddToPlaylist {
            playlist: None,
            tracks: (*dropped).clone(),
        });
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
