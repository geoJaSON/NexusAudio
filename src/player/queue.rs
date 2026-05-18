//! Playback queue: ordered items, a cursor, shuffle order, and repeat modes.
//! The engine plays whatever `current()` points at; the App advances on track
//! end or transport input. The Phase 4 queue UI is built on top of this.

use crate::library::models::{RepeatMode, Track};

#[derive(Default)]
pub struct Queue {
    items: Vec<Track>,
    /// Playback order over `items` indices (identity, or a shuffle permutation).
    order: Vec<usize>,
    /// Cursor into `order`.
    pos: Option<usize>,
    pub repeat: RepeatMode,
    pub shuffle: bool,
}

impl Queue {
    /// Replace the queue with `tracks`, starting playback at `start`.
    pub fn set(&mut self, tracks: Vec<Track>, start: usize) {
        self.items = tracks;
        self.rebuild_order(Some(start));
    }

    pub fn append(&mut self, t: Track) {
        let idx = self.items.len();
        self.items.push(t);
        self.order.push(idx);
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.pos = None;
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn current(&self) -> Option<&Track> {
        self.pos.and_then(|p| self.order.get(p)).map(|&i| &self.items[i])
    }

    /// Advance per repeat mode. Returns the new current track, if any.
    pub fn next(&mut self) -> Option<&Track> {
        let n = self.order.len();
        if n == 0 {
            return None;
        }
        self.pos = match (self.pos, &self.repeat) {
            (Some(p), RepeatMode::One) => Some(p),
            (Some(p), _) if p + 1 < n => Some(p + 1),
            (Some(_), RepeatMode::All) => Some(0),
            (Some(_), RepeatMode::None) => None,
            (None, _) => Some(0),
            _ => None,
        };
        self.current()
    }

    /// Step back one (no repeat wrap). Restart-vs-prev is the caller's call.
    pub fn prev(&mut self) -> Option<&Track> {
        self.pos = match self.pos {
            Some(p) if p > 0 => Some(p - 1),
            other => other,
        };
        self.current()
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        // Keep the currently-playing item as the new cursor anchor.
        let cur_item = self.pos.and_then(|p| self.order.get(p)).copied();
        self.rebuild_order_keeping(cur_item);
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::None => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::None,
        };
    }

    fn rebuild_order(&mut self, start_item: Option<usize>) {
        self.order = (0..self.items.len()).collect();
        if self.shuffle {
            shuffle(&mut self.order);
        }
        self.pos = match start_item {
            Some(item) => self.order.iter().position(|&i| i == item).or(Some(0)),
            None if self.items.is_empty() => None,
            None => Some(0),
        };
    }

    fn rebuild_order_keeping(&mut self, keep_item: Option<usize>) {
        self.order = (0..self.items.len()).collect();
        if self.shuffle {
            shuffle(&mut self.order);
        }
        self.pos = keep_item
            .and_then(|item| self.order.iter().position(|&i| i == item))
            .or(self.pos);
    }
}

/// Tiny xorshift Fisher–Yates — no rand dependency needed for a play order.
fn shuffle(v: &mut [usize]) {
    let mut s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
        | 1;
    for i in (1..v.len()).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let j = (s as usize) % (i + 1);
        v.swap(i, j);
    }
}
