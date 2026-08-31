use crate::error::{EchoraError, Result};
use crate::models::{QueueView, Track};

/// In-memory playback queue. Rust owns this; the frontend only ever sees a
/// `QueueView` snapshot over IPC (see commands/queue.rs).
#[derive(Debug, Default)]
pub struct Queue {
    items: Vec<Track>,
    /// Index into `items` of the track that's current. `None` means
    /// nothing has ever been queued/started.
    position: Option<usize>,
}

impl Queue {
    pub fn new() -> Self {
        Queue::default()
    }

    pub fn current(&self) -> Option<&Track> {
        self.position.and_then(|p| self.items.get(p))
    }

    pub fn upcoming(&self) -> &[Track] {
        match self.position {
            Some(p) => &self.items[p + 1..],
            None => &[],
        }
    }

    /// Every track in the queue's lifetime, in order — current, past, and
    /// upcoming. Unlike `upcoming()` (only what's left to play), this is what
    /// "save the whole session as a Scene" needs to capture.
    pub fn all_tracks(&self) -> &[Track] {
        &self.items
    }

    /// The current track's ordinal position in this queue's lifetime —
    /// used as the `position` argument to `Db::record_play`, so replaying
    /// the same slot after going back and forward just overwrites that
    /// slot's history row instead of colliding.
    pub fn position(&self) -> Option<usize> {
        self.position
    }

    /// Appends new candidates to the tail. If nothing is playing yet, the
    /// first appended track becomes current.
    pub fn add_candidates(&mut self, tracks: impl IntoIterator<Item = Track>) {
        self.items.extend(tracks);
        if self.position.is_none() && !self.items.is_empty() {
            self.position = Some(0);
        }
    }

    /// Advances to the next track. Returns `None` if the queue has no more
    /// tracks (callers should treat this as "needs more candidates", not
    /// necessarily a hard error).
    pub fn next(&mut self) -> Option<&Track> {
        let next_pos = self.position.map(|p| p + 1).unwrap_or(0);
        if next_pos < self.items.len() {
            self.position = Some(next_pos);
            self.items.get(next_pos)
        } else {
            None
        }
    }

    /// Moves back to the previous track. No-op (returns `None`) if already
    /// at the first track.
    pub fn previous(&mut self) -> Option<&Track> {
        match self.position {
            Some(p) if p > 0 => {
                self.position = Some(p - 1);
                self.items.get(p - 1)
            }
            _ => None,
        }
    }

    pub fn skip_to(&mut self, index: usize) -> Result<&Track> {
        if index >= self.items.len() {
            return Err(EchoraError::QueueIndexOutOfBounds(index));
        }
        self.position = Some(index);
        Ok(&self.items[index])
    }

    pub fn remove(&mut self, index: usize) -> Result<()> {
        if index >= self.items.len() {
            return Err(EchoraError::QueueIndexOutOfBounds(index));
        }
        self.items.remove(index);
        self.position = match self.position {
            None => None,
            Some(_) if self.items.is_empty() => None,
            Some(pos) if index < pos => Some(pos - 1),
            Some(pos) if pos >= self.items.len() => Some(self.items.len() - 1),
            Some(pos) => Some(pos),
        };
        Ok(())
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.position = None;
    }

    pub fn view(&self) -> QueueView {
        QueueView {
            current: self.current().cloned(),
            upcoming: self.upcoming().to_vec(),
            position: self.position,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            artist: None,
            duration_seconds: None,
            thumbnail_url: None,
        }
    }

    #[test]
    fn new_queue_is_empty() {
        let q = Queue::new();
        assert_eq!(q.current(), None);
        assert!(q.upcoming().is_empty());
    }

    #[test]
    fn position_tracks_the_current_index() {
        let mut q = Queue::new();
        assert_eq!(q.position(), None);
        q.add_candidates([track("a"), track("b")]);
        assert_eq!(q.position(), Some(0));
        q.next();
        assert_eq!(q.position(), Some(1));
    }

    #[test]
    fn adding_candidates_to_empty_queue_sets_current_to_first() {
        let mut q = Queue::new();
        q.add_candidates([track("a"), track("b"), track("c")]);
        assert_eq!(q.current().unwrap().id, "a");
        assert_eq!(
            q.upcoming()
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "c"]
        );
    }

    #[test]
    fn adding_candidates_while_playing_does_not_move_current() {
        let mut q = Queue::new();
        q.add_candidates([track("a"), track("b")]);
        q.next(); // current is now "b"
        q.add_candidates([track("c")]);
        assert_eq!(q.current().unwrap().id, "b");
        assert_eq!(
            q.upcoming()
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c"]
        );
    }

    #[test]
    fn next_advances_through_the_queue() {
        let mut q = Queue::new();
        q.add_candidates([track("a"), track("b"), track("c")]);
        assert_eq!(q.next().unwrap().id, "b");
        assert_eq!(q.current().unwrap().id, "b");
        assert_eq!(q.next().unwrap().id, "c");
    }

    #[test]
    fn next_returns_none_at_end_and_does_not_move() {
        let mut q = Queue::new();
        q.add_candidates([track("a"), track("b")]);
        q.next();
        assert_eq!(q.next(), None);
        assert_eq!(q.current().unwrap().id, "b");
    }

    #[test]
    fn previous_moves_back_one() {
        let mut q = Queue::new();
        q.add_candidates([track("a"), track("b"), track("c")]);
        q.next();
        q.next();
        assert_eq!(q.previous().unwrap().id, "b");
        assert_eq!(q.current().unwrap().id, "b");
    }

    #[test]
    fn previous_at_start_returns_none_and_does_not_move() {
        let mut q = Queue::new();
        q.add_candidates([track("a"), track("b")]);
        assert_eq!(q.previous(), None);
        assert_eq!(q.current().unwrap().id, "a");
    }

    #[test]
    fn skip_to_jumps_to_a_valid_index() {
        let mut q = Queue::new();
        q.add_candidates([track("a"), track("b"), track("c")]);
        assert_eq!(q.skip_to(2).unwrap().id, "c");
        assert_eq!(q.current().unwrap().id, "c");
    }

    #[test]
    fn skip_to_invalid_index_errors_without_moving() {
        let mut q = Queue::new();
        q.add_candidates([track("a"), track("b")]);
        let err = q.skip_to(5).unwrap_err();
        assert!(matches!(err, EchoraError::QueueIndexOutOfBounds(5)));
        assert_eq!(q.current().unwrap().id, "a");
    }

    #[test]
    fn remove_before_current_shifts_position_left() {
        let mut q = Queue::new();
        q.add_candidates([track("a"), track("b"), track("c")]);
        q.next(); // current = "b" (index 1)
        q.remove(0).unwrap(); // remove "a"
        assert_eq!(q.current().unwrap().id, "b");
        assert_eq!(
            q.upcoming()
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c"]
        );
    }

    #[test]
    fn remove_current_advances_to_what_was_next() {
        let mut q = Queue::new();
        q.add_candidates([track("a"), track("b"), track("c")]);
        q.remove(0).unwrap(); // remove "a" (the current track)
        assert_eq!(q.current().unwrap().id, "b");
    }

    #[test]
    fn remove_last_remaining_track_leaves_queue_empty() {
        let mut q = Queue::new();
        q.add_candidates([track("a")]);
        q.remove(0).unwrap();
        assert_eq!(q.current(), None);
        assert!(q.upcoming().is_empty());
    }

    #[test]
    fn remove_out_of_bounds_errors() {
        let mut q = Queue::new();
        q.add_candidates([track("a")]);
        let err = q.remove(9).unwrap_err();
        assert!(matches!(err, EchoraError::QueueIndexOutOfBounds(9)));
    }

    #[test]
    fn clear_empties_the_queue() {
        let mut q = Queue::new();
        q.add_candidates([track("a"), track("b")]);
        q.clear();
        assert_eq!(q.current(), None);
        assert!(q.upcoming().is_empty());
    }

    #[test]
    fn all_tracks_returns_every_track_regardless_of_position() {
        let mut q = Queue::new();
        q.add_candidates([track("a"), track("b"), track("c")]);
        q.next(); // current is now "b" — "a" is in the past

        let all: Vec<&str> = q.all_tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(all, vec!["a", "b", "c"]);
    }
}
