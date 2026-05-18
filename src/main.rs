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
            .with_min_inner_size([820.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "NEXUS//AUDIO v2.4.1",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
