//! Source/current-authority-bound global transactions; all allocation is control-side.

use super::AudioEngine;
use super::constants::{MAX_VOICES, NUM_SAMPLES};
use super::input_runtime_binding::{self, InputPadBinding, InputRuntimePadBinding};
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
            || binding.binding != current.binding
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
    for entry in &mut effects {
        if start {
            entry.launch_revision = engine.input_runtime_ownership.launch_revision(entry.id);
            if !engine
                .input_runtime_ownership
                .launch_current(entry.id, entry.launch_revision)
            {
                return Err(PyRuntimeError::new_err(
                    "global playback launch revision exhausted",
                ));
            }
        } else {
            engine.input_runtime_ownership.cancel_launches(entry.id);
        }
    }
    for entry in &effects {
        if start {
            engine
                .input_runtime_ownership
                .mark_launch_admitted(entry.id);
        } else {
            engine
                .input_runtime_ownership
                .clear_launch_admitted(entry.id);
        }
    }
    let status = Arc::new(AtomicU8::new(PENDING));
    let batch = Arc::new(GlobalPlaybackBatch {
        entries: effects.into_boxed_slice(),
        start,
        received_at_ns,
        status: status.clone(),
    });
    producer
        .push(ControlMessage::GlobalPlaybackBatch(batch))
        .map_err(|_| {
            PyRuntimeError::new_err("Failed to send global playback batch - buffer may be full")
        })?;
    Ok(GlobalPlaybackBatchTicket { status })
}
