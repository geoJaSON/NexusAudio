//! Minimal MP4 box reader + chapter extraction for M4B.
//!
//! Neither symphonia nor lofty surfaces M4B chapter timestamps, so this
//! hand-rolls just enough MP4. Two sources are tried, in order:
//!   1. Nero `moov > udta > chpl` (simple chapter list).
//!   2. QuickTime/iTunes chapter *text track* (Audible-style): the audio
//!      track's `tref > chap` points at a text track whose samples are the
//!      chapter titles, timed via that track's sample table.
//!
//! `moov` is read into memory once (small relative to the media) and walked
//! as slices; sample *text* is read from the file at absolute chunk offsets.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::library::models::Chapter;

/// Authoritative, codec-independent duration from `moov > mvhd`. Needed
/// because symphonia mis-derives HE-AAC M4B duration (counts base-rate frames
/// but divides by the SBR-doubled rate → half the real length).
pub fn mp4_duration_secs(path: &Path) -> Option<f64> {
    let mut f = File::open(path).ok()?;
    let file_end = f.seek(SeekFrom::End(0)).ok()?;
    let (moov_off, moov_len) = find_top(&mut f, file_end, b"moov")?;
    let mut moov = vec![0u8; moov_len.min(64 << 20) as usize];
    f.seek(SeekFrom::Start(moov_off)).ok()?;
    f.read_exact(&mut moov).ok()?;
    let mvhd = find_child(&moov, &["mvhd"])?;
    if mvhd.is_empty() {
        return None;
    }
    let (ts, dur) = if mvhd[0] == 1 {
        // v1: creation(8) mod(8) timescale(4) duration(8)
        if mvhd.len() < 4 + 16 + 4 + 8 {
            return None;
        }
        let ts = u32::from_be_bytes(mvhd[20..24].try_into().ok()?) as f64;
        let d = u64::from_be_bytes(mvhd[24..32].try_into().ok()?) as f64;
        (ts, d)
    } else {
        // v0: creation(4) mod(4) timescale(4) duration(4)
        if mvhd.len() < 4 + 8 + 4 + 4 {
            return None;
        }
        let ts = u32::from_be_bytes(mvhd[12..16].try_into().ok()?) as f64;
        let d = u32::from_be_bytes(mvhd[16..20].try_into().ok()?) as f64;
        (ts, d)
    };
    if ts > 0.0 {
        Some(dur / ts)
    } else {
        None
    }
}

pub fn read_m4b_chapters(path: &Path) -> Vec<Chapter> {
    let Ok(mut f) = File::open(path) else {
        return Vec::new();
    };
    let file_end = f.seek(SeekFrom::End(0)).unwrap_or(0);

    // Locate + slurp `moov`.
    let Some((moov_off, moov_len)) = find_top(&mut f, file_end, b"moov") else {
        return Vec::new();
    };
    let mut moov = vec![0u8; moov_len.min(64 << 20) as usize];
    if f.seek(SeekFrom::Start(moov_off)).is_err() || f.read_exact(&mut moov).is_err() {
        return Vec::new();
    }

    // 1) Nero chpl.
    if let Some(chpl) = find_child(&moov, &["udta", "chpl"]) {
        let ch = parse_chpl(chpl);
        if !ch.is_empty() {
            return ch;
        }
    }
    // 2) QuickTime text track.
    quicktime_chapters(&mut f, &moov)
}

// ───────────────────────── box walking ─────────────────────────

/// (payload_offset_in_file, payload_len) of a top-level box, via seeks.
fn find_top(f: &mut File, end: u64, want: &[u8; 4]) -> Option<(u64, u64)> {
    let mut pos = 0u64;
    while pos + 8 <= end {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let mut h = [0u8; 8];
        if f.read_exact(&mut h).is_err() {
            return None;
        }
        let s32 = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
        let (size, payload) = match s32 {
            1 => {
                let mut e = [0u8; 8];
                f.read_exact(&mut e).ok()?;
                (u64::from_be_bytes(e), pos + 16)
            }
            0 => (end - pos, pos + 8),
            n => (n, pos + 8),
        };
        if size < 8 || pos + size > end {
            return None;
        }
        if &h[4..8] == want {
            return Some((payload, pos + size - payload));
        }
        pos += size;
    }
    None
}

/// Iterate immediate child boxes of an in-memory container: `(type, payload)`.
fn children(buf: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 8 <= buf.len() {
        let s32 =
            u32::from_be_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]) as usize;
        let kind = &buf[p + 4..p + 8];
        let (size, hdr) = match s32 {
            1 => {
                if p + 16 > buf.len() {
                    break;
                }
                let e = u64::from_be_bytes(buf[p + 8..p + 16].try_into().unwrap());
                (e as usize, 16)
            }
            0 => (buf.len() - p, 8),
            n => (n, 8),
        };
        if size < hdr || p + size > buf.len() {
            break;
        }
        out.push((kind, &buf[p + hdr..p + size]));
        p += size;
    }
    out
}

/// Descend a path of container types from an in-memory container.
fn find_child<'a>(buf: &'a [u8], path: &[&str]) -> Option<&'a [u8]> {
    let (head, rest) = path.split_first()?;
    for (k, payload) in children(buf) {
        if k == head.as_bytes() {
            return if rest.is_empty() {
                Some(payload)
            } else {
                find_child(payload, rest)
            };
        }
    }
    None
}

// ───────────────────────── Nero chpl ─────────────────────────

fn parse_chpl(b: &[u8]) -> Vec<Chapter> {
    if b.len() < 5 {
        return Vec::new();
    }
    let version = b[0];
    let mut p = 4usize;
    if version == 1 {
        p += 4;
    }
    if p >= b.len() {
        return Vec::new();
    }
    let count = b[p] as usize;
    p += 1;
    let mut out = Vec::with_capacity(count);
    for idx in 0..count {
        if p + 9 > b.len() {
            break;
        }
        let start = u64::from_be_bytes(b[p..p + 8].try_into().unwrap());
        p += 8;
        let tlen = b[p] as usize;
        p += 1;
        if p + tlen > b.len() {
            break;
        }
        out.push(mk_chapter(
            idx,
            String::from_utf8_lossy(&b[p..p + tlen]).into_owned(),
            start as f64 / 10_000_000.0,
        ));
        p += tlen;
    }
    if out.windows(2).any(|w| w[1].start_secs < w[0].start_secs) {
        return Vec::new();
    }
    out
}

// ─────────────────── QuickTime chapter text track ───────────────────

fn quicktime_chapters(f: &mut File, moov: &[u8]) -> Vec<Chapter> {
    // Index every trak by its track id; note the audio trak's chap ref.
    let mut traks: HashMap<u32, &[u8]> = HashMap::new();
    let mut chap_id: Option<u32> = None;
    for (k, payload) in children(moov) {
        if k != b"trak" {
            continue;
        }
        let Some(tkhd) = find_child(payload, &["tkhd"]) else { continue };
        let tid = tkhd_track_id(tkhd);
        traks.insert(tid, payload);

        let handler = find_child(payload, &["mdia", "hdlr"])
            .map(handler_type)
            .unwrap_or_default();
        if handler == *b"soun" {
            if let Some(chap) = find_child(payload, &["tref", "chap"]) {
                // chap payload = sequence of u32 track ids; take the first.
                if chap.len() >= 4 {
                    chap_id = Some(u32::from_be_bytes(chap[0..4].try_into().unwrap()));
                }
            }
        }
    }

    let Some(text_trak) = chap_id.and_then(|id| traks.get(&id)).copied() else {
        return Vec::new();
    };
    let timescale = find_child(text_trak, &["mdia", "mdhd"])
        .map(mdhd_timescale)
        .filter(|&t| t > 0)
        .unwrap_or(1000);
    let Some(stbl) = find_child(text_trak, &["mdia", "minf", "stbl"]) else {
        return Vec::new();
    };

    let starts = sample_start_times(find_child(stbl, &["stts"]), timescale);
    let sizes = sample_sizes(find_child(stbl, &["stsz"]));
    let offsets = sample_file_offsets(
        find_child(stbl, &["stsc"]),
        find_child(stbl, &["stco"]),
        find_child(stbl, &["co64"]),
        &sizes,
    );

    let n = starts.len().min(sizes.len()).min(offsets.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let mut raw = vec![0u8; sizes[i].min(4096) as usize];
        if f.seek(SeekFrom::Start(offsets[i])).is_err() || f.read_exact(&mut raw).is_err()
        {
            break;
        }
        // Text sample: u16 length + UTF-8 text (trailing modifier atoms ignored).
        let title = if raw.len() >= 2 {
            let tl = u16::from_be_bytes([raw[0], raw[1]]) as usize;
            let tl = tl.min(raw.len().saturating_sub(2));
            String::from_utf8_lossy(&raw[2..2 + tl]).trim().to_string()
        } else {
            String::new()
        };
        out.push(mk_chapter(i, title, starts[i]));
    }
    out
}

fn tkhd_track_id(b: &[u8]) -> u32 {
    if b.is_empty() {
        return 0;
    }
    let off = if b[0] == 1 { 4 + 16 } else { 4 + 8 }; // after creation/mod
    if off + 4 <= b.len() {
        u32::from_be_bytes(b[off..off + 4].try_into().unwrap())
    } else {
        0
    }
}

fn mdhd_timescale(b: &[u8]) -> u32 {
    if b.is_empty() {
        return 0;
    }
    let off = if b[0] == 1 { 4 + 16 } else { 4 + 8 };
    if off + 4 <= b.len() {
        u32::from_be_bytes(b[off..off + 4].try_into().unwrap())
    } else {
        0
    }
}

fn handler_type(b: &[u8]) -> [u8; 4] {
    if b.len() >= 12 {
        [b[8], b[9], b[10], b[11]]
    } else {
        [0; 4]
    }
}

/// stts → cumulative per-sample start times in seconds.
fn sample_start_times(stts: Option<&[u8]>, timescale: u32) -> Vec<f64> {
    let Some(b) = stts else { return Vec::new() };
    if b.len() < 8 {
        return Vec::new();
    }
    let entries = u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize;
    let mut starts = Vec::new();
    let mut t: u64 = 0;
    let mut p = 8;
    for _ in 0..entries {
        if p + 8 > b.len() {
            break;
        }
        let count = u32::from_be_bytes(b[p..p + 4].try_into().unwrap());
        let delta = u32::from_be_bytes(b[p + 4..p + 8].try_into().unwrap()) as u64;
        p += 8;
        for _ in 0..count {
            starts.push(t as f64 / timescale as f64);
            t += delta;
        }
    }
    starts
}

fn sample_sizes(stsz: Option<&[u8]>) -> Vec<u64> {
    let Some(b) = stsz else { return Vec::new() };
    if b.len() < 12 {
        return Vec::new();
    }
    let uniform = u32::from_be_bytes(b[4..8].try_into().unwrap());
    let count = u32::from_be_bytes(b[8..12].try_into().unwrap()) as usize;
    if uniform != 0 {
        return vec![uniform as u64; count];
    }
    let mut v = Vec::with_capacity(count);
    let mut p = 12;
    for _ in 0..count {
        if p + 4 > b.len() {
            break;
        }
        v.push(u32::from_be_bytes(b[p..p + 4].try_into().unwrap()) as u64);
        p += 4;
    }
    v
}

/// Combine stsc + stco/co64 + sizes into an absolute file offset per sample.
fn sample_file_offsets(
    stsc: Option<&[u8]>,
    stco: Option<&[u8]>,
    co64: Option<&[u8]>,
    sizes: &[u64],
) -> Vec<u64> {
    // Chunk base offsets.
    let chunks: Vec<u64> = if let Some(b) = stco {
        let n = if b.len() >= 8 {
            u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize
        } else {
            0
        };
        (0..n)
            .map(|i| 8 + i * 4)
            .filter(|&p| p + 4 <= b.len())
            .map(|p| u32::from_be_bytes(b[p..p + 4].try_into().unwrap()) as u64)
            .collect()
    } else if let Some(b) = co64 {
        let n = if b.len() >= 8 {
            u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize
        } else {
            0
        };
        (0..n)
            .map(|i| 8 + i * 8)
            .filter(|&p| p + 8 <= b.len())
            .map(|p| u64::from_be_bytes(b[p..p + 8].try_into().unwrap()))
            .collect()
    } else {
        Vec::new()
    };
    if chunks.is_empty() || sizes.is_empty() {
        return Vec::new();
    }

    // stsc: (first_chunk, samples_per_chunk) run-length over chunks.
    let Some(b) = stsc else { return Vec::new() };
    if b.len() < 8 {
        return Vec::new();
    }
    let entries = u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize;
    let mut runs: Vec<(u32, u32)> = Vec::new(); // (first_chunk, samples_per_chunk)
    let mut p = 8;
    for _ in 0..entries {
        if p + 12 > b.len() {
            break;
        }
        let first = u32::from_be_bytes(b[p..p + 4].try_into().unwrap());
        let spc = u32::from_be_bytes(b[p + 4..p + 8].try_into().unwrap());
        runs.push((first, spc));
        p += 12;
    }
    if runs.is_empty() {
        return Vec::new();
    }

    // Walk chunks, emitting an absolute offset per sample.
    let mut offsets = Vec::with_capacity(sizes.len());
    let mut sample = 0usize;
    for (ci, &chunk_off) in chunks.iter().enumerate() {
        let chunk_no = ci as u32 + 1;
        // samples-per-chunk = the last run whose first_chunk <= chunk_no.
        let spc = runs
            .iter()
            .rev()
            .find(|(fc, _)| *fc <= chunk_no)
            .map(|(_, s)| *s)
            .unwrap_or(runs[0].1) as usize;
        let mut off = chunk_off;
        for _ in 0..spc {
            if sample >= sizes.len() {
                return offsets;
            }
            offsets.push(off);
            off += sizes[sample];
            sample += 1;
        }
    }
    offsets
}

fn mk_chapter(idx: usize, title: String, start_secs: f64) -> Chapter {
    Chapter {
        index: idx as u32,
        title: if title.trim().is_empty() {
            format!("Chapter {}", idx + 1)
        } else {
            title
        },
        start_secs,
        end_secs: 0.0, // caller fills from book duration
    }
}

/// Headless spike: `nexus-audio --chapter-spike <file.m4b>`.
pub fn spike(path: &Path) {
    println!("chapter-spike: {}", path.display());
    println!(
        "  mvhd duration = {:?} s (authoritative, codec-independent)",
        mp4_duration_secs(path).map(|s| s as u64)
    );
    let ch = read_m4b_chapters(path);
    if ch.is_empty() {
        println!("  no parseable chapters (flat timeline; resume still works)");
        return;
    }
    println!("  {} chapters:", ch.len());
    for c in ch.iter().take(50) {
        let s = c.start_secs as u64;
        println!(
            "  [{:>3}] {:>2}:{:02}:{:02}  {}",
            c.index,
            s / 3600,
            (s % 3600) / 60,
            s % 60,
            c.title
        );
    }
    if ch.len() > 50 {
        println!("  … {} more", ch.len() - 50);
    }
}
