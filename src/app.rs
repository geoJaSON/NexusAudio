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
use crate::ui::theme::{self, AMBER, CRT_DIM, CRT_GREEN, CRT_MID, CRT_PANEL, RED_ALERT};
use crate::ui::views::{
    albums, artists, audiobooks as audiobooks_view, genres, playlists as playlists_view,
    queue as queue_view, settings as settings_view, tracks, LibraryUi, ViewAction,
};
use crate::ui::{sidebar, titlebar, View};

#[cfg(any(target_os = "linux", target_os = "windows"))]
struct MediaCache {
    track_title: Option<String>,
    track_subtitle: Option<String>,
    has_track: bool,
    playing: bool,
    volume: f32,
    last_position_secs: f64,
    last_position_update: std::time::Instant,
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
impl Default for MediaCache {
    fn default() -> Self {
        Self {
            track_title: None,
            track_subtitle: None,
            has_track: false,
            playing: false,
            volume: 0.0,
            last_position_secs: 0.0,
            last_position_update: std::time::Instant::now(),
        }
    }
}

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
    /// Open tag-editor dialog: (file path, editable fields).
    pending_tag_edit: Option<(PathBuf, crate::library::scanner::TagEdit)>,
    tag_edit_error: Option<String>,
    show_queue: bool,
    /// Every track started this session (newest last), independent of queue
    /// replacement — drives the slide-out's "played this session".
    session_history: Vec<crate::library::models::Track>,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    media_controls: Option<souvlaki::MediaControls>,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[allow(dead_code)]
    media_tx: std::sync::mpsc::Sender<souvlaki::MediaControlEvent>,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    media_rx: std::sync::mpsc::Receiver<souvlaki::MediaControlEvent>,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    media_cache: MediaCache,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);

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

        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let (media_tx, media_rx) = std::sync::mpsc::channel();
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let _ = &media_tx;

        #[cfg(target_os = "linux")]
        let media_controls = {
            let config = souvlaki::PlatformConfig {
                dbus_name: "org.mpris.MediaPlayer2.nexus_audio",
                display_name: "NEXUS//AUDIO",
                hwnd: None,
            };
            match souvlaki::MediaControls::new(config) {
                Ok(mut controls) => {
                    let tx = media_tx.clone();
                    let _ = controls.attach(move |event| {
                        let _ = tx.send(event);
                    });
                    Some(controls)
                }
                Err(e) => {
                    eprintln!("Failed to initialize media controls: {e}");
                    None
                }
            }
        };
        #[cfg(not(target_os = "linux"))]
        let media_controls = None;

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
            pending_tag_edit: None,
            tag_edit_error: None,
            show_queue: false,
            session_history: Vec::new(),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            media_controls,
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            media_tx,
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            media_rx,
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            media_cache: MediaCache::default(),
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
            self.settings.audiobook_folders.clone(),
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
            let t = t.clone();
            self.engine.load(t.path.clone(), 0.0);
            self.engine.play();
            // Log to session history (skip consecutive dupes; cap length).
            if self.session_history.last().map(|p| p.id) != Some(t.id) {
                self.session_history.push(t);
                if self.session_history.len() > 300 {
                    self.session_history.remove(0);
                }
            }
        }
        self.update_preload();
    }

    /// Keep the engine's gapless preload slot in sync with `queue.peek_next()`.
    /// Audiobooks never preload (each book is its own context, not a queue).
    fn update_preload(&self) {
        if self.current_book.is_some() {
            self.engine.preload_next(None);
            return;
        }
        let next_path = self.queue.peek_next().map(|t| t.path.clone());
        self.engine.preload_next(next_path);
    }

    /// React to the engine's gapless advance signal: it already swapped to the
    /// preloaded next file; we only need to move the queue cursor + session
    /// history forward, and stage the new "after-next" preload.
    fn on_gapless_advance(&mut self) {
        if self.queue.next().is_some() {
            if let Some(t) = self.queue.current().cloned() {
                if self.session_history.last().map(|p| p.id) != Some(t.id) {
                    self.session_history.push(t);
                    if self.session_history.len() > 300 {
                        self.session_history.remove(0);
                    }
                }
            }
            self.update_preload();
            self.save_queue();
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
                let key = p.display().to_string();
                self.settings.music_folders.retain(|f| f != &p);
                self.settings.folder_stats.remove(&key);
                self.save_settings();
                // Unwatching a folder evicts its tracks from the library.
                self.db.remove_tracks_under(&[p]);
                self.track_count = self.db.track_count().unwrap_or(0);
                self.lib.invalidate();
            }
            ViewAction::ScanAll => self.start_scan(),
            ViewAction::Play { list, index, shuffle } => {
                // Sync the mode BEFORE set() builds the order — the old
                // toggle-after dance rebuilt the order twice and left the
                // cursor stranded mid-list.
                self.queue.shuffle = shuffle;
                self.queue.set(list, index);
                self.play_current();
                self.save_queue();
            }
            ViewAction::Enqueue { track, next } => {
                if next {
                    self.queue.play_next(track);
                } else {
                    self.queue.enqueue(track);
                }
                self.update_preload();
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
                self.update_preload();
                self.save_queue();
            }
            ViewAction::QueueMove { i, up } => {
                self.queue.move_upcoming(i, up);
                self.update_preload();
                self.save_queue();
            }
            ViewAction::QueueClear => {
                self.queue.clear();
                self.handle_player(crate::app::PlayerCmd::Stop);
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
                let key = p.display().to_string();
                self.settings.audiobook_folders.retain(|f| f != &p);
                self.settings.folder_stats.remove(&key);
                self.save_settings();
                // Unwatching evicts those books; clear playing/pending if affected.
                if self
                    .current_book
                    .as_ref()
                    .map(|b| b.path.starts_with(&p))
                    .unwrap_or(false)
                {
                    self.engine.stop();
                    self.current_book = None;
                }
                self.pending_resume = None;
                self.db.remove_audiobooks_under(&[p]);
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
            ViewAction::CreatePlaylistFromQueue => {
                let ids: Vec<uuid::Uuid> =
                    self.queue.ordered().iter().map(|t| t.id).collect();
                if !ids.is_empty() {
                    let id = self.playlists.create("QUEUE PLAYLIST");
                    for tid in ids {
                        self.playlists.add_track(id, tid);
                    }
                    self.lib.selected_playlist = Some(id);
                    self.lib.rename_buf = None;
                    self.view = View::Playlists;
                    self.save_playlists();
                }
            }
            ViewAction::ClearQuickResume => {
                self.resume = ResumeStore::default();
                if let Some(dir) = &self.data_dir {
                    self.resume.save(dir);
                }
            }
            ViewAction::SettingsChanged => self.save_settings(),
            ViewAction::ToggleQueuePanel => {
                self.show_queue = !self.show_queue;
            }
            ViewAction::BulkEnqueue(tracks) => {
                if !tracks.is_empty() {
                    for t in tracks {
                        self.queue.enqueue(t);
                    }
                    self.lib.selected.clear();
                    self.update_preload();
                    self.save_queue();
                }
            }
            ViewAction::BulkAddToPlaylist { playlist, tracks } => {
                if !tracks.is_empty() {
                    let id = playlist.unwrap_or_else(|| {
                        let id = self.playlists.create("NEW PLAYLIST");
                        self.lib.selected_playlist = Some(id);
                        id
                    });
                    for t in &tracks {
                        self.playlists.add_track(id, t.id);
                    }
                    self.lib.selected.clear();
                    self.save_playlists();
                }
            }
            ViewAction::ClearSelection => {
                self.lib.selected.clear();
            }
            ViewAction::EditTags(t) => {
                let fmt_opt = |o: Option<u32>| o.map(|v| v.to_string()).unwrap_or_default();
                self.tag_edit_error = None;
                self.pending_tag_edit = Some((
                    t.path.clone(),
                    crate::library::scanner::TagEdit {
                        title: t.title.clone(),
                        artist: t.artist.clone(),
                        album: t.album.clone(),
                        album_artist: t.album_artist.clone(),
                        genre: t.genre.clone(),
                        year: fmt_opt(t.year),
                        track: fmt_opt(t.track_number),
                        disc: fmt_opt(t.disc_number),
                    },
                ));
            }
        }
    }

    fn handle_player(&mut self, cmd: PlayerCmd) {
        // UI toggle — independent of music/audiobook context.
        if let PlayerCmd::ToggleQueue = cmd {
            self.show_queue = !self.show_queue;
            return;
        }
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
                PlayerCmd::ToggleShuffle
                | PlayerCmd::CycleRepeat
                | PlayerCmd::ToggleQueue => {}
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
            PlayerCmd::ToggleShuffle => {
                self.queue.toggle_shuffle();
                self.update_preload();
            }
            PlayerCmd::CycleRepeat => {
                self.queue.cycle_repeat();
                self.update_preload();
            }
            PlayerCmd::ToggleQueue => {} // handled above
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
        // Gapless first: engine has already swapped, queue cursor just needs
        // to catch up. Falls through to legacy `ended` handling for the case
        // where there was no preload (end of queue, repeat=None tail).
        if self.engine.take_advanced() {
            self.on_gapless_advance();
        }
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
                i.key_pressed(egui::Key::Q),
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
        // Route through handle_player so the gapless preload is re-staged
        // for the new play order / repeat mode.
        if k.7 {
            self.handle_player(PlayerCmd::ToggleShuffle);
        }
        if k.8 {
            self.handle_player(PlayerCmd::CycleRepeat);
        }
        if k.9 {
            self.handle_player(PlayerCmd::ToggleQueue);
        }
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn poll_media_events(&mut self) {
        while let Ok(event) = self.media_rx.try_recv() {
            match event {
                souvlaki::MediaControlEvent::Play => {
                    if !self.engine.is_playing() {
                        if self.engine.has_track() {
                            self.engine.play();
                        } else {
                            self.play_current();
                        }
                    }
                }
                souvlaki::MediaControlEvent::Pause => {
                    if self.engine.is_playing() {
                        if self.current_book.is_some() {
                            self.save_resume();
                        }
                        self.engine.pause();
                    }
                }
                souvlaki::MediaControlEvent::Toggle => {
                    self.handle_player(PlayerCmd::PlayPause);
                }
                souvlaki::MediaControlEvent::Next => {
                    self.handle_player(PlayerCmd::Next);
                }
                souvlaki::MediaControlEvent::Previous => {
                    self.handle_player(PlayerCmd::Prev);
                }
                souvlaki::MediaControlEvent::Stop => {
                    self.handle_player(PlayerCmd::Stop);
                }
                souvlaki::MediaControlEvent::SetVolume(vol) => {
                    self.engine.set_volume(vol as f32);
                }
                souvlaki::MediaControlEvent::SetPosition(souvlaki::MediaPosition(duration)) => {
                    self.engine.seek(duration.as_secs_f64());
                }
                souvlaki::MediaControlEvent::SeekBy(direction, duration) => {
                    let delta = duration.as_secs_f64();
                    let delta = match direction {
                        souvlaki::SeekDirection::Forward => delta,
                        souvlaki::SeekDirection::Backward => -delta,
                    };
                    self.engine.seek_rel(delta);
                }
                _ => {}
            }
        }
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn update_media_controls(&mut self) {
        if self.media_controls.is_none() {
            return;
        }

        let is_playing = self.engine.is_playing();
        let has_track = self.engine.has_track();
        let np = self.now_playing();
        let vol = self.engine.volume();

        let changed_track = np.as_ref().map(|n| &n.title) != self.media_cache.track_title.as_ref()
            || np.as_ref().map(|n| &n.subtitle) != self.media_cache.track_subtitle.as_ref()
            || has_track != self.media_cache.has_track;

        let changed_playback = is_playing != self.media_cache.playing || changed_track;
        let changed_volume = (vol - self.media_cache.volume).abs() > 0.01;

        // Detect manual seek or unexpected position jumps
        let pos_secs = self.engine.position_secs();
        let expected_pos = if is_playing {
            self.media_cache.last_position_secs + self.media_cache.last_position_update.elapsed().as_secs_f64()
        } else {
            self.media_cache.last_position_secs
        };
        let is_seek = (pos_secs - expected_pos).abs() > 1.5;

        // Query extra properties from self BEFORE borrowing media_controls mutably
        let current_book_author = self.current_book.as_ref().map(|b| b.author.clone());
        let queue_current_artist = self.queue.current().map(|t| t.artist.clone());
        let queue_current_album = self.queue.current().map(|t| t.album.clone());
        let duration_secs = self.engine.duration_secs();
        let is_current_book = self.current_book.is_some();

        let controls = self.media_controls.as_mut().unwrap();

        if changed_track {
            if has_track {
                if let Some(n) = &np {
                    let title = n.title.clone();
                    let artist = if is_current_book {
                        current_book_author.unwrap_or_default()
                    } else {
                        queue_current_artist.unwrap_or_default()
                    };
                    let album = if is_current_book {
                        String::new()
                    } else {
                        queue_current_album.unwrap_or_default()
                    };

                    let metadata = souvlaki::MediaMetadata {
                        title: Some(&title),
                        artist: Some(&artist),
                        album: Some(&album),
                        cover_url: None,
                        duration: Some(std::time::Duration::from_secs_f64(duration_secs)),
                    };
                    let _ = controls.set_metadata(metadata);
                    
                    self.media_cache.track_title = Some(n.title.clone());
                    self.media_cache.track_subtitle = Some(n.subtitle.clone());
                }
            } else {
                let metadata = souvlaki::MediaMetadata::default();
                let _ = controls.set_metadata(metadata);
                self.media_cache.track_title = None;
                self.media_cache.track_subtitle = None;
            }
            self.media_cache.has_track = has_track;
        }

        if changed_playback || is_seek {
            let progress = if has_track {
                Some(souvlaki::MediaPosition(std::time::Duration::from_secs_f64(pos_secs)))
            } else {
                None
            };
            let state = if !has_track {
                souvlaki::MediaPlayback::Stopped
            } else if is_playing {
                souvlaki::MediaPlayback::Playing { progress }
            } else {
                souvlaki::MediaPlayback::Paused { progress }
            };
            let _ = controls.set_playback(state);
            self.media_cache.playing = is_playing;
            self.media_cache.last_position_secs = pos_secs;
            self.media_cache.last_position_update = std::time::Instant::now();
        }

        if changed_volume {
            #[cfg(target_os = "linux")]
            let _ = controls.set_volume(vol as f64);
            self.media_cache.volume = vol;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "windows")]
        if self.media_controls.is_none() {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = _frame.window_handle() {
                if let RawWindowHandle::Win32(h) = handle.as_raw() {
                    let hwnd = h.hwnd.get() as *mut std::ffi::c_void;
                    let config = souvlaki::PlatformConfig {
                        dbus_name: "org.mpris.MediaPlayer2.nexus_audio",
                        display_name: "NEXUS//AUDIO",
                        hwnd: Some(hwnd),
                    };
                    match souvlaki::MediaControls::new(config) {
                        Ok(mut controls) => {
                            let tx = self.media_tx.clone();
                            let _ = controls.attach(move |event| {
                                let _ = tx.send(event);
                            });
                            self.media_controls = Some(controls);
                            println!("Media controls initialized on Windows");
                        }
                        Err(e) => {
                            eprintln!("Failed to initialize media controls on Windows: {e}");
                        }
                    }
                }
            }
        }

        #[cfg(any(target_os = "linux", target_os = "windows"))]
        self.poll_media_events();

        self.poll_scan();
        self.poll_ab_scan();
        self.poll_playback();
        self.shortcuts(ctx);
        // Idempotent; reflects live accent/text color changes from Settings.
        theme::apply_visuals(ctx, self.settings.accent_color, self.settings.text_color);

        // Repaint while scanning or playing so progress/clock stay live.
        if self.scan_rx.is_some() || self.engine.is_playing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        let eq = self.settings.eq_enabled;
        let is_playing = self.engine.is_playing();
        egui::TopBottomPanel::top("titlebar")
            .frame(panel_frame())
            .show(ctx, |ui| titlebar::show(ui, eq, is_playing));

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
        let queue_len = self.queue.len();
        let show_queue = self.show_queue;
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
                    show_queue,
                    queue_len,
                );
            });

        // Right slide-out queue panel.
        let mut queue_action = None;
        egui::SidePanel::right("queue_panel")
            .exact_width(280.0)
            .resizable(false)
            .frame(panel_frame())
            .show_animated(ctx, self.show_queue, |ui| {
                queue_action =
                    queue_view::show(ui, &self.queue, &self.session_history);
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
            View::Genres => {
                view_action = genres::show(ui, &self.db, &mut self.lib, &pls)
            }
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
            View::Settings => {
                view_action = settings_view::show(
                    ui,
                    &mut self.settings,
                    self.scan_status.as_deref(),
                    ab_status.as_deref(),
                );
            }
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

        // Tag editor (modal). Take the buffer out so the window can edit it
        // freely without aliasing `self`; decide on close what to do.
        if let Some((path, mut edit)) = self.pending_tag_edit.take() {
            let mut do_save = false;
            let mut keep = true;
            let err = self.tag_edit_error.clone();
            egui::Window::new("EDIT TAGS")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(path.display().to_string())
                            .size(9.0)
                            .color(CRT_MID),
                    );
                    ui.add_space(4.0);
                    let field = |ui: &mut egui::Ui, label: &str, v: &mut String| {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [90.0, 18.0],
                                egui::Label::new(
                                    RichText::new(label).size(10.0).color(CRT_DIM),
                                ),
                            );
                            ui.add(
                                egui::TextEdit::singleline(v).desired_width(280.0),
                            );
                        });
                    };
                    field(ui, "TITLE", &mut edit.title);
                    field(ui, "ARTIST", &mut edit.artist);
                    field(ui, "ALBUM", &mut edit.album);
                    field(ui, "ALBUM ARTIST", &mut edit.album_artist);
                    field(ui, "GENRE", &mut edit.genre);
                    field(ui, "YEAR", &mut edit.year);
                    field(ui, "TRACK #", &mut edit.track);
                    field(ui, "DISC #", &mut edit.disc);
                    if let Some(e) = &err {
                        ui.add_space(4.0);
                        ui.label(RichText::new(e).size(10.0).color(RED_ALERT));
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new("[ SAVE ]").color(CRT_GREEN))
                            .clicked()
                        {
                            do_save = true;
                            keep = false;
                        }
                        if ui
                            .button(RichText::new("CANCEL").color(CRT_MID))
                            .clicked()
                        {
                            keep = false;
                        }
                    });
                });

            if do_save {
                match crate::library::scanner::write_tags(&self.db, &path, &edit) {
                    Ok(()) => {
                        self.tag_edit_error = None;
                        self.lib.invalidate();
                        self.track_count = self.db.track_count().unwrap_or(0);
                    }
                    Err(e) => {
                        self.tag_edit_error = Some(format!("write failed: {e}"));
                        self.pending_tag_edit = Some((path, edit)); // reopen
                    }
                }
            } else if keep {
                self.pending_tag_edit = Some((path, edit));
            } else {
                self.tag_edit_error = None;
            }
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
        if let Some(a) = queue_action {
            self.handle_view(a);
        }

        #[cfg(any(target_os = "linux", target_os = "windows"))]
        self.update_media_controls();
    }

    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.save_settings();
        self.save_queue();
        self.save_playlists();
        self.save_resume();

        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            if let Some(mut controls) = self.media_controls.take() {
                let _ = controls.set_playback(souvlaki::MediaPlayback::Stopped);
            }
        }
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

fn panel_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(CRT_PANEL)
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
}

pub fn data_dir() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("nexus-audio"))
}
