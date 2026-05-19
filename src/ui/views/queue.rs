//! Queue panel: Now Playing → Up Next → Recently Played.
//!
//! Rows carry inline buttons (move/remove/jump), so this view uses plain
//! horizontals rather than `list_row` — `list_row`'s full-row interact would
//! sit on top of the buttons and eat their clicks (the same occlusion rule
//! that, elsewhere, is exactly what we want).

use eframe::egui::{self, RichText};

use super::ViewAction;
use crate::player::queue::Queue;
use crate::ui::theme::{AMBER, CRT_DIM, CRT_GREEN, CRT_MID};

pub fn show(
    ui: &mut egui::Ui,
    queue: &Queue,
    _session_history: &[crate::library::models::Track],
) -> Option<ViewAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("// PLAYBACK QUEUE").size(10.0).color(CRT_MID));
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        if ui
            .button(RichText::new("+ PLAYLIST").size(9.0).color(CRT_GREEN))
            .on_hover_text("Create a playlist from the queue")
            .clicked()
        {
            action = Some(ViewAction::CreatePlaylistFromQueue);
        }
        if ui
            .button(RichText::new("CLEAR UPCOMING").size(9.0).color(AMBER))
            .on_hover_text("Clear upcoming tracks from the queue")
            .clicked()
        {
            action = Some(ViewAction::QueueClear);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} QUEUED", queue.len()))
                    .size(9.0)
                    .color(CRT_MID),
            );
        });
    });
    ui.separator();

    let ordered = queue.ordered();
    let active_pos = queue.pos();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if ordered.is_empty() {
                idle(ui, "queue is empty — double-click or right-click tracks");
            } else {
                for (i, t) in ordered.iter().enumerate() {
                    let is_active = active_pos == Some(i);
                    let id = ui.make_persistent_id(("drag_handle", &t.path));
                    let is_dragging_this = ui.ctx().is_being_dragged(id);

                    let mut frame = egui::Frame::none();
                    if is_dragging_this {
                        frame = frame.fill(crate::ui::theme::ROW_HOVER);
                    }

                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);

                            // Render active indicator or number index
                            let index_w = 20.0;
                            if is_active {
                                ui.add_sized(
                                    [index_w, 18.0],
                                    egui::Label::new(
                                        RichText::new("> ").size(12.0).color(CRT_GREEN).strong(),
                                    ),
                                );
                            } else {
                                ui.add_sized(
                                    [index_w, 18.0],
                                    egui::Label::new(
                                        RichText::new(format!("{:>2}", i + 1)).size(10.0).color(CRT_MID),
                                    ),
                                );
                            }

                            // Track title and artist
                            let text_color = if is_active { CRT_GREEN } else { CRT_DIM };
                            ui.add_sized(
                                [ui.available_width() - 130.0, 18.0],
                                egui::Label::new(
                                    RichText::new(format!("{}  —  {}", t.title, t.artist))
                                        .size(11.0)
                                        .color(text_color),
                                )
                                .truncate(),
                            );

                            // Buttons
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if btn(ui, "x").on_hover_text("Remove from queue").clicked() {
                                        action = Some(ViewAction::QueueRemove(i));
                                    }

                                    // Drag-and-drop reorder handle
                                    let (handle_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(16.0, 18.0),
                                        egui::Sense::hover(),
                                    );
                                    let handle_resp = ui.interact(handle_rect, id, egui::Sense::drag());
                                    
                                    // Visuals: Render ☰ drag icon
                                    if ui.is_rect_visible(handle_rect) {
                                        let is_hovered = handle_resp.hovered() || handle_resp.dragged();
                                        let handle_color = if is_hovered {
                                            CRT_GREEN
                                        } else if is_active {
                                            CRT_MID
                                        } else {
                                            CRT_DIM
                                        };
                                        
                                        // Change cursor to pointing hand or grabbing when dragging
                                        if handle_resp.dragged() {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                        } else if handle_resp.hovered() {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                                        }
                                        
                                        ui.painter().text(
                                            handle_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            "☰",
                                            egui::FontId::proportional(11.0),
                                            handle_color,
                                        );
                                    }

                                    // Handle reordering logic based on mouse drag position
                                    if handle_resp.dragged() {
                                        if let Some(pointer_pos) = ui.ctx().pointer_interact_pos() {
                                            let threshold = 4.0;
                                            if pointer_pos.y < handle_rect.min.y - threshold && i > 0 {
                                                action = Some(ViewAction::QueueMove { i, up: true });
                                            } else if pointer_pos.y > handle_rect.max.y + threshold && i + 1 < ordered.len() {
                                                action = Some(ViewAction::QueueMove { i, up: false });
                                            }
                                        }
                                    }

                                    // Jump button (plays this song directly)
                                    if btn(ui, ">").on_hover_text("Jump to this track").clicked() {
                                        action = Some(ViewAction::QueueJump(i));
                                    }

                                    ui.add_space(6.0);
                                    let time_color = if is_active { CRT_GREEN } else { CRT_MID };
                                    ui.label(
                                        RichText::new(fmt(t.duration_secs))
                                            .size(9.0)
                                            .color(time_color),
                                    );
                                },
                            );
                        });
                    });
                    ui.separator();
                }
            }
            ui.add_space(12.0);
        });

    action
}

fn idle(ui: &mut egui::Ui, msg: &str) {
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(RichText::new(msg).size(10.0).color(CRT_MID));
    });
}

fn btn(ui: &mut egui::Ui, glyph: &str) -> egui::Response {
    ui.add_space(2.0);
    ui.button(RichText::new(glyph).size(10.0).color(CRT_DIM))
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
