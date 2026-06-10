//! SQLite-backed library store. The rest of the app talks to this narrow API
//! and never touches SQL directly. Sized for 50k+ tracks: indexed columns,
//! FTS5 search, and SQL-side sort/paging (never an in-memory sort of the lot).
//!
//! FTS is a standalone fts5 table kept in sync from Rust on upsert/prune —
//! deliberately not external-content + triggers, which is fiddly to get right.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::models::{Audiobook, Chapter, Track};

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS tracks (
  id           TEXT PRIMARY KEY,
  path         TEXT UNIQUE NOT NULL,
  mtime        INTEGER NOT NULL,
  file_size    INTEGER NOT NULL,
  title        TEXT, artist TEXT, album TEXT, album_artist TEXT, genre TEXT,
  year         INTEGER, track_no INTEGER, disc_no INTEGER,
  duration_secs REAL, bitrate INTEGER, sample_rate INTEGER, bit_depth INTEGER,
  codec        TEXT, date_added TEXT,
  play_count   INTEGER NOT NULL DEFAULT 0, rating INTEGER, last_played TEXT
);
CREATE INDEX IF NOT EXISTS idx_tracks_artist       ON tracks(artist);
CREATE INDEX IF NOT EXISTS idx_tracks_album        ON tracks(album);
CREATE INDEX IF NOT EXISTS idx_tracks_album_artist ON tracks(album_artist);
CREATE INDEX IF NOT EXISTS idx_tracks_date_added   ON tracks(date_added);

CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(
  track_id UNINDEXED, title, artist, album, genre
);

CREATE TABLE IF NOT EXISTS audiobooks (
  id TEXT PRIMARY KEY, path TEXT UNIQUE NOT NULL,
  title TEXT, author TEXT, narrator TEXT, genre TEXT, year INTEGER,
  duration_secs REAL, codec TEXT, date_added TEXT
);
CREATE TABLE IF NOT EXISTS chapters (
  book_id TEXT NOT NULL REFERENCES audiobooks(id) ON DELETE CASCADE,
  idx INTEGER NOT NULL, title TEXT, start_secs REAL, end_secs REAL,
  PRIMARY KEY (book_id, idx)
);
"#;

const TRACK_COLS: &str = "id,path,mtime,file_size,title,artist,album,album_artist,genre,\
     year,track_no,disc_no,duration_secs,bitrate,sample_rate,bit_depth,\
     codec,date_added,play_count,rating,last_played";

pub struct Db {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct AlbumRow {
    pub album: String,
    pub album_artist: String,
    pub year: Option<u32>,
    pub track_count: u32,
}

#[derive(Debug, Clone)]
pub struct ArtistRow {
    pub artist: String,
    pub album_count: u32,
    pub track_count: u32,
}

/// Whitelisted sort columns — never interpolate user input into SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Title,
    Artist,
    Album,
    DateAdded,
}

impl SortKey {
    /// ORDER BY clause, every column qualified with `p` (e.g. "t" or "tracks")
    /// so it is unambiguous inside the FTS join.
    fn order_by(self, p: &str) -> String {
        match self {
            SortKey::Title => format!("{p}.title COLLATE NOCASE"),
            SortKey::Artist => format!(
                "{p}.artist COLLATE NOCASE, {p}.album COLLATE NOCASE, {p}.disc_no, {p}.track_no"
            ),
            SortKey::Album => format!("{p}.album COLLATE NOCASE, {p}.disc_no, {p}.track_no"),
            SortKey::DateAdded => format!("{p}.date_added DESC"),
        }
    }
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path).with_context(|| format!("open db {path:?}"))?;
        conn.execute_batch(SCHEMA).context("init schema")?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn track_count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0))?
            as u64)
    }

    /// Incremental-scan key: stored (mtime, size) for a path so the scanner can
    /// skip unchanged files without re-tagging them.
    pub fn file_stamp(&self, path: &Path) -> Result<Option<(i64, u64)>> {
        let p = path.to_string_lossy();
        Ok(self
            .conn
            .query_row(
                "SELECT mtime, file_size FROM tracks WHERE path = ?1",
                params![p],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u64)),
            )
            .optional()?)
    }

    pub fn upsert_track(&self, t: &Track) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO tracks
               (id,path,mtime,file_size,title,artist,album,album_artist,genre,
                year,track_no,disc_no,duration_secs,bitrate,sample_rate,bit_depth,
                codec,date_added,play_count,rating,last_played)
               VALUES
               (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)
               ON CONFLICT(path) DO UPDATE SET
                 mtime=excluded.mtime, file_size=excluded.file_size,
                 title=excluded.title, artist=excluded.artist, album=excluded.album,
                 album_artist=excluded.album_artist, genre=excluded.genre,
                 year=excluded.year, track_no=excluded.track_no, disc_no=excluded.disc_no,
                 duration_secs=excluded.duration_secs, bitrate=excluded.bitrate,
                 sample_rate=excluded.sample_rate, bit_depth=excluded.bit_depth,
                 codec=excluded.codec"#,
            params![
                t.id.to_string(),
                t.path.to_string_lossy(),
                t.mtime,
                t.file_size_bytes as i64,
                t.title,
                t.artist,
                t.album,
                t.album_artist,
                t.genre,
                t.year,
                t.track_number,
                t.disc_number,
                t.duration_secs,
                t.bitrate_kbps,
                t.sample_rate,
                t.bit_depth.map(|b| b as u32),
                t.codec,
                t.date_added.to_rfc3339(),
                t.play_count,
                t.rating.map(|r| r as u32),
                t.last_played.map(|d| d.to_rfc3339()),
            ],
        )?;
        // Keep FTS in sync (idempotent: clear then insert).
        let id = t.id.to_string();
        self.conn
            .execute("DELETE FROM tracks_fts WHERE track_id = ?1", params![id])?;
        self.conn.execute(
            "INSERT INTO tracks_fts (track_id,title,artist,album,genre)
             VALUES (?1,?2,?3,?4,?5)",
            params![id, t.title, t.artist, t.album, t.genre],
        )?;
        Ok(())
    }

    /// Drop rows whose files no longer exist on disk (post-scan prune).
    ///
    /// Only paths under a currently-reachable watch root are considered: if a
    /// whole root is absent (offline network/external drive), its rows are
    /// kept — a temporarily unplugged drive must not wipe the library.
    pub fn prune_missing(&self, roots: &[PathBuf]) -> Result<usize> {
        let live: Vec<&PathBuf> = roots.iter().filter(|r| r.exists()).collect();
        if live.is_empty() {
            return Ok(0);
        }
        let mut stmt = self.conn.prepare("SELECT id, path FROM tracks")?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        let mut removed = 0;
        for (id, p) in rows {
            let pb = PathBuf::from(&p);
            if live.iter().any(|r| pb.starts_with(r)) && !pb.exists() {
                self.conn
                    .execute("DELETE FROM tracks WHERE id = ?1", params![id])?;
                self.conn
                    .execute("DELETE FROM tracks_fts WHERE track_id = ?1", params![id])?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Delete tracks whose file lives under any of `dirs` (used to evict
    /// audiobook MP3s that leaked into the music library before exclusion).
    pub fn remove_tracks_under(&self, dirs: &[PathBuf]) -> usize {
        if dirs.is_empty() {
            return 0;
        }
        let rows: Vec<(String, String)> = match self
            .conn
            .prepare("SELECT id, path FROM tracks")
        {
            Ok(mut stmt) => stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map(|it| it.filter_map(|r| r.ok()).collect())
                .unwrap_or_default(),
            Err(_) => return 0,
        };
        let mut removed = 0;
        for (id, p) in rows {
            let pb = PathBuf::from(&p);
            if dirs.iter().any(|d| pb.starts_with(d)) {
                let _ = self
                    .conn
                    .execute("DELETE FROM tracks WHERE id = ?1", params![id]);
                let _ = self
                    .conn
                    .execute("DELETE FROM tracks_fts WHERE track_id = ?1", params![id]);
                removed += 1;
            }
        }
        removed
    }

    /// Delete audiobooks (and their chapters) whose path lives under any of
    /// `dirs` — used when a watched audiobook folder is removed.
    pub fn remove_audiobooks_under(&self, dirs: &[PathBuf]) -> usize {
        if dirs.is_empty() {
            return 0;
        }
        let rows: Vec<(String, String)> = match self
            .conn
            .prepare("SELECT id, path FROM audiobooks")
        {
            Ok(mut stmt) => stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map(|it| it.filter_map(|r| r.ok()).collect())
                .unwrap_or_default(),
            Err(_) => return 0,
        };
        let mut removed = 0;
        for (id, p) in rows {
            if dirs.iter().any(|d| PathBuf::from(&p).starts_with(d)) {
                let _ = self
                    .conn
                    .execute("DELETE FROM chapters WHERE book_id = ?1", params![id]);
                let _ = self
                    .conn
                    .execute("DELETE FROM audiobooks WHERE id = ?1", params![id]);
                removed += 1;
            }
        }
        removed
    }

    /// How many rows a query/search returns (for the scrollbar's virtual size).
    pub fn count(&self, search: &str) -> Result<i64> {
        if search.trim().is_empty() {
            Ok(self
                .conn
                .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?)
        } else {
            Ok(self.conn.query_row(
                "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH ?1",
                params![fts_query(search)],
                |r| r.get(0),
            )?)
        }
    }

    /// One page of tracks, optionally FTS-filtered, always sorted SQL-side.
    pub fn tracks_page(
        &self,
        search: &str,
        sort: SortKey,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Track>> {
        if search.trim().is_empty() {
            let order = sort.order_by("tracks");
            let sql = format!(
                "SELECT {TRACK_COLS} FROM tracks ORDER BY {order} LIMIT ?1 OFFSET ?2"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt
                .query_map(params![limit, offset], row_to_track)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        } else {
            // Every selected/ordered column qualified with `t.` — `tracks_fts`
            // shares column names (title/artist/album/genre) so bare names are
            // ambiguous and SQLite would reject the statement.
            let cols = TRACK_COLS
                .split(',')
                .map(|c| format!("t.{c}"))
                .collect::<Vec<_>>()
                .join(",");
            let order = sort.order_by("t");
            let sql = format!(
                "SELECT {cols} FROM tracks_fts f JOIN tracks t ON t.id = f.track_id
                 WHERE tracks_fts MATCH ?1 ORDER BY {order} LIMIT ?2 OFFSET ?3"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt
                .query_map(params![fts_query(search), limit, offset], row_to_track)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        }
    }

    pub fn albums(&self) -> Result<Vec<AlbumRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT album,
                    COALESCE(NULLIF(MAX(album_artist),''), MAX(artist)) AS aa,
                    MAX(year), COUNT(*)
             FROM tracks WHERE album <> ''
             GROUP BY album COLLATE NOCASE
             ORDER BY album COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(AlbumRow {
                    album: r.get(0)?,
                    album_artist: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    year: r.get(2)?,
                    track_count: r.get::<_, i64>(3)? as u32,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn artists(&self) -> Result<Vec<ArtistRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT artist, COUNT(DISTINCT album), COUNT(*)
             FROM tracks WHERE artist <> ''
             GROUP BY artist COLLATE NOCASE
             ORDER BY artist COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ArtistRow {
                    artist: r.get(0)?,
                    album_count: r.get::<_, i64>(1)? as u32,
                    track_count: r.get::<_, i64>(2)? as u32,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn genres(&self) -> Result<Vec<(String, u32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT genre, COUNT(*) FROM tracks WHERE genre <> ''
             GROUP BY genre COLLATE NOCASE ORDER BY genre COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u32))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn tracks_where(&self, column: &str, value: &str) -> Result<Vec<Track>> {
        // `column` is caller-controlled and whitelisted here, never user input.
        let col = match column {
            "album" => "album",
            "artist" => "artist",
            "genre" => "genre",
            _ => return Ok(Vec::new()),
        };
        let sql = format!(
            "SELECT {TRACK_COLS} FROM tracks WHERE {col} = ?1
             ORDER BY disc_no, track_no, title COLLATE NOCASE"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![value], row_to_track)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn track_by_id(&self, id: Uuid) -> Option<Track> {
        let sql = format!("SELECT {TRACK_COLS} FROM tracks WHERE id = ?1");
        self.conn
            .query_row(&sql, params![id.to_string()], row_to_track)
            .optional()
            .ok()
            .flatten()
    }

    pub fn track_id_by_path(&self, path: &Path) -> Option<Uuid> {
        self.conn
            .query_row(
                "SELECT id FROM tracks WHERE path = ?1",
                params![path.to_string_lossy()],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
            .and_then(|s| Uuid::parse_str(&s).ok())
    }

    /// Resolve an ordered id list to tracks, preserving order and dropping
    /// ids that no longer exist in the library.
    pub fn tracks_by_ids(&self, ids: &[Uuid]) -> Vec<Track> {
        ids.iter().filter_map(|id| self.track_by_id(*id)).collect()
    }

    // ---- audiobooks (rebuilt wholesale each scan — few enough to not need
    //      incremental; ids are deterministic v5(path) so resume survives) ----

    pub fn audiobook_count(&self) -> u64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM audiobooks", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap_or(0) as u64
    }

    pub fn clear_audiobooks(&self) -> Result<()> {
        self.conn.execute("DELETE FROM chapters", [])?;
        self.conn.execute("DELETE FROM audiobooks", [])?;
        Ok(())
    }

    pub fn upsert_audiobook(&self, b: &Audiobook) -> Result<()> {
        let id = b.id.to_string();
        self.conn.execute(
            "INSERT INTO audiobooks
               (id,path,title,author,narrator,genre,year,duration_secs,codec,date_added)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(id) DO UPDATE SET
               path=excluded.path, title=excluded.title, author=excluded.author,
               narrator=excluded.narrator, genre=excluded.genre, year=excluded.year,
               duration_secs=excluded.duration_secs, codec=excluded.codec",
            params![
                id,
                b.path.to_string_lossy(),
                b.title,
                b.author,
                b.narrator,
                b.genre,
                b.year,
                b.duration_secs,
                b.codec,
                b.date_added.to_rfc3339(),
            ],
        )?;
        self.conn
            .execute("DELETE FROM chapters WHERE book_id = ?1", params![id])?;
        for c in &b.chapters {
            self.conn.execute(
                "INSERT INTO chapters (book_id,idx,title,start_secs,end_secs)
                 VALUES (?1,?2,?3,?4,?5)",
                params![id, c.index, c.title, c.start_secs, c.end_secs],
            )?;
        }
        Ok(())
    }

    pub fn audiobooks(&self) -> Vec<Audiobook> {
        let mut stmt = match self.conn.prepare(
            "SELECT id,path,title,author,narrator,genre,year,duration_secs,codec,date_added
             FROM audiobooks ORDER BY title COLLATE NOCASE",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows: Vec<Audiobook> = stmt
            .query_map([], |r| {
                let id = Uuid::parse_str(&r.get::<_, String>(0)?)
                    .unwrap_or_else(|_| Uuid::nil());
                Ok(Audiobook {
                    id,
                    path: PathBuf::from(r.get::<_, String>(1)?),
                    title: r.get(2)?,
                    author: r.get(3)?,
                    narrator: r.get(4)?,
                    genre: r.get(5)?,
                    year: r.get(6)?,
                    duration_secs: r.get(7)?,
                    chapters: Vec::new(),
                    codec: r.get(8)?,
                    date_added: r
                        .get::<_, String>(9)
                        .ok()
                        .and_then(|s| {
                            chrono::DateTime::parse_from_rfc3339(&s).ok()
                        })
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                })
            })
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();

        rows.into_iter()
            .map(|mut b| {
                b.chapters = self.chapters_of(b.id);
                b
            })
            .collect()
    }

    /// Cheap single-row title lookup (no chapter join) — for the sidebar
    /// "resume last book" label rendered every frame.
    pub fn audiobook_title(&self, id: Uuid) -> Option<String> {
        self.conn
            .query_row(
                "SELECT title FROM audiobooks WHERE id = ?1",
                params![id.to_string()],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    fn chapters_of(&self, book_id: Uuid) -> Vec<Chapter> {
        let mut stmt = match self.conn.prepare(
            "SELECT idx,title,start_secs,end_secs FROM chapters
             WHERE book_id = ?1 ORDER BY idx",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(params![book_id.to_string()], |r| {
            Ok(Chapter {
                index: r.get(0)?,
                title: r.get(1)?,
                start_secs: r.get(2)?,
                end_secs: r.get(3)?,
            })
        })
        .map(|it| it.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }
}

/// Turn raw user input into a safe FTS5 prefix query: each token quoted and
/// suffixed with `*`, combined with implicit AND. Quotes neutralize FTS syntax.
fn fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn row_to_track(r: &rusqlite::Row) -> rusqlite::Result<Track> {
    let parse_dt = |s: Option<String>| -> Option<DateTime<Utc>> {
        s.and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&Utc))
    };
    Ok(Track {
        id: Uuid::parse_str(&r.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
        path: PathBuf::from(r.get::<_, String>(1)?),
        mtime: r.get(2)?,
        file_size_bytes: r.get::<_, i64>(3)? as u64,
        title: r.get(4)?,
        artist: r.get(5)?,
        album: r.get(6)?,
        album_artist: r.get(7)?,
        genre: r.get(8)?,
        year: r.get(9)?,
        track_number: r.get(10)?,
        disc_number: r.get(11)?,
        duration_secs: r.get(12)?,
        bitrate_kbps: r.get(13)?,
        sample_rate: r.get(14)?,
        bit_depth: r.get::<_, Option<u32>>(15)?.map(|b| b as u8),
        codec: r.get(16)?,
        date_added: parse_dt(r.get(17)?).unwrap_or_else(Utc::now),
        play_count: r.get(18)?,
        rating: r.get::<_, Option<u32>>(19)?.map(|x| x as u8),
        last_played: parse_dt(r.get(20)?),
    })
}
