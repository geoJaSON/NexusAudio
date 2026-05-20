//! Top titlebar: logo, sys-info line, animated EQ bars, clock.

use eframe::egui::{self, FontFamily, FontId, RichText};

use super::theme::{AMBER, CRT_DIM, CRT_GREEN, CRT_MID, FONT_LOGO};

pub fn show(ui: &mut egui::Ui, eq_enabled: bool, is_playing: bool) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(
            RichText::new("NEXUS//AUDIO")
                .font(FontId::new(22.0, FontFamily::Name(FONT_LOGO.into())))
                .color(CRT_GREEN),
        );
        ui.add_space(12.0);

        ui.label(
            RichText::new("* SYS READY  |  BUILD 2.4.1  |  SYMPHONIA ENGINE ACTIVE")
                .size(10.0)
                .color(CRT_DIM),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Clock (amber), updated every frame.
            let now = chrono::Local::now().format("%H:%M:%S").to_string();
            ui.label(RichText::new(now).size(11.0).color(AMBER));
            ui.add_space(12.0);

            if eq_enabled {
                eq_bars(ui, is_playing);
            }
        });
    });
}

/// Five bars whose heights are driven by a time-seeded pseudo-random walk.
/// Tracks animation time using egui's temporary storage, and increments it only
/// when audio is actively playing.
fn eq_bars(ui: &mut egui::Ui, is_playing: bool) {
    let id = ui.make_persistent_id("eq_bars_time");
    let mut t: f64 = ui.data(|d| d.get_temp(id).unwrap_or(0.0));

    if is_playing {
        let dt = ui.input(|i| i.stable_dt) as f64;
        t += dt;
        ui.data_mut(|d| d.insert_temp(id, t));
        ui.ctx().request_repaint(); // keep the animation running while playing
    }

    let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 18.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    for i in 0..5 {
        let phase = t * 6.0 + i as f64 * 1.3;
        let frac = (phase.sin() * 0.5 + 0.5) as f32;
        let h = 3.0 + frac * 15.0;
        let x = rect.left() + i as f32 * 5.0;
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x, rect.bottom() - h),
                egui::pos2(x + 3.0, rect.bottom()),
            ),
            0.0,
            CRT_GREEN,
        );
    }
    let _ = CRT_MID;
}
