# NEXUS//AUDIO — Implementation Plan
> Retro terminal music player & audiobook manager · Rust + egui
> Built as a cross-platform (Linux-first) replacement for MusicBee.

---

## 1. Project Overview

**NEXUS//AUDIO** is a desktop media player with a retro 80s CRT terminal aesthetic (green-on-black phosphor display, scanlines, monospace typography). It manages a large local music library (50k+ tracks), audiobooks, and playlists, with a live playback queue and reliable audiobook auto-resume.

**Design reference:** dark terminal palette (`#020f04` background, `#00ff41` phosphor green, `#ffb700` amber for audiobook/resume states), Share Tech Mono + VT323 fonts, scanline overlay effect. See `retro_music_player_mockup.html`.

**Design decisions (locked):**
- **Library at scale** — target 50k+ tracks. SQLite-backed, incremental scanning, virtualized list rendering. Not negotiable; JSON would not survive this size.
- **Playback accuracy where it counts** — interactive music seeking is *not* a priority. **Audiobook auto-resume is the single critical correctness requirement.** ENGINE LOCKED: the Phase 1 spike passed — symphonia accurate-seek landed within **29 ms** seeking 4h12m into a real 5-hour M4B. symphonia + cpal is the engine.
- **Engine work items surfaced by the spike:** (1) a proper anti-aliased resampler (`rubato` sinc) is required — the spike's linear stand-in aliased on bright 44.1k→48k music; (2) HE-AAC decodes at base-layer rate (SBR not applied) — fine for spoken-word audiobooks, a quality caveat only for HE-AAC music.
- **No playback speed control** — explicitly out of scope. No time-stretch subsystem.
- **Tag editing** — deferred. File is the canonical source of tag truth; the DB is a cache. This keeps write-back *possible* later without an architecture rewrite. A basic tag editor is a later optional phase.

---

## 2. Technology Stack

| Layer | Crate | Purpose |
|---|---|---|
| GUI | `egui` + `eframe` | Immediate-mode UI, custom CRT rendering |
| Audio decode | `symphonia` | Accurate sample-position seek, gapless, broad codec + M4B/AAC support |
| Audio output | `cpal` | Cross-platform output stream (WASAPI / ALSA / CoreAudio) |
| *(Spike fallback)* | `rodio` | Only if the Phase 1 M4B-resume spike proves rodio sufficient — see §9 |
| Tag read/write | `lofty` | ID3v2, FLAC, Ogg, AAC, M4A/M4B metadata. Write kept available for future tag editor. |
| Library store | `rusqlite` (bundled) | Indexed track/album/artist/audiobook store, FTS5 search |
| Small-state store | `serde` + `serde_json` | Playlists, resume, queue, settings (small, human-inspectable) |
| Directory scanning | `walkdir` | Recursive folder traversal |
| IDs | `uuid` (v4, serde) | Stable entity IDs |
| File dialogs | `rfd` | Native add-folder dialog |
| Time | `chrono` | Timestamps, duration formatting |
| Config paths | `dirs` | OS-appropriate config/data directories |
| Error handling | `anyhow` | Ergonomic error propagation |

> No `tokio`. eframe is not async; background scanning uses a plain `std::thread` + `mpsc` channel.

### `Cargo.toml` key dependencies
```toml
[dependencies]
eframe   = "0.29"          # verify latest at scaffold time; egui API churns
egui     = "0.29"
# (egui_extras dropped — cover art is out of scope)
symphonia = { version = "0.5", features = ["mp3", "flac", "vorbis", "aac", "isomp4", "alac"] }
cpal     = "0.15"
lofty    = "0.21"
rusqlite = { version = "0.31", features = ["bundled"] }
walkdir  = "2"
uuid     = { version = "1", features = ["v4", "serde"] }
rfd      = "0.14"
serde    = { version = "1", features = ["derive"] }
serde_json = "1"
chrono   = { version = "0.4", features = ["serde"] }
dirs     = "5"
anyhow   = "1"
```

---

## 3. Project Structure

```
nexus-audio/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── assets/
│   └── fonts/
│       ├── ShareTechMono-Regular.ttf
│       └── VT323-Regular.ttf
├── src/
│   ├── main.rs                  # eframe entry point, app bootstrap
│   ├── app.rs                   # Root App struct, top-level state, egui update()
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── titlebar.rs          # Logo, EQ bars, clock, sys info
│   │   ├── sidebar.rs           # Nav items, playlist list
│   │   ├── player_bar.rs        # Transport, scrub bar, volume, now-playing
│   │   ├── status_bar.rs        # Codec, bitrate, sample rate, mem usage
│   │   ├── views/
│   │   │   ├── mod.rs
│   │   │   ├── tracks.rs        # All Tracks (virtualized list)
│   │   │   ├── albums.rs        # Album grid/list view
│   │   │   ├── artists.rs       # Artist list + drill-down
│   │   │   ├── folders.rs       # Watched directories, scan controls
│   │   │   ├── playlists.rs     # Playlist editor
│   │   │   ├── queue.rs         # Live playback queue panel
│   │   │   ├── audiobooks.rs    # Audiobook library, resume state
│   │   │   └── settings.rs      # App settings panel
│   │   └── theme.rs             # CRT palette, font sizes, egui visuals
│   ├── library/
│   │   ├── mod.rs
│   │   ├── scanner.rs           # Incremental walkdir + lofty scanner (bg thread)
│   │   ├── models.rs            # Track, Album, Artist structs
│   │   └── db.rs                # SQLite (rusqlite) — schema, queries, FTS
│   ├── store/
│   │   ├── mod.rs
│   │   └── json_store.rs        # Atomic JSON load/save (playlists, resume, queue, settings)
│   ├── player/
│   │   ├── mod.rs
│   │   ├── engine.rs            # symphonia decode → cpal output, play/pause/seek/volume
│   │   └── queue.rs             # Queue state: current, upcoming, history
│   ├── audiobooks/
│   │   ├── mod.rs
│   │   ├── models.rs            # Audiobook, Chapter structs
│   │   └── resume.rs            # Resume position persistence (CRITICAL)
│   └── playlists/
│       ├── mod.rs
│       └── models.rs            # Playlist struct, CRUD ops
└── (runtime data — see §6)
```

---

## 4. Core Data Models

### Track
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: Uuid,
    pub path: PathBuf,
    pub mtime: i64,              // file mtime (unix secs) — incremental-scan key
    pub file_size_bytes: u64,    // also part of the change-detection key
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
    pub codec: String,           // "FLAC", "MP3", "AAC", etc.
    pub date_added: DateTime<Utc>,
    pub play_count: u32,
    pub rating: Option<u8>,      // 1–5
    pub last_played: Option<DateTime<Utc>>,
}
```
> **File is canonical for tag fields.** The DB row is a cache keyed by `(path, mtime, file_size_bytes)`. A future tag editor writes to the file via `lofty`, then refreshes the row.

### Audiobook
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Audiobook {
    pub id: Uuid,
    pub path: PathBuf,           // file or directory (multi-part)
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
```

### ResumeState  *(critical path)*
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeState {
    pub audiobook_id: Uuid,
    pub position_secs: f64,
    pub chapter_index: u32,
    pub last_updated: DateTime<Utc>,
}
```
> Stored in `resume.json` as `Map<Uuid, ResumeState>`. Written with atomic temp-file + rename so a crash mid-write never corrupts resume data. Saved every 15s during audiobook playback and on pause/stop/close.

### Playlist
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: Uuid,
    pub name: String,
    pub track_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### PlaybackQueue
```rust
#[derive(Debug, Clone)]
pub struct PlaybackQueue {
    pub current: Option<QueueEntry>,
    pub upcoming: VecDeque<QueueEntry>,
    pub history: Vec<QueueEntry>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

#[derive(Debug, Clone)]
pub struct QueueEntry { pub track: Track, pub source: QueueSource }

#[derive(Debug, Clone)]
pub enum RepeatMode { None, One, All }

#[derive(Debug, Clone)]
pub enum QueueSource { Library, Playlist(Uuid), Album(String), Artist(String), Manual }
```

---

## 5. Feature Breakdown

### 5.1 Music Library (50k+ scale)

**Incremental Folder Scanner** (`library/scanner.rs`)
- Recursively walk watched directories with `walkdir`.
- Filter by extension: `.mp3 .flac .ogg .wav .aiff .m4a .aac .opus .m4b`.
- For each file, compare `(mtime, size)` against the DB. **Only unchanged files are skipped — only new/modified files are re-tagged with `lofty`.** Removed files (in DB, missing on disk) are pruned.
- Runs on a background `std::thread`; progress streamed to the UI via `mpsc` (files scanned / total / current path).
- A full rescan of an unchanged 50k library must be a quick stat-only pass, not a full re-tag.

**Library Views** (all virtualized via `egui::ScrollArea::show_rows`)
- **All Tracks** — sortable list (Title / Artist / Album / Genre / Duration / Date Added). Sort + page via SQL `ORDER BY ... LIMIT/OFFSET`, never an in-memory sort of 50k rows.
- **Albums** — grouped by album, track count, year. (No cover art — out of scope, text/ASCII only.)
- **Artists** — grouped by artist, drill down to albums then tracks.
- **Folders** — watched directory manager (add, remove, scan now, file count + last scan time, error state for missing paths).

**Search** — backed by a SQLite **FTS5** virtual table over title/artist/album/genre (case-insensitive). Debounced input; query the DB, do not filter a Vec.

### 5.2 Playback Queue

**Queue Panel** (sidebar nav item / slide-in from player bar)
- **Now Playing** → **Up Next** (ordered) → **Recently Played** (last 20).
- Each entry: queue index, title, artist, duration, source tag (e.g. `[PLAYLIST: SYNTHWAVE MIX]`).
- **Add to Queue** / **Play Next** / **Remove** (× on upcoming) / **Clear Queue** (upcoming only).
- **Queue from Album/Artist/Playlist** — replace or append.
- Drag-and-drop reorder of upcoming entries (custom egui — no native data grid).
- Shuffle re-randomizes upcoming in place. Repeat: None / One / All (loops queue when exhausted).
- **Persistence** — `queue.json` saved on exit, restored on launch.

### 5.3 Playback Engine

Default path: **`symphonia` decode → ring buffer → `cpal` output stream**, driven by a dedicated playback thread. Chosen because the one hard requirement — open a long M4B/AAC audiobook and start playback at a saved offset, correctly — is precisely where simpler libraries are weakest. The Phase 1 spike (§9) confirms this and may substitute rodio if it passes the same test with less code.

**Controls**
- Play / Pause / Stop.
- Previous (restart track if >3s played, else go to history) / Next (advance queue).
- **Music seeking is best-effort, low priority.** Scrub bar + `←/→` provided but coarse seek is acceptable for music.
- **Audiobook seek/resume must be correct** — see 5.5.
- Volume — scroll wheel or `↑/↓`.
- Gapless album playback: pre-buffer the next track so album segues have no silence gap (enabled by the symphonia path).

**Audio Info Display** — codec, bitrate, sample rate, bit depth in status bar + player bar; elapsed / total time.

**Keyboard Shortcuts**
| Key | Action |
|---|---|
| `Space` | Play / Pause |
| `←` / `→` | Seek ±10s (best-effort for music; accurate for audiobooks) |
| `[` / `]` | Previous / Next |
| `↑` / `↓` | Volume ±5% |
| `S` | Toggle shuffle |
| `R` | Cycle repeat mode |
| `Q` | Toggle queue panel |
| `:` | Open command palette |

### 5.4 Playlist Management
- Create, rename, delete, duplicate.
- Add tracks via context menu (right-click → Add to Playlist → submenu).
- Reorder within playlist (drag-and-drop).
- Export / import M3U.

### 5.5 Audiobooks  *(auto-resume is the headline correctness feature)*

**Library**
- Separate configurable audiobook scan paths.
- Formats: `.m4b .mp3 .flac .ogg`.
- Multi-file books: a folder of files treated as one book (ordered by track number / filename).
- Parse chapters from M4B chapter atoms when present (QuickTime/Nero chapter markers; `lofty` may not surface timestamps — MP4 atom parsing may be required, budgeted as real work in Phase 6).

**Sort/Filter**
- Sort: Title / Author / Genre / Duration / Date Added / Progress.
- Filter by genre or author; search title/author/narrator.

**Playback**
- Chapter navigation: previous/next chapter, chapter list panel (click to jump).
- Sleep timer: stop after N minutes.
- *(No playback speed control — out of scope.)*

**Auto-Resume — critical**
- Resume position saved every 15s during playback and on pause/stop/close, written atomically.
- Opening an in-progress audiobook shows: `▸ RESUME AT 4:12:37  [RESUME] [RESTART]`.
- "Resume" must reliably begin playback at the saved offset on a 20+ hour M4B. This is the acceptance test for the whole project (see §9).

### 5.6 Command Palette (`:` key)
- `:play <query>` — search and play first match
- `:queue <query>` — add to queue
- `:goto <timestamp>` — seek to position (e.g. `:goto 1:23:45`) — primarily for audiobooks
- `:add-to <playlist>` — add current track to playlist
- `:scan` — trigger incremental rescan
- `:vol <0-100>` — set volume

### 5.7 Settings Panel
- Watched music folders (add / remove)
- Watched audiobook folders (add / remove)
- Auto-scan on startup (toggle)
- Resume save interval (seconds)
- EQ bars in titlebar (toggle)
- Scanline overlay intensity (slider)
- Color theme: Phosphor Green / Amber / Blue-White / Red Alert

---

## 6. Persistence Strategy

Data lives under `dirs::data_dir() / "nexus-audio"/`.

| Store | Backend | Contents | Save trigger |
|---|---|---|---|
| `library.db` | **SQLite** | Tracks, albums, artists, audiobooks, chapters; FTS5 index | Incremental, after scan |
| `playlists.json` | JSON (atomic) | Playlist records | On any CRUD change |
| `resume.json` | JSON (atomic) | `Map<audiobook_id, ResumeState>` | Every 15s during playback, on pause/close |
| `queue.json` | JSON (atomic) | Current queue state | On exit |
| `settings.json` | JSON (atomic) | App settings | On any change |

**SQLite schema sketch**
```sql
CREATE TABLE tracks (
  id TEXT PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,
  mtime INTEGER NOT NULL,
  file_size INTEGER NOT NULL,
  title TEXT, artist TEXT, album TEXT, album_artist TEXT, genre TEXT,
  year INTEGER, track_no INTEGER, disc_no INTEGER,
  duration_secs REAL, bitrate INTEGER, sample_rate INTEGER, bit_depth INTEGER,
  codec TEXT, date_added TEXT,
  play_count INTEGER DEFAULT 0, rating INTEGER, last_played TEXT
);
CREATE INDEX idx_tracks_artist       ON tracks(artist);
CREATE INDEX idx_tracks_album        ON tracks(album);
CREATE INDEX idx_tracks_album_artist ON tracks(album_artist);
CREATE INDEX idx_tracks_date_added   ON tracks(date_added);

CREATE VIRTUAL TABLE tracks_fts USING fts5(
  title, artist, album, genre, content='tracks', content_rowid='rowid'
);

CREATE TABLE audiobooks (
  id TEXT PRIMARY KEY, path TEXT UNIQUE NOT NULL,
  title TEXT, author TEXT, narrator TEXT, genre TEXT, year INTEGER,
  duration_secs REAL, codec TEXT, date_added TEXT
);
CREATE TABLE chapters (
  book_id TEXT NOT NULL REFERENCES audiobooks(id) ON DELETE CASCADE,
  idx INTEGER NOT NULL, title TEXT, start_secs REAL, end_secs REAL,
  PRIMARY KEY (book_id, idx)
);
```
> `db.rs` exposes a narrow trait-like API (query/insert/upsert/prune/search) so the rest of the app never touches SQL directly.

---

## 7. UI Theme Implementation

All colors in `ui/theme.rs` as `egui::Color32` constants:

```rust
pub const CRT_BG:     Color32 = Color32::from_rgb(2,   15,  4);
pub const CRT_GREEN:  Color32 = Color32::from_rgb(0,   255, 65);
pub const CRT_DIM:    Color32 = Color32::from_rgb(0,   179, 44);
pub const CRT_DARK:   Color32 = Color32::from_rgb(0,   61,  15);
pub const CRT_MID:    Color32 = Color32::from_rgb(0,   92,  20);
pub const AMBER:      Color32 = Color32::from_rgb(255, 183, 0);
pub const AMBER_DIM:  Color32 = Color32::from_rgb(179, 127, 0);
pub const RED_ALERT:  Color32 = Color32::from_rgb(255, 49,  49);
```

Fonts loaded at startup via `egui::FontDefinitions` from bundled TTFs (Share Tech Mono for body, VT323 for the logo). Scanline effect: a per-frame repeating horizontal stripe drawn over the whole window with `egui::Painter::rect` at low alpha, intensity from settings. Track-row grid, drag-drop reorder, and context menus are custom egui widgets — egui has no native data grid; budget this as the bulk of Phase 4.

---

## 8. Build & Platform Targets

| Platform | Notes |
|---|---|
| Linux | **Primary target.** cpal → ALSA (or PipeWire/Jack); `rfd` native dialogs. |
| Windows | cpal → WASAPI. |
| macOS | cpal → CoreAudio; eframe native. |

```bash
cargo run                 # development
cargo build --release     # release build
```

---

## 9. Phased Implementation Roadmap

### Phase 1 — Foundation + **Playback Spike (do this first)**
- [ ] Project scaffold (`Cargo.toml`, directory structure, font assets).
- [x] **Spike: audiobook resume — PASSED (2026-05-18).** Harness opened a real 5h `.m4b` (AAC), seeked to 4h12m, landed within 29 ms, decoded coherent narration; also clean on MP3 and M4A. symphonia + cpal **locked** as the engine. ~60% of project risk retired. Spike lives at `src/bin/spike.rs` as a regression reference.
- [ ] `ui/theme.rs` — CRT palette, egui Visuals, font loading.
- [ ] `library/models.rs` — core structs with serde.
- [ ] `library/db.rs` — SQLite schema, migrations, FTS, narrow query API.
- [ ] `store/json_store.rs` — atomic (temp-file + rename) JSON load/save.
- [ ] Basic `app.rs` — eframe window, fonts, CRT theme, titlebar + sidebar skeleton.

### Phase 2 — Library & Scanner — COMPLETE (2026-05-18, verified headless)
- [x] `library/scanner.rs` — incremental walkdir + lofty, `(mtime,size)` change detection, prune-missing.
- [x] Background scan thread with `mpsc` progress reporting (App polls non-blocking).
- [x] `ui/views/tracks.rs` — virtualized (`show_rows`) sortable list, FTS5-backed search, one-page cache.
- [x] `ui/views/albums.rs` and `artists.rs` — grouped lists with drill-down. (Cover art is OUT OF SCOPE per user — text/ASCII only, keep it simple.)
- [x] `ui/views/folders.rs` — watched dir management, missing-path error states, per-folder scan stats.
- Verified end-to-end via `nexus-audio --scan-smoke <dir>` (kept as a regression tool, like the spike). Caught + fixed a real FTS-join ambiguous-column bug.
- Fixed: logo font family now falls back to monospace when the VT323 TTF is absent (was panicking at render).
- Carry-over: `(mtime,size)` skip path is implemented but not yet load-tested at 50k scale.
- **Hit-area bug — ROOT-CAUSED & FIXED (2026-05-18, user-verified ✓):** Not a sizing/`show_rows` issue. `list_row` took its `Response` from the allocated rect, then drew `Label`s on top inside a child Ui; egui resolves hover to the topmost widget, so every pixel a Label covered reported the row as *not hovered* — only label-free slivers stayed live (All Tracks had the densest labels → worst). Fix: draw content first, then claim the whole row with a single `ui.interact(rect, id, Sense::click())` registered *after* the children (wins occlusion order); highlight driven by `rect_contains_pointer`, not widget hover. The two earlier rewrites failed because they only changed sizing, never the occlusion.

### Phase 3 — Playback + Audiobook Resume Core — COMPLETE (2026-05-18, engine verified headless)
- [x] `player/engine.rs` — symphonia+cpal audio thread, command/atomic-state channels, play/pause/stop/volume, accurate seek, **proper `rubato` sinc resampler** (replaces the spike's linear stand-in). Verified headless on 48k-bypass and 44.1k→48k paths: real-time position, correct codec/SR, clean output.
- [x] `player/queue.rs` — queue, cursor, shuffle permutation, None/All/One repeat.
- [x] `audiobooks/resume.rs` — atomic `audiobook_id→ResumeState` store (groundwork). 15 s cadence + resume dialog wire in at Phase 6 with the audiobook UI; storage contract fixed now.
- [x] `ui/player_bar.rs` — full-width scrub (click/drag seek), now-playing, transport, shuffle/repeat, time, volume slider.
- [x] Status bar — live codec/bitrate/sample-rate (engine info + DB bitrate fallback).
- [x] Keyboard shortcuts — Space, ←/→ (±10s), [ / ] (prev/next), ↑/↓ (vol ±5%), S (shuffle), R (repeat); suppressed while a text field is focused.
- Headless harness kept: `nexus-audio --play-smoke <file> [start]`.
- Double-click any track (All Tracks / Album / Artist drill-down) builds the queue from that list and plays; auto-advances on track end per repeat mode.

### Phase 4 — Queue UI
- [ ] `ui/views/queue.rs` — now playing / up next / history.
- [ ] Right-click context menus (Play, Play Next, Add to Queue, Add to Playlist).
- [ ] Drag-and-drop reorder.
- [ ] Queue persistence (`queue.json`).

### Phase 5 — Playlists
- [ ] `playlists/models.rs` — CRUD.
- [ ] `ui/views/playlists.rs` — editor.
- [ ] Add-to-playlist from context menu.
- [ ] M3U import/export.

### Phase 6 — Audiobooks (full)
- [ ] `audiobooks/models.rs` — Audiobook, Chapter.
- [ ] M4B chapter-atom parsing (budget real MP4 work; lofty may not surface timestamps).
- [ ] Multi-file book folder detection.
- [ ] `ui/views/audiobooks.rs` — full view, sort/filter, progress bars.
- [ ] Chapter navigation panel.
- [ ] Sleep timer.

### Phase 7 — Polish
- [ ] Command palette (`:` key).
- [ ] Scanline overlay rendering + intensity setting.
- [ ] Animated EQ bars (titlebar).
- [ ] Settings panel + color theme switcher.
- [ ] Track ratings (1–5) and play-count tracking.

### Phase 8 — Tag Editor *(deferred / optional — "what we can")*
- [ ] Single-track tag edit, write-back via `lofty`, refresh DB row.
- [ ] Batch edit across selection.
- [ ] Rename-file-from-tags (pattern-based).
- (Embedded cover art replace — dropped; cover art is out of scope.)

---

## 10. Claude Code Kickoff Prompt

```
Build NEXUS//AUDIO, a Rust desktop music + audiobook player using eframe/egui,
as a cross-platform replacement for MusicBee (Linux primary).

Reference NEXUS_AUDIO_IMPLEMENTATION_PLAN.md.

Start with Phase 1, and do the Playback Spike FIRST:
1. Create Cargo.toml with §2 dependencies (verify latest versions).
2. Create the §3 directory structure + font assets.
3. SPIKE: a minimal harness that opens a long .m4b and a VBR .mp3, seeks to an
   arbitrary offset (e.g. 4:12:37), and plays correctly. Decide the audio engine
   from the result: symphonia+cpal by default, rodio only if it passes the same
   M4B-resume test with less code. Do not build the rest of playback until this
   passes.
4. ui/theme.rs — CRT color constants (§7) + egui Visuals + Share Tech Mono / VT323.
5. library/models.rs — all structs from §4 with serde.
6. library/db.rs — SQLite schema from §6 (rusqlite, bundled), FTS5, narrow query API.
7. store/json_store.rs — atomic temp-file+rename JSON load/save.
8. app.rs — eframe App: window, fonts, CRT theme, titlebar + sidebar skeleton.

Window title: "NEXUS//AUDIO v2.4.1". All UI text uses Share Tech Mono.
```
