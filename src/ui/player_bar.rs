//! Player bar: scrub line, now-playing, transport, time, volume.
//! Emits a `PlayerCmd` for anything the App must coordinate (queue + engine).

use eframe::egui::{self, RichText};

use crate::library::models::{RepeatMode, Track};
use crate::player::engine::Engine;
use crate::ui::theme::{AMBER, CRT_DARK, CRT_DIM, CRT_GREEN, CRT_MID};

#[derive(Debug, Clone)]
pub enum PlayerCmd {
    PlayPause,
    Stop,
    Prev,
    Next,
    Seek(f64),
    ToggleShuffle,
    CycleRepeat,
}

pub fn show(
    ui: &mut egui::Ui,
    engine: &Engine,
    current: Option<&Track>,
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
            match current {
                Some(t) => {
                    ui.label(RichText::new(&t.title).size(13.0).color(CRT_GREEN));
                    let mut sub = format!("{} · {}", t.artist, t.album);
                    if let Some(y) = t.year {
                        sub.push_str(&format!(" · {y}"));
                    }
                    ui.label(RichText::new(sub).size(10.0).color(CRT_DIM));
                    let info = engine.info();
                    let state = if engine.is_playing() { "▸ PLAYING" } else { "❚❚ PAUSED" };
                    ui.label(
                        RichText::new(format!(
                            "{state} · {} {} Hz",
                            info.codec, info.sample_rate
                        ))
                        .size(9.0)
                        .color(AMBER),
                    );
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
                if tbtn(ui, "⏮").clicked() {
                    cmd = Some(PlayerCmd::Prev);
                }
                let play_lbl = if engine.is_playing() { "❚❚" } else { "▶" };
                if tbtn(ui, play_lbl).clicked() {
                    cmd = Some(PlayerCmd::PlayPause);
                }
                if tbtn(ui, "⏭").clicked() {
                    cmd = Some(PlayerCmd::Next);
                }
                if tbtn(ui, "⏹").clicked() {
                    cmd = Some(PlayerCmd::Stop);
                }
                let sh = if shuffle { CRT_GREEN } else { CRT_MID };
                if ui
                    .button(RichText::new("⇄").size(11.0).color(sh))
                    .clicked()
                {
                    cmd = Some(PlayerCmd::ToggleShuffle);
                }
                let (rsym, rcol) = match repeat {
                    RepeatMode::None => ("↻", CRT_MID),
                    RepeatMode::All => ("↻", CRT_GREEN),
                    RepeatMode::One => ("↺¹", CRT_GREEN),
                };
                if ui
                    .button(RichText::new(rsym).size(11.0).color(rcol))
                    .clicked()
                {
                    cmd = Some(PlayerCmd::CycleRepeat);
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
