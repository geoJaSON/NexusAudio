//! Resume-position persistence — the project's critical correctness feature.
//!
//! Phase 3 groundwork: an atomically-persisted `audiobook_id → ResumeState`
//! map. The 15-second autosave cadence and the resume dialog wire in at
//! Phase 6 when the audiobook library/UI exists; the storage contract is
//! fixed now so the engine has a safe place to record positions.
//!
//! Whole module is Phase 6 groundwork — unused until then, by design.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::library::models::ResumeState;
use crate::store::json_store;

const FILE: &str = "resume.json";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ResumeStore {
    #[serde(flatten)]
    map: HashMap<Uuid, ResumeState>,
}

impl ResumeStore {
    pub fn load(dir: &Path) -> Self {
        json_store::load_or_default(&dir.join(FILE))
    }

    /// Atomic write (temp + rename) so a crash mid-save never corrupts the
    /// map — this is why resume survives an unclean shutdown.
    pub fn save(&self, dir: &Path) {
        if let Err(e) = json_store::save(&dir.join(FILE), self) {
            eprintln!("resume save failed: {e}");
        }
    }

    pub fn get(&self, audiobook_id: &Uuid) -> Option<&ResumeState> {
        self.map.get(audiobook_id)
    }

    /// Record/refresh a position. Caller persists via `save` on the 15 s
    /// cadence and on pause/stop/close (Phase 6).
    pub fn set(&mut self, audiobook_id: Uuid, position_secs: f64, chapter_index: u32) {
        self.map.insert(
            audiobook_id,
            ResumeState {
                audiobook_id,
                position_secs,
                chapter_index,
                last_updated: Utc::now(),
            },
        );
    }

    pub fn clear(&mut self, audiobook_id: &Uuid) {
        self.map.remove(audiobook_id);
    }
}
