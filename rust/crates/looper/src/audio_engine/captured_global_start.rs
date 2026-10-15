//! One original GLOBAL gesture, retained before resident readiness can change.

use super::AudioEngine;
use super::constants::{MAX_VOICES, NUM_SAMPLES};
use super::global_playback_batch::{self, GlobalPlaybackBatchTicket};
use super::input_runtime_binding::{
    InputPadBinding, InputRuntimeOwnership, InputRuntimePadBinding,
};
use super::pad_start::{CapturedPadStart, PadStartGuard};
use super::pitch_actions::PitchActionReject;
use super::resident_relocation::{
    ResidentLaunchGuard, ResidentStartDependency, ResidentWindowTicket,
};
use crate::messages::ControlMessage;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct GlobalStartMember {
    start: CapturedPadStart,
    dependency: Option<ResidentStartDependency>,
}

struct GlobalStartOwner {
    ownership: Arc<InputRuntimeOwnership>,
    members: Box<[GlobalStartMember]>,
    received_at_ns: u64,
    published: AtomicBool,
}

/// Opaque native ownership, never reconstructed from current Python fields.
#[pyclass(frozen)]
#[derive(Clone)]
pub struct CapturedGlobalStart {
    owner: Arc<GlobalStartOwner>,
}

impl std::fmt::Debug for CapturedGlobalStart {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapturedGlobalStart")
            .field("participants", &self.owner.members.len())
            .finish_non_exhaustive()
    }
}

pub(crate) struct ResolvedGlobalStart {
    pub(crate) start: CapturedPadStart,
    pub(crate) guard: PadStartGuard,
    pub(crate) resident_guard: Option<ResidentLaunchGuard>,
}

impl CapturedGlobalStart {
    pub(crate) fn belongs_to(&self, ownership: &Arc<InputRuntimeOwnership>) -> bool {
        Arc::ptr_eq(&self.owner.ownership, ownership)
    }

    pub(crate) fn received_timestamp(&self) -> u64 {
        self.owner.received_at_ns
    }

    /// The single callback performs the existing quantization once for the group.
    pub(crate) fn capture_target(&self, target: u64) {
        for member in &self.owner.members {
            if let Err(reason) = member.start.capture_target(target) {
                self.reject(reason);
                return;
            }
        }
    }

    pub(crate) fn target_frame(&self) -> Option<u64> {
        self.owner
            .members
            .first()
            .and_then(|member| member.start.target_frame())
    }

    pub(crate) fn reject(&self, reason: PitchActionReject) {
        for member in &self.owner.members {
            member.start.reject(reason);
        }
    }

    pub(crate) fn resolve(&self) -> PyResult<Option<Vec<ResolvedGlobalStart>>> {
        if self.owner.published.load(Ordering::Acquire) {
            return Err(PyValueError::new_err("global start already published"));
        }
        let mut resolved = Vec::with_capacity(self.owner.members.len());
        for member in &self.owner.members {
            if member.start.state() == "rejected" {
                return Err(PyValueError::new_err(
                    member.start.reason().unwrap_or("global start rejected"),
                ));
            }
            let (binding, resident_guard): (InputPadBinding, Option<ResidentLaunchGuard>) =
                match member.dependency.as_ref() {
                    Some(dependency) => {
                        let Some((binding, guard)) = dependency.resolve(&member.start)? else {
                            return Ok(None);
                        };
                        (binding, Some(guard))
                    }
                    None => match member.start.control_guard() {
                        Ok(guard) => (guard.binding, None),
                        Err(
                            PitchActionReject::PlacementPending | PitchActionReject::WindowPending,
                        ) => return Ok(None),
                        Err(reason) => return Err(PyValueError::new_err(reason.as_str())),
                    },
                };
            let guard = match member
                .start
                .preflight_work_with_binding(binding, resident_guard.as_ref())
            {
                Ok(guard) => guard,
                Err(PitchActionReject::PlacementPending | PitchActionReject::WindowPending) => {
                    return Ok(None);
                }
                Err(reason) => return Err(PyValueError::new_err(reason.as_str())),
            };
            resolved.push(ResolvedGlobalStart {
                start: member.start.clone(),
                guard,
                resident_guard,
            });
        }
        Ok(Some(resolved))
    }

    pub(crate) fn mark_published_claimed(&self) -> bool {
        // Members are private to this group and publication is serialized by the
        // engine request mutex. No other path can publish a prefix independently;
        // after complete preflight these one-shot CAS operations cannot diverge.
        !self.owner.published.load(Ordering::Acquire)
            && self
                .owner
                .members
                .iter()
                .all(|member| member.start.mark_published_claimed())
            && self
                .owner
                .published
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }
}

#[pymethods]
impl CapturedGlobalStart {
    pub fn diagnostic_slots(&self) -> Vec<usize> {
        self.owner
            .members
            .iter()
            .filter_map(|member| member.start.diagnostic_slot())
            .collect()
    }
    pub fn original_frame(&self) -> Option<u64> {
        self.target_frame()
    }
    pub fn state(&self) -> &'static str {
        if self
            .owner
            .members
            .iter()
            .any(|member| member.start.state() == "rejected")
        {
            "rejected"
        } else if self
            .owner
            .members
            .iter()
            .all(|member| member.start.state() == "applied")
        {
            "applied"
        } else if self.owner.published.load(Ordering::Acquire) {
            "published"
        } else {
            "captured"
        }
    }
    pub fn reason(&self) -> Option<&'static str> {
        self.owner
            .members
            .iter()
            .find_map(|member| member.start.reason())
    }
    /// Cancels only the attacks; the independently requested resident edit survives.
    pub fn cancel(&self) -> bool {
        let mut cancelled = false;
        for member in &self.owner.members {
            cancelled |= member.start.cancel();
        }
        cancelled
    }
}

pub(super) fn capture(
    engine: &AudioEngine,
    entries: Vec<(
        &InputRuntimePadBinding,
        f64,
        Option<f64>,
        Option<&ResidentWindowTicket>,
    )>,
    received_at_ns: u64,
) -> PyResult<CapturedGlobalStart> {
    if entries.is_empty() || entries.len() > MAX_VOICES {
        return Err(PyValueError::new_err(
            "global start exceeds participant capacity or is empty",
        ));
    }
    let format = engine.ordinary_start_render_format()?;
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let mut producer = engine
        .control_producer()?
        .lock()
        .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "global start capture queue is full",
        ));
    }
    let mut seen = [false; NUM_SAMPLES];
    let mut members = Vec::with_capacity(entries.len());
    for (binding, start_s, end_s, ticket) in &entries {
        if binding.id >= NUM_SAMPLES
            || seen[binding.id]
            || !Arc::ptr_eq(&binding.ownership, &engine.input_runtime_ownership)
            || !binding.current()
            || !binding.available()
        {
            return Err(PyValueError::new_err(
                "invalid, duplicate or unavailable global source",
            ));
        }
        seen[binding.id] = true;
        let start = CapturedPadStart::capture_from_binding(
            binding,
            *start_s,
            *end_s,
            1.0,
            false,
            received_at_ns,
            format,
            None,
        )
        .map_err(|reason| PyValueError::new_err(reason.as_str()))?;
        let dependency = ticket
            .map(|ticket| ticket.capture_start_dependency(&start))
            .transpose()?;
        members.push(GlobalStartMember { start, dependency });
    }
    if entries.iter().any(|(binding, _, _, _)| !binding.current()) {
        return Err(PyValueError::new_err(
            "global source changed during original capture",
        ));
    }
    let captured = CapturedGlobalStart {
        owner: Arc::new(GlobalStartOwner {
            ownership: engine.input_runtime_ownership.clone(),
            members: members.into_boxed_slice(),
            received_at_ns,
            published: AtomicBool::new(false),
        }),
    };
    if producer
        .push(ControlMessage::CaptureGlobalStartTarget(captured.clone()))
        .is_err()
    {
        captured.reject(PitchActionReject::Capacity);
        return Err(PyRuntimeError::new_err(
            "global start capture queue is full",
        ));
    }
    Ok(captured)
}

pub(super) fn publish(
    engine: &AudioEngine,
    captured: &CapturedGlobalStart,
) -> PyResult<Option<GlobalPlaybackBatchTicket>> {
    if !captured.belongs_to(&engine.input_runtime_ownership) {
        return Err(PyValueError::new_err(
            "global start belongs to another audio engine",
        ));
    }
    global_playback_batch::enqueue_captured(engine, captured)
}

#[cfg(test)]
#[path = "captured_global_start_tests.rs"]
mod tests;
