//! Back/Forward history (SPEC §7): files, pasted text and in-document anchor jumps, each with the
//! scroll offset it was left at.

use std::path::PathBuf;
use std::sync::Arc;

/// What a history entry shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Location {
    File(PathBuf),
    /// Pasted text is kept so Forward can return to it.
    Pasted(Arc<str>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub location: Location,
    /// Scroll offset (points) when the reader left this entry.
    pub scroll: f32,
}

#[derive(Debug, Default)]
pub struct History {
    entries: Vec<Entry>,
    index: usize,
}

const MAX_ENTRIES: usize = 100;

impl History {
    #[cfg(test)]
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.index)
    }

    /// Back/Forward buttons are shown only once there is somewhere to go.
    pub fn has_history(&self) -> bool {
        self.entries.len() > 1
    }

    pub fn can_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_forward(&self) -> bool {
        self.index + 1 < self.entries.len()
    }

    /// Navigate somewhere new: remember where we were, drop the forward stack, add the entry.
    pub fn push(&mut self, location: Location, current_scroll: f32) {
        if let Some(cur) = self.entries.get_mut(self.index) {
            cur.scroll = current_scroll;
            self.entries.truncate(self.index + 1);
        }
        self.entries.push(Entry {
            location,
            scroll: 0.0,
        });
        if self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.index = self.entries.len() - 1;
    }

    /// Go back one entry, remembering the current scroll offset. Returns the entry to show.
    pub fn back(&mut self, current_scroll: f32) -> Option<&Entry> {
        if !self.can_back() {
            return None;
        }
        self.entries[self.index].scroll = current_scroll;
        self.index -= 1;
        self.entries.get(self.index)
    }

    pub fn forward(&mut self, current_scroll: f32) -> Option<&Entry> {
        if !self.can_forward() {
            return None;
        }
        self.entries[self.index].scroll = current_scroll;
        self.index += 1;
        self.entries.get(self.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> Location {
        Location::File(PathBuf::from(name))
    }

    #[test]
    fn empty_history_goes_nowhere() {
        let mut h = History::default();
        assert!(!h.has_history() && !h.can_back() && !h.can_forward());
        assert!(h.back(0.0).is_none());
        assert!(h.forward(0.0).is_none());
        assert!(h.current().is_none());
    }

    #[test]
    fn back_and_forward_restore_scroll() {
        let mut h = History::default();
        h.push(file("a.md"), 0.0);
        assert!(!h.has_history());
        h.push(file("b.md"), 420.0); // left a.md at 420
        assert!(h.has_history() && h.can_back() && !h.can_forward());

        let e = h.back(77.0).unwrap(); // left b.md at 77
        assert_eq!((e.location.clone(), e.scroll), (file("a.md"), 420.0));
        assert!(h.can_forward());

        let e = h.forward(10.0).unwrap();
        assert_eq!((e.location.clone(), e.scroll), (file("b.md"), 77.0));
        assert_eq!(h.back(0.0).unwrap().scroll, 10.0);
    }

    #[test]
    fn push_drops_forward_stack() {
        let mut h = History::default();
        h.push(file("a.md"), 0.0);
        h.push(file("b.md"), 0.0);
        h.back(0.0);
        h.push(file("c.md"), 5.0);
        assert!(!h.can_forward());
        assert_eq!(h.current().unwrap().location, file("c.md"));
        assert_eq!(h.back(0.0).unwrap().location, file("a.md"));
    }

    #[test]
    fn anchor_jumps_are_entries_of_the_same_file() {
        let mut h = History::default();
        h.push(file("a.md"), 0.0);
        h.push(file("a.md"), 1200.0); // jumped to #rollback-plan from 1200
        let e = h.back(3400.0).unwrap();
        assert_eq!((e.location.clone(), e.scroll), (file("a.md"), 1200.0));
    }

    #[test]
    fn pasted_text_survives_forward() {
        let mut h = History::default();
        h.push(file("a.md"), 0.0);
        h.push(Location::Pasted(Arc::from("# Pasted")), 0.0);
        h.back(0.0);
        assert_eq!(
            h.forward(0.0).unwrap().location,
            Location::Pasted(Arc::from("# Pasted"))
        );
    }

    #[test]
    fn history_is_capped() {
        let mut h = History::default();
        for i in 0..250 {
            h.push(file(&format!("{i}.md")), 0.0);
        }
        assert_eq!(h.entries.len(), MAX_ENTRIES);
        assert_eq!(h.current().unwrap().location, file("249.md"));
    }
}
