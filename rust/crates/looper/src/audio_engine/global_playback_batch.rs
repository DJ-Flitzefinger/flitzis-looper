//! Source/current-authority-bound global transactions; all allocation is control-side.

use super::AudioEngine;
use super::constants::{MAX_VOICES, NUM_SAMPLES};
use super::input_runtime_binding::{
    self, InputPadBinding, InputRuntimePadBinding, LaunchWriteKind,
};
use crate::messages::ControlMessage;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
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

#[derive(Debug)]
pub(crate) struct GlobalPlaybackBatch {
    pub(crate) entries: Box<[GlobalPlaybackEntry]>,
    pub(crate) start: bool,
    pub(crate) received_at_ns: Option<u64>,
    status: Arc<AtomicU8>,
}

impl PartialEq for GlobalPlaybackBatch {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl GlobalPlaybackBatch {
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
        status: status.clone(),
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

#[cfg(test)]
#[path = "global_launch_publication_tests.rs"]
mod global_launch_publication_tests;
