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

    let app_title = format!("NEXUS//AUDIO v{}", env!("CARGO_PKG_VERSION"));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(app_title.clone())
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([820.0, 560.0])
            .with_icon(std::sync::Arc::new(app_icon())),
        ..Default::default()
    };

    eframe::run_native(
        &app_title,
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}

/// Window/taskbar icon, decoded from `icon.ico` at the repo root (baked into
/// the binary at compile time — same file the .exe embeds). Swap that file
/// and rebuild to change the icon.
fn app_icon() -> egui::IconData {
    let dir = ico::IconDir::read(std::io::Cursor::new(
        include_bytes!("../icon.ico").as_slice(),
    ))
    .expect("icon.ico is a valid .ico");
    // Largest entry → crispest source for the window/taskbar.
    let entry = dir
        .entries()
        .iter()
        .max_by_key(|e| e.width())
        .expect("icon.ico has at least one image");
    let img = entry.decode().expect("icon.ico entry decodes");
    egui::IconData {
        rgba: img.rgba_data().to_vec(),
        width: img.width(),
        height: img.height(),
    }
}
