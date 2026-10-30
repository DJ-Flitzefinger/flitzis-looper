use super::*;
use crate::audio_engine::ordinary_endpoint_reserve::{
    EndpointDescriptor, SelectedEndpointSnapshot,
};
use crate::audio_engine::ordinary_start::OrdinaryStartWork;
use crate::audio_engine::pitch_actions::{
    PitchActionPreparationMode, PitchActionVariant, PitchActionWork,
};
use crate::audio_engine::prepared_native_history::NativeHistoryRequest;

impl RtMixer {
    pub(crate) fn input_ownership(
        &self,
    ) -> &Arc<super::super::input_runtime_binding::InputRuntimeOwnership> {
        &self.input_runtime_ownership
    }
    pub(crate) fn timing_acknowledgements(&self) -> &Arc<CurrentTimingAcknowledgements> {
        &self.current_timing_acknowledgements
    }

    /// Called after control/parameter drains; no desired Python or RuntimeState tuple is read.
    pub(crate) fn capture_selected_normal_endpoint(&self) -> Option<SelectedEndpointSnapshot> {
        if !self.global_parameters_drained {
            return None;
        }
        let selection = self.input_runtime_ownership.endpoint_selection()?;
        let key = selection.1?;
        let id = self.input_runtime_ownership.callback_key_slot(key)?;
        if self.input_runtime_ownership.callback_feedback_key(id) != Some(key) {
            return None;
        }
        let rate = f64::from(self.sample_rate_hz);
        let snapshot = self
            .normal_start_snapshot(
                id,
                self.pad_loop_start_frame[id] as f64 / rate,
                self.pad_loop_end_frame[id].map(|end| end as f64 / rate),
            )
            .ok()?;
        let render = self.ordinary_start_render(id);
        if render
            .pitch
            .native_pitch_scale(snapshot.playback.tempo_ratio(), render.key_lock)
            .ok()?
            == 1.0
        {
            return None;
        }
        let generation = self.input_runtime_ownership.source_generation(
            id,
            &snapshot.sample,
            snapshot.sample_rate_hz,
        )?;
        let descriptor = EndpointDescriptor::new(
            id,
            key,
            &snapshot,
            generation,
            self.input_runtime_ownership.authority_revision(id),
            self.input_runtime_ownership
                .key_source_request_incarnation(key)?,
            render,
        );
        (self.input_runtime_ownership.endpoint_selection() == Some(selection)).then_some(
            SelectedEndpointSnapshot {
                id,
                descriptor,
                snapshot,
                render,
            },
        )
    }

    /// A cache marker is useful only behind this fresh original action/source guard.
    pub(crate) fn ordinary_endpoint_matches(
        &self,
        work: &OrdinaryStartWork,
        source: &NativeHistoryRequest,
    ) -> bool {
        let Ok(guard) = self.ordinary_start_guard(work) else {
            return false;
        };
        let Some(selected) = self.capture_selected_normal_endpoint() else {
            return false;
        };
        if selected.id != guard.id {
            return false;
        }
        let Ok(snapshot) = self.ordinary_start_snapshot(work, guard.id) else {
            return false;
        };
        let render = self.ordinary_start_render(guard.id);
        let Some(permit) = work.capture_history_permit(&snapshot.sample, snapshot.sample_rate_hz)
        else {
            return false;
        };
        source.matches_reserved_endpoint(&snapshot, render)
            && source.permit.same_endpoint_authority(&permit)
    }

    pub(crate) fn pitch_endpoint_matches(
        &self,
        work: &PitchActionWork,
        source: &NativeHistoryRequest,
        projection_frames: u64,
    ) -> bool {
        if work.action.variant() != PitchActionVariant::SetRetrigger
            || work.preparation_mode() != PitchActionPreparationMode::Endpoint
            || !work.validates_render_format(self.sample_rate_hz as u32, self.channels)
        {
            return false;
        }
        let Ok(guard) = self.pitch_action_guard(work) else {
            return false;
        };
        let Some(selected) = self.capture_selected_normal_endpoint() else {
            return false;
        };
        if selected.id != guard.id || work.action.render() != self.ordinary_start_render(guard.id) {
            return false;
        }
        let Ok(snapshot) = self.pitch_action_start_snapshot(work, guard.id, projection_frames)
        else {
            return false;
        };
        let Some(permit) = work.capture_history_permit(&snapshot.sample, snapshot.sample_rate_hz)
        else {
            return false;
        };
        source.matches_reserved_endpoint(&snapshot, work.action.render())
            && source.permit.same_endpoint_authority(&permit)
    }
}
