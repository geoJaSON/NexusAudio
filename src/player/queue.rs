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
    pub fn pos(&self) -> Option<usize> {
        self.pos
    }

    // ---- cursor / navigation ----

    pub fn current(&self) -> Option<&Track> {
        self.pos.and_then(|p| self.order.get(p)).map(|&i| &self.items[i])
    }

    /// What [`next`] WOULD return, without mutating the cursor. Used by the
    /// engine's gapless preload to know which file to open ahead of EOF.
    pub fn peek_next(&self) -> Option<&Track> {
        let n = self.order.len();
        if n == 0 {
            return None;
        }
        let next_pos = match (self.pos, &self.repeat) {
            (Some(p), RepeatMode::One) => Some(p),
            (Some(p), _) if p + 1 < n => Some(p + 1),
            (Some(_), RepeatMode::All) => Some(0),
            (Some(_), RepeatMode::None) => None,
            (None, _) => Some(0),
        };
        next_pos.and_then(|p| self.order.get(p)).map(|&i| &self.items[i])
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

    /// Jump straight to the Nth entry in the play order.
    pub fn jump_upcoming(&mut self, idx: usize) -> Option<&Track> {
        if idx < self.order.len() {
            self.pos = Some(idx);
        }
        self.current()
    }

    // ---- views for the queue panel ----

    /// Upcoming tracks in play order (after the cursor).
    #[allow(dead_code)]
    pub fn upcoming(&self) -> Vec<&Track> {
        let start = self.pos.map(|p| p + 1).unwrap_or(0);
        self.order[start.min(self.order.len())..]
            .iter()
            .map(|&i| &self.items[i])
            .collect()
    }

    /// Every queued track in play order (for "create playlist from queue").
    pub fn ordered(&self) -> Vec<&Track> {
        self.order.iter().map(|&i| &self.items[i]).collect()
    }

    /// Already-played tracks, oldest→newest (before the cursor). Superseded
    /// for the UI by App's session history; kept as queue-model API.
    #[allow(dead_code)]
    pub fn history(&self) -> Vec<&Track> {
        let end = self.pos.unwrap_or(0);
        self.order[..end.min(self.order.len())]
            .iter()
            .map(|&i| &self.items[i])
            .collect()
    }

    // ---- mutation ----

    /// Position in `order` (absolute) where `id` is current-or-upcoming, if
    /// it is. Tracks before the cursor are history — re-adding those is fine.
    fn upcoming_pos_of(&self, id: uuid::Uuid) -> Option<usize> {
        let start = self.pos.unwrap_or(0).min(self.order.len());
        self.order[start..]
            .iter()
            .position(|&i| self.items[i].id == id)
            .map(|p| start + p)
    }

    /// Append to the very end of the play order. No-op if the track is
    /// already current or upcoming — repeated "add to queue" gestures must
    /// not stack duplicates.
    pub fn enqueue(&mut self, t: Track) {
        if self.upcoming_pos_of(t.id).is_some() {
            return;
        }
        let idx = self.items.len();
        self.items.push(t);
        self.order.push(idx);
        if self.pos.is_none() {
            self.pos = Some(0);
        }
    }

    /// Insert so it plays immediately after the current track. If the track
    /// is already upcoming it is MOVED into that slot instead of duplicated.
    pub fn play_next(&mut self, t: Track) {
        if let Some(abs) = self.upcoming_pos_of(t.id) {
            // Already the current track → nothing to do.
            if Some(abs) != self.pos {
                let item = self.order.remove(abs);
                let at = self.pos.map(|p| p + 1).unwrap_or(0).min(self.order.len());
                self.order.insert(at, item);
            }
            if self.pos.is_none() {
                self.pos = Some(0);
            }
            return;
        }
        let idx = self.items.len();
        self.items.push(t);
        let at = self.pos.map(|p| p + 1).unwrap_or(0);
        self.order.insert(at.min(self.order.len()), idx);
        if self.pos.is_none() {
            self.pos = Some(0);
        }
    }

    pub fn remove_upcoming(&mut self, idx: usize) {
        if idx < self.order.len() {
            self.order.remove(idx);
            // Adjust cursor if necessary:
            if let Some(p) = self.pos {
                if p >= self.order.len() {
                    if self.order.is_empty() {
                        self.pos = None;
                    } else {
                        self.pos = Some(self.order.len() - 1);
                    }
                } else if p > idx {
                    self.pos = Some(p - 1);
                } else if p == idx {
                    if self.order.is_empty() {
                        self.pos = None;
                    } else {
                        self.pos = Some(p.min(self.order.len() - 1));
                    }
                }
            }
            self.gc_items();
        }
    }

    #[allow(dead_code)]
    pub fn clear_upcoming(&mut self) {
        let start = self.pos.map(|p| p + 1).unwrap_or(0);
        self.order.truncate(start.min(self.order.len()));
        self.gc_items();
    }

    /// Drop `items` entries the play order no longer references (removal only
    /// edits `order`); without this, items — and queue.json — grow forever.
    fn gc_items(&mut self) {
        if self.items.len() == self.order.len() {
            return; // every item still referenced (order indices are unique)
        }
        let mut map = vec![usize::MAX; self.items.len()];
        let mut kept: Vec<Track> = Vec::with_capacity(self.order.len());
        for o in &mut self.order {
            if map[*o] == usize::MAX {
                map[*o] = kept.len();
                kept.push(self.items[*o].clone());
            }
            *o = map[*o];
        }
        self.items = kept;
    }

    /// Move an entry up (toward start) or down.
    pub fn move_upcoming(&mut self, idx: usize, up: bool) {
        let a = idx;
        let b = if up { a.wrapping_sub(1) } else { a + 1 };
        if a < self.order.len() && b < self.order.len() {
            self.order.swap(a, b);
            // Adjust the cursor if it was one of the swapped items!
            if let Some(p) = self.pos {
                if p == a {
                    self.pos = Some(b);
                } else if p == b {
                    self.pos = Some(a);
                }
            }
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
        if self.items.is_empty() {
            self.pos = None;
            return;
        }
        if self.shuffle {
            // Playback always starts at the TOP of the shuffled order — the
            // first track is whatever the shuffle picked, never a cursor
            // stranded mid-list.
            shuffle(&mut self.order);
            self.pos = Some(0);
        } else {
            self.pos = match start_item {
                Some(item) => self.order.iter().position(|&i| i == item).or(Some(0)),
                None => Some(0),
            };
        }
    }

    fn rebuild_order_keeping(&mut self, keep_item: Option<usize>) {
        self.order = (0..self.items.len()).collect();
        if self.order.is_empty() {
            self.pos = None;
            return;
        }
        if self.shuffle {
            shuffle(&mut self.order);
            // The currently-playing track leads the new shuffled order so
            // everything below the cursor is genuinely "up next".
            if let Some(item) = keep_item {
                if let Some(p) = self.order.iter().position(|&i| i == item) {
                    self.order.swap(0, p);
                }
            }
            self.pos = Some(0);
        } else {
            self.pos = keep_item
                .and_then(|item| self.order.iter().position(|&i| i == item))
                .or(self.pos)
                .map(|p| p.min(self.order.len() - 1));
        }
    }
}

impl Queue {
    /// Test-only visibility into the backing item store (for GC assertions).
    #[cfg(test)]
    fn items_len(&self) -> usize {
        self.items.len()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn track(n: u32) -> Track {
        Track {
            id: uuid::Uuid::new_v4(),
            path: std::path::PathBuf::from(format!("{n}.mp3")),
            mtime: 0,
            file_size_bytes: 0,
            title: format!("T{n}"),
            artist: String::new(),
            album: String::new(),
            album_artist: String::new(),
            genre: String::new(),
            year: None,
            track_number: Some(n),
            disc_number: None,
            duration_secs: 0.0,
            bitrate_kbps: None,
            sample_rate: None,
            bit_depth: None,
            codec: String::new(),
            date_added: chrono::Utc::now(),
            play_count: 0,
            rating: None,
            last_played: None,
        }
    }

    fn tracks(n: u32) -> Vec<Track> {
        (0..n).map(track).collect()
    }

    #[test]
    fn shuffled_set_starts_at_top_of_order() {
        let mut q = Queue::default();
        q.shuffle = true;
        q.set(tracks(20), 0);
        assert_eq!(q.pos(), Some(0));
        assert!(q.current().is_some());
        assert_eq!(q.upcoming().len(), 19);
    }

    #[test]
    fn toggle_shuffle_moves_current_to_front() {
        let mut q = Queue::default();
        q.set(tracks(20), 7);
        let playing = q.current().unwrap().id;
        q.toggle_shuffle();
        assert!(q.shuffle);
        assert_eq!(q.pos(), Some(0));
        assert_eq!(q.current().unwrap().id, playing);
    }

    #[test]
    fn toggle_shuffle_off_restores_linear_position() {
        let mut q = Queue::default();
        q.set(tracks(10), 4);
        let playing = q.current().unwrap().id;
        q.toggle_shuffle();
        q.toggle_shuffle();
        assert!(!q.shuffle);
        assert_eq!(q.current().unwrap().id, playing);
        assert_eq!(q.pos(), Some(4));
    }

    #[test]
    fn enqueue_skips_track_already_upcoming() {
        let mut q = Queue::default();
        q.set(tracks(5), 0);
        let dup = q.ordered()[3].clone();
        q.enqueue(dup);
        assert_eq!(q.len(), 5);
    }

    #[test]
    fn enqueue_allows_replay_of_already_played() {
        let mut q = Queue::default();
        q.set(tracks(5), 4);
        let played = q.ordered()[0].clone();
        q.enqueue(played);
        assert_eq!(q.len(), 6);
    }

    #[test]
    fn play_next_moves_existing_upcoming_entry() {
        let mut q = Queue::default();
        q.set(tracks(6), 0);
        let later = q.ordered()[4].clone();
        q.play_next(later.clone());
        assert_eq!(q.len(), 6); // moved, not duplicated
        assert_eq!(q.upcoming()[0].id, later.id);
        assert_eq!(q.items_len(), 6);
    }

    #[test]
    fn play_next_on_current_track_is_a_noop() {
        let mut q = Queue::default();
        q.set(tracks(3), 1);
        let cur = q.current().unwrap().clone();
        q.play_next(cur.clone());
        assert_eq!(q.len(), 3);
        assert_eq!(q.current().unwrap().id, cur.id);
    }

    #[test]
    fn removal_garbage_collects_items() {
        let mut q = Queue::default();
        q.set(tracks(4), 0);
        q.remove_upcoming(3);
        assert_eq!(q.len(), 3);
        assert_eq!(q.items_len(), 3);
        // Remaining order indices must still resolve after the remap.
        assert_eq!(q.ordered().len(), 3);
        assert_eq!(q.current().unwrap().track_number, Some(0));
    }

    #[test]
    fn clear_upcoming_garbage_collects_items() {
        let mut q = Queue::default();
        q.set(tracks(10), 2);
        q.clear_upcoming();
        assert_eq!(q.len(), 3); // history + current stay
        assert_eq!(q.items_len(), 3);
        assert_eq!(q.current().unwrap().track_number, Some(2));
    }

    #[test]
    fn restore_drops_out_of_range_indices() {
        let mut q = Queue::default();
        q.set(tracks(3), 0);
        let mut snap = q.snapshot();
        snap.order.push(99);
        let q2 = Queue::restore(snap);
        assert_eq!(q2.len(), 3);
    }
}
