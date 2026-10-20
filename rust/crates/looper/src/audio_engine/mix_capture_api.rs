//! Control ownership for one bounded internal float spool, before codec/UI integration.

use super::AudioEngine;
use super::mix_capture::publication::CapturePublication;
use super::mix_capture::{self, CaptureConfig, CaptureControl, CaptureFailure, CaptureObserver};
use crate::messages::{ControlMessage, MixCaptureTarget};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::Path;
use std::sync::{Arc, MutexGuard};

fn payload_sha256_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

#[cfg(test)]
mod tests;

/// Bounded metadata inspection only. A sealed internal spool is never an
/// encoded/public recording, and its checksum grants no source/publication authority.
#[pyfunction]
pub fn inspect_mix_capture_spool(py: Python<'_>, path: &str) -> PyResult<Py<PyDict>> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(PyValueError::new_err(
            "mix capture spool path must be absolute",
        ));
    }
    let info: mix_capture::SpoolInfo = mix_capture::inspect_spool(path).map_err(|error| {
        PyRuntimeError::new_err(format!("mix capture spool inspection failed: {error}"))
    })?;
    let result = PyDict::new(py);
    result.set_item("session_id", info.session_id)?;
    result.set_item("revision", info.revision)?;
    result.set_item("sample_rate_hz", info.sample_rate_hz)?;
    result.set_item("channels", info.channels)?;
    result.set_item(
        "state",
        match info.state {
            mix_capture::SpoolState::PreparedIncomplete => "prepared_incomplete",
            mix_capture::SpoolState::RecordingIncomplete => "recording_incomplete",
            mix_capture::SpoolState::SealedCandidate => "sealed_candidate",
            mix_capture::SpoolState::FailedIncomplete => "failed_incomplete",
        },
    )?;
    result.set_item("failure", info.failure.map(CaptureFailure::label))?;
    result.set_item("failure_code", info.failure.map(CaptureFailure::code))?;
    result.set_item("first_frame", info.first_frame)?;
    result.set_item("declared_submitted_frames", info.declared_submitted_frames)?;
    result.set_item("declared_written_frames", info.declared_written_frames)?;
    result.set_item("declared_end_frame", info.declared_end_frame)?;
    result.set_item("physical_whole_frames", info.physical_whole_frames)?;
    result.set_item("partial_frame_bytes", info.partial_frame_bytes)?;
    result.set_item("requires_verification", info.requires_verification)?;
    result.set_item("publication_ready", info.publication_ready)?;
    Ok(result.unbind())
}

/// Observation and original-engine identity only; dropping a ticket cannot stop audio.
#[pyclass]
pub struct MixCaptureTicket {
    observer: CaptureObserver,
    owner: Arc<()>,
}

#[pymethods]
impl MixCaptureTicket {
    pub fn session_id(&self) -> u64 {
        self.observer.status().session_id
    }
}

pub(super) struct EngineCapture {
    control: CaptureControl,
    publication: Arc<CapturePublication>,
    owner: Arc<()>,
    start_request: Option<MixCaptureTarget>,
    stop_request: Option<MixCaptureTarget>,
}

impl EngineCapture {
    pub(super) fn matches(&self, ticket: &MixCaptureTicket) -> bool {
        Arc::ptr_eq(&self.owner, &ticket.owner)
            && self.publication.session_id() == ticket.session_id()
    }

    fn poll(&mut self, stream_failed: bool) {
        let status = self.control.status();
        if stream_failed && !status.worker_closed {
            self.control
                .request_failure(status.session_id, CaptureFailure::Stream);
        }
        if self.start_request.is_none() && self.control.status().failure.is_some() {
            // The original publication never entered the FIFO. Its unique heavy
            // owner is therefore released here on control, not by a UI ticket.
            drop(self.publication.claim());
        }
        // A panic becomes sticky failure in the shared status. Observation must
        // still return that status instead of hiding it behind a Python error.
        let _ = self.control.join_if_finished();
    }

    fn settled(&self) -> bool {
        let status = self.control.status();
        status.rt_released && status.worker_joined
    }

    pub(super) fn global_start_ready(
        &mut self,
        ticket: &MixCaptureTicket,
        stream_failed: bool,
    ) -> PyResult<bool> {
        if !self.matches(ticket) || self.start_request.is_some() {
            return Err(PyValueError::new_err(
                "foreign, retired or already queued mix capture ticket",
            ));
        }
        self.poll(stream_failed);
        let status = self.control.status();
        if status.failure.is_some() || status.rt_sealed {
            return Err(PyValueError::new_err("original mix capture is terminal"));
        }
        Ok(status.writer_ready)
    }

    pub(super) fn global_publication(&self) -> Arc<CapturePublication> {
        self.publication.clone()
    }

    pub(super) fn global_start_unqueued(&self) -> bool {
        self.start_request.is_none()
    }

    pub(super) fn mark_global_queued(&mut self, target: u64) {
        self.start_request = Some(MixCaptureTarget::At(target));
    }

    pub(super) fn reject_unqueued_global(&mut self) {
        if self.start_request.is_none() {
            self.control
                .request_failure(self.publication.session_id(), CaptureFailure::Rejected);
            self.poll(false);
        }
    }
}

impl AudioEngine {
    pub(super) fn mix_capture_lock(&self) -> PyResult<MutexGuard<'_, Option<EngineCapture>>> {
        self.mix_capture_owner
            .lock()
            .map_err(|_| PyRuntimeError::new_err("mix capture owner lock poisoned"))
    }

    pub(super) fn mix_capture_stream_failed(&self) -> bool {
        self.audio_stream_failed()
    }

    pub(super) fn poll_mix_capture_stream_failure(&self) -> PyResult<()> {
        let mut slot = self.mix_capture_lock()?;
        if let Some(capture) = slot.as_mut() {
            capture.poll(true);
        }
        Ok(())
    }

    pub(super) fn ensure_mix_capture_settled_before_run(&self) -> PyResult<()> {
        let mut slot = self.mix_capture_lock()?;
        if let Some(capture) = slot.as_mut() {
            capture.poll(self.mix_capture_stream_failed());
            if !capture.settled() {
                return Err(PyRuntimeError::new_err(
                    "previous mix capture is not released and joined before stream restart",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn prepare_mix_capture_internal(
        &self,
        spool_path: &str,
        block_frames: usize,
        block_count: usize,
        max_pool_pcm_bytes: usize,
    ) -> PyResult<MixCaptureTicket> {
        self.control_producer()?;
        if self.mix_capture_stream_failed() {
            return Err(PyRuntimeError::new_err("audio stream has failed"));
        }
        // Reuse actual stream geometry, including the existing explicit test
        // geometry seam. Never derive a device format from source material.
        let (sample_rate_hz, channels) = self.ordinary_start_render_format()?;
        let mut slot = self.mix_capture_lock()?;
        if let Some(previous) = slot.as_mut() {
            previous.poll(false);
            if !previous.settled() {
                return Err(PyRuntimeError::new_err(
                    "previous mix capture is not released and joined",
                ));
            }
        }
        let (control, work) = mix_capture::prepare(
            Path::new(spool_path),
            CaptureConfig {
                sample_rate_hz,
                channels,
                block_frames,
                block_count,
                max_pool_pcm_bytes,
            },
        )
        .map_err(|error| {
            PyRuntimeError::new_err(format!("mix capture preparation failed: {error}"))
        })?;
        let observer = control.observer();
        let owner = Arc::new(());
        let ticket = MixCaptureTicket {
            observer,
            owner: owner.clone(),
        };
        *slot = Some(EngineCapture {
            control,
            publication: Arc::new(CapturePublication::new(work)),
            owner,
            start_request: None,
            stop_request: None,
        });
        Ok(ticket)
    }

    pub(super) fn start_mix_capture_internal(
        &self,
        ticket: &MixCaptureTicket,
        target_frame: u64,
    ) -> PyResult<bool> {
        self.start_mix_capture_target_internal(ticket, MixCaptureTarget::At(target_frame))
    }

    pub(super) fn start_mix_capture_target_internal(
        &self,
        ticket: &MixCaptureTicket,
        target: MixCaptureTarget,
    ) -> PyResult<bool> {
        let mut slot = self.mix_capture_lock()?;
        let capture = slot
            .as_mut()
            .filter(|capture| capture.matches(ticket))
            .ok_or_else(|| PyValueError::new_err("foreign or retired mix capture ticket"))?;
        capture.poll(self.mix_capture_stream_failed());
        let status = capture.control.status();
        if capture.start_request.is_some() || status.failure.is_some() || status.rt_sealed {
            return Ok(false);
        }
        if !status.writer_ready {
            return Ok(false);
        }
        let mut producer = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("control producer lock poisoned"))?;
        if producer
            .push(ControlMessage::StartMixCapture {
                publication: capture.publication.clone(),
                target_frame: target,
            })
            .is_err()
        {
            return Ok(false);
        }
        capture.start_request = Some(target);
        Ok(true)
    }

    pub(super) fn stop_mix_capture_internal(
        &self,
        ticket: &MixCaptureTicket,
        target_frame: u64,
    ) -> PyResult<bool> {
        self.stop_mix_capture_target_internal(ticket, MixCaptureTarget::At(target_frame))
    }

    pub(super) fn stop_mix_capture_target_internal(
        &self,
        ticket: &MixCaptureTicket,
        target: MixCaptureTarget,
    ) -> PyResult<bool> {
        let mut slot = self.mix_capture_lock()?;
        let capture = slot
            .as_mut()
            .filter(|capture| capture.matches(ticket))
            .ok_or_else(|| PyValueError::new_err("foreign or retired mix capture ticket"))?;
        capture.poll(self.mix_capture_stream_failed());
        let status = capture.control.status();
        if let Some(original) = capture.stop_request {
            return Ok(original == target);
        }
        if capture.start_request.is_none() || status.failure.is_some() || status.rt_sealed {
            return Ok(false);
        }
        let mut producer = self
            .control_producer()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("control producer lock poisoned"))?;
        if producer
            .push(ControlMessage::StopMixCapture {
                session_id: status.session_id,
                target_frame: target,
            })
            .is_err()
        {
            return Ok(false);
        }
        capture.stop_request = Some(target);
        Ok(true)
    }

    pub(super) fn cancel_mix_capture_internal(&self, ticket: &MixCaptureTicket) -> PyResult<bool> {
        let mut slot = self.mix_capture_lock()?;
        let capture = slot
            .as_mut()
            .filter(|capture| capture.matches(ticket))
            .ok_or_else(|| PyValueError::new_err("foreign or retired mix capture ticket"))?;
        let accepted = capture
            .control
            .request_failure(ticket.session_id(), CaptureFailure::Cancelled);
        capture.poll(self.mix_capture_stream_failed());
        Ok(accepted)
    }

    pub(super) fn mix_capture_status_internal(
        &self,
        py: Python<'_>,
        ticket: &MixCaptureTicket,
    ) -> PyResult<Py<PyDict>> {
        let mut slot = self.mix_capture_lock()?;
        let capture = slot
            .as_mut()
            .filter(|capture| capture.matches(ticket))
            .ok_or_else(|| PyValueError::new_err("foreign or retired mix capture ticket"))?;
        capture.poll(self.mix_capture_stream_failed());
        let status = capture.control.status();
        let result = PyDict::new(py);
        result.set_item("session_id", status.session_id)?;
        result.set_item("spool_path", capture.control.spool_path().to_string_lossy())?;
        result.set_item("start_queued", capture.start_request.is_some())?;
        result.set_item("stop_queued", capture.stop_request.is_some())?;
        result.set_item(
            "start_target",
            capture
                .start_request
                .and_then(MixCaptureTarget::explicit_frame),
        )?;
        result.set_item(
            "stop_target",
            capture
                .stop_request
                .and_then(MixCaptureTarget::explicit_frame),
        )?;
        result.set_item(
            "start_target_mode",
            capture.start_request.map(MixCaptureTarget::mode),
        )?;
        result.set_item(
            "stop_target_mode",
            capture.stop_request.map(MixCaptureTarget::mode),
        )?;
        result.set_item("sample_rate_hz", status.config.sample_rate_hz)?;
        result.set_item("channels", status.config.channels)?;
        result.set_item("writer_ready", status.writer_ready)?;
        result.set_item("started", status.started)?;
        result.set_item("rt_sealed", status.rt_sealed)?;
        result.set_item("rt_released", status.rt_released)?;
        result.set_item("worker_closed", status.worker_closed)?;
        result.set_item("worker_joined", status.worker_joined)?;
        result.set_item("terminal_decided", status.terminal_decided)?;
        // Only this original live ticket exposes the writer-owned payload
        // witness. The bounded recovery inspector grants no such authority.
        result.set_item(
            "payload_sha256",
            status.payload_sha256.map(payload_sha256_hex),
        )?;
        result.set_item("failure", status.failure.map(CaptureFailure::label))?;
        result.set_item("failure_code", status.failure.map(CaptureFailure::code))?;
        result.set_item("error_detail", capture.control.observer().error_detail())?;
        result.set_item("first_frame", status.first_frame)?;
        result.set_item("captured_end", status.captured_end)?;
        result.set_item("buffered_frames", status.buffered_frames)?;
        result.set_item("submitted_end", status.submitted_end)?;
        result.set_item("written_end", status.written_end)?;
        result.set_item("end_frame", status.end_frame)?;
        result.set_item("submitted_frames", status.submitted_frames)?;
        result.set_item("written_frames", status.written_frames)?;
        result.set_item("submitted_blocks", status.submitted_blocks)?;
        result.set_item("written_blocks", status.written_blocks)?;
        result.set_item("allocated_blocks", status.allocated_blocks)?;
        result.set_item("pool_pcm_bytes", status.pool_pcm_bytes)?;
        result.set_item("writer_scratch_bytes", status.writer_scratch_bytes)?;
        result.set_item("empty", status.empty())?;
        result.set_item("successful_spool", status.successful_spool())?;
        Ok(result.unbind())
    }

    /// Control-side shutdown signal. Actual callback teardown seals/releases the
    /// complete image; a signal alone cannot claim a written or closed extent.
    pub(super) fn close_mix_capture_admission(&self) {
        if let Ok(mut slot) = self.mix_capture_owner.lock()
            && let Some(capture) = slot.as_mut()
        {
            // Preserve an already observed device failure before shutdown adds
            // its ordinary cancellation, without replacing any prior reason.
            capture.poll(self.mix_capture_stream_failed());
            let status = capture.control.status();
            if !status.rt_sealed {
                capture
                    .control
                    .request_failure(status.session_id, CaptureFailure::Cancelled);
            }
            capture.poll(false);
        }
    }

    #[cfg(test)]
    pub(super) fn poll_mix_capture_shutdown(&self) {
        let _ = self.poll_mix_capture_shutdown_checked();
    }

    pub(super) fn poll_mix_capture_shutdown_checked(&self) -> PyResult<()> {
        let mut slot = self.mix_capture_lock()?;
        if let Some(capture) = slot.as_mut() {
            // Called only after the stream/CallbackCloseOwner has quiesced. A
            // start left unread in its FIFO may still live in the engine's
            // original publication; claim/release it on control exactly once.
            drop(capture.publication.claim());
            capture.poll(false);
        }
        Ok(())
    }
}
