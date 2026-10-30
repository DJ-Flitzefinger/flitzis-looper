//! Existing MIDI content rows follow a pair; physical mappings keep their slots.

use super::{InputRuntime, RuntimePadState, RuntimeState};
use crate::audio_engine::content_placement::SlotSnapshot;
use crate::messages::SampleBuffer;
use std::sync::{Arc, MutexGuard, atomic::Ordering};

pub(in crate::audio_engine) struct CapturedPairRuntime {
    original: [SlotSnapshot; 2],
    rows: [RuntimePadState; 2],
}

pub(in crate::audio_engine) struct PreparedPairRuntimeMirror<'a> {
    // Retain the source owner until the prepared mirror and its lock retire.
    _ownership: Arc<super::InputRuntimeOwnership>,
    state: MutexGuard<'a, RuntimeState>,
    original: [SlotSnapshot; 2],
    after: [SlotSnapshot; 2],
    rows: [RuntimePadState; 2],
    next_rows: [RuntimePadState; 2],
}

impl InputRuntime {
    pub(in crate::audio_engine) fn capture_pair_rows(
        &self,
        original: [SlotSnapshot; 2],
    ) -> Result<CapturedPairRuntime, String> {
        let state = self
            .runtime_state
            .lock()
            .map_err(|_| "input runtime lock poisoned")?;
        let rows = original.map(|snapshot| state.pads[snapshot.slot()].clone());
        for (snapshot, row) in original.iter().zip(&rows) {
            if row.loaded
                && row.binding.is_none_or(|binding| {
                    !binding
                        .placement
                        .is_some_and(|placement| placement.same_projection(*snapshot))
                        || !self.ownership.current(snapshot.slot(), binding)
                })
            {
                return Err("pair MIDI content row is stale".into());
            }
        }
        Ok(CapturedPairRuntime { original, rows })
    }

    pub(in crate::audio_engine) fn prepare_pair_mirror<'a>(
        &'a self,
        captured: &CapturedPairRuntime,
        after: [SlotSnapshot; 2],
        samples: &[Option<SampleBuffer>; 2],
    ) -> Result<PreparedPairRuntimeMirror<'a>, String> {
        let state = self
            .runtime_state
            .lock()
            .map_err(|_| "input runtime lock poisoned")?;
        let mut next_rows = [captured.rows[1].clone(), captured.rows[0].clone()];
        for (index, sample) in samples.iter().enumerate() {
            let row = &mut next_rows[1 - index];
            let destination = after[1 - index];
            if destination.lifetime() == 0 {
                *row = RuntimePadState::default();
            } else if let Some(binding) = &mut row.binding {
                let sample = sample
                    .as_ref()
                    .ok_or("pair MIDI source custody unavailable")?;
                if binding.placement.and_then(|p| p.content_key()) != destination.content_key()
                    || binding.source_address != sample.source_address()
                    || binding.sample_count != sample.source_sample_count()
                    || binding.channels != sample.channels
                {
                    return Err("pair MIDI immutable source owner changed".into());
                }
                binding.placement = Some(destination);
                binding.resident = sample.resident_binding();
                // Reserved control rows remain frozen while a global publication
                // advances the actual content epochs. Rebase only this successor
                // to its acknowledged content cell; the parent's marker still
                // checks the complete binding against current Native authority.
                binding.runtime_revision = self
                    .ownership
                    .runtime_for_cell(destination.cell())
                    .load(Ordering::Acquire);
            }
        }
        let prepared = PreparedPairRuntimeMirror {
            _ownership: self.ownership.clone(),
            state,
            original: captured.original,
            after,
            rows: captured.rows.clone(),
            next_rows,
        };
        if !prepared.original_current() {
            return Err("pair MIDI content rows changed before mirror".into());
        }
        Ok(prepared)
    }
}

impl PreparedPairRuntimeMirror<'_> {
    pub(in crate::audio_engine) fn publication_bindings(
        &self,
    ) -> [(bool, Option<super::InputPadBinding>); 2] {
        self.next_rows
            .each_ref()
            .map(|row| (row.loaded, row.binding))
    }
    pub(in crate::audio_engine) fn original_current(&self) -> bool {
        self.original
            .iter()
            .zip(&self.rows)
            .all(|(snapshot, row)| self.state.pads[snapshot.slot()].same_projection(row))
    }

    /// All validation and lock acquisition precedes the parent's shared mutation claim.
    pub(in crate::audio_engine) fn apply(&mut self) {
        for (index, snapshot) in self.after.iter().enumerate() {
            std::mem::swap(
                &mut self.state.pads[snapshot.slot()],
                &mut self.next_rows[index],
            );
        }
        // The parent's original marker owns the complete two-row publication.
        // The runtime mutex remains held until both actual rows and this commit exist.
    }
}

#[cfg(test)]
impl InputRuntime {
    pub(in crate::audio_engine) fn pair_binding_for_test(
        &self,
        id: usize,
    ) -> Option<super::InputPadBinding> {
        self.runtime_state.lock().unwrap().pads[id].binding
    }
}
