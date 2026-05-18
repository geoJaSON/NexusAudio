//! Incremental library scanner. Runs on a background thread with its own
//! SQLite connection (WAL mode handles the concurrent UI reader). Files whose
//! (mtime, size) match the stored stamp are skipped without re-tagging — this
//! is what keeps a 50k-track rescan a fast stat-only pass.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::UNIX_EPOCH;

use chrono::Utc;
use lofty::prelude::*;
use uuid::Uuid;
use walkdir::WalkDir;

use super::db::Db;
use super::models::Track;

/// Music extensions. `.m4b` is intentionally excluded — audiobooks are a
/// separate scan path (Phase 6).
const MUSIC_EXTS: &[&str] = &["mp3", "flac", "ogg", "wav", "aiff", "aif", "m4a", "aac", "opus"];

#[derive(Debug, Clone)]
pub enum ScanMsg {
    Started { total: usize },
    Progress { done: usize, total: usize, current: String },
    Done { added: usize, updated: usize, removed: usize, errors: usize },
    Failed(String),
}

/// Spawn an incremental scan of `folders` into the DB at `db_path`. Returns a
/// receiver the UI polls (non-blocking) each frame.
pub fn spawn_scan(db_path: PathBuf, folders: Vec<PathBuf>) -> Receiver<ScanMsg> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let db = match Db::open(&db_path) {
            Ok(db) => db,
            Err(e) => {
                let _ = tx.send(ScanMsg::Failed(format!("open db: {e}")));
                return;
            }
        };

        // Pass 1: enumerate candidate files so progress has a real total.
        let mut files: Vec<PathBuf> = Vec::new();
        for root in &folders {
            for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
                let p = entry.path();
                if entry.file_type().is_file() && is_music(p) {
                    files.push(p.to_path_buf());
                }
            }
        }
        let total = files.len();
        let _ = tx.send(ScanMsg::Started { total });

        // Pass 2: incremental tag + upsert.
        let (mut added, mut updated, mut errors) = (0usize, 0usize, 0usize);
        for (i, path) in files.iter().enumerate() {
            let (mtime, size) = match file_stamp(path) {
                Some(v) => v,
                None => {
                    errors += 1;
                    continue;
                }
            };
            match db.file_stamp(path) {
                Ok(Some((m, s))) if m == mtime && s == size => {} // unchanged → skip
                Ok(existing) => match read_track(path, mtime, size) {
                    Ok(track) => {
                        if db.upsert_track(&track).is_ok() {
                            if existing.is_some() {
                                updated += 1;
                            } else {
                                added += 1;
                            }
                        } else {
                            errors += 1;
                        }
                    }
                    Err(_) => errors += 1,
                },
                Err(_) => errors += 1,
            }

            if i % 32 == 0 || i + 1 == total {
                let _ = tx.send(ScanMsg::Progress {
                    done: i + 1,
                    total,
                    current: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                });
            }
        }

        let removed = db.prune_missing().unwrap_or(0);
        let _ = tx.send(ScanMsg::Done { added, updated, removed, errors });
    });
    rx
}

/// Count music files under a folder (for the Folders view's per-dir tally).
pub fn count_files(root: &Path) -> u64 {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && is_music(e.path()))
        .count() as u64
}

/// Headless smoke test of the full scan→DB→query pipeline. Invoked via
/// `nexus-audio --scan-smoke <folder>` so it exercises real code paths
/// without launching the GUI.
pub fn smoke(folder: &Path) {
    let db = Db::open_in_memory().expect("mem db");
    let files: Vec<PathBuf> = WalkDir::new(folder)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && is_music(e.path()))
        .map(|e| e.path().to_path_buf())
        .collect();
    println!("scan-smoke: {} music files under {}", files.len(), folder.display());

    let (mut ok, mut err) = (0, 0);
    for p in &files {
        let Some((m, s)) = file_stamp(p) else {
            err += 1;
            continue;
        };
        match read_track(p, m, s) {
            Ok(t) => {
                println!(
                    "  + {:<28} | {:<22} | {:<22} | {:>7.1}s | {}",
                    truncate(&t.title, 28),
                    truncate(&t.artist, 22),
                    truncate(&t.album, 22),
                    t.duration_secs,
                    t.codec
                );
                db.upsert_track(&t).ok();
                ok += 1;
            }
            Err(e) => {
                println!("  ! {} :: {e}", p.display());
                err += 1;
            }
        }
    }
    println!("tagged ok={ok} err={err}");
    println!("db.track_count = {:?}", db.track_count());
    println!("db.albums      = {} groups", db.albums().map(|v| v.len()).unwrap_or(0));
    println!("db.artists     = {} groups", db.artists().map(|v| v.len()).unwrap_or(0));
    for q in ["loverboy", "accept", "oyster", "balls", "zzznomatch"] {
        let hits = db.count(q).unwrap_or(-1);
        let page = db
            .tracks_page(q, crate::library::db::SortKey::Title, 5, 0)
            .unwrap_or_default();
        let titles: Vec<&str> = page.iter().map(|t| t.title.as_str()).collect();
        println!("FTS {q:<11} = {hits} hits {titles:?}");
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
    } else {
        s.to_string()
    }
}

fn is_music(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| MUSIC_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn file_stamp(p: &Path) -> Option<(i64, u64)> {
    let md = std::fs::metadata(p).ok()?;
    let mtime = md
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Some((mtime, md.len()))
}

fn read_track(path: &Path, mtime: i64, size: u64) -> anyhow::Result<Track> {
    let tagged = lofty::read_from_path(path)?;
    let props = tagged.properties();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let s = |k: ItemKey| -> String {
        tag.and_then(|t| t.get_string(&k).map(|v| v.to_string()))
            .unwrap_or_default()
    };
    let title = tag
        .and_then(|t| t.title().map(|c| c.to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    let artist = tag
        .and_then(|t| t.artist().map(|c| c.to_string()))
        .unwrap_or_default();
    let album = tag
        .and_then(|t| t.album().map(|c| c.to_string()))
        .unwrap_or_default();
    let genre = tag
        .and_then(|t| t.genre().map(|c| c.to_string()))
        .unwrap_or_default();
    let album_artist = {
        let aa = s(ItemKey::AlbumArtist);
        if aa.is_empty() {
            artist.clone()
        } else {
            aa
        }
    };

    Ok(Track {
        id: Uuid::new_v4(),
        path: path.to_path_buf(),
        mtime,
        file_size_bytes: size,
        title,
        artist,
        album,
        album_artist,
        genre,
        year: tag.and_then(|t| t.year()),
        track_number: tag.and_then(|t| t.track()),
        disc_number: tag.and_then(|t| t.disk()),
        duration_secs: props.duration().as_secs_f64(),
        bitrate_kbps: props.audio_bitrate(),
        sample_rate: props.sample_rate(),
        bit_depth: props.bit_depth(),
        codec: codec_label(&tagged),
        date_added: Utc::now(),
        play_count: 0,
        rating: None,
        last_played: None,
    })
}

fn codec_label(tagged: &lofty::file::TaggedFile) -> String {
    use lofty::file::FileType::*;
    match tagged.file_type() {
        Mpeg => "MP3",
        Flac => "FLAC",
        Vorbis => "OGG",
        Opus => "OPUS",
        Wav => "WAV",
        Aiff => "AIFF",
        Mp4 => "AAC",
        Ape => "APE",
        _ => "AUDIO",
    }
    .to_string()
}
