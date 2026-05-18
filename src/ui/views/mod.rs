//! Library views (Phase 2): Tracks, Albums, Artists, Folders.
//!
//! Views are read-only over the DB and return a `ViewAction` for anything that
//! mutates app state (folder management, scans) so the App stays in control of
//! the scanner thread and settings persistence.

pub mod albums;
pub mod artists;
pub mod audiobooks;
pub mod folders;
pub mod playlists;
pub mod queue;
pub mod settings;
pub mod tracks;

use std::path::PathBuf;

use eframe::egui;

use crate::library::db::SortKey;
use crate::library::models::Track;

/// `(playlist id, name)` pairs for the "Add to Playlist" submenu.
pub type Playlists<'a> = &'a [(uuid::Uuid, String)];

/// A full-width, full-height clickable list row. Forcing the region to the
/// available width makes the *entire* row the hit target (egui otherwise sizes
/// the response to just the laid-out content, leaving dead space around it).
/// Returns the click-sensing response; paints the hover highlight itself.
#[allow(dead_code)] // no-trailing-actions convenience wrapper over list_row_actions
pub fn list_row(
    ui: &mut egui::Ui,
    height: f32,
    add: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    list_row_actions(ui, height, 0.0, add, |_| {})
}

/// As [`list_row`], plus a right-aligned `actions_w`-wide strip rendered by
/// `trailing`. That strip is *excluded* from the row's hit rect, so inline
/// buttons there stay clickable (the row's whole-row `ui.interact` would
/// otherwise sit on top of them — the same occlusion rule, inverted).
pub fn list_row_actions(
    ui: &mut egui::Ui,
    height: f32,
    actions_w: f32,
    add: impl FnOnce(&mut egui::Ui),
    trailing: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    // Reserve an exact-size band (also keeps a virtualized `show_rows` stride
    // consistent). The hit area is then claimed with a single `ui.interact`
    // AFTER the content is drawn so it wins egui's occlusion order over the
    // inner Labels — minus the trailing strip, which owns its own widgets.
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, height), egui::Sense::hover());
    let hit_rect = if actions_w > 0.0 {
        egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.max.x - actions_w, rect.max.y),
        )
    } else {
        rect
    };

    if ui.is_rect_visible(rect) {
        if ui.rect_contains_pointer(hit_rect) {
            ui.painter()
                .rect_filled(rect, 0.0, crate::ui::theme::ROW_HOVER);
        }
        let mut content = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(hit_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        content.set_clip_rect(hit_rect.intersect(ui.clip_rect()));
        add(&mut content);

        if actions_w > 0.0 {
            let arect = egui::Rect::from_min_max(
                egui::pos2(rect.max.x - actions_w, rect.min.y),
                rect.max,
            );
            let mut acts = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(arect)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            acts.set_clip_rect(arect.intersect(ui.clip_rect()));
            trailing(&mut acts);
        }
    }

    let id = ui.make_persistent_id(("nexus_list_row", rect.min.y.to_bits()));
    let resp = ui.interact(hit_rect, id, egui::Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Side-effect requests a view hands back to the App.
#[derive(Debug, Clone)]
pub enum ViewAction {
    AddMusicFolder,
    RemoveFolder(PathBuf),
    ScanAll,
    /// Play `list`, starting at `index`, replacing the queue (optionally shuffled).
    Play { list: Vec<Track>, index: usize, shuffle: bool },
    /// Add one track to the queue (`next` = jump to position 1).
    Enqueue { track: Track, next: bool },
    /// Queue-panel operations (indices are into `upcoming`).
    QueueJump(usize),
    QueueRemove(usize),
    QueueMove { i: usize, up: bool },
    QueueClear,
    /// Playlist operations.
    PlaylistSelect(uuid::Uuid),
    PlaylistNew,
    PlaylistDelete(uuid::Uuid),
    PlaylistDuplicate(uuid::Uuid),
    PlaylistRename(uuid::Uuid, String),
    PlaylistRemoveAt(uuid::Uuid, usize),
    PlaylistMoveAt { id: uuid::Uuid, i: usize, up: bool },
    PlaylistExport(uuid::Uuid),
    PlaylistImport,
    /// Add a track to an existing playlist, or `None` = new playlist.
    PlaylistAddTrack { playlist: Option<uuid::Uuid>, track: Track },
    /// Audiobooks.
    AddAudiobookFolder,
    RemoveAudiobookFolder(PathBuf),
    ScanAudiobooks,
    OpenAudiobook(uuid::Uuid),
    ChapterSeek(f64),
    SetSleepTimer(Option<u64>), // minutes; None = off
    ResumeLastBook,
    ClearQuickResume,
    /// A settings widget changed — persist + re-apply visuals.
    SettingsChanged,
}

/// Per-row interaction: double-click or a context-menu pick.
#[derive(Clone)]
pub enum RowAction {
    Play,
    PlayNext,
    AddToQueue,
    /// Add to an existing playlist (`Some`) or a brand-new one (`None`).
    AddToPlaylist(Option<uuid::Uuid>),
}

/// Standard track-row affordance: double-click = play, right-click = menu.
/// `playlists` populates the "Add to Playlist" submenu.
pub fn row_actions(
    resp: &egui::Response,
    playlists: &[(uuid::Uuid, String)],
) -> Option<RowAction> {
    let mut act = None;
    if resp.double_clicked() {
        act = Some(RowAction::Play);
    }
    resp.context_menu(|ui| {
        if ui.button(">   PLAY").clicked() {
            act = Some(RowAction::Play);
            ui.close_menu();
        }
        if ui.button(">>  PLAY NEXT").clicked() {
            act = Some(RowAction::PlayNext);
            ui.close_menu();
        }
        if ui.button("+   ADD TO QUEUE").clicked() {
            act = Some(RowAction::AddToQueue);
            ui.close_menu();
        }
        ui.menu_button("+   ADD TO PLAYLIST", |ui| {
            if ui.button("[ NEW PLAYLIST ]").clicked() {
                act = Some(RowAction::AddToPlaylist(None));
                ui.close_menu();
            }
            if !playlists.is_empty() {
                ui.separator();
            }
            for (id, name) in playlists {
                if ui.button(name).clicked() {
                    act = Some(RowAction::AddToPlaylist(Some(*id)));
                    ui.close_menu();
                }
            }
        });
    });
    act
}

/// Map a drill-down list pick into a `ViewAction` (Play replaces the queue
/// with the whole list; the others enqueue just that track).
pub fn list_action(list: Vec<Track>, pick: Option<(usize, RowAction)>) -> Option<ViewAction> {
    let (index, a) = pick?;
    Some(match a {
        RowAction::Play => ViewAction::Play { list, index, shuffle: false },
        RowAction::PlayNext => ViewAction::Enqueue {
            track: list.get(index)?.clone(),
            next: true,
        },
        RowAction::AddToQueue => ViewAction::Enqueue {
            track: list.get(index)?.clone(),
            next: false,
        },
        RowAction::AddToPlaylist(playlist) => ViewAction::PlaylistAddTrack {
            playlist,
            track: list.get(index)?.clone(),
        },
    })
}

/// Persistent state shared across the library views (search box, sort, the
/// drill-down selection, and a one-page track cache so we don't re-query
/// SQLite every frame for an unchanged scroll position).
pub struct LibraryUi {
    pub search: String,
    pub sort: SortKey,
    pub artist_filter: Option<String>,
    pub album_filter: Option<String>,
    pub selected_playlist: Option<uuid::Uuid>,
    /// Inline rename buffer for the selected playlist (None = not renaming).
    pub rename_buf: Option<String>,
    pub ab_search: String,
    /// 0=title 1=author 2=genre 3=progress
    pub ab_sort: u8,

    cache_key: Option<CacheKey>,
    cache: Vec<Track>,
    cache_offset: i64,
    pub total: i64,
}

#[derive(PartialEq, Clone)]
struct CacheKey {
    search: String,
    sort: SortKey,
    offset: i64,
    limit: i64,
}

impl Default for LibraryUi {
    fn default() -> Self {
        Self {
            search: String::new(),
            sort: SortKey::Title,
            artist_filter: None,
            album_filter: None,
            selected_playlist: None,
            rename_buf: None,
            ab_search: String::new(),
            ab_sort: 0,
            cache_key: None,
            cache: Vec::new(),
            cache_offset: 0,
            total: 0,
        }
    }
}

impl LibraryUi {
    /// Return the cached page covering `[offset, offset+limit)`, re-querying
    /// the DB only when the search/sort/window actually changed.
    fn page<'a>(
        &'a mut self,
        db: &crate::library::db::Db,
        offset: i64,
        limit: i64,
    ) -> &'a [Track] {
        let key = CacheKey {
            search: self.search.clone(),
            sort: self.sort,
            offset,
            limit,
        };
        if self.cache_key.as_ref() != Some(&key) {
            self.cache = db
                .tracks_page(&self.search, self.sort, limit, offset)
                .unwrap_or_default();
            self.cache_offset = offset;
            self.cache_key = Some(key);
        }
        &self.cache
    }

    /// Force the next `page()` call to re-query (after a scan or search edit).
    pub fn invalidate(&mut self) {
        self.cache_key = None;
    }
}
