//! Playlist store: CRUD over `Playlist` records, persisted atomically to
//! playlists.json, plus M3U import/export. Tracks are referenced by `Uuid`;
//! the App resolves ids → `Track` via the library DB when playing.

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::library::models::{Playlist, Track};
use crate::store::json_store;

const FILE: &str = "playlists.json";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlaylistStore {
    pub lists: Vec<Playlist>,
}

impl PlaylistStore {
    pub fn load(dir: &Path) -> Self {
        json_store::load_or_default(&dir.join(FILE))
    }

    pub fn save(&self, dir: &Path) {
        if let Err(e) = json_store::save(&dir.join(FILE), self) {
            eprintln!("playlists save failed: {e}");
        }
    }

    pub fn get(&self, id: Uuid) -> Option<&Playlist> {
        self.lists.iter().find(|p| p.id == id)
    }

    pub fn create(&mut self, name: &str) -> Uuid {
        let now = Utc::now();
        let p = Playlist {
            id: Uuid::new_v4(),
            name: unique_name(&self.lists, name),
            track_ids: Vec::new(),
            created_at: now,
            updated_at: now,
        };
        let id = p.id;
        self.lists.push(p);
        id
    }

    pub fn delete(&mut self, id: Uuid) {
        self.lists.retain(|p| p.id != id);
    }

    pub fn rename(&mut self, id: Uuid, name: &str) {
        if let Some(p) = self.lists.iter_mut().find(|p| p.id == id) {
            p.name = name.to_string();
            p.updated_at = Utc::now();
        }
    }

    pub fn duplicate(&mut self, id: Uuid) -> Option<Uuid> {
        let src = self.get(id)?.clone();
        let now = Utc::now();
        let copy = Playlist {
            id: Uuid::new_v4(),
            name: unique_name(&self.lists, &format!("{} COPY", src.name)),
            track_ids: src.track_ids,
            created_at: now,
            updated_at: now,
        };
        let new_id = copy.id;
        self.lists.push(copy);
        Some(new_id)
    }

    pub fn add_track(&mut self, id: Uuid, track_id: Uuid) {
        if let Some(p) = self.lists.iter_mut().find(|p| p.id == id) {
            if !p.track_ids.contains(&track_id) {
                p.track_ids.push(track_id);
                p.updated_at = Utc::now();
            }
        }
    }

    pub fn remove_at(&mut self, id: Uuid, idx: usize) {
        if let Some(p) = self.lists.iter_mut().find(|p| p.id == id) {
            if idx < p.track_ids.len() {
                p.track_ids.remove(idx);
                p.updated_at = Utc::now();
            }
        }
    }

    pub fn move_at(&mut self, id: Uuid, idx: usize, up: bool) {
        if let Some(p) = self.lists.iter_mut().find(|p| p.id == id) {
            let j = if up { idx.wrapping_sub(1) } else { idx + 1 };
            if idx < p.track_ids.len() && j < p.track_ids.len() {
                p.track_ids.swap(idx, j);
                p.updated_at = Utc::now();
            }
        }
    }

    /// Export a playlist as an extended M3U of absolute file paths.
    pub fn export_m3u(&self, id: Uuid, resolve: impl Fn(Uuid) -> Option<Track>, out: &Path) -> std::io::Result<()> {
        let mut s = String::from("#EXTM3U\n");
        if let Some(p) = self.get(id) {
            for tid in &p.track_ids {
                if let Some(t) = resolve(*tid) {
                    s.push_str(&format!(
                        "#EXTINF:{},{} - {}\n{}\n",
                        t.duration_secs as i64,
                        t.artist,
                        t.title,
                        t.path.display()
                    ));
                }
            }
        }
        std::fs::write(out, s)
    }

    /// Import an M3U: every path that resolves to a library track id becomes a
    /// new playlist named after the file.
    pub fn import_m3u(
        &mut self,
        path: &Path,
        resolve_path: impl Fn(&Path) -> Option<Uuid>,
    ) -> std::io::Result<Uuid> {
        let text = std::fs::read_to_string(path)?;
        let mut ids = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(tid) = resolve_path(&PathBuf::from(line)) {
                ids.push(tid);
            }
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_uppercase())
            .unwrap_or_else(|| "IMPORTED".into());
        let id = self.create(&name);
        if let Some(p) = self.lists.iter_mut().find(|p| p.id == id) {
            p.track_ids = ids;
        }
        Ok(id)
    }
}

fn unique_name(lists: &[Playlist], base: &str) -> String {
    let base = if base.trim().is_empty() { "UNTITLED" } else { base.trim() };
    if !lists.iter().any(|p| p.name == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|cand| !lists.iter().any(|p| &p.name == cand))
        .unwrap()
}
