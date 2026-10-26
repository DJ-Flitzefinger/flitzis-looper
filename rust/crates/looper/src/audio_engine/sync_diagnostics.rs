//! Demand-only scalar observations. These records never authorize playback.

use super::{AudioEngine, constants::MAX_VOICES, mixer::RtMixer, transport::TransportTimeline};
use pyo3::{
    exceptions::PyRuntimeError,
    prelude::*,
    types::{PyDict, PyList},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

pub(crate) const HEADER_WORDS: usize = 12;
pub(crate) const ROW_WORDS: usize = 48;
const WORDS: usize = HEADER_WORDS + MAX_VOICES * ROW_WORDS;

#[derive(Clone, Copy, Debug)]
pub(crate) struct DiagnosticRead {
    pub(crate) generation: u64,
    pub(crate) output_start: u64,
    pub(crate) output_end: u64,
    pub(crate) source_frame: usize,
    pub(crate) source_fraction: f64,
    pub(crate) domain: u64,
    pub(crate) loop_start: usize,
    pub(crate) loop_end: usize,
    pub(crate) stems: (u64, u64, u64),
    pub(crate) transition: bool,
}

pub(crate) struct DiagnosticSnapshot(pub(crate) [u64; WORDS]);

pub(crate) struct DiagnosticSlot {
    pub(crate) birth: u64,
    demand: AtomicU64,
    serviced: AtomicU64,
    sequence: AtomicU64,
    fields: [AtomicU64; WORDS],
}

impl DiagnosticSlot {
    fn new(birth: u64) -> Arc<Self> {
        Arc::new(Self {
            birth,
            demand: AtomicU64::new(0),
            serviced: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
            fields: std::array::from_fn(|_| AtomicU64::new(0)),
        })
    }

    pub(crate) fn pending(&self) -> Option<u64> {
        let request = self.demand.load(Ordering::SeqCst);
        (request != 0 && request != self.serviced.load(Ordering::SeqCst)).then_some(request)
    }

    pub(crate) fn observe(
        &self,
        mixer: &RtMixer,
        timeline: &TransportTimeline,
        ns: u64,
        frames: usize,
    ) {
        let Some(request) = self.pending() else {
            return;
        };
        let Some(snapshot) =
            mixer.sync_diagnostic_snapshot(request, self.birth, timeline, ns, frames)
        else {
            return;
        };
        if self.demand.load(Ordering::SeqCst) != request {
            return;
        }
        let sequence = self.sequence.load(Ordering::SeqCst).wrapping_add(1) | 1;
        self.sequence.store(sequence, Ordering::SeqCst);
        for (field, value) in self.fields.iter().zip(snapshot.0) {
            field.store(value, Ordering::SeqCst);
        }
        self.sequence
            .store(sequence.wrapping_add(1), Ordering::SeqCst);
        self.serviced.store(request, Ordering::SeqCst);
    }

    fn read(&self, request: u64) -> Option<DiagnosticSnapshot> {
        let sequence = self.sequence.load(Ordering::SeqCst);
        if sequence == 0 || sequence & 1 != 0 || self.demand.load(Ordering::SeqCst) != request {
            return None;
        }
        let result = DiagnosticSnapshot(
            self.fields
                .each_ref()
                .map(|field| field.load(Ordering::SeqCst)),
        );
        (self.sequence.load(Ordering::SeqCst) == sequence
            && self.demand.load(Ordering::SeqCst) == request
            && result.0[0] == request
            && result.0[1] == self.birth)
            .then_some(result)
    }
}

struct RegistryState {
    birth: u64,
    request: u64,
    current: Option<Arc<DiagnosticSlot>>,
}

pub(crate) struct DiagnosticRegistry {
    state: Mutex<RegistryState>,
}

impl Default for DiagnosticRegistry {
    fn default() -> Self {
        Self {
            state: Mutex::new(RegistryState {
                birth: 0,
                request: 0,
                current: None,
            }),
        }
    }
}

impl DiagnosticRegistry {
    /// Control construction only. Displaced callbacks retain private old slots.
    pub(crate) fn register(&self) -> Option<Arc<DiagnosticSlot>> {
        let mut state = self.state.lock().ok()?;
        let Some(birth) = state.birth.checked_add(1) else {
            state.current = None;
            return None;
        };
        state.birth = birth;
        let slot = DiagnosticSlot::new(birth);
        state.current = Some(slot.clone());
        Some(slot)
    }

    pub(crate) fn reset(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.current = None;
        }
    }

    pub(crate) fn request(&self) -> PyResult<u64> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Diagnostic registry unavailable"))?;
        let slot = state
            .current
            .clone()
            .ok_or_else(|| PyRuntimeError::new_err("No active audio callback for diagnostics"))?;
        let request = state
            .request
            .checked_add(1)
            .ok_or_else(|| PyRuntimeError::new_err("Diagnostic request identities exhausted"))?;
        state.request = request;
        slot.demand.store(request, Ordering::SeqCst);
        Ok(request)
    }

    fn owns(&self, slot: &Arc<DiagnosticSlot>, request: u64) -> bool {
        self.state.lock().ok().is_some_and(|state| {
            state.current.as_ref().is_some_and(|current| {
                Arc::ptr_eq(current, slot) && current.demand.load(Ordering::SeqCst) == request
            })
        })
    }
}

fn optional_float(dict: &Bound<'_, PyDict>, name: &str, word: u64) -> PyResult<()> {
    let value = f64::from_bits(word);
    dict.set_item(name, value.is_finite().then_some(value))
}

pub(crate) fn metadata(
    engine: &AudioEngine,
    py: Python<'_>,
    request: u64,
) -> PyResult<Option<Py<PyDict>>> {
    let registry = &engine.loop_acceptance.diagnostics;
    let slot = registry
        .state
        .lock()
        .map_err(|_| PyRuntimeError::new_err("Diagnostic registry unavailable"))?
        .current
        .clone();
    let Some(slot) = slot else {
        return Ok(None);
    };
    let Some(snapshot) = slot.read(request) else {
        return Ok(None);
    };
    let header = &snapshot.0[..HEADER_WORDS];
    let result = PyDict::new(py);
    result.set_item("schema", "sync-diagnostic-cohort-v1")?;
    result.set_item("request_id", request)?;
    result.set_item("callback_birth", header[1])?;
    result.set_item("callback_observed_at_ns", header[2])?;
    result.set_item("output_frame", header[3])?;
    result.set_item("callback_frames", header[4])?;
    result.set_item("sample_rate_hz", header[5])?;
    result.set_item("channels", header[6])?;
    optional_float(&result, "master_beat", header[7])?;
    optional_float(&result, "master_period_seconds", header[8])?;
    result.set_item("master_phase_generation", header[9])?;
    result.set_item("global_timing_revision", header[10])?;
    result.set_item("boundary", "next-output-frame-after-callback")?;
    let rows = PyList::empty(py);
    for words in snapshot.0[HEADER_WORDS..]
        .chunks_exact(ROW_WORDS)
        .take(header[11] as usize)
    {
        let row = PyDict::new(py);
        for (index, name) in [
            (0, "pad_id"),
            (1, "voice_slot"),
            (2, "voice_generation"),
            (3, "source_address"),
            (4, "source_generation"),
            (5, "content_lifetime"),
            (6, "window_revision"),
            (7, "observed_content_request_incarnation"),
            (8, "logical_next_source_frame"),
            (10, "physical_loop_start_frame"),
            (11, "physical_loop_end_frame"),
            (17, "policy_mode_code"),
            (19, "intent_revision"),
            (25, "native_input_fifo"),
            (26, "native_output_fifo"),
            (27, "native_block_size"),
            (40, "full_source_frames"),
            (41, "loaded_sample_rate_hz"),
            (43, "accepted_publication_epoch"),
        ] {
            row.set_item(name, words[index])?;
        }
        for (index, name) in [
            (9, "logical_next_source_fraction"),
            (12, "source_period_seconds"),
            (13, "source_grid_origin_frame"),
            (14, "applied_source_rate"),
            (15, "target_source_rate"),
            (23, "launch_master_beat"),
            (24, "native_pitch_scale"),
            (42, "musical_loop_period_frames"),
        ] {
            optional_float(&row, name, words[index])?;
        }
        let flags = words[16];
        for (bit, name) in [
            (0, "paused"),
            (1, "native_active"),
            (2, "phase_launch_qualified"),
            (3, "phase_read_witness_current"),
            (4, "phase_program_installed"),
            (5, "accepted_timing_present"),
            (6, "source_admission_present"),
            (10, "mapped_clock"),
            (11, "static_phase_clock"),
            (12, "key_lock_effective"),
            (13, "key_lock_requested"),
            (14, "phase_birth_unadvanced"),
            (15, "physical_normalized_static_clock"),
        ] {
            row.set_item(name, flags & (1 << bit) != 0)?;
        }
        row.set_item(
            "global_policy_revision",
            (flags & (1 << 16) != 0).then_some(words[18]),
        )?;
        row.set_item(
            "phase_policy_snapshot_revision",
            (flags & (1 << 17) != 0).then_some(words[20]),
        )?;
        row.set_item(
            "phase_policy_timing_revision",
            (flags & (1 << 17) != 0).then_some(words[21]),
        )?;
        row.set_item(
            "original_launch_frame",
            (flags & (1 << 18) != 0).then_some(words[22]),
        )?;
        row.set_item("pitch_semitones", words[28] as i64)?;
        row.set_item(
            "original_launch_phase_generation",
            (flags & (1 << 18) != 0).then_some(words[44]),
        )?;
        // Historical callback evidence is never upgraded to current source authority.
        // In particular, observation must not run admission/reconciliation during Move/load.
        row.set_item("source_identity_scope", "callback_boundary_only")?;
        if flags & (1 << 7) != 0 {
            let read = PyDict::new(py);
            for (index, name) in [
                (29, "output_start_frame"),
                (30, "output_end_frame"),
                (31, "next_source_frame"),
                (34, "physical_loop_start_frame"),
                (35, "physical_loop_end_frame"),
                (37, "stem_source_version_hash"),
                (38, "stem_enabled_mask"),
            ] {
                read.set_item(name, words[index])?;
            }
            optional_float(&read, "next_source_fraction", words[32])?;
            read.set_item(
                "domain",
                if words[33] == 1 {
                    "dry_output"
                } else {
                    "native_input"
                },
            )?;
            read.set_item("all_stems", words[36] != 0)?;
            read.set_item("stem_transition", words[39] != 0)?;
            read.set_item(
                "interval_meaning",
                "render_feed_interval; native_input_is_not_audible_output_phase",
            )?;
            row.set_item("last_complete_read", read)?;
        } else {
            row.set_item("last_complete_read", py.None())?;
        }
        rows.append(row)?;
    }
    result.set_item("voices", rows)?;
    result.set_item("separate_control_feedback", control_feedback(engine, py)?)?;
    if !registry.owns(&slot, request) {
        return Ok(None);
    }
    Ok(Some(result.unbind()))
}

fn control_feedback<'py>(engine: &AudioEngine, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("observed_at_ns", engine.input_clock.capture_ns())?;
    result.set_item(
        "scope",
        "separate_control_observation; not_current_source_or_phase_authority",
    )?;
    let snapshot = engine
        .input_runtime
        .as_ref()
        .and_then(|runtime| runtime.runtime_policy_snapshot().ok().flatten());
    if let Some(attempt) = snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.observation())
    {
        let value = PyDict::new(py);
        value.set_item("snapshot_revision", snapshot.as_ref().unwrap().revision())?;
        value.set_item("attempt", attempt.attempt)?;
        value.set_item("state", attempt.state)?;
        value.set_item("reason", attempt.reason.map(|reason| reason.name()))?;
        value.set_item("execution_frame", attempt.execution_frame)?;
        result.set_item("policy_attempt", value)?;
    } else {
        result.set_item("policy_attempt", py.None())?;
    }
    let pads = PyList::empty(py);
    for id in 0..super::constants::NUM_SAMPLES {
        let sync = engine.input_runtime_ownership.source_sync_status(id);
        let key_lock = engine.input_runtime_ownership.key_lock_status(id);
        if sync.is_none() && key_lock.is_none() {
            continue;
        }
        let row = PyDict::new(py);
        row.set_item("pad_id", id)?;
        if let Some(status) = sync {
            row.set_item("source_generation", status.source_generation)?;
            row.set_item("source_address", status.source_address)?;
            row.set_item("content_lifetime", status.content_lifetime)?;
            row.set_item("window_revision", status.window_revision)?;
            row.set_item("active", status.active)?;
            row.set_item(
                "map_revision_words",
                status.witness.map(|witness| witness.map_revision),
            )?;
            if let Some(birth) = status.birth {
                row.set_item("birth_original_target_frame", birth.original)?;
                row.set_item("birth_execution_frame", birth.execution)?;
                row.set_item("birth_emitted", birth.emitted)?;
                row.set_item(
                    "birth_source_request_incarnation",
                    status.birth_source_request_incarnation,
                )?;
            }
        }
        if let Some(status) = key_lock {
            let value = PyDict::new(py);
            value.set_item("source_generation", status.source_generation)?;
            value.set_item("source_address", status.source_address)?;
            value.set_item("window_revision", status.window_revision)?;
            value.set_item("request_id", status.request_id)?;
            value.set_item("effective", status.effective)?;
            value.set_item("ready", status.ready)?;
            value.set_item("state", status.state)?;
            value.set_item("error", status.error)?;
            row.set_item("key_lock", value)?;
        }
        pads.append(row)?;
    }
    result.set_item("raw_source_status", pads)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publish(slot: &DiagnosticSlot, request: u64) {
        slot.sequence.store(1, Ordering::SeqCst);
        slot.fields[0].store(request, Ordering::SeqCst);
        slot.fields[1].store(slot.birth, Ordering::SeqCst);
        slot.sequence.store(2, Ordering::SeqCst);
        slot.serviced.store(request, Ordering::SeqCst);
    }

    #[test]
    fn sync_diagnostic_private_births_reset_and_supersession_reject_stale_slots() {
        let registry = DiagnosticRegistry::default();
        let old = registry.register().unwrap();
        assert_eq!(old.pending(), None);
        let first = registry.request().unwrap();
        publish(&old, first);
        assert!(old.read(first).is_some());
        assert!(registry.owns(&old, first));
        let next = registry.request().unwrap();
        assert!(old.read(first).is_none());
        assert!(old.read(next).is_none());
        let replacement = registry.register().unwrap();
        assert!(replacement.birth > old.birth);
        let third = registry.request().unwrap();
        publish(&old, next); // A displaced callback cannot overwrite its successor.
        assert!(!registry.owns(&old, next));
        assert!(replacement.read(third).is_none());
        publish(&replacement, third);
        assert!(replacement.read(third).is_some());
        replacement.sequence.store(3, Ordering::SeqCst);
        assert!(replacement.read(third).is_none());
        registry.reset();
        assert!(!registry.owns(&replacement, third));
        assert!(registry.request().is_err());
        let after = registry.register().unwrap();
        assert!(after.birth > replacement.birth);
        assert!(registry.request().unwrap() > third);
    }

    #[test]
    fn sync_diagnostic_mid_render_supersession_waits_for_new_capture() {
        let registry = DiagnosticRegistry::default();
        let slot = registry.register().unwrap();
        let mut mixer = RtMixer::try_new(1, 8_000.0, None).unwrap();
        let timeline = TransportTimeline::new(8_000);
        let old = registry.request().unwrap();
        mixer.begin_sync_diagnostic_capture(old);
        let new = registry.request().unwrap();
        slot.observe(&mixer, &timeline, 1, 0);
        assert!(slot.read(new).is_none());
        assert_eq!(slot.pending(), Some(new));
        mixer.begin_sync_diagnostic_capture(new);
        slot.observe(&mixer, &timeline, 2, 0);
        assert_eq!(slot.read(new).unwrap().0[0], new);
        assert_eq!(slot.pending(), None);
    }
}
