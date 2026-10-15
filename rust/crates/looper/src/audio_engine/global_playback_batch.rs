//! Source/current-authority-bound global transactions; all allocation is control-side.

use super::AudioEngine;
use super::captured_global_start::CapturedGlobalStart;
use super::constants::{MAX_VOICES, NUM_SAMPLES};
use super::input_runtime_binding::{
    self, InputPadBinding, InputRuntimePadBinding, LaunchWriteKind,
};
use super::ordinary_start::OrdinaryStartWork;
use super::pad_start::CapturedPadStart;
use crate::messages::ControlMessage;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use std::cell::UnsafeCell;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

const PENDING: u8 = 0;
const ACCEPTED: u8 = 1;
const REJECTED: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GlobalPlaybackEntry {
    pub(crate) id: usize,
    pub(crate) binding: InputPadBinding,
    pub(crate) start_s: f64,
    pub(crate) end_s: Option<f64>,
    pub(crate) launch_revision: u64,
}

pub(crate) struct GlobalPlaybackBatch {
    pub(crate) entries: Box<[GlobalPlaybackEntry]>,
    pub(crate) start: bool,
    pub(crate) received_at_ns: Option<u64>,
    pub(crate) captured_start: Option<CapturedGlobalStart>,
    status: Arc<AtomicU8>,
    prepared_claimed: AtomicBool,
    prepared_starts: UnsafeCell<Option<Box<[Option<Box<OrdinaryStartWork>>]>>>,
}

// Control finishes initialization before queue publication. Exactly one callback
// consumer wins this payload's atomic claim; losing Arc clones never access it.
unsafe impl Send for GlobalPlaybackBatch {}
unsafe impl Sync for GlobalPlaybackBatch {}

impl std::fmt::Debug for GlobalPlaybackBatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GlobalPlaybackBatch")
            .field("entries", &self.entries)
            .field("start", &self.start)
            .field("received_at_ns", &self.received_at_ns)
            .finish_non_exhaustive()
    }
}

impl PartialEq for GlobalPlaybackBatch {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl GlobalPlaybackBatch {
    /// Transfer the complete control-allocated group exactly once. Runtime must
    /// reserve every participant before capture and retain one atomic batch effect.
    pub(crate) fn take_prepared_starts(&self) -> Option<Box<[Option<Box<OrdinaryStartWork>>]>> {
        if self
            .prepared_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return None;
        }
        // Only the successful claim accesses this cell for its remaining lifetime.
        unsafe { (&mut *self.prepared_starts.get()).take() }
    }

    pub(crate) fn finish(&self, accepted: bool) {
        self.status.store(
            if accepted { ACCEPTED } else { REJECTED },
            Ordering::Release,
        );
    }
}

impl Drop for GlobalPlaybackBatch {
    fn drop(&mut self) {
        // Stream/queue teardown happens off callback and rejects unexecuted transactions.
        let _ =
            self.status
                .compare_exchange(PENDING, REJECTED, Ordering::AcqRel, Ordering::Acquire);
    }
}

#[pyclass(frozen)]
pub struct GlobalPlaybackBatchTicket {
    status: Arc<AtomicU8>,
}

#[pymethods]
impl GlobalPlaybackBatchTicket {
    pub fn publication_status(&self) -> &'static str {
        match self.status.load(Ordering::Acquire) {
            ACCEPTED => "accepted",
            REJECTED => "rejected",
            _ => "pending",
        }
    }
}

/// The native request owner serializes source/intent writes with the whole admission.
pub(super) fn enqueue(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    entries: Vec<(&InputRuntimePadBinding, f64, Option<f64>)>,
    start: bool,
    received_at_ns: Option<u64>,
) -> PyResult<GlobalPlaybackBatchTicket> {
    enqueue_before_commit(engine, producer, entries, start, received_at_ns, || {})
}

fn enqueue_before_commit(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    entries: Vec<(&InputRuntimePadBinding, f64, Option<f64>)>,
    start: bool,
    received_at_ns: Option<u64>,
    before_commit: impl FnOnce(),
) -> PyResult<GlobalPlaybackBatchTicket> {
    if (start && (entries.len() > MAX_VOICES || entries.is_empty()))
        || (!start && entries.len() > NUM_SAMPLES)
    {
        return Err(PyValueError::new_err(
            "global playback batch exceeds start/stop target capacity or is empty",
        ));
    }
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let mut seen = [false; NUM_SAMPLES];
    let mut effects = Vec::with_capacity(entries.len());
    let mut captures = Vec::with_capacity(entries.len());
    let mut prepared_starts = Vec::with_capacity(if start { entries.len() } else { 0 });
    let mut start_owners = Vec::with_capacity(if start { entries.len() } else { 0 });
    let render_format = start
        .then(|| engine.ordinary_start_render_format())
        .transpose()?;
    for (binding, start_s, end_s) in &entries {
        let id = binding.id;
        if id >= NUM_SAMPLES
            || seen[id]
            || !start_s.is_finite()
            || *start_s < 0.0
            || end_s.is_some_and(|end| !end.is_finite() || end < 0.0)
        {
            return Err(PyValueError::new_err(
                "invalid or duplicate global playback entry",
            ));
        }
        seen[id] = true;
        let current = input_runtime_binding::capture_under_request_lock(engine, id)?
            .ok_or_else(|| PyRuntimeError::new_err("global playback source unavailable"))?;
        if !Arc::ptr_eq(&binding.ownership, &engine.input_runtime_ownership)
            || !binding.current()
            || !binding.available()
            || !binding.binding.same_projection(&current.binding)
            || !current.current()
            || !current.available()
        {
            return Err(PyRuntimeError::new_err(
                "stale or unavailable global playback binding",
            ));
        }
        effects.push(GlobalPlaybackEntry {
            id,
            binding: current.binding,
            start_s: *start_s,
            end_s: *end_s,
            launch_revision: 0,
        });
        if let Some(format) = render_format {
            let owner = CapturedPadStart::capture_from_binding(
                &current,
                *start_s,
                *end_s,
                1.0,
                false,
                received_at_ns.unwrap_or(0),
                format,
                None,
            )
            .map_err(|reason| PyRuntimeError::new_err(reason.as_str()))?;
            let work = owner
                .take_work_with_binding(current.binding, None)
                .map_err(|reason| PyRuntimeError::new_err(reason.as_str()))?
                .ok_or_else(|| PyRuntimeError::new_err("global start owner already published"))?;
            start_owners.push(owner);
            prepared_starts.push(Some(work));
        }
        captures.push(current);
    }
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
    if entries.iter().any(|(binding, _, _)| !binding.current()) {
        return Err(PyRuntimeError::new_err(
            "global playback binding changed during admission",
        ));
    }
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "Failed to send global playback batch - buffer may be full",
        ));
    }
    let ownership = &engine.input_runtime_ownership;
    let mut writes = Vec::with_capacity(effects.len());
    for entry in &mut effects {
        let placement = entry
            .binding
            .placement
            .or_else(|| ownership.slot_snapshot(entry.id))
            .ok_or_else(|| PyRuntimeError::new_err("global playback placement is pending"))?;
        let prepared = ownership
            .prepare_launch_write(
                placement,
                if start {
                    LaunchWriteKind::Admit
                } else {
                    LaunchWriteKind::Stop
                },
            )
            .map_err(PyRuntimeError::new_err)?;
        entry.launch_revision = prepared.previous_revision;
        writes.push(prepared);
    }
    let anchor = writes
        .first()
        .map(|prepared| prepared.placement)
        .or_else(|| ownership.slot_snapshot(0))
        .ok_or_else(|| PyRuntimeError::new_err("global playback placement is pending"))?;
    let write = ownership
        .prepare_slot_write_fixed(anchor, anchor.lifetime())
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    let status = Arc::new(AtomicU8::new(PENDING));
    let batch = Arc::new(GlobalPlaybackBatch {
        entries: effects.into_boxed_slice(),
        start,
        received_at_ns,
        captured_start: None,
        status: status.clone(),
        prepared_claimed: AtomicBool::new(false),
        prepared_starts: UnsafeCell::new(start.then(|| prepared_starts.into_boxed_slice())),
    });
    let message = ControlMessage::GlobalPlaybackBatch(batch);
    before_commit();
    ownership
        .commit_slot_write_fixed(
            &write,
            || {
                writes
                    .iter()
                    .all(|prepared| ownership.launch_write_current(*prepared))
                    && captures
                        .iter()
                        .all(InputRuntimePadBinding::control_publication_current)
                    && start_owners
                        .iter()
                        .all(CapturedPadStart::mark_published_claimed)
            },
            |_| {
                for prepared in &writes {
                    ownership.apply_launch_write(*prepared);
                }
                let sent = producer.push(message);
                debug_assert!(sent.is_ok());
            },
        )
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    Ok(GlobalPlaybackBatchTicket { status })
}

/// Publish an already captured complete gesture. No current source/loop/timing
/// capture occurs here; only its retained original owners can grant authority.
pub(super) fn enqueue_captured(
    engine: &AudioEngine,
    captured: &CapturedGlobalStart,
) -> PyResult<Option<GlobalPlaybackBatchTicket>> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let mut producer = engine
        .control_producer()?
        .lock()
        .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
    // Capacity is checked before any exact Work is transferred from its owner.
    if producer.is_full() {
        return Err(PyRuntimeError::new_err("global start queue is full"));
    }
    let Some(resolved) = captured.resolve()? else {
        return Ok(None);
    };
    let ownership = &engine.input_runtime_ownership;
    let mut effects = Vec::with_capacity(resolved.len());
    let mut writes = Vec::with_capacity(resolved.len());
    for member in &resolved {
        let placement = member
            .guard
            .binding
            .placement
            .ok_or_else(|| PyRuntimeError::new_err("global placement is pending"))?;
        let prepared = ownership
            .prepare_launch_write(placement, LaunchWriteKind::Admit)
            .map_err(PyRuntimeError::new_err)?;
        let (start_s, end_s) = member.start.loop_region();
        effects.push(GlobalPlaybackEntry {
            id: member.guard.id,
            binding: member.guard.binding,
            start_s,
            end_s,
            launch_revision: prepared.previous_revision,
        });
        writes.push(prepared);
    }
    let anchor = writes
        .first()
        .ok_or_else(|| PyValueError::new_err("empty global start"))?
        .placement;
    let write = match ownership.prepare_slot_write_fixed(anchor, anchor.lifetime()) {
        Ok(write) => write,
        Err(super::content_placement::PlacementError::Pending) => return Ok(None),
        Err(error) => return Err(PyRuntimeError::new_err(error.to_string())),
    };
    let mut prepared_starts = Vec::with_capacity(resolved.len());
    for member in &resolved {
        match member
            .start
            .take_work_with_binding(member.guard.binding, member.resident_guard.clone())
        {
            Ok(Some(work)) => prepared_starts.push(Some(work)),
            result => {
                for (staged, work) in resolved.iter().zip(prepared_starts.into_iter().flatten()) {
                    let _ = staged.start.restore_unpublished_work(work);
                }
                let reason = result
                    .err()
                    .map_or("global start work already consumed", |reason| {
                        reason.as_str()
                    });
                return Err(PyValueError::new_err(reason));
            }
        }
    }
    let status = Arc::new(AtomicU8::new(PENDING));
    let batch = Arc::new(GlobalPlaybackBatch {
        entries: effects.into_boxed_slice(),
        start: true,
        received_at_ns: Some(captured.received_timestamp()),
        captured_start: Some(captured.clone()),
        status: status.clone(),
        prepared_claimed: AtomicBool::new(false),
        prepared_starts: UnsafeCell::new(Some(prepared_starts.into_boxed_slice())),
    });
    let reclaim = batch.clone();
    let result = ownership.commit_slot_write_fixed(
        &write,
        || {
            writes
                .iter()
                .all(|prepared| ownership.launch_write_current(*prepared))
                && resolved
                    .iter()
                    .all(|member| member.start.claim_current(member.guard))
                && captured.mark_published_claimed()
        },
        |_| {
            for prepared in &writes {
                ownership.apply_launch_write(*prepared);
            }
            let sent = producer.push(ControlMessage::GlobalPlaybackBatch(batch));
            debug_assert!(sent.is_ok());
        },
    );
    if let Err(error) = result {
        if let Some(work) = reclaim.take_prepared_starts() {
            for (member, work) in resolved.iter().zip(work.into_vec()) {
                if let Some(work) = work {
                    let _ = member.start.restore_unpublished_work(work);
                }
            }
        }
        reclaim.finish(false);
        if matches!(
            error,
            super::content_placement::PlacementError::Pending
                | super::content_placement::PlacementError::ClaimRejected
        ) {
            return Ok(None);
        }
        return Err(PyRuntimeError::new_err(error.to_string()));
    }
    Ok(Some(GlobalPlaybackBatchTicket { status }))
}

#[cfg(test)]
#[path = "global_launch_publication_tests.rs"]
mod global_launch_publication_tests;
