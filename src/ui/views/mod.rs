//! Library views (Phase 2): Tracks, Albums, Artists, Folders.
//!
//! Views are read-only over the DB and return a `ViewAction` for anything that
//! mutates app state (folder management, scans) so the App stays in control of
//! the scanner thread and settings persistence.

pub mod albums;
pub mod artists;
pub mod audiobooks;
pub mod folders;
pub mod genres;
pub mod playlists;
pub mod queue;
pub mod settings;
pub mod tracks;

use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui::{self, RichText};

use crate::library::db::SortKey;
use crate::library::models::Track;
use crate::ui::theme::{AMBER, CRT_GREEN, CRT_MID};

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
    let resp = ui.interact(hit_rect, id, egui::Sense::click_and_drag());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// How long the pointer must be held still on a row before it counts as a
/// long-press (used for "click-and-hold" multi-select entry).
const LONG_PRESS_SECS: f64 = 0.35;

/// Tracks a click-and-hold gesture on a row. `just_fired` is true exactly
/// on the frame the threshold is crossed; `suppress_click` stays true until
/// the user releases, so the normal click/double-click handlers can skip
/// firing on the same press.
pub struct LongPress {
    pub just_fired: bool,
    pub suppress_click: bool,
}

pub fn check_long_press(ui: &mut egui::Ui, resp: &egui::Response, flag_id: egui::Id) -> LongPress {
    let mut fired: bool = ui.data(|d| d.get_temp(flag_id).unwrap_or(false));
    let mut just_fired = false;
    // If the pointer has moved past egui's drag threshold, this press is a
    // drag-and-drop attempt — never let it count as a long-press select.
    if resp.is_pointer_button_down_on() && !resp.dragged() {
        if !fired {
            let press_start = ui.ctx().input(|i| i.pointer.press_start_time());
            let now = ui.ctx().input(|i| i.time);
            if let Some(start) = press_start {
                let elapsed = now - start;
                if elapsed > LONG_PRESS_SECS {
                    fired = true;
                    just_fired = true;
                    ui.data_mut(|d| d.insert_temp(flag_id, true));
                    // Force a repaint so the selection outline appears
                    // immediately (we just toggled state mid-frame).
                    ui.ctx().request_repaint();
                } else {
                    // egui only repaints on input change — a perfectly-still
                    // hold would otherwise sit idle past the threshold and
                    // miss the long-press. Schedule a wake-up.
                    let remaining = LONG_PRESS_SECS - elapsed;
                    ui.ctx().request_repaint_after(
                        std::time::Duration::from_secs_f64(remaining.max(0.02)),
                    );
                }
            }
        }
    } else if fired {
        // Press has just ended — clear for the next press, but keep
        // suppress_click=true this frame so the trailing click is ignored.
        ui.data_mut(|d| d.remove_temp::<bool>(flag_id));
    }
    LongPress { just_fired, suppress_click: fired }
}

/// Paint an amber outline around a row to indicate it is in the multi-select.
pub fn paint_selected_outline(ui: &egui::Ui, rect: egui::Rect) {
    ui.painter().rect_stroke(
        rect.shrink(1.0),
        0.0,
        egui::Stroke::new(1.0, AMBER),
    );
}

/// Bulk action chosen from the selection toolbar.
#[derive(Debug, Clone)]
pub enum SelectionAction {
    AddToQueue,
    AddToPlaylist(Option<uuid::Uuid>),
    Clear,
}

/// Toolbar shown above a track list. Always rendered so the page layout
/// doesn't jump when a selection begins or ends — buttons are disabled when
/// nothing is selected. Returns the chosen bulk action, if any.
pub fn selection_toolbar(
    ui: &mut egui::Ui,
    count: usize,
    playlists: Playlists,
) -> Option<SelectionAction> {
    let mut act = None;
    let enabled = count > 0;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let (label_text, label_color) = if enabled {
            (format!("[{count} SELECTED]"), AMBER)
        } else {
            ("[NONE SELECTED]".to_string(), CRT_MID)
        };
        ui.label(RichText::new(label_text).size(10.0).color(label_color));
        ui.add_space(4.0);
        let action_color = if enabled { CRT_GREEN } else { CRT_MID };
        let clear_color = if enabled { AMBER } else { CRT_MID };
        ui.add_enabled_ui(enabled, |ui| {
            if ui
                .button(RichText::new("+Q TO QUEUE").size(10.0).color(action_color))
                .on_hover_text("Add selection to the playback queue")
                .clicked()
            {
                act = Some(SelectionAction::AddToQueue);
            }
            ui.menu_button(
                RichText::new("+ TO PLAYLIST").size(10.0).color(action_color),
                |ui| {
                    if ui.button("[ NEW PLAYLIST ]").clicked() {
                        act = Some(SelectionAction::AddToPlaylist(None));
                        ui.close_menu();
                    }
                    if !playlists.is_empty() {
                        ui.separator();
                    }
                    for (id, name) in playlists {
                        if ui.button(name).clicked() {
                            act = Some(SelectionAction::AddToPlaylist(Some(*id)));
                            ui.close_menu();
                        }
                    }
                },
            );
            if ui
                .button(RichText::new("CLEAR").size(10.0).color(clear_color))
                .clicked()
            {
                act = Some(SelectionAction::Clear);
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new("HOLD A ROW TO SELECT  ·  CLICK TO TOGGLE")
                    .size(9.0)
                    .color(CRT_MID),
            );
        });
    });
    ui.separator();
    act
}

/// Convert a selection action + tracks (the ones currently visible) and the
/// full selected-id set into the matching `ViewAction`. The caller is
/// responsible for resolving track ids → tracks via DB if needed.
pub fn selection_to_view_action(
    sel: SelectionAction,
    tracks: Vec<Track>,
) -> Option<ViewAction> {
    match sel {
        SelectionAction::AddToQueue => Some(ViewAction::BulkEnqueue(tracks)),
        SelectionAction::AddToPlaylist(playlist) => Some(ViewAction::BulkAddToPlaylist {
            playlist,
            tracks,
        }),
        SelectionAction::Clear => Some(ViewAction::ClearSelection),
    }
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
    EditTags(Track),
    CreatePlaylistFromQueue,
    /// Toggle the slide-out queue panel.
    ToggleQueuePanel,
    /// Append the given tracks to the queue in order.
    BulkEnqueue(Vec<Track>),
    /// Add tracks to an existing playlist (or a brand-new one if `playlist`
    /// is `None`). Used by the multi-select toolbar and dnd of `Vec<Track>`.
    BulkAddToPlaylist {
        playlist: Option<uuid::Uuid>,
        tracks: Vec<Track>,
    },
    /// Empty the multi-select set.
    ClearSelection,
}

/// Per-row interaction: double-click or a context-menu pick.
#[derive(Clone)]
pub enum RowAction {
    Play,
    PlayNext,
    AddToQueue,
    /// Add to an existing playlist (`Some`) or a brand-new one (`None`).
    AddToPlaylist(Option<uuid::Uuid>),
    EditTags,
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
        if ui.button("~   EDIT TAGS").clicked() {
            act = Some(RowAction::EditTags);
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
        RowAction::Play => ViewAction::Play {
            list: vec![list.get(index)?.clone()],
            index: 0,
            shuffle: false,
        },
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
        RowAction::EditTags => ViewAction::EditTags(list.get(index)?.clone()),
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
    pub genre_filter: Option<String>,
    pub selected_playlist: Option<uuid::Uuid>,
    /// Inline rename buffer for the selected playlist (None = not renaming).
    pub rename_buf: Option<String>,
    pub ab_search: String,
    /// 0=title 1=author 2=genre 3=progress
    pub ab_sort: u8,
    /// Track ids the user has multi-selected (click-and-hold + click to toggle).
    /// Spans views — selections persist when you navigate between Tracks /
    /// Artists / Albums / Genres / Playlists.
    pub selected: HashSet<uuid::Uuid>,

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
            genre_filter: None,
            selected_playlist: None,
            rename_buf: None,
            ab_search: String::new(),
            ab_sort: 0,
            selected: HashSet::new(),
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
