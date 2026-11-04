//! Fixed callback application and observation of the original legacy stem intent.

use super::RtMixer;
use crate::audio_engine::audio_output_legacy::LegacyStemObservation;
use crate::messages::StemMixMode;
use std::sync::Arc;
use std::sync::atomic::Ordering;

impl RtMixer {
    pub(in crate::audio_engine) fn observe_legacy_stem_restore(
        &mut self,
        observation: &LegacyStemObservation,
    ) {
        let source = (|| {
            if !Arc::ptr_eq(&observation.ownership, &self.input_runtime_ownership)
                || observation.epoch
                    != observation
                        .ownership
                        .output_change_epoch
                        .load(Ordering::Acquire)
                || observation
                    .ownership
                    .output_unavailable
                    .load(Ordering::Acquire)
                || !observation.publication.current()
                || self.voices.iter().any(|voice| voice.active)
            {
                return None;
            }
            let id = observation
                .publication
                .callback_slot(&self.input_runtime_ownership, observation.id)?;
            let sample = self.sample_bank[id].as_ref()?;
            let stems = self.prepared_stems[id].as_ref()?;
            let current = self.input_runtime_ownership.callback_source_generation(
                id,
                sample,
                observation.rate,
            ) == Some(observation.generation)
                && stems.publication.same_publication(&observation.publication)
                && stems.source_version_hash == observation.source_hash;
            current.then_some(id)
        })();
        let accepted = source.is_some_and(|id| {
            self.set_stem_mix_mode(id, observation.mode, observation.source_hash)
                && self.set_stem_enabled_mask(id, observation.mask, observation.source_hash)
                && self.stem_mix_mode[id] == observation.mode
                && self.stem_enabled_mask[id] == observation.mask
                && (observation.mode == StemMixMode::FullMix
                    || self.stem_mix_source_version_hash[id] == observation.source_hash)
        });
        observation.settle(accepted);
    }
}
