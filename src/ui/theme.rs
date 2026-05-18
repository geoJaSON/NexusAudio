//! CRT phosphor-terminal theme. Colors taken verbatim from the mockup palette.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, Rounding, Stroke};

pub const CRT_BG: Color32 = Color32::from_rgb(2, 15, 4);
pub const CRT_PANEL: Color32 = Color32::from_rgb(3, 13, 5);
pub const CRT_GREEN: Color32 = Color32::from_rgb(0, 255, 65);
pub const CRT_DIM: Color32 = Color32::from_rgb(0, 179, 44);
pub const CRT_DARK: Color32 = Color32::from_rgb(0, 61, 15);
pub const CRT_MID: Color32 = Color32::from_rgb(0, 92, 20);
pub const AMBER: Color32 = Color32::from_rgb(255, 183, 0);
pub const AMBER_DIM: Color32 = Color32::from_rgb(179, 127, 0);
pub const RED_ALERT: Color32 = Color32::from_rgb(255, 49, 49);

/// Font family aliases. `Body` = Share Tech Mono, `Logo` = VT323.
pub const FONT_BODY: &str = "share_tech_mono";
pub const FONT_LOGO: &str = "vt323";

/// Load bundled TTFs if present, otherwise fall back to egui's monospace so the
/// app still runs before the user drops fonts into assets/fonts/.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    let body = include_font("assets/fonts/ShareTechMono-Regular.ttf");
    let logo = include_font("assets/fonts/VT323-Regular.ttf");

    if let Some(bytes) = body {
        fonts
            .font_data
            .insert(FONT_BODY.to_owned(), FontData::from_owned(bytes));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, FONT_BODY.to_owned());
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, FONT_BODY.to_owned());
    }
    if let Some(bytes) = logo {
        fonts
            .font_data
            .insert(FONT_LOGO.to_owned(), FontData::from_owned(bytes));
        fonts
            .families
            .insert(FontFamily::Name(FONT_LOGO.into()), vec![FONT_LOGO.to_owned()]);
    }

    // The logo family must always resolve or egui panics at render time.
    // Without the bundled TTF, alias it to whatever Monospace resolves to.
    if !fonts.families.contains_key(&FontFamily::Name(FONT_LOGO.into())) {
        let mono = fonts
            .families
            .get(&FontFamily::Monospace)
            .cloned()
            .unwrap_or_default();
        fonts
            .families
            .insert(FontFamily::Name(FONT_LOGO.into()), mono);
    }

    ctx.set_fonts(fonts);
}

fn include_font(rel: &str) -> Option<Vec<u8>> {
    std::fs::read(rel).ok()
}

/// Apply the green-on-black phosphor visuals to egui.
pub fn apply_visuals(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(CRT_DIM);
    v.panel_fill = CRT_BG;
    v.window_fill = CRT_PANEL;
    v.extreme_bg_color = CRT_BG;
    v.faint_bg_color = Color32::from_rgba_unmultiplied(0, 255, 65, 12);
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(0, 255, 65, 40);
    v.selection.stroke = Stroke::new(1.0, CRT_GREEN);
    v.hyperlink_color = AMBER;

    for ws in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        ws.bg_fill = CRT_PANEL;
        ws.weak_bg_fill = CRT_PANEL;
        ws.rounding = Rounding::ZERO;
        ws.bg_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 255, 65, 51));
        ws.fg_stroke = Stroke::new(1.0, CRT_DIM);
    }
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, CRT_GREEN);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, CRT_GREEN);
    v.widgets.active.fg_stroke = Stroke::new(1.0, CRT_GREEN);

    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    ctx.set_style(style);
}
