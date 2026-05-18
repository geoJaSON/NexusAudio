//! Core library data models. The audio *file* is canonical for tag fields;
//! the SQLite row is a cache keyed by (path, mtime, file_size_bytes), which is
//! what makes incremental scanning and a future tag editor possible.

use std::collections::VecDeque;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: Uuid,
    pub path: PathBuf,
    /// File mtime (unix secs) — part of the incremental-scan change key.
    pub mtime: i64,
    pub file_size_bytes: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genre: String,
    pub year: Option<u32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_secs: f64,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub codec: String,
    pub date_added: DateTime<Utc>,
    pub play_count: u32,
    pub rating: Option<u8>,
    pub last_played: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Audiobook {
    pub id: Uuid,
    pub path: PathBuf,
    pub title: String,
    pub author: String,
    pub narrator: Option<String>,
    pub genre: String,
    pub year: Option<u32>,
    pub duration_secs: f64,
    pub chapters: Vec<Chapter>,
    pub codec: String,
    pub date_added: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub index: u32,
    pub title: String,
    pub start_secs: f64,
    pub end_secs: f64,
}

/// CRITICAL state. Persisted atomically to resume.json; see store::json_store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeState {
    pub audiobook_id: Uuid,
    pub position_secs: f64,
    pub chapter_index: u32,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: Uuid,
    pub name: String,
    pub track_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct PlaybackQueue {
    pub current: Option<QueueEntry>,
    pub upcoming: VecDeque<QueueEntry>,
    pub history: Vec<QueueEntry>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub track: Track,
    pub source: QueueSource,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatMode {
    #[default]
    None,
    One,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueSource {
    Library,
    Playlist(Uuid),
    Album(String),
    Artist(String),
    Manual,
}
