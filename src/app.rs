//! Root application: holds top-level state and drives the egui frame.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use eframe::egui::{self, RichText};

use crate::audiobooks::resume::ResumeStore;
use crate::audiobooks::scanner::{self as ab_scanner, AbScanMsg};
use crate::library::db::Db;
use crate::library::models::Audiobook;
use crate::library::scanner::{self, ScanMsg};
use crate::player::engine::Engine;
use crate::player::queue::{Queue, QueueSnapshot};
use crate::playlists::PlaylistStore;
use crate::settings::Settings;
use crate::store::json_store;
use crate::ui::player_bar::{self, NowPlaying, PlayerCmd};
use crate::ui::theme::{self, AMBER, CRT_DIM, CRT_GREEN, CRT_MID, CRT_PANEL};
use crate::ui::views::{
    albums, artists, audiobooks as audiobooks_view, folders, playlists as playlists_view,
    queue as queue_view, tracks, LibraryUi, ViewAction,
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
    resume: ResumeStore,
    ab_scan_rx: Option<Receiver<AbScanMsg>>,
    ab_scan_status: Option<String>,
    /// The audiobook currently loaded in the engine (if any).
    current_book: Option<Audiobook>,
    /// Pending resume dialog: (book, saved_position_secs).
    pending_resume: Option<(Audiobook, f64)>,
    sleep_deadline: Option<Instant>,
    last_resume_save: Instant,
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
        let resume = data_dir
            .as_ref()
            .map(|d| ResumeStore::load(d))
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
            resume,
            ab_scan_rx: None,
            ab_scan_status: None,
            current_book: None,
            pending_resume: None,
            sleep_deadline: None,
            last_resume_save: Instant::now(),
        };
        // Restore the saved queue (list + cursor + modes), idle until played.
        if let Some(dir) = &app.data_dir {
            let snap: QueueSnapshot = json_store::load_or_default(&dir.join("queue.json"));
            app.queue = Queue::restore(snap);
        }
        if app.settings.auto_scan_on_startup {
            if !app.settings.music_folders.is_empty() {
                app.start_scan();
            }
            if !app.settings.audiobook_folders.is_empty() {
                app.start_ab_scan();
            }
        }
        app
    }

    fn start_ab_scan(&mut self) {
        let Some(db_path) = self.db_path.clone() else {
            self.ab_scan_status = Some("SCAN UNAVAILABLE (no data dir)".into());
            return;
        };
        if self.ab_scan_rx.is_some() {
            return;
        }
        self.ab_scan_status = Some("SCANNING…".into());
        self.ab_scan_rx = Some(ab_scanner::spawn_scan(
            db_path,
            self.settings.audiobook_folders.clone(),
        ));
    }

    fn poll_ab_scan(&mut self) {
        let msgs: Vec<AbScanMsg> = match &self.ab_scan_rx {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };
        let mut done = false;
        for m in msgs {
            match m {
                AbScanMsg::Started { total } => {
                    self.ab_scan_status = Some(format!("SCANNING 0 / {total}"));
                }
                AbScanMsg::Progress { done, total, current } => {
                    self.ab_scan_status =
                        Some(format!("SCANNING {done} / {total}  ·  {current}"));
                }
                AbScanMsg::Done { count, errors } => {
                    self.ab_scan_status =
                        Some(format!("SCAN DONE · {count} books · !{errors}"));
                    done = true;
                }
                AbScanMsg::Failed(e) => {
                    self.ab_scan_status = Some(format!("SCAN FAILED: {e}"));
                    done = true;
                }
            }
        }
        if done {
            self.ab_scan_rx = None;
        }
    }

    /// Begin (or resume) playing an audiobook with its authoritative duration.
    fn play_book(&mut self, book: Audiobook, start_secs: f64) {
        self.engine
            .load_book(book.path.clone(), start_secs, book.duration_secs);
        self.engine.play();
        self.current_book = Some(book);
        self.last_resume_save = Instant::now();
    }

    fn save_resume(&mut self) {
        let (Some(book), Some(dir)) = (&self.current_book, &self.data_dir) else {
            return;
        };
        let pos = self.engine.position_secs();
        if pos <= 0.0 {
            return;
        }
        let ch = book
            .chapters
            .iter()
            .rposition(|c| pos + 0.5 >= c.start_secs)
            .unwrap_or(0) as u32;
        self.resume.set(book.id, pos, ch);
        self.resume.save(dir);
        self.last_resume_save = Instant::now();
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
    fn play_current(&mut self) {
        // Starting music ends any audiobook context (clears the now-playing
        // book + its sleep timer; the engine load also drops the duration
        // override so music duration is correct again).
        if self.current_book.is_some() {
            self.save_resume();
            self.current_book = None;
            self.sleep_deadline = None;
        }
        if let Some(t) = self.queue.current() {
            self.engine.load(t.path.clone(), 0.0);
            self.engine.play();
        }
    }

    /// Build the player-bar "now playing" from the current book (with chapter)
    /// or the music queue.
    fn now_playing(&self) -> Option<NowPlaying> {
        if self.current_book.is_some() && self.engine.has_track() {
            let book = self.current_book.as_ref().unwrap();
            let mut subtitle = book.author.clone();
            if !book.chapters.is_empty() {
                let i = self.current_chapter_idx();
                subtitle.push_str(&format!(
                    "   ·   CH {}/{}: {}",
                    i + 1,
                    book.chapters.len(),
                    book.chapters[i].title
                ));
            }
            let info = self.engine.info();
            return Some(NowPlaying {
                title: book.title.clone(),
                subtitle,
                badge: format!("AUDIOBOOK · {}", info.codec),
            });
        }
        let t = self.queue.current()?;
        let mut subtitle = format!("{} · {}", t.artist, t.album);
        if let Some(y) = t.year {
            subtitle.push_str(&format!(" · {y}"));
        }
        let info = self.engine.info();
        Some(NowPlaying {
            title: t.title.clone(),
            subtitle,
            badge: format!("{} {} Hz", info.codec, info.sample_rate),
        })
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
            ViewAction::AddAudiobookFolder => {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    if !self.settings.audiobook_folders.contains(&dir) {
                        self.settings.audiobook_folders.push(dir);
                        self.save_settings();
                        self.start_ab_scan();
                    }
                }
            }
            ViewAction::RemoveAudiobookFolder(p) => {
                self.settings.audiobook_folders.retain(|f| f != &p);
                self.save_settings();
            }
            ViewAction::ScanAudiobooks => self.start_ab_scan(),
            ViewAction::OpenAudiobook(id) => {
                if let Some(book) = self.db.audiobooks().into_iter().find(|b| b.id == id) {
                    let saved = self.resume.get(&id).map(|r| r.position_secs).unwrap_or(0.0);
                    // Offer resume only if meaningfully into the book.
                    if saved > 5.0 && saved < book.duration_secs - 2.0 {
                        self.pending_resume = Some((book, saved));
                    } else {
                        self.play_book(book, 0.0);
                    }
                }
            }
            ViewAction::ChapterSeek(secs) => self.engine.seek(secs),
            ViewAction::ResumeLastBook => {
                if let Some((id, pos)) = self.resume.most_recent() {
                    if let Some(book) =
                        self.db.audiobooks().into_iter().find(|b| b.id == id)
                    {
                        self.view = View::Audiobooks;
                        self.play_book(book, pos);
                    }
                }
            }
            ViewAction::SetSleepTimer(min) => {
                self.sleep_deadline = min.map(|m| {
                    Instant::now() + std::time::Duration::from_secs(m * 60)
                });
            }
        }
    }

    fn handle_player(&mut self, cmd: PlayerCmd) {
        // While an audiobook is loaded, transport is book-aware: prev/next
        // move by chapter, stop persists resume.
        if self.current_book.is_some() {
            match cmd {
                PlayerCmd::PlayPause => self.engine.toggle_pause(),
                PlayerCmd::Stop => {
                    self.save_resume();
                    self.engine.stop();
                    self.current_book = None;
                }
                PlayerCmd::Next => self.chapter_step(true),
                PlayerCmd::Prev => {
                    if self.engine.position_secs() > 3.0
                        && !self.at_chapter_start()
                    {
                        // restart current chapter
                        let s = self.current_chapter_start();
                        self.engine.seek(s);
                    } else {
                        self.chapter_step(false);
                    }
                }
                PlayerCmd::Seek(s) => self.engine.seek(s),
                PlayerCmd::ToggleShuffle | PlayerCmd::CycleRepeat => {}
            }
            return;
        }

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

    fn current_chapter_idx(&self) -> usize {
        let pos = self.engine.position_secs();
        self.current_book
            .as_ref()
            .map(|b| {
                b.chapters
                    .iter()
                    .rposition(|c| pos + 0.5 >= c.start_secs)
                    .unwrap_or(0)
            })
            .unwrap_or(0)
    }
    fn current_chapter_start(&self) -> f64 {
        let i = self.current_chapter_idx();
        self.current_book
            .as_ref()
            .and_then(|b| b.chapters.get(i))
            .map(|c| c.start_secs)
            .unwrap_or(0.0)
    }
    fn at_chapter_start(&self) -> bool {
        self.engine.position_secs() - self.current_chapter_start() < 2.0
    }
    fn chapter_step(&mut self, forward: bool) {
        let Some(book) = &self.current_book else { return };
        if book.chapters.is_empty() {
            // No chapters: ±30s nudge.
            self.engine.seek_rel(if forward { 30.0 } else { -30.0 });
            return;
        }
        let i = self.current_chapter_idx();
        let target = if forward {
            (i + 1).min(book.chapters.len() - 1)
        } else {
            i.saturating_sub(1)
        };
        let s = book.chapters[target].start_secs;
        self.engine.seek(s);
    }

    /// Track finished naturally → advance the queue (or end the book).
    fn poll_playback(&mut self) {
        if self.engine.take_ended() {
            if self.current_book.is_some() {
                self.save_resume();
                self.current_book = None;
            } else {
                if self.queue.next().is_some() {
                    self.play_current();
                }
                self.save_queue();
            }
        }
        // Periodic resume autosave + sleep timer.
        if self.current_book.is_some() && self.engine.is_playing() {
            let iv = self.settings.resume_save_interval_secs.max(5);
            if self.last_resume_save.elapsed().as_secs() >= iv {
                self.save_resume();
            }
        }
        if let Some(deadline) = self.sleep_deadline {
            if Instant::now() >= deadline {
                self.save_resume();
                self.engine.pause();
                self.sleep_deadline = None;
            }
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
        self.poll_ab_scan();
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

        let np = self.now_playing();
        let mut player_cmd = None;
        egui::TopBottomPanel::bottom("playerbar")
            .frame(panel_frame())
            .show(ctx, |ui| {
                player_cmd = player_bar::show(
                    ui,
                    &self.engine,
                    np.as_ref(),
                    self.queue.shuffle,
                    &self.queue.repeat,
                );
            });

        // Owned snapshot so the sidebar/views don't hold a borrow on
        // self.playlists while other &mut self fields are in use.
        let pls = self.playlist_names();
        let selected_pl = self.lib.selected_playlist;
        let resume_hint = self.resume.most_recent().and_then(|(id, pos)| {
            self.db.audiobook_title(id).map(|t| {
                let s = pos as u64;
                format!("{t}  {}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
            })
        });

        let mut sidebar_action = None;
        egui::SidePanel::left("sidebar")
            .exact_width(180.0)
            .resizable(false)
            .frame(panel_frame())
            .show(ctx, |ui| {
                sidebar_action = sidebar::show(
                    ui,
                    &mut self.view,
                    &pls,
                    selected_pl,
                    resume_hint.as_deref(),
                );
            });

        // Audiobook view inputs (owned so no self-borrow spans handle_view).
        let books = if self.view == View::Audiobooks {
            self.db.audiobooks()
        } else {
            Vec::new()
        };
        let playing_book = self.current_book.clone();
        let book_pos = self.engine.position_secs();
        let sleep_left = self.sleep_deadline.map(|d| {
            (d.saturating_duration_since(Instant::now()).as_secs() / 60) + 1
        });
        let ab_status = self.ab_scan_status.clone();

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
            View::Audiobooks => {
                let resume = &self.resume;
                let rp = |id: uuid::Uuid| resume.get(&id).map(|r| r.position_secs);
                let playing = playing_book.as_ref().map(|b| (b, book_pos));
                view_action = audiobooks_view::show(
                    ui,
                    &mut self.lib,
                    &books,
                    &rp,
                    playing,
                    sleep_left,
                );
            }
            View::Folders => {
                view_action = folders::show(
                    ui,
                    &self.settings,
                    self.scan_status.as_deref(),
                    ab_status.as_deref(),
                );
            }
            View::Settings => placeholder(ui, "SETTINGS", "Phase 7"),
        });

        // Resume dialog (modal-ish): offered when opening an in-progress book.
        if let Some((book, pos)) = self.pending_resume.clone() {
            egui::Window::new("RESUME")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(&book.title).size(12.0).color(CRT_GREEN),
                    );
                    let s = pos as u64;
                    ui.label(
                        RichText::new(format!(
                            "> RESUME AT {}:{:02}:{:02}",
                            s / 3600,
                            (s % 3600) / 60,
                            s % 60
                        ))
                        .size(11.0)
                        .color(AMBER),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new("[ RESUME ]").color(CRT_GREEN))
                            .clicked()
                        {
                            self.pending_resume = None;
                            self.play_book(book.clone(), pos);
                        }
                        if ui
                            .button(RichText::new("[ RESTART ]").color(CRT_DIM))
                            .clicked()
                        {
                            self.pending_resume = None;
                            self.play_book(book.clone(), 0.0);
                        }
                        if ui.button(RichText::new("CANCEL").color(CRT_MID)).clicked() {
                            self.pending_resume = None;
                        }
                    });
                });
        }

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
        self.save_resume();
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
            item(ui, "BOOKS:", &format!("{}", self.db.audiobook_count()));
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
