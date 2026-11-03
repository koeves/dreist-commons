//! Undo / redo ([re:28aea1]). The original records messages; the port
//! keeps document snapshots and rebuilds the engine on a step.

use std::collections::VecDeque;

use dreist_formats::Node;

#[derive(Default)]
pub struct History {
    /// Newest last.
    pub(crate) undo: VecDeque<(String, Node)>,
    pub(crate) redo: Vec<(String, Node)>,
    /// The current gesture (a knob or mouse drag) already has its step.
    stroke_recorded: bool,
    /// The last event was part of a gesture.
    in_gesture: bool,
}

impl History {
    /// Record `before` (the document before an edit) as a step named
    /// `label`, keeping at most `limit` steps; a new edit drops the redo
    /// steps.
    pub fn push(&mut self, label: String, before: Node, limit: usize) {
        self.undo.push_back((label, before));
        while self.undo.len() > limit.max(1) {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    /// Whether the event continues a gesture whose step is recorded: its
    /// document snapshot would be thrown away.
    pub fn mid_gesture(&self, gesture: bool) -> bool {
        gesture && self.in_gesture && self.stroke_recorded
    }

    /// After an event: record a step if the document changed, once per
    /// gesture (a knob drag's value changes, or a mouse press and its
    /// drags, are one step).
    /// `before` is the snapshot taken (none mid-gesture), `changed`
    /// whether the document differs from it.
    pub fn after_event(&mut self, gesture: bool, before: Option<Node>, changed: bool, label: String, limit: usize) {
        if !(gesture && self.in_gesture) {
            self.stroke_recorded = false;
        }
        if let Some(before) = before.filter(|_| changed && !self.stroke_recorded) {
            self.push(label, before, limit);
            self.stroke_recorded = gesture;
        }
        self.in_gesture = gesture;
    }

    /// Move the newest step to the other list, swapping in its document:
    /// returns the document to restore (`current` goes on the other list;
    /// `carry` first copies from it what a step keeps).
    pub fn step(&mut self, redo: bool, current: Node, carry: impl FnOnce(&Node, &mut Node)) -> Result<Node, Node> {
        let next = if redo { self.redo.pop() } else { self.undo.pop_back() };
        let Some((label, mut doc)) = next else { return Err(current) };
        carry(&current, &mut doc);
        if redo {
            self.undo.push_back((label, current));
        } else {
            self.redo.push((label, current));
        }
        self.in_gesture = false;
        Ok(doc)
    }

    /// The newest step names, newest first.
    pub fn labels(&self, redo: bool) -> Vec<&str> {
        if redo { self.redo.iter().rev().map(|(l, _)| l.as_str()).collect() } else { self.undo.iter().rev().map(|(l, _)| l.as_str()).collect() }
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.in_gesture = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(n: usize) -> Node {
        let mut d = Node::new("root", None);
        d.set("n", n);
        d
    }

    #[test]
    fn limit_drops_the_oldest_and_a_lowered_limit_applies() {
        let mut h = History::default();
        for i in 0..10 {
            h.push(format!("{i}"), doc(i), 5);
        }
        assert_eq!(h.labels(false), ["9", "8", "7", "6", "5"]);
        h.push("10".into(), doc(10), 2);
        assert_eq!(h.labels(false), ["10", "9"]);
    }

    #[test]
    fn a_gesture_is_one_step() {
        let mut h = History::default();
        // Press (no change), drags that change, release.
        h.after_event(true, Some(doc(0)), false, "x".into(), 20);
        for i in 0..5 {
            assert_eq!(h.mid_gesture(true), i > 0);
            let before = (!h.mid_gesture(true)).then(|| doc(i));
            h.after_event(true, before, true, "drag".into(), 20);
        }
        h.after_event(false, Some(doc(9)), false, "x".into(), 20);
        assert_eq!(h.labels(false), ["drag"]);
        // The next gesture is a new step.
        h.after_event(true, Some(doc(10)), true, "drag2".into(), 20);
        assert_eq!(h.labels(false), ["drag2", "drag"]);
    }

    #[test]
    fn step_swaps_documents() {
        let mut h = History::default();
        h.push("a".into(), doc(1), 20);
        let restored = h.step(false, doc(2), |_, _| {}).unwrap();
        assert_eq!(restored, doc(1));
        assert_eq!(h.labels(true), ["a"]);
        assert_eq!(h.step(true, restored, |_, _| {}).unwrap(), doc(2));
        assert!(h.step(true, doc(3), |_, _| {}).is_err());
    }
}
