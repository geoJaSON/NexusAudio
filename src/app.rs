//! Root application: holds top-level state and drives the egui frame.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use eframe::egui::{self, RichText};

use crate::library::db::Db;
use crate::library::scanner::{self, ScanMsg};
use crate::player::engine::Engine;
use crate::player::queue::{Queue, QueueSnapshot};
use crate::playlists::PlaylistStore;
use crate::settings::Settings;
use crate::store::json_store;
use crate::ui::player_bar::{self, PlayerCmd};
use crate::ui::theme::{self, AMBER, CRT_DIM, CRT_GREEN, CRT_MID, CRT_PANEL};
use crate::ui::views::{
    albums, artists, folders, playlists as playlists_view, queue as queue_view, tracks,
    LibraryUi, ViewAction,
};
use crate::ui::{sidebar, titlebar, View};

pub struct App {
    db: Db,
    db_path: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    settings: Settings,
    view: View,
    lib: LibraryUi,
    track_count: u64,
    scan_rx: Option<Receiver<ScanMsg>>,
    scan_status: Option<String>,
    engine: Engine,
    queue: Queue,
    playlists: PlaylistStore,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        theme::apply_visuals(&cc.egui_ctx);

        let data_dir = data_dir();
        let (db, db_path) = match &data_dir {
            Some(dir) => {
                let p = dir.join("library.db");
                match Db::open(&p) {
                    Ok(db) => (db, Some(p)),
                    Err(e) => {
                        eprintln!("library.db unavailable ({e}); in-memory");
                        (Db::open_in_memory().expect("in-memory db"), None)
                    }
                }
            }
            None => (Db::open_in_memory().expect("in-memory db"), None),
        };
        let settings = data_dir
            .as_ref()
            .map(|d| Settings::load(d))
            .unwrap_or_default();
        let track_count = db.track_count().unwrap_or(0);
        let playlists = data_dir
            .as_ref()
            .map(|d| PlaylistStore::load(d))
            .unwrap_or_default();

        let mut app = Self {
            db,
            db_path,
            data_dir,
            settings,
            view: View::default(),
            lib: LibraryUi::default(),
            track_count,
            scan_rx: None,
            scan_status: None,
            engine: Engine::new(),
            queue: Queue::default(),
            playlists,
        };
        // Restore the saved queue (list + cursor + modes), idle until played.
        if let Some(dir) = &app.data_dir {
            let snap: QueueSnapshot = json_store::load_or_default(&dir.join("queue.json"));
            app.queue = Queue::restore(snap);
        }
        if app.settings.auto_scan_on_startup && !app.settings.music_folders.is_empty() {
            app.start_scan();
        }
        app
    }

    fn save_queue(&self) {
        if let Some(dir) = &self.data_dir {
            if let Err(e) = json_store::save(&dir.join("queue.json"), &self.queue.snapshot()) {
                eprintln!("queue save failed: {e}");
            }
        }
    }

    fn save_playlists(&self) {
        if let Some(dir) = &self.data_dir {
            self.playlists.save(dir);
        }
    }

    fn playlist_names(&self) -> Vec<(uuid::Uuid, String)> {
        self.playlists
            .lists
            .iter()
            .map(|p| (p.id, p.name.clone()))
            .collect()
    }

    fn start_scan(&mut self) {
        let Some(db_path) = self.db_path.clone() else {
            self.scan_status = Some("SCAN UNAVAILABLE (no data dir)".into());
            return;
        };
        if self.scan_rx.is_some() {
            return;
        }
        self.scan_status = Some("SCAN STARTING…".into());
        self.scan_rx = Some(scanner::spawn_scan(
            db_path,
            self.settings.music_folders.clone(),
        ));
    }

    fn poll_scan(&mut self) {
        let msgs: Vec<ScanMsg> = match &self.scan_rx {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };
        let mut finished = false;
        for msg in msgs {
            match msg {
                ScanMsg::Started { total } => {
                    self.scan_status = Some(format!("SCANNING 0 / {total}"));
                }
                ScanMsg::Progress { done, total, current } => {
                    self.scan_status =
                        Some(format!("SCANNING {done} / {total}  ·  {current}"));
                }
                ScanMsg::Done { added, updated, removed, errors } => {
                    self.scan_status = Some(format!(
                        "SCAN DONE  ·  +{added} ~{updated} -{removed} !{errors}"
                    ));
                    self.on_scan_complete();
                    finished = true;
                }
                ScanMsg::Failed(e) => {
                    self.scan_status = Some(format!("SCAN FAILED: {e}"));
                    finished = true;
                }
            }
        }
        if finished {
            self.scan_rx = None;
        }
    }

    fn on_scan_complete(&mut self) {
        self.track_count = self.db.track_count().unwrap_or(0);
        self.lib.invalidate();
        let now = chrono::Utc::now();
        for folder in self.settings.music_folders.clone() {
            let key = folder.display().to_string();
            let count = scanner::count_files(&folder);
            let stat = self.settings.folder_stats.entry(key).or_default();
            stat.last_scan = Some(now);
            stat.file_count = count;
        }
        self.save_settings();
    }

    fn save_settings(&self) {
        if let Some(dir) = &self.data_dir {
            self.settings.save(dir);
        }
    }

    /// Load + play whatever the queue cursor currently points at.
    fn play_current(&self) {
        if let Some(t) = self.queue.current() {
            self.engine.load(t.path.clone(), 0.0);
            self.engine.play();
        }
    }

    fn handle_view(&mut self, action: ViewAction) {
        match action {
            ViewAction::AddMusicFolder => {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    if !self.settings.music_folders.contains(&dir) {
                        self.settings.music_folders.push(dir);
                        self.save_settings();
                        self.start_scan();
                    }
                }
            }
            ViewAction::RemoveFolder(p) => {
                self.settings.music_folders.retain(|f| f != &p);
                self.save_settings();
            }
            ViewAction::ScanAll => self.start_scan(),
            ViewAction::Play { list, index, shuffle } => {
                self.queue.set(list, index);
                if shuffle != self.queue.shuffle {
                    self.queue.toggle_shuffle();
                }
                self.play_current();
                self.save_queue();
            }
            ViewAction::Enqueue { track, next } => {
                if next {
                    self.queue.play_next(track);
                } else {
                    self.queue.enqueue(track);
                }
                self.save_queue();
            }
            ViewAction::QueueJump(i) => {
                if self.queue.jump_upcoming(i).is_some() {
                    self.play_current();
                    self.save_queue();
                }
            }
            ViewAction::QueueRemove(i) => {
                self.queue.remove_upcoming(i);
                self.save_queue();
            }
            ViewAction::QueueMove { i, up } => {
                self.queue.move_upcoming(i, up);
                self.save_queue();
            }
            ViewAction::QueueClear => {
                self.queue.clear_upcoming();
                self.save_queue();
            }
            ViewAction::PlaylistSelect(id) => {
                self.lib.selected_playlist = Some(id);
                self.lib.rename_buf = None;
                self.view = View::Playlists;
            }
            ViewAction::PlaylistNew => {
                let id = self.playlists.create("NEW PLAYLIST");
                self.lib.selected_playlist = Some(id);
                self.lib.rename_buf = Some(self.playlists.get(id).unwrap().name.clone());
                self.view = View::Playlists;
                self.save_playlists();
            }
            ViewAction::PlaylistDelete(id) => {
                self.playlists.delete(id);
                if self.lib.selected_playlist == Some(id) {
                    self.lib.selected_playlist = None;
                }
                self.save_playlists();
            }
            ViewAction::PlaylistDuplicate(id) => {
                if let Some(new_id) = self.playlists.duplicate(id) {
                    self.lib.selected_playlist = Some(new_id);
                }
                self.save_playlists();
            }
            ViewAction::PlaylistRename(id, name) => {
                self.playlists.rename(id, name.trim());
                self.lib.rename_buf = None;
                self.save_playlists();
            }
            ViewAction::PlaylistRemoveAt(id, idx) => {
                self.playlists.remove_at(id, idx);
                self.save_playlists();
            }
            ViewAction::PlaylistMoveAt { id, i, up } => {
                self.playlists.move_at(id, i, up);
                self.save_playlists();
            }
            ViewAction::PlaylistAddTrack { playlist, track } => {
                let id = playlist.unwrap_or_else(|| {
                    let id = self.playlists.create("NEW PLAYLIST");
                    self.lib.selected_playlist = Some(id);
                    id
                });
                self.playlists.add_track(id, track.id);
                self.save_playlists();
            }
            ViewAction::PlaylistExport(id) => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("M3U", &["m3u"])
                    .set_file_name("playlist.m3u")
                    .save_file()
                {
                    let db = &self.db;
                    let _ = self.playlists.export_m3u(
                        id,
                        |tid| db.track_by_id(tid),
                        &path,
                    );
                }
            }
            ViewAction::PlaylistImport => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("M3U", &["m3u", "m3u8"])
                    .pick_file()
                {
                    let db = &self.db;
                    if let Ok(id) =
                        self.playlists.import_m3u(&path, |p| db.track_id_by_path(p))
                    {
                        self.lib.selected_playlist = Some(id);
                        self.view = View::Playlists;
                    }
                    self.save_playlists();
                }
            }
        }
    }

    fn handle_player(&mut self, cmd: PlayerCmd) {
        match cmd {
            PlayerCmd::PlayPause => {
                if self.queue.current().is_none() {
                } else if self.engine.has_track() {
                    self.engine.toggle_pause();
                } else {
                    self.play_current();
                }
            }
            PlayerCmd::Stop => self.engine.stop(),
            PlayerCmd::Next => {
                if self.queue.next().is_some() {
                    self.play_current();
                } else {
                    self.engine.stop();
                }
            }
            PlayerCmd::Prev => {
                // Restart current if >3s in, else step back.
                if self.engine.position_secs() > 3.0 {
                    self.engine.seek(0.0);
                } else if self.queue.prev().is_some() {
                    self.play_current();
                }
            }
            PlayerCmd::Seek(s) => self.engine.seek(s),
            PlayerCmd::ToggleShuffle => self.queue.toggle_shuffle(),
            PlayerCmd::CycleRepeat => self.queue.cycle_repeat(),
        }
    }

    /// Track finished naturally → advance the queue.
    fn poll_playback(&mut self) {
        if self.engine.take_ended() {
            if self.queue.next().is_some() {
                self.play_current();
            }
            self.save_queue();
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return; // a text field (e.g. search) has focus
        }
        let k = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::OpenBracket),
                i.key_pressed(egui::Key::CloseBracket),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::S),
                i.key_pressed(egui::Key::R),
            )
        });
        if k.0 {
            self.handle_player(PlayerCmd::PlayPause);
        }
        if k.1 {
            self.engine.seek_rel(-10.0);
        }
        if k.2 {
            self.engine.seek_rel(10.0);
        }
        if k.3 {
            self.handle_player(PlayerCmd::Prev);
        }
        if k.4 {
            self.handle_player(PlayerCmd::Next);
        }
        if k.5 {
            self.engine.add_volume(0.05);
        }
        if k.6 {
            self.engine.add_volume(-0.05);
        }
        if k.7 {
            self.queue.toggle_shuffle();
        }
        if k.8 {
            self.queue.cycle_repeat();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_scan();
        self.poll_playback();
        self.shortcuts(ctx);

        // Repaint while scanning or playing so progress/clock stay live.
        if self.scan_rx.is_some() || self.engine.is_playing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        let eq = self.settings.eq_enabled;
        egui::TopBottomPanel::top("titlebar")
            .frame(panel_frame())
            .show(ctx, |ui| titlebar::show(ui, eq));

        egui::TopBottomPanel::bottom("statusbar")
            .frame(panel_frame())
            .show(ctx, |ui| self.status_bar(ui));

        let mut player_cmd = None;
        egui::TopBottomPanel::bottom("playerbar")
            .frame(panel_frame())
            .show(ctx, |ui| {
                player_cmd = player_bar::show(
                    ui,
                    &self.engine,
                    self.queue.current(),
                    self.queue.shuffle,
                    &self.queue.repeat,
                );
            });

        // Owned snapshot so the sidebar/views don't hold a borrow on
        // self.playlists while other &mut self fields are in use.
        let pls = self.playlist_names();
        let selected_pl = self.lib.selected_playlist;

        let mut sidebar_action = None;
        egui::SidePanel::left("sidebar")
            .exact_width(180.0)
            .resizable(false)
            .frame(panel_frame())
            .show(ctx, |ui| {
                sidebar_action = sidebar::show(ui, &mut self.view, &pls, selected_pl);
            });

        let mut view_action = None;
        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Tracks => {
                view_action = tracks::show(ui, &self.db, &mut self.lib, &pls)
            }
            View::Albums => {
                view_action = albums::show(ui, &self.db, &mut self.lib, &pls)
            }
            View::Artists => {
                view_action = artists::show(ui, &self.db, &mut self.lib, &pls)
            }
            View::Queue => view_action = queue_view::show(ui, &self.queue),
            View::Playlists => {
                view_action =
                    playlists_view::show(ui, &self.db, &self.playlists, &mut self.lib)
            }
            View::Folders => {
                view_action =
                    folders::show(ui, &self.settings, self.scan_status.as_deref());
            }
            View::Audiobooks => placeholder(ui, "AUDIOBOOKS", "Phase 6"),
            View::Settings => placeholder(ui, "SETTINGS", "Phase 7"),
        });

        if let Some(c) = player_cmd {
            self.handle_player(c);
        }
        if let Some(a) = sidebar_action {
            self.handle_view(a);
        }
        if let Some(a) = view_action {
            self.handle_view(a);
        }
    }

    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.save_settings();
        self.save_queue();
        self.save_playlists();
    }
}

impl App {
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let info = self.engine.info();
        let cur = self.queue.current().cloned();
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let item = |ui: &mut egui::Ui, k: &str, v: &str| {
                ui.label(RichText::new(k).size(9.0).color(CRT_MID));
                ui.label(RichText::new(v).size(9.0).color(CRT_DIM));
                ui.add_space(12.0);
            };
            item(ui, "LIBRARY:", &format!("{} TRACKS", self.track_count));
            item(ui, "QUEUE:", &format!("{} TRACKS", self.queue.len()));
            if let Some(t) = &cur {
                item(ui, "CODEC:", if info.codec.is_empty() { &t.codec } else { &info.codec });
                if let Some(b) = t.bitrate_kbps {
                    item(ui, "BITRATE:", &format!("{b} kbps"));
                }
                if info.sample_rate > 0 {
                    item(ui, "SAMPLE:", &format!("{} Hz", info.sample_rate));
                }
            }
            if let Some(s) = &self.scan_status {
                item(ui, "SCAN:", s);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("SYS OK").size(9.0).color(AMBER));
                ui.label(RichText::new("#").size(9.0).color(CRT_GREEN));
            });
        });
    }
}

fn placeholder(ui: &mut egui::Ui, title: &str, phase: &str) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(format!("[ {title} ]")).size(14.0).color(CRT_GREEN));
        ui.label(RichText::new(format!("lands in {phase}")).size(11.0).color(CRT_MID));
    });
}

fn panel_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(CRT_PANEL)
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
}

pub fn data_dir() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("nexus-audio"))
}
