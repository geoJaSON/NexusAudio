//! Library views (Phase 2): Tracks, Albums, Artists, Folders.
//!
//! Views are read-only over the DB and return a `ViewAction` for anything that
//! mutates app state (folder management, scans) so the App stays in control of
//! the scanner thread and settings persistence.

pub mod albums;
pub mod artists;
pub mod folders;
pub mod tracks;

use std::path::PathBuf;

use eframe::egui;

use crate::library::db::SortKey;
use crate::library::models::Track;

/// A full-width, full-height clickable list row. Forcing the region to the
/// available width makes the *entire* row the hit target (egui otherwise sizes
/// the response to just the laid-out content, leaving dead space around it).
/// Returns the click-sensing response; paints the hover highlight itself.
pub fn list_row(
    ui: &mut egui::Ui,
    height: f32,
    add: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    // Reserve an exact-size band (also keeps a virtualized `show_rows` stride
    // consistent). The hit area is then claimed with a single `ui.interact`
    // AFTER the content is drawn: the row's interaction is registered last so
    // it wins egui's occlusion order over the inner Labels. Taking the
    // response from the allocation instead (the previous approach) loses every
    // pixel a Label covers — leaving only thin label-free slivers live.
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, height), egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        // Highlight driven by pointer containment, not widget hover, so it is
        // independent of what the child lays on top.
        if ui.rect_contains_pointer(rect) {
            ui.painter().rect_filled(
                rect,
                0.0,
                egui::Color32::from_rgba_unmultiplied(0, 255, 65, 18),
            );
        }
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        add(&mut child);
    }

    let id = ui.make_persistent_id(("nexus_list_row", rect.min.y.to_bits()));
    let resp = ui.interact(rect, id, egui::Sense::click());
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
    /// Play `list`, starting at `index`, replacing the queue.
    Play { list: Vec<Track>, index: usize },
}

/// Persistent state shared across the library views (search box, sort, the
/// drill-down selection, and a one-page track cache so we don't re-query
/// SQLite every frame for an unchanged scroll position).
pub struct LibraryUi {
    pub search: String,
    pub sort: SortKey,
    pub artist_filter: Option<String>,
    pub album_filter: Option<String>,

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
