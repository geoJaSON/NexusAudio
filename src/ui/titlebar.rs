//! Top titlebar: logo, sys-info line, animated EQ bars, clock.

use eframe::egui::{self, FontFamily, FontId, RichText};

use super::theme::{AMBER, CRT_DIM, CRT_GREEN, CRT_MID, FONT_LOGO};

pub fn show(ui: &mut egui::Ui, eq_enabled: bool) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(
            RichText::new("NEXUS//AUDIO")
                .font(FontId::new(22.0, FontFamily::Name(FONT_LOGO.into())))
                .color(CRT_GREEN),
        );
        ui.add_space(12.0);

        ui.label(
            RichText::new("● SYS READY  |  BUILD 2.4.1  |  RODIO ENGINE PENDING SPIKE")
                .size(10.0)
                .color(CRT_DIM),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Clock (amber), updated every frame.
            let now = chrono::Local::now().format("%H:%M:%S").to_string();
            ui.label(RichText::new(now).size(11.0).color(AMBER));
            ui.add_space(12.0);

            if eq_enabled {
                eq_bars(ui);
            }
        });
    });
}

/// Five bars whose heights are driven by a time-seeded pseudo-random walk so
/// they animate without owning any state.
fn eq_bars(ui: &mut egui::Ui) {
    let t = ui.input(|i| i.time);
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
    ui.ctx().request_repaint(); // keep the animation running
}
