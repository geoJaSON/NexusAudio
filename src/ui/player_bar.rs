//! Player bar: scrub line, now-playing, transport, time, volume.
//! Emits a `PlayerCmd` for anything the App must coordinate (queue + engine).

use eframe::egui::{self, RichText};

use crate::library::models::RepeatMode;
use crate::player::engine::Engine;
use crate::ui::theme::{AMBER, CRT_DARK, CRT_DIM, CRT_GREEN, CRT_MID};

/// What the player bar shows as "now playing" — built by the App from either
/// the music queue or the current audiobook (+ chapter), so the bar doesn't
/// need to know about `Track` vs `Audiobook`.
#[derive(Default, Clone)]
pub struct NowPlaying {
    pub title: String,
    pub subtitle: String,
    /// Trailing detail on the status line (codec/sr, or chapter).
    pub badge: String,
}

#[derive(Debug, Clone)]
pub enum PlayerCmd {
    PlayPause,
    Stop,
    Prev,
    Next,
    Seek(f64),
    ToggleShuffle,
    CycleRepeat,
    ToggleQueue,
}

pub fn show(
    ui: &mut egui::Ui,
    engine: &Engine,
    now: Option<&NowPlaying>,
    shuffle: bool,
    repeat: &RepeatMode,
) -> Option<PlayerCmd> {
    let mut cmd = None;
    let dur = engine.duration_secs();
    let pos = engine.position_secs();

    // ---- scrub line (full width, click to seek) ----
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 4.0),
        egui::Sense::click_and_drag(),
    );
    let frac = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let p = ui.painter();
    p.rect_filled(rect, 0.0, CRT_DARK);
    let mut fill = rect;
    fill.set_width(rect.width() * frac);
    p.rect_filled(fill, 0.0, CRT_GREEN);
    if (resp.clicked() || resp.dragged()) && dur > 0.0 {
        if let Some(m) = resp.interact_pointer_pos() {
            let f = ((m.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
            cmd = Some(PlayerCmd::Seek(f * dur));
        }
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);

        // now playing
        ui.vertical(|ui| {
            match now {
                Some(n) => {
                    ui.label(RichText::new(&n.title).size(13.0).color(CRT_GREEN));
                    ui.label(RichText::new(&n.subtitle).size(10.0).color(CRT_DIM));
                    let state = if engine.is_playing() { "> PLAYING" } else { "|| PAUSED" };
                    let line = if n.badge.is_empty() {
                        state.to_string()
                    } else {
                        format!("{state} · {}", n.badge)
                    };
                    ui.label(RichText::new(line).size(9.0).color(AMBER));
                }
                None => {
                    ui.label(RichText::new("— NOTHING PLAYING —").size(13.0).color(CRT_DIM));
                    ui.label(
                        RichText::new("double-click a track to play")
                            .size(10.0)
                            .color(CRT_MID),
                    );
                }
            }
        });

        // transport (centered-ish)
        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.horizontal(|ui| {
                if tbtn(ui, "|<").clicked() {
                    cmd = Some(PlayerCmd::Prev);
                }
                let play_lbl = if engine.is_playing() { "||" } else { ">" };
                if tbtn(ui, play_lbl).clicked() {
                    cmd = Some(PlayerCmd::PlayPause);
                }
                if tbtn(ui, ">|").clicked() {
                    cmd = Some(PlayerCmd::Next);
                }
                if tbtn(ui, "[ ]").clicked() {
                    cmd = Some(PlayerCmd::Stop);
                }
                let sh = if shuffle { CRT_GREEN } else { CRT_MID };
                if ui
                    .button(RichText::new("SHUF").size(10.0).color(sh))
                    .clicked()
                {
                    cmd = Some(PlayerCmd::ToggleShuffle);
                }
                let (rsym, rcol) = match repeat {
                    RepeatMode::None => ("RPT", CRT_MID),
                    RepeatMode::All => ("RPT", CRT_GREEN),
                    RepeatMode::One => ("RP1", CRT_GREEN),
                };
                if ui
                    .button(RichText::new(rsym).size(10.0).color(rcol))
                    .clicked()
                {
                    cmd = Some(PlayerCmd::CycleRepeat);
                }
                if ui
                    .button(RichText::new("QUEUE").size(10.0).color(CRT_MID))
                    .on_hover_text("Toggle queue (Q)")
                    .clicked()
                {
                    cmd = Some(PlayerCmd::ToggleQueue);
                }
            });
            ui.label(
                RichText::new(format!("{} / {}", fmt(pos), fmt(dur)))
                    .size(10.0)
                    .color(CRT_DIM),
            );
        });

        // volume
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut v = engine.volume();
            ui.label(
                RichText::new(format!("{:>3.0}%", v * 100.0))
                    .size(9.0)
                    .color(CRT_DIM),
            );
            if ui
                .add(
                    egui::Slider::new(&mut v, 0.0..=1.0)
                        .show_value(false)
                        .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.4 }),
                )
                .changed()
            {
                engine.set_volume(v);
            }
            ui.label(RichText::new("VOL").size(9.0).color(CRT_MID));
        });
    });
    ui.add_space(4.0);
    cmd
}

fn tbtn(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.button(RichText::new(label).size(12.0).color(CRT_GREEN))
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
