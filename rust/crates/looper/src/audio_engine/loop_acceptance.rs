//! Demand-only, fixed-size observation of an effective productive voice.
//!
//! One callback writer copies scalars into preallocated atomics. Control reads once,
//! without waiting, then resolves actual current source/acknowledgement outside RT.
//! Observation never changes source progression or schedules an audio command.

use super::AudioEngine;
use super::constant_timing::AcceptedTimingProjection;
use super::constants::NUM_SAMPLES;
use super::input_runtime_binding;
use super::mixer::RtMixer;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const WORDS: usize = 54;
const PAD_BITS: u32 = 8;
const PAD_MASK: u64 = (1 << PAD_BITS) - 1;

/// Fixed scalar words; the first nineteen retain request/source/projection identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoopAcceptanceSnapshot(pub(crate) [u64; WORDS]);

impl LoopAcceptanceSnapshot {
    pub(crate) fn empty(request: u64, observed_ns: u64, frame: u64, frames: usize) -> Self {
        let mut words = [0; WORDS];
        words[1] = request;
        words[2] = request & PAD_MASK;
        words[3] = observed_ns;
        words[4] = frame;
        words[5] = frames as u64;
        Self(words)
    }

    fn projection(self) -> AcceptedTimingProjection {
        let mut revision = [0; 32];
        for (chunk, word) in revision.chunks_exact_mut(8).zip(&self.0[15..19]) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        AcceptedTimingProjection {
            revision,
            period_seconds: f64::from_bits(self.0[12]),
            origin_seconds: f64::from_bits(self.0[13]),
            sample_rate_hz: self.0[14] as u32,
            publication_epoch: self.0[11],
        }
    }

    fn matches(self, binding: &input_runtime_binding::InputRuntimePadBinding) -> bool {
        self.0[0] == 1
            && binding.id as u64 == self.0[2]
            && binding.source_generation == self.0[9]
            && binding
                .ownership
                .binding_source_generation(binding.id, binding.binding)
                == Some(self.0[9])
            && binding.binding.source_address as u64 == self.0[6]
            && binding.binding.sample_count as u64 == self.0[7]
            && binding.binding.channels as u64 == self.0[8]
            && binding.binding.sample_rate_hz as u64 == self.0[14]
            && binding.binding.authority_revision == self.0[10]
            && binding.binding.accepted == Some(self.projection())
            && binding.current()
    }
}

/// One outstanding pad request; a newer request supersedes an older observation.
pub(crate) struct SharedLoopAcceptance {
    demand: AtomicU64,
    serviced: AtomicU64,
    sequence: AtomicU64,
    fields: [AtomicU64; WORDS],
}

impl Default for SharedLoopAcceptance {
    fn default() -> Self {
        Self {
            demand: AtomicU64::new(0),
            serviced: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
            fields: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl SharedLoopAcceptance {
    pub(crate) fn request(&self, id: usize) -> PyResult<u64> {
        if id >= NUM_SAMPLES {
            return Err(PyValueError::new_err("pad id out of range"));
        }
        self.demand
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |previous| {
                (previous >> PAD_BITS)
                    .checked_add(1)
                    .filter(|next| *next <= u64::MAX >> PAD_BITS)
                    .map(|next| (next << PAD_BITS) | id as u64)
            })
            .map(|previous| (((previous >> PAD_BITS) + 1) << PAD_BITS) | id as u64)
            .map_err(|_| PyRuntimeError::new_err("loop observation request identity exhausted"))
    }

    pub(crate) fn reset(&self) {
        self.serviced
            .store(self.demand.load(Ordering::SeqCst), Ordering::SeqCst);
        self.sequence.store(0, Ordering::SeqCst);
    }

    /// Constant inactive cost; active work scans only the existing bounded voice array.
    pub(crate) fn observe(
        &self,
        mixer: &RtMixer,
        observed_ns: u64,
        next_output_frame: u64,
        callback_frames: usize,
    ) {
        let request = self.demand.load(Ordering::SeqCst);
        if request == self.serviced.load(Ordering::SeqCst) {
            return;
        }
        let snapshot = mixer.loop_acceptance_snapshot(
            request,
            observed_ns,
            next_output_frame,
            callback_frames,
        );
        self.publish(snapshot);
        self.serviced.store(request, Ordering::SeqCst);
    }

    fn publish(&self, snapshot: LoopAcceptanceSnapshot) {
        let sequence = self.sequence.load(Ordering::SeqCst).wrapping_add(1) | 1;
        self.sequence.store(sequence, Ordering::SeqCst);
        for (field, value) in self.fields.iter().zip(snapshot.0) {
            field.store(value, Ordering::SeqCst);
        }
        self.sequence
            .store(sequence.wrapping_add(1), Ordering::SeqCst);
    }

    fn read(&self, id: usize, request: u64) -> Option<LoopAcceptanceSnapshot> {
        let before = self.sequence.load(Ordering::SeqCst);
        if before == 0 || before & 1 != 0 || self.demand.load(Ordering::SeqCst) != request {
            return None;
        }
        let snapshot = LoopAcceptanceSnapshot(
            self.fields
                .each_ref()
                .map(|field| field.load(Ordering::SeqCst)),
        );
        (self.sequence.load(Ordering::SeqCst) == before
            && self.demand.load(Ordering::SeqCst) == request
            && snapshot.0[1] == request
            && snapshot.0[2] == id as u64)
            .then_some(snapshot)
    }
}

enum ScalarKind {
    Integer,
    Boolean,
    Float,
    OptionalFloat,
}

const SCALARS: [(usize, &str, ScalarKind); 41] = [
    (3, "callback_observed_at_ns", ScalarKind::Integer),
    (4, "output_frame", ScalarKind::Integer),
    (5, "callback_frames", ScalarKind::Integer),
    (7, "sample_count", ScalarKind::Integer),
    (8, "channels", ScalarKind::Integer),
    (9, "source_generation", ScalarKind::Integer),
    (10, "authority_revision", ScalarKind::Integer),
    (11, "publication_epoch", ScalarKind::Integer),
    (14, "loaded_sample_rate_hz", ScalarKind::Integer),
    (19, "voice_slot", ScalarKind::Integer),
    (20, "paused", ScalarKind::Boolean),
    (21, "source_frame", ScalarKind::Integer),
    (22, "source_fraction", ScalarKind::Float),
    (23, "physical_loop_start_frame", ScalarKind::Integer),
    (24, "physical_loop_end_frame", ScalarKind::Integer),
    (25, "musical_loop_period_frames", ScalarKind::OptionalFloat),
    (26, "applied_source_rate", ScalarKind::Float),
    (27, "target_source_rate", ScalarKind::Float),
    (28, "key_lock_requested", ScalarKind::Boolean),
    (29, "key_lock_native_active", ScalarKind::Boolean),
    (30, "native_pitch_scale", ScalarKind::Float),
    (31, "stem_all_requested", ScalarKind::Boolean),
    (32, "stem_source_version_hash", ScalarKind::Integer),
    (33, "stem_enabled_mask", ScalarKind::Integer),
    (34, "prepared_stems_current", ScalarKind::Boolean),
    (35, "stem_transition_active", ScalarKind::Boolean),
    (42, "applied_pad_gain_linear", ScalarKind::Float),
    (43, "target_pad_gain_linear", ScalarKind::Float),
    (44, "master_volume", ScalarKind::Float),
    (45, "voice_volume", ScalarKind::Float),
    (46, "bpm_lock", ScalarKind::Boolean),
    (47, "master_period_seconds", ScalarKind::OptionalFloat),
    (48, "native_input_fifo_frames", ScalarKind::Integer),
    (49, "native_output_fifo_frames", ScalarKind::Integer),
    (50, "native_block_frames", ScalarKind::Integer),
    (
        12,
        "effective_period_seconds_per_quarter",
        ScalarKind::Float,
    ),
    (13, "effective_origin_seconds", ScalarKind::Float),
    (2, "pad_id", ScalarKind::Integer),
    (51, "stem_all_applied", ScalarKind::Boolean),
    (52, "source_seek_mode", ScalarKind::Integer),
    (53, "rate_settled", ScalarKind::Boolean),
];

pub(super) fn metadata(
    engine: &AudioEngine,
    py: Python<'_>,
    id: usize,
    request: u64,
) -> PyResult<Option<Py<PyAny>>> {
    if id >= NUM_SAMPLES || request >> PAD_BITS == 0 || request & PAD_MASK != id as u64 {
        return Err(PyValueError::new_err(
            "invalid pad/request observation identity",
        ));
    }
    let Some(snapshot) = engine.loop_acceptance.read(id, request) else {
        return Ok(None);
    };
    let binding = input_runtime_binding::capture(engine, id)?;
    let now = engine.input_clock.capture_ns();
    let snapshot_age = now.checked_sub(snapshot.0[3]);
    let freshness_ns =
        (snapshot.0[5].saturating_mul(4_000_000_000) / snapshot.0[14].max(1)).max(100_000_000);
    let fresh = snapshot_age.is_some_and(|age| age <= freshness_ns);
    let current = fresh
        && binding
            .as_ref()
            .is_some_and(|value| snapshot.matches(value));
    let dict = PyDict::new(py);
    dict.set_item("schema", "productive-loop-snapshot-v1")?;
    dict.set_item("request_id", request)?;
    dict.set_item("current_acknowledged", current)?;
    dict.set_item("fresh", fresh)?;
    dict.set_item("snapshot_age_ns", snapshot_age)?;
    dict.set_item("status", if current { "available" } else { "unavailable" })?;
    dict.set_item("boundary", "next-output-frame-after-callback")?;
    dict.set_item(
        "reason",
        match snapshot.0[0] {
            0 => "no effective voice",
            2 => "multiple effective voices for pad",
            3 => "effective voice has no current accepted source",
            _ if !fresh => "effective voice observation is stale",
            _ if !current => "effective voice differs from current source/authority/accepted ACK",
            _ => "current source and effective accepted voice agree",
        },
    )?;
    dict.set_item(
        "current_binding",
        if current {
            binding
                .as_ref()
                .map(|value| value.metadata(py))
                .transpose()?
        } else {
            None
        },
    )?;
    if snapshot.0[0] == 1 {
        let projection = snapshot.projection();
        let revision = projection
            .revision
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        dict.set_item(
            "effective_accepted_revision",
            format!("accepted-constant-timing-v1:{revision}"),
        )?;
        for (index, key, kind) in SCALARS {
            let word = snapshot.0[index];
            match kind {
                ScalarKind::Integer => dict.set_item(key, word)?,
                ScalarKind::Boolean => dict.set_item(key, word != 0)?,
                ScalarKind::Float => dict.set_item(key, f64::from_bits(word))?,
                ScalarKind::OptionalFloat => {
                    let value = f64::from_bits(word);
                    dict.set_item(key, value.is_finite().then_some(value))?;
                }
            }
        }
        dict.set_item(
            "eq_applied_normalized",
            snapshot.0[36..39]
                .iter()
                .map(|word| f64::from_bits(*word))
                .collect::<Vec<_>>(),
        )?;
        dict.set_item(
            "eq_target_normalized",
            snapshot.0[39..42]
                .iter()
                .map(|word| f64::from_bits(*word))
                .collect::<Vec<_>>(),
        )?;
    }
    // Conversion may overlap a callback adoption or a successful control revocation.
    if current
        && !binding
            .as_ref()
            .is_some_and(|value| snapshot.matches(value))
    {
        return Ok(None);
    }
    if engine.loop_acceptance.demand.load(Ordering::SeqCst) != request {
        return Ok(None);
    }
    Ok(Some(dict.into_any().unbind()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demand_observation_is_single_attempt_superseded_and_session_fenced() {
        let shared = SharedLoopAcceptance::default();
        let first = shared.request(0).unwrap();
        assert_eq!(shared.read(0, first), None);
        shared.publish(LoopAcceptanceSnapshot::empty(first, 10, 512, 512));
        assert!(shared.read(0, first).is_some());
        let second = shared.request(1).unwrap();
        assert!(second > first);
        assert_eq!(shared.read(0, first), None);
        shared.publish(LoopAcceptanceSnapshot::empty(second, 20, 1024, 512));
        assert!(shared.read(1, second).is_some());
        shared.sequence.store(3, Ordering::SeqCst);
        assert_eq!(shared.read(1, second), None);
        shared.reset();
        assert_eq!(shared.read(1, second), None);
        assert!(shared.request(NUM_SAMPLES).is_err());
        shared.demand.store(u64::MAX, Ordering::SeqCst);
        assert!(shared.request(0).is_err());
    }

    #[test]
    fn no_voice_response_is_observed_only_on_demand_without_audio_device() {
        let shared = SharedLoopAcceptance::default();
        let mixer = RtMixer::new(1, 8_000.0);
        shared.observe(&mixer, 10, 512, 512);
        assert_eq!(shared.sequence.load(Ordering::SeqCst), 0);
        let request = shared.request(0).unwrap();
        shared.observe(&mixer, 10, 512, 512);
        let snapshot = shared.read(0, request).unwrap();
        assert_eq!(snapshot.0[0], 0);
        assert_eq!(snapshot.0[4], 512);
        let sequence = shared.sequence.load(Ordering::SeqCst);
        shared.observe(&mixer, 20, 1024, 512);
        assert_eq!(shared.sequence.load(Ordering::SeqCst), sequence);
    }
}
