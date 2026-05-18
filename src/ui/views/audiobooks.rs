//! Audiobooks view: library list with progress, plus a Now-Playing panel
//! (chapter navigation + sleep timer) when a book is playing.

use eframe::egui::{self, RichText};

use super::{LibraryUi, ViewAction};
use crate::library::models::Audiobook;
use crate::ui::theme::{AMBER, CRT_DIM, CRT_GREEN, CRT_MID};

const SLEEP_OPTS: &[(&str, Option<u64>)] = &[
    ("OFF", None),
    ("15", Some(15)),
    ("30", Some(30)),
    ("45", Some(45)),
    ("60", Some(60)),
];

pub fn show(
    ui: &mut egui::Ui,
    state: &mut LibraryUi,
    books: &[Audiobook],
    resume_pos: &dyn Fn(uuid::Uuid) -> Option<f64>,
    playing: Option<(&Audiobook, f64)>,
    sleep_left: Option<u64>,
) -> Option<ViewAction> {
    let mut action = None;

    // ---- Now Playing: chapter nav + sleep timer ----
    if let Some((book, pos)) = playing {
        now_playing(ui, book, pos, sleep_left, &mut action);
        ui.add_space(8.0);
    }

    // ---- toolbar ----
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let r = ui.add(
            egui::TextEdit::singleline(&mut state.ab_search)
                .hint_text("> SEARCH AUDIOBOOKS...")
                .desired_width(220.0),
        );
        let _ = r;
        ui.add_space(8.0);
        for (i, label) in ["TITLE", "AUTHOR", "GENRE", "PROGRESS"].iter().enumerate() {
            if ui
                .selectable_label(state.ab_sort == i as u8, RichText::new(*label).size(10.0))
                .clicked()
            {
                state.ab_sort = i as u8;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} BOOKS", books.len())).size(9.0).color(CRT_MID),
            );
        });
    });
    ui.separator();

    if books.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("NO AUDIOBOOKS — ADD A FOLDER IN [ FOLDERS ] AND SCAN")
                    .size(11.0)
                    .color(CRT_MID),
            );
        });
        return action;
    }

    // filter + sort (small N — fine to do in-memory)
    let q = state.ab_search.to_lowercase();
    let mut rows: Vec<&Audiobook> = books
        .iter()
        .filter(|b| {
            q.is_empty()
                || b.title.to_lowercase().contains(&q)
                || b.author.to_lowercase().contains(&q)
                || b.narrator
                    .as_deref()
                    .map(|n| n.to_lowercase().contains(&q))
                    .unwrap_or(false)
        })
        .collect();
    let prog = |b: &Audiobook| {
        if b.duration_secs > 0.0 {
            resume_pos(b.id).unwrap_or(0.0) / b.duration_secs
        } else {
            0.0
        }
    };
    match state.ab_sort {
        1 => rows.sort_by(|a, b| a.author.to_lowercase().cmp(&b.author.to_lowercase())),
        2 => rows.sort_by(|a, b| a.genre.to_lowercase().cmp(&b.genre.to_lowercase())),
        3 => rows.sort_by(|a, b| {
            prog(b).partial_cmp(&prog(a)).unwrap_or(std::cmp::Ordering::Equal)
        }),
        _ => rows.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for b in rows {
                let p = prog(b);
                let resumed = resume_pos(b.id).unwrap_or(0.0);
                let row = super::list_row(ui, 44.0, |ui| {
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&b.title).size(12.0).color(CRT_DIM));
                        let mut sub = b.author.clone();
                        if let Some(n) = &b.narrator {
                            sub.push_str(&format!("  ·  narr. {n}"));
                        }
                        ui.label(RichText::new(sub).size(10.0).color(CRT_MID));
                    });
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add_space(10.0);
                            let (txt, col) = if p >= 0.999 {
                                ("COMPLETE".to_string(), CRT_DIM)
                            } else if resumed > 1.0 {
                                (format!("{}%  >  {}", (p * 100.0) as u32, hms(resumed)), AMBER)
                            } else {
                                ("UNPLAYED".to_string(), CRT_MID)
                            };
                            ui.label(RichText::new(txt).size(9.0).color(col));
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(hms(b.duration_secs))
                                    .size(9.0)
                                    .color(CRT_MID),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(&b.genre).size(9.0).color(CRT_MID),
                            );
                        },
                    );
                });
                // thin progress bar along the row bottom
                if p > 0.0 {
                    let r = row.rect;
                    let y = r.bottom() - 2.0;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(r.left(), y),
                            egui::pos2(r.left() + r.width() * p as f32, y + 2.0),
                        ),
                        0.0,
                        AMBER,
                    );
                }
                if row.clicked() || row.double_clicked() {
                    action = Some(ViewAction::OpenAudiobook(b.id));
                }
                ui.separator();
            }
        });

    action
}

fn now_playing(
    ui: &mut egui::Ui,
    book: &Audiobook,
    pos: f64,
    sleep_left: Option<u64>,
    action: &mut Option<ViewAction>,
) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("> NOW PLAYING").size(9.0).color(CRT_MID));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // sleep timer
            if let Some(m) = sleep_left {
                ui.label(RichText::new(format!("SLEEP {m}m")).size(9.0).color(AMBER));
            }
            for (label, val) in SLEEP_OPTS.iter().rev() {
                if ui
                    .button(RichText::new(*label).size(9.0).color(CRT_MID))
                    .clicked()
                {
                    *action = Some(ViewAction::SetSleepTimer(*val));
                }
            }
            ui.label(RichText::new("SLEEP:").size(9.0).color(CRT_MID));
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(RichText::new(&book.title).size(13.0).color(CRT_GREEN));
    });

    if book.chapters.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(
                RichText::new("no chapters (flat timeline)").size(10.0).color(CRT_MID),
            );
        });
        return;
    }

    let cur = book
        .chapters
        .iter()
        .rposition(|c| pos + 0.5 >= c.start_secs)
        .unwrap_or(0);

    ui.horizontal(|ui| {
        ui.add_space(14.0);
        if ui.button(RichText::new("|< PREV CH").size(9.0).color(CRT_DIM)).clicked()
            && cur > 0
        {
            *action = Some(ViewAction::ChapterSeek(book.chapters[cur - 1].start_secs));
        }
        if ui.button(RichText::new("NEXT CH >|").size(9.0).color(CRT_DIM)).clicked()
            && cur + 1 < book.chapters.len()
        {
            *action = Some(ViewAction::ChapterSeek(book.chapters[cur + 1].start_secs));
        }
        ui.label(
            RichText::new(format!(
                "CH {}/{}: {}",
                cur + 1,
                book.chapters.len(),
                book.chapters[cur].title
            ))
            .size(10.0)
            .color(AMBER),
        );
    });

    egui::ScrollArea::vertical()
        .id_salt("chapter_list")
        .max_height(160.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, c) in book.chapters.iter().enumerate() {
                let here = i == cur;
                let resp = super::list_row(ui, 22.0, |ui| {
                    ui.add_space(16.0);
                    ui.label(
                        RichText::new(format!("{:>2}", i + 1))
                            .size(9.0)
                            .color(CRT_MID),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(&c.title)
                            .size(10.0)
                            .color(if here { CRT_GREEN } else { CRT_DIM }),
                    );
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new(hms(c.start_secs))
                                    .size(9.0)
                                    .color(CRT_MID),
                            );
                        },
                    );
                });
                if resp.clicked() {
                    *action = Some(ViewAction::ChapterSeek(c.start_secs));
                }
            }
        });
}

fn hms(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}
