pub mod player_bar;
pub mod sidebar;
pub mod theme;
pub mod titlebar;
pub mod views;

/// Top-level navigation target (sidebar selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Tracks,
    Albums,
    Artists,
    Queue,
    Folders,
    Audiobooks,
    Settings,
}

impl Default for View {
    fn default() -> Self {
        View::Tracks
    }
}
