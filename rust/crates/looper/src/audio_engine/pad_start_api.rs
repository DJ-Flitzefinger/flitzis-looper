//! Control-side ordinary-start API. Python never reconstructs a captured owner.

use super::AudioEngine;
use super::constants::NUM_SAMPLES;
use super::input_runtime_binding;
use super::ordinary_start::OrdinaryStartPublication;
use super::pad_start::CapturedPadStart;
use super::pitch_actions::PitchActionReject;
use super::resident_relocation::{self, ResidentWindowTicket};
use super::{parse_input_timestamp, validated_input_timestamp};
use crate::messages::ControlMessage;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use std::sync::Arc;

impl AudioEngine {
    // Rust callers retain their existing ordinary capture signature.
    #[allow(clippy::too_many_arguments)]
    pub fn capture_pad_start(
        &self,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
        volume: f32,
        exclusive: bool,
        received_at_ns: Option<&Bound<'_, PyAny>>,
        expected_source_generation: Option<u64>,
    ) -> PyResult<CapturedPadStart> {
        self._capture_pad_start(
            id,
            start_s,
            end_s,
            volume,
            exclusive,
            received_at_ns,
            expected_source_generation,
        )
    }
    pub(super) fn ordinary_start_render_format(&self) -> PyResult<(u32, usize)> {
        let format = self
            .stream_handle
            .as_ref()
            .map(|handle| (handle.output_sample_rate, handle.output_channels));
        #[cfg(test)]
        let format = format
            .or(self.test_output_format)
            .or_else(|| self.test_control_channels.as_ref().map(|_| (48_000, 1)));
        format
            .filter(|(rate, channels)| *rate != 0 && *channels != 0)
            .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))
    }

    // One capture retains the complete original source/launch envelope before residency.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn _capture_pad_start(
        &self,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
        volume: f32,
        exclusive: bool,
        received_at_ns: Option<&Bound<'_, PyAny>>,
        expected_source_generation: Option<u64>,
    ) -> PyResult<CapturedPadStart> {
        self._capture_pad_start_guarded(
            id,
            start_s,
            end_s,
            volume,
            exclusive,
            received_at_ns,
            expected_source_generation,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn _capture_pad_start_guarded(
        &self,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
        volume: f32,
        exclusive: bool,
        received_at_ns: Option<&Bound<'_, PyAny>>,
        expected_source_generation: Option<u64>,
        explicit_target: Option<&super::ExplicitPlaybackTarget>,
    ) -> PyResult<CapturedPadStart> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("id out of range"));
        }
        if !volume.is_finite()
            || !(super::constants::VOLUME_MIN..=super::constants::VOLUME_MAX).contains(&volume)
        {
            return Err(PyValueError::new_err("volume out of range"));
        }
        if !start_s.is_finite()
            || start_s < 0.0
            || end_s.is_some_and(|end| !end.is_finite() || end < 0.0)
        {
            return Err(PyValueError::new_err("loop bounds out of range"));
        }
        let now = self.input_clock.capture_ns();
        let timestamp =
            validated_input_timestamp(parse_input_timestamp(received_at_ns)?, now).unwrap_or(now);
        let format = self.ordinary_start_render_format()?;
        let _requests = self
            .pad_request_ids
            .lock()
            .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
        let binding = input_runtime_binding::capture_under_request_lock(self, id)?;
        input_runtime_binding::validate_expected_source_generation(
            binding.as_ref(),
            expected_source_generation,
        )?;
        let binding = binding
            .filter(|binding| binding.available() && binding.current())
            .ok_or_else(|| PyValueError::new_err("current complete source/timing unavailable"))?;
        if let Some(target) = explicit_target {
            if !Arc::ptr_eq(&target.ownership, &self.input_runtime_ownership)
                || !self
                    .input_runtime_ownership
                    .explicit_target_current(Some(target.value))
                || !self
                    .input_runtime_ownership
                    .explicit_source_current(target.value)
                || binding
                    .binding
                    .placement
                    .and_then(|placement| placement.content_key())
                    != Some(target.value.key)
            {
                return Err(PyValueError::new_err(
                    "explicit playback target changed or unavailable",
                ));
            }
        }
        let mut producer = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
        if producer.is_full() {
            return Err(PyRuntimeError::new_err(
                "ordinary start capture queue is full",
            ));
        }
        let captured = CapturedPadStart::capture_from_binding(
            &binding, start_s, end_s, volume, exclusive, timestamp, format, None,
        )
        .map_err(|reason| PyValueError::new_err(reason.as_str()))?;
        // This original target capture precedes every own WindowACK publication. It
        // transfers no Work and changes no playback state or requested pitch tuple.
        let message = ControlMessage::CapturePadStartTarget(captured.clone());
        let queued = if let Some(target) = explicit_target {
            let placement = binding
                .binding
                .placement
                .expect("complete source placement");
            let write = self
                .input_runtime_ownership
                .prepare_slot_write_fixed(placement, placement.lifetime())
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
            self.input_runtime_ownership
                .commit_slot_write_fixed(
                    &write,
                    || {
                        self.input_runtime_ownership
                            .explicit_target_current(Some(target.value))
                            && self
                                .input_runtime_ownership
                                .explicit_source_current(target.value)
                            && binding.control_publication_current()
                    },
                    |_| {
                        producer
                            .push(message)
                            .expect("reserved explicit start capture capacity")
                    },
                )
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
            true
        } else {
            producer.push(message).is_ok()
        };
        if !queued {
            captured.reject(PitchActionReject::Capacity);
            return Err(PyRuntimeError::new_err(
                "ordinary start capture queue is full",
            ));
        }
        Ok(captured)
    }

    pub(super) fn _prepare_captured_pad_start(
        &self,
        captured: &CapturedPadStart,
        window: Option<&ResidentWindowTicket>,
    ) -> PyResult<ResidentWindowTicket> {
        if !captured.belongs_to(&self.input_runtime_ownership) {
            return Err(PyValueError::new_err(
                "pad start belongs to another audio engine",
            ));
        }
        resident_relocation::prepare_captured_pad_start_with_dependency(self, captured, window)
    }

    // Existing Rust callers have no independently retained controller dependency.
    pub(super) fn prepare_captured_pad_start(
        &self,
        captured: &CapturedPadStart,
    ) -> PyResult<ResidentWindowTicket> {
        self._prepare_captured_pad_start(captured, None)
    }

    pub(super) fn _publish_pad_start(
        &self,
        captured: &CapturedPadStart,
        window: Option<&ResidentWindowTicket>,
    ) -> PyResult<bool> {
        self.publish_pad_start_work(captured, window, false)
    }

    pub(super) fn publish_pad_start_work(
        &self,
        captured: &CapturedPadStart,
        window: Option<&ResidentWindowTicket>,
        inherit_loop: bool,
    ) -> PyResult<bool> {
        if !captured.belongs_to(&self.input_runtime_ownership) {
            return Err(PyValueError::new_err(
                "pad start belongs to another audio engine",
            ));
        }
        if captured.is_published() {
            if captured.state() == "rejected" {
                return Err(PyValueError::new_err(
                    captured.reason().unwrap_or("captured pad start rejected"),
                ));
            }
            return Ok(true);
        }
        let mut producer = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
        if producer.is_full() {
            // The exact owner and its Work remain intact for the same bounded retry.
            return Err(PyRuntimeError::new_err("ordinary start queue is full"));
        }
        let Some(mut work) = captured.take_work(window)? else {
            return Ok(captured.is_published());
        };
        if inherit_loop {
            work.use_current_loop();
        }
        if let Err(reason) = captured.mark_published() {
            if let Err(work) = captured.restore_unpublished_work(work) {
                work.reject(reason);
            }
            return Err(PyValueError::new_err(reason.as_str()));
        }
        let publication = Arc::new(OrdinaryStartPublication::new(work));
        if producer
            .push(ControlMessage::PreparedStart(publication))
            .is_err()
        {
            captured.reject(PitchActionReject::Capacity);
            return Err(PyRuntimeError::new_err("ordinary start queue is full"));
        }
        Ok(true)
    }
}
