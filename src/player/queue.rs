//! Playback queue: ordered items, a cursor, shuffle order, and repeat modes.
//! The engine plays whatever `current()` points at; the App advances on track
//! end or transport input. The Phase 4 queue UI is built on top of this.

use serde::{Deserialize, Serialize};

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

/// Serializable snapshot for queue.json (save on exit / restore on launch).
#[derive(Default, Serialize, Deserialize)]
pub struct QueueSnapshot {
    items: Vec<Track>,
    order: Vec<usize>,
    pos: Option<usize>,
    repeat: RepeatMode,
    shuffle: bool,
}

impl Queue {
    // ---- construction / bulk ----

    pub fn set(&mut self, tracks: Vec<Track>, start: usize) {
        self.items = tracks;
        self.rebuild_order(Some(start));
    }

    #[allow(dead_code)] // full-queue clear; UI currently clears "up next" only
    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.pos = None;
    }

    #[allow(dead_code)] // convenience accessor kept alongside len()
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn len(&self) -> usize {
        self.order.len()
    }

    // ---- cursor / navigation ----

    pub fn current(&self) -> Option<&Track> {
        self.pos.and_then(|p| self.order.get(p)).map(|&i| &self.items[i])
    }

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
        };
        self.current()
    }

    pub fn prev(&mut self) -> Option<&Track> {
        self.pos = match self.pos {
            Some(p) if p > 0 => Some(p - 1),
            other => other,
        };
        self.current()
    }

    /// Jump straight to the Nth upcoming entry (0 = the next one).
    pub fn jump_upcoming(&mut self, i: usize) -> Option<&Track> {
        if let Some(p) = self.pos {
            let target = p + 1 + i;
            if target < self.order.len() {
                self.pos = Some(target);
            }
        } else if i < self.order.len() {
            self.pos = Some(i);
        }
        self.current()
    }

    // ---- views for the queue panel ----

    /// Upcoming tracks in play order (after the cursor).
    pub fn upcoming(&self) -> Vec<&Track> {
        let start = self.pos.map(|p| p + 1).unwrap_or(0);
        self.order[start.min(self.order.len())..]
            .iter()
            .map(|&i| &self.items[i])
            .collect()
    }

    /// Already-played tracks, oldest→newest (before the cursor).
    pub fn history(&self) -> Vec<&Track> {
        let end = self.pos.unwrap_or(0);
        self.order[..end.min(self.order.len())]
            .iter()
            .map(|&i| &self.items[i])
            .collect()
    }

    // ---- mutation ----

    /// Append to the very end of the play order.
    pub fn enqueue(&mut self, t: Track) {
        let idx = self.items.len();
        self.items.push(t);
        self.order.push(idx);
        if self.pos.is_none() {
            self.pos = Some(0);
        }
    }

    /// Insert so it plays immediately after the current track.
    pub fn play_next(&mut self, t: Track) {
        let idx = self.items.len();
        self.items.push(t);
        let at = self.pos.map(|p| p + 1).unwrap_or(0);
        self.order.insert(at.min(self.order.len()), idx);
        if self.pos.is_none() {
            self.pos = Some(0);
        }
    }

    pub fn remove_upcoming(&mut self, i: usize) {
        let start = self.pos.map(|p| p + 1).unwrap_or(0);
        let at = start + i;
        if at < self.order.len() {
            self.order.remove(at); // item left orphaned in `items` — harmless
        }
    }

    pub fn clear_upcoming(&mut self) {
        let start = self.pos.map(|p| p + 1).unwrap_or(0);
        self.order.truncate(start.min(self.order.len()));
    }

    /// Move an upcoming entry up (toward now-playing) or down.
    pub fn move_upcoming(&mut self, i: usize, up: bool) {
        let start = self.pos.map(|p| p + 1).unwrap_or(0);
        let a = start + i;
        let b = if up { a.wrapping_sub(1) } else { a + 1 };
        if a < self.order.len() && b >= start && b < self.order.len() {
            self.order.swap(a, b);
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
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

    // ---- persistence ----

    pub fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            items: self.items.clone(),
            order: self.order.clone(),
            pos: self.pos,
            repeat: self.repeat.clone(),
            shuffle: self.shuffle,
        }
    }

    pub fn restore(snap: QueueSnapshot) -> Self {
        // Guard against a hand-edited / stale file: drop out-of-range indices.
        let n = snap.items.len();
        let order: Vec<usize> = snap.order.into_iter().filter(|&i| i < n).collect();
        let pos = snap.pos.filter(|&p| p < order.len());
        Self { items: snap.items, order, pos, repeat: snap.repeat, shuffle: snap.shuffle }
    }

    // ---- internals ----

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
