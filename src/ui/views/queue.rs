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
    session_history: &[crate::library::models::Track],
) -> Option<ViewAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("// QUEUE").size(10.0).color(CRT_MID));
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
            .button(RichText::new("CLEAR").size(9.0).color(AMBER))
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

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // ---- NOW PLAYING ----
            section(ui, "> NOW PLAYING");
            match queue.current() {
                Some(t) => {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&t.title).size(12.0).color(CRT_GREEN));
                            ui.label(
                                RichText::new(format!("{} · {}", t.artist, t.album))
                                    .size(10.0)
                                    .color(CRT_DIM),
                            );
                        });
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.add_space(14.0);
                                ui.label(
                                    RichText::new(fmt(t.duration_secs))
                                        .size(10.0)
                                        .color(CRT_MID),
                                );
                            },
                        );
                    });
                }
                None => idle(ui, "nothing playing"),
            }

            // ---- UP NEXT ----
            ui.add_space(8.0);
            let up = queue.upcoming();
            section(ui, &format!("UP NEXT ({})", up.len()));
            if up.is_empty() {
                idle(ui, "queue is empty — double-click or right-click tracks");
            }
            for (i, t) in up.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.add_sized(
                        [26.0, 18.0],
                        egui::Label::new(
                            RichText::new(format!("{:>2}", i + 1)).size(10.0).color(CRT_MID),
                        ),
                    );
                    ui.add_sized(
                        [ui.available_width() - 150.0, 18.0],
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
                            if btn(ui, "x").clicked() {
                                action = Some(ViewAction::QueueRemove(i));
                            }
                            if btn(ui, "v").clicked() {
                                action = Some(ViewAction::QueueMove { i, up: false });
                            }
                            if btn(ui, "^").clicked() {
                                action = Some(ViewAction::QueueMove { i, up: true });
                            }
                            if btn(ui, ">").clicked() {
                                action = Some(ViewAction::QueueJump(i));
                            }
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(fmt(t.duration_secs))
                                    .size(9.0)
                                    .color(CRT_MID),
                            );
                        },
                    );
                });
                ui.separator();
            }

            // ---- SESSION HISTORY (everything played this run, newest first) ----
            ui.add_space(8.0);
            section(ui, "PLAYED THIS SESSION");
            if session_history.is_empty() {
                idle(ui, "nothing played yet");
            }
            for t in session_history.iter().rev().take(50) {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.add_sized(
                        [ui.available_width() - 60.0, 16.0],
                        egui::Label::new(
                            RichText::new(format!("{}  —  {}", t.title, t.artist))
                                .size(10.0)
                                .color(CRT_MID),
                        )
                        .truncate(),
                    );
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.label(
                                RichText::new(fmt(t.duration_secs))
                                    .size(9.0)
                                    .color(CRT_MID),
                            );
                        },
                    );
                });
            }
            ui.add_space(12.0);
        });

    action
}

fn section(ui: &mut egui::Ui, label: &str) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(label).size(9.0).color(CRT_MID));
    });
    ui.add_space(2.0);
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
