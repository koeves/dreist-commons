//! Pattern retro recording ([re:6b0323], owned by the pattern
//! editor): every live-recorded step goes into the front buffer
//! ([re:0b14ce], [re:135172] for each
//! [re:4e20da], record on or off); when the selected engine's
//! pattern cycles (`e_updatePatternCycled` 0x1a2) a non-empty front becomes
//! the back buffer ([re:dfedb0]). Engine selection
//! changes and preset loads clear both ([re:180e71]).
//!
//! The header Retro button with the song off ([re:fc4a20]
//!  [re:667221]) copies the back buffer's steps into the
//! current pattern ([re:46cb15]: steps below the pattern's
//! step count; undo "Retro recorded") and clears the buffers. The original
//! replaces the pattern (0xa7); the port posts the steps.

use std::collections::BTreeMap;

use dreist_engine::pattern::StepData;
use dreist_engine::songrec::RetroEvent;

use crate::command::Command;
use crate::model::Model;

#[derive(Default)]
pub struct PatternRetro {
    front: BTreeMap<(usize, usize), StepData>,
    back: BTreeMap<(usize, usize), StepData>,
}

impl PatternRetro {
    pub fn event(&mut self, ev: &RetroEvent, selected_engine: usize) {
        match ev {
            RetroEvent::Step { track, step, data, .. } => {
                self.front.insert((*track, *step), data.clone());
            }
            RetroEvent::Cycled { engine } if *engine == selected_engine => {
                if !self.front.is_empty() {
                    self.back = std::mem::take(&mut self.front);
                }
            }
            RetroEvent::Cycled { .. } => {}
        }
    }

    pub fn clear(&mut self) {
        self.front.clear();
        self.back.clear();
    }

    /// Apply the last complete cycle to the selected engine's current
    /// pattern; returns whether anything was written.
    pub fn apply(&mut self, model: &Model, out: &mut Vec<Command>) -> bool {
        let e = model.engine;
        let pat = model.current_pattern(e);
        let steps = model.pattern(e, pat).steps as usize;
        let mut any = false;
        for ((t, s), d) in &self.back {
            if d.on && *s < steps {
                crate::pattern::write_step(out, e, pat as i32, *t, *s, d);
                any = true;
            }
        }
        self.clear();
        any
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_swap_and_apply_the_last_pass() {
        let mut r = PatternRetro::default();
        let mut d = StepData::off();
        d.on = true;
        r.event(&RetroEvent::Step { engine: 0, track: 2, step: 3, data: d.clone() }, 0);
        // Another engine's cycle does not swap.
        r.event(&RetroEvent::Cycled { engine: 1 }, 0);
        assert!(r.back.is_empty());
        r.event(&RetroEvent::Cycled { engine: 0 }, 0);
        assert_eq!(r.back.len(), 1);
        // An empty front keeps the back buffer.
        r.event(&RetroEvent::Cycled { engine: 0 }, 0);
        assert_eq!(r.back.len(), 1);
        let model = Model::new(crate::project::new_document());
        let mut out = Vec::new();
        assert!(r.apply(&model, &mut out));
        assert!(!out.is_empty());
        assert!(r.back.is_empty() && r.front.is_empty());
    }
}
