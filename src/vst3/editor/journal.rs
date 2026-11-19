use std::collections::VecDeque;

const LIMIT: usize = 64;

#[derive(Default)]
pub struct Journal {
    undo: VecDeque<Vec<Change>>,
    redo: Vec<Vec<Change>>,
    start: Vec<(usize, f64)>,
    pub replaying: bool,
}

#[derive(Clone, Copy)]
struct Change {
    id: usize,
    before: f64,
    after: f64,
}

impl Journal {
    pub fn begin(&mut self, ids: &[usize], read: impl Fn(usize) -> f64) {
        if !self.replaying {
            self.start.extend(ids.iter().map(|&id| (id, read(id))));
        }
    }

    pub fn finish(&mut self, read: impl Fn(usize) -> f64) {
        let changes: Vec<_> = self
            .start
            .drain(..)
            .filter_map(|(id, before)| {
                let after = read(id);
                (before != after).then_some(Change { id, before, after })
            })
            .collect();
        if changes.is_empty() {
            return;
        }
        self.redo.clear();
        if self.undo.len() == LIMIT {
            self.undo.pop_front();
        }
        self.undo.push_back(changes);
    }

    pub fn available(&self, redo: bool) -> bool {
        if redo {
            !self.redo.is_empty()
        } else {
            !self.undo.is_empty()
        }
    }

    pub fn step(&mut self, redo: bool) -> Option<Vec<(usize, f64)>> {
        let change = if redo {
            self.redo.pop()
        } else {
            self.undo.pop_back()
        }?;
        let values = change
            .iter()
            .map(|c| (c.id, if redo { c.after } else { c.before }))
            .collect();
        if redo {
            self.undo.push_back(change);
        } else {
            self.redo.push(change);
        }
        Some(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_group_edits_and_drops_redo_after_a_new_edit() {
        let mut journal = Journal::default();
        journal.begin(&[2, 7], |_| 0.25);
        journal.finish(|_| 0.75);
        assert_eq!(journal.step(false), Some(vec![(2, 0.25), (7, 0.25)]));
        assert_eq!(journal.step(true), Some(vec![(2, 0.75), (7, 0.75)]));
        journal.step(false);
        journal.begin(&[3], |_| 0.2);
        journal.finish(|_| 0.6);
        assert!(!journal.available(true));
    }
}
