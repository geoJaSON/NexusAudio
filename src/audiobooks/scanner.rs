//! Audiobook scanner. Rebuilds the audiobooks/chapters tables wholesale each
//! run (few enough books to not need incremental). Book ids are deterministic
//! UUIDv5(path) so saved resume positions survive a rescan.
//!
//! Book detection:
//!   - every `.m4b` file is its own single-file book (QuickTime/Nero chapters);
//!   - other audio grouped by parent directory: a dir with >1 file is one
//!     multi-file book (each file = a chapter); a lone file is a single book.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use chrono::Utc;
use lofty::prelude::*;
use uuid::Uuid;
use walkdir::WalkDir;

use super::chapters;
use crate::library::db::Db;
use crate::library::models::{Audiobook, Chapter};

const AB_EXTS: &[&str] = &["m4b", "mp3", "m4a", "flac", "ogg", "opus", "aac"];

#[derive(Debug, Clone)]
pub enum AbScanMsg {
    Started { total: usize },
    Progress { done: usize, total: usize, current: String },
    Done { count: usize, errors: usize },
    Failed(String),
}

/// Stable id for a book from its identifying path (the .m4b file, or the
/// directory for a multi-file book).
pub fn book_id(path: &Path) -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, path.to_string_lossy().as_bytes())
}

pub fn spawn_scan(db_path: PathBuf, folders: Vec<PathBuf>) -> Receiver<AbScanMsg> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let db = match Db::open(&db_path) {
            Ok(d) => d,
            Err(e) => {
                let _ = tx.send(AbScanMsg::Failed(format!("open db: {e}")));
                return;
            }
        };

        // m4b files → single books. Other audio → grouped by parent dir.
        let mut m4b: Vec<PathBuf> = Vec::new();
        let mut by_dir: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
        for root in &folders {
            for e in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
                if !e.file_type().is_file() {
                    continue;
                }
                let p = e.path();
                match ext(p).as_deref() {
                    Some("m4b") => m4b.push(p.to_path_buf()),
                    Some(x) if AB_EXTS.contains(&x) => {
                        by_dir
                            .entry(p.parent().unwrap_or(root).to_path_buf())
                            .or_default()
                            .push(p.to_path_buf());
                    }
                    _ => {}
                }
            }
        }

        let total = m4b.len() + by_dir.len();
        let _ = tx.send(AbScanMsg::Started { total });
        if db.clear_audiobooks().is_err() {
            let _ = tx.send(AbScanMsg::Failed("clear failed".into()));
            return;
        }

        let (mut done, mut count, mut errors) = (0usize, 0usize, 0usize);
        let progress = |done: usize, total: usize, name: &str, tx: &mpsc::Sender<_>| {
            let _ = tx.send(AbScanMsg::Progress {
                done,
                total,
                current: name.to_string(),
            });
        };

        for p in &m4b {
            match single_m4b(p) {
                Some(b) => {
                    if db.upsert_audiobook(&b).is_ok() {
                        count += 1;
                    } else {
                        errors += 1;
                    }
                }
                None => errors += 1,
            }
            done += 1;
            progress(done, total, &file_name(p), &tx);
        }

        for (dir, mut files) in by_dir {
            files.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
            if let Some(b) = multi_or_single(&dir, &files) {
                if db.upsert_audiobook(&b).is_ok() {
                    count += 1;
                } else {
                    errors += 1;
                }
            } else {
                errors += 1;
            }
            done += 1;
            progress(done, total, &file_name(&dir), &tx);
        }

        let _ = tx.send(AbScanMsg::Done { count, errors });
    });
    rx
}

/// Headless smoke: `nexus-audio --ab-smoke <folder>`.
pub fn smoke(root: &Path) {
    println!("ab-smoke: {}", root.display());
    let mut m4b = Vec::new();
    let mut by_dir: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for e in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        if !e.file_type().is_file() {
            continue;
        }
        let p = e.path();
        match ext(p).as_deref() {
            Some("m4b") => m4b.push(p.to_path_buf()),
            Some(x) if AB_EXTS.contains(&x) => by_dir
                .entry(p.parent().unwrap_or(root).to_path_buf())
                .or_default()
                .push(p.to_path_buf()),
            _ => {}
        }
    }
    let mut books: Vec<Audiobook> = Vec::new();
    for p in &m4b {
        if let Some(b) = single_m4b(p) {
            books.push(b);
        }
    }
    for (dir, mut files) in by_dir {
        files.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        if let Some(b) = multi_or_single(&dir, &files) {
            books.push(b);
        }
    }
    println!("found {} book(s):", books.len());
    for b in &books {
        let d = b.duration_secs as u64;
        println!(
            "  \"{}\" by {} | {}:{:02}:{:02} | {} chapters | id={}",
            b.title,
            if b.author.is_empty() { "?" } else { &b.author },
            d / 3600,
            (d % 3600) / 60,
            d % 60,
            b.chapters.len(),
            b.id
        );
        for c in b.chapters.iter().take(3) {
            let s = c.start_secs as u64;
            println!(
                "      ch{:>2} {:>2}:{:02}:{:02}  {}",
                c.index,
                s / 3600,
                (s % 3600) / 60,
                s % 60,
                c.title
            );
        }
        if b.chapters.len() > 3 {
            println!("      … {} more", b.chapters.len() - 3);
        }
    }
}

fn single_m4b(path: &Path) -> Option<Audiobook> {
    let tag = read_tags(path);
    let duration = chapters::mp4_duration_secs(path)
        .or_else(|| lofty_duration(path))
        .unwrap_or(0.0);
    let mut chs = chapters::read_m4b_chapters(path);
    fill_ends(&mut chs, duration);
    // Prefer album (book title for audiobooks) over the ©nam tag, which is
    // often the first chapter's name. If the candidate just echoes chapter 1,
    // the tags are unhelpful — use the filename.
    let cand = tag.album_or_title.clone().or_else(|| tag.title.clone());
    let title = match cand {
        Some(t) if chs.first().map(|c| c.title == t).unwrap_or(false) => stem(path),
        Some(t) => t,
        None => stem(path),
    };
    Some(Audiobook {
        id: book_id(path),
        path: path.to_path_buf(),
        title,
        author: tag.author,
        narrator: tag.narrator,
        genre: tag.genre,
        year: tag.year,
        duration_secs: duration,
        chapters: chs,
        codec: "AAC".into(),
        date_added: Utc::now(),
    })
}

fn multi_or_single(dir: &Path, files: &[PathBuf]) -> Option<Audiobook> {
    let first = files.first()?;
    if files.len() == 1 {
        // Lone file → single-file book (no embedded chapters).
        let tag = read_tags(first);
        let dur = lofty_duration(first).unwrap_or(0.0);
        return Some(Audiobook {
            id: book_id(first),
            path: first.clone(),
            title: tag.title.unwrap_or_else(|| stem(first)),
            author: tag.author,
            narrator: tag.narrator,
            genre: tag.genre,
            year: tag.year,
            duration_secs: dur,
            chapters: Vec::new(),
            codec: ext(first).unwrap_or_default().to_uppercase(),
            date_added: Utc::now(),
        });
    }

    // Multi-file: the directory is the book; each file is one chapter.
    let book_tag = read_tags(first);
    let mut chs = Vec::with_capacity(files.len());
    let mut acc = 0.0f64;
    for (i, f) in files.iter().enumerate() {
        let d = lofty_duration(f).unwrap_or(0.0);
        let t = read_tags(f);
        chs.push(Chapter {
            index: i as u32,
            title: t.title.unwrap_or_else(|| stem(f)),
            start_secs: acc,
            end_secs: acc + d,
        });
        acc += d;
    }
    Some(Audiobook {
        id: book_id(dir),
        path: dir.to_path_buf(),
        title: book_tag
            .album_or_title
            .or(book_tag.title)
            .unwrap_or_else(|| stem(dir)),
        author: book_tag.author,
        narrator: book_tag.narrator,
        genre: book_tag.genre,
        year: book_tag.year,
        duration_secs: acc,
        chapters: chs,
        codec: ext(first).unwrap_or_default().to_uppercase(),
        date_added: Utc::now(),
    })
}

#[derive(Default)]
struct Tags {
    title: Option<String>,
    album_or_title: Option<String>,
    author: String,
    narrator: Option<String>,
    genre: String,
    year: Option<u32>,
}

fn read_tags(path: &Path) -> Tags {
    let Ok(tf) = lofty::read_from_path(path) else {
        return Tags::default();
    };
    let tag = tf.primary_tag().or_else(|| tf.first_tag());
    let g = |k: ItemKey| tag.and_then(|t| t.get_string(&k).map(|s| s.to_string()));
    let nonempty = |s: Option<String>| s.filter(|v| !v.trim().is_empty());

    let album = nonempty(tag.and_then(|t| t.album().map(|c| c.to_string())));
    let title = nonempty(tag.and_then(|t| t.title().map(|c| c.to_string())));
    let artist = nonempty(tag.and_then(|t| t.artist().map(|c| c.to_string())));
    let album_artist = nonempty(g(ItemKey::AlbumArtist));
    Tags {
        // Audiobooks usually carry the book title in `album`.
        album_or_title: album.clone(),
        title: title.or(album),
        author: album_artist.or(artist).unwrap_or_default(),
        narrator: nonempty(g(ItemKey::Composer)),
        genre: nonempty(tag.and_then(|t| t.genre().map(|c| c.to_string())))
            .unwrap_or_default(),
        year: tag.and_then(|t| t.year()),
    }
}

fn lofty_duration(path: &Path) -> Option<f64> {
    lofty::read_from_path(path)
        .ok()
        .map(|t| t.properties().duration().as_secs_f64())
}

fn fill_ends(chs: &mut [Chapter], total: f64) {
    let n = chs.len();
    for i in 0..n {
        chs[i].end_secs = if i + 1 < n {
            chs[i + 1].start_secs
        } else {
            total.max(chs[i].start_secs)
        };
    }
}

fn ext(p: &Path) -> Option<String> {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}
fn stem(p: &Path) -> String {
    p.file_stem()
        .or(p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}
fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}
/// Order multi-file parts by track number then filename.
fn sort_key(p: &Path) -> (u32, String) {
    let track = lofty::read_from_path(p)
        .ok()
        .and_then(|t| t.primary_tag().and_then(|t| t.track()))
        .unwrap_or(u32::MAX);
    (track, file_name(p).to_lowercase())
}
