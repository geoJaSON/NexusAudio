//! App settings — watched folders, scan bookkeeping, UI prefs. Persisted
//! atomically to <data_dir>/nexus-audio/settings.json.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::store::json_store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub music_folders: Vec<PathBuf>,
    pub audiobook_folders: Vec<PathBuf>,
    pub auto_scan_on_startup: bool,
    pub resume_save_interval_secs: u64,
    pub eq_enabled: bool,
    pub scanline_intensity: f32,
    /// Accent (selection/hover/active strokes) and default text color.
    /// Default = phosphor green / dim green. Note: explicitly-colored UI bits
    /// stay green until a full runtime-palette pass.
    pub accent_color: [u8; 3],
    pub text_color: [u8; 3],
    /// Per-folder bookkeeping shown in the Folders view: last scan time and
    /// the file count found on that scan. Keyed by the folder's display path.
    pub folder_stats: HashMap<String, FolderStat>,
}

pub const DEFAULT_ACCENT: [u8; 3] = [0, 255, 65]; // CRT_GREEN
pub const DEFAULT_TEXT: [u8; 3] = [0, 179, 44]; // CRT_DIM

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FolderStat {
    pub last_scan: Option<DateTime<Utc>>,
    pub file_count: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            music_folders: Vec::new(),
            audiobook_folders: Vec::new(),
            auto_scan_on_startup: true,
            resume_save_interval_secs: 15,
            eq_enabled: true,
            scanline_intensity: 0.08,
            accent_color: DEFAULT_ACCENT,
            text_color: DEFAULT_TEXT,
            folder_stats: HashMap::new(),
        }
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Self {
        json_store::load_or_default(&dir.join("settings.json"))
    }

    pub fn save(&self, dir: &Path) {
        if let Err(e) = json_store::save(&dir.join("settings.json"), self) {
            eprintln!("settings save failed: {e}");
        }
    }
}
