//! NEXUS//AUDIO — eframe entry point.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audiobooks;
mod library;
mod player;
mod playlists;
mod settings;
mod store;
mod ui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("--scan-smoke") {
        let folder = args.get(2).map(std::path::PathBuf::from).unwrap_or_default();
        library::scanner::smoke(&folder);
        return Ok(());
    }
    if args.get(1).map(|s| s.as_str()) == Some("--ab-smoke") {
        let p = args.get(2).map(std::path::PathBuf::from).unwrap_or_default();
        audiobooks::scanner::smoke(&p);
        return Ok(());
    }
    if args.get(1).map(|s| s.as_str()) == Some("--chapter-spike") {
        let p = args.get(2).map(std::path::PathBuf::from).unwrap_or_default();
        audiobooks::chapters::spike(&p);
        return Ok(());
    }
    if args.get(1).map(|s| s.as_str()) == Some("--play-smoke") {
        let path = args.get(2).map(std::path::PathBuf::from).unwrap_or_default();
        let start: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let eng = player::engine::Engine::new();
        eng.set_volume(0.5);
        eng.load(path, start);
        for _ in 0..12 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let i = eng.info();
            println!(
                "pos={:.2}s dur={:.1}s playing={} ended={} codec={} sr={}",
                eng.position_secs(),
                eng.duration_secs(),
                eng.is_playing(),
                "n/a",
                i.codec,
                i.sample_rate
            );
        }
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NEXUS//AUDIO v2.4.1")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([820.0, 560.0])
            .with_icon(std::sync::Arc::new(app_icon())),
        ..Default::default()
    };

    eframe::run_native(
        "NEXUS//AUDIO v2.4.1",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}

/// Procedural window/taskbar icon — CRT phosphor look, no asset file: dark
/// rounded tile, scanlines, green border, bold play triangle.
fn app_icon() -> egui::IconData {
    const S: usize = 256;
    let bg = [2u8, 15, 4];
    let green = [0u8, 255, 65];
    let mut rgba = vec![0u8; S * S * 4];

    let radius = S as f32 * 0.18;
    let put = |buf: &mut [u8], x: usize, y: usize, c: [u8; 3], a: u8| {
        let i = (y * S + x) * 4;
        buf[i] = c[0];
        buf[i + 1] = c[1];
        buf[i + 2] = c[2];
        buf[i + 3] = a;
    };
    // Rounded-rect corner mask.
    let inside = |x: f32, y: f32| -> bool {
        let (mut dx, mut dy) = (0.0f32, 0.0f32);
        if x < radius {
            dx = radius - x;
        } else if x > S as f32 - radius {
            dx = x - (S as f32 - radius);
        }
        if y < radius {
            dy = radius - y;
        } else if y > S as f32 - radius {
            dy = y - (S as f32 - radius);
        }
        dx * dx + dy * dy <= radius * radius
    };
    // Play triangle vertices.
    let (ax, ay) = (S as f32 * 0.37, S as f32 * 0.27);
    let (bx, by) = (S as f32 * 0.37, S as f32 * 0.73);
    let (cx, cy) = (S as f32 * 0.73, S as f32 * 0.50);
    let sign = |px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32| {
        (px - x2) * (y1 - y2) - (x1 - x2) * (py - y2)
    };

    for y in 0..S {
        for x in 0..S {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            if !inside(fx, fy) {
                put(&mut rgba, x, y, bg, 0);
                continue;
            }
            // Base + faint scanlines.
            let mut col = bg;
            if y % 6 < 1 {
                col = [1, 9, 3];
            }
            // Green border ring.
            let edge = (x as f32)
                .min(y as f32)
                .min((S - 1 - x) as f32)
                .min((S - 1 - y) as f32);
            if edge < 6.0 {
                col = green;
            }
            // Play triangle (point-in-triangle).
            let d1 = sign(fx, fy, ax, ay, bx, by);
            let d2 = sign(fx, fy, bx, by, cx, cy);
            let d3 = sign(fx, fy, cx, cy, ax, ay);
            let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
            let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
            if !(has_neg && has_pos) {
                col = green;
            }
            put(&mut rgba, x, y, col, 255);
        }
    }

    egui::IconData { rgba, width: S as u32, height: S as u32 }
}
