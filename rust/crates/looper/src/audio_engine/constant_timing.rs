//! Explicit accepted timing owned by current native pad state, outside realtime work.

use super::analysis_pcm::{LoadedPcmSnapshot, PcmIdentity, resample_mono_cancellable};
use super::constants::NUM_SAMPLES;
use super::content_placement::{ContentKey, PlacementError, PlacementObservation, SlotSnapshot};
use super::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
use super::prepared_source::{PreparedSourcePermit, next_epoch};
use super::{AudioEngine, PadRequestAdvance};
use crate::messages::{ControlMessage, ControlParameterMessage, SampleBuffer};
use flitzis_looper_analysis::{
    AnalysisConfig, analyze_bpm_raw,
    tempo_acceptance::{
        AcceptedConstantTiming, IndependentTimingOrigin, TimingAcceptanceDecision,
        TimingAdoptionGuard, TimingAdoptionTicket, TimingIntent,
    },
    tempo_evidence::{
        BackendEvidence, BoundTempoEvidence, JobIdentity, MONO_REVISION, PcmBinding,
        PcmBindingMetadata, QmInputDescriptor, QmInputTransform, TimingBound, f32_pcm_sha256,
        f64_input_sha256,
    },
    tempo_summary::{MAX_HYPOTHESES, QuarterNoteHypothesis, QuarterNoteVerification},
};
use pyo3::exceptions::{PyBlockingIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rtrb::Producer;
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub(super) const MAX_PCM_BYTES: usize = 512 * 1024 * 1024;
const MAX_HYPOTHESIS_JSON_BYTES: usize = 32 * 1024 * 1024;
const SOURCE_PROVENANCE: &str = "native-loader-sha256-before-decode-and-project-copy-check-v1; loaded Arc association; immutable copy-first decode ABA is not proved";

mod copy_binding;
mod pcm_budget;
mod persistence;
pub(super) use copy_binding::rebind_copy_timing;
use pcm_budget::PcmBudget;
pub use persistence::SavedConstantTimingTicket;
#[cfg(test)]
pub(super) use persistence::capture_saved_with_measurement_limit;
#[cfg(test)]
pub(super) use persistence::migration_test_fixture;
pub(super) use persistence::{SavedTimingExportWork, SavedTimingRestoreWork, capture_saved};
#[cfg(test)]
pub(super) use persistence::{export_current, restore_saved};

pub(super) fn validated_pcm_limit_bytes(limit_bytes: usize) -> Result<usize, String> {
    PcmBudget::new(limit_bytes).map(PcmBudget::limit_bytes)
}

/// Fixed callback feedback. Publication epochs identify retained native records;
/// zero means no accepted projection is effective in the current mixer bank.
/// These slots never own evidence or source buffers and polling cannot change them.
pub(crate) struct CurrentTimingAcknowledgements {
    epochs: [AtomicU64; NUM_SAMPLES],
    revoked_through: [AtomicU64; NUM_SAMPLES],
    acknowledged_lifetimes: [AtomicU64; NUM_SAMPLES],
    revoked_lifetimes: [AtomicU64; NUM_SAMPLES],
}

impl Default for CurrentTimingAcknowledgements {
    fn default() -> Self {
        Self {
            epochs: std::array::from_fn(|_| AtomicU64::new(0)),
            revoked_through: std::array::from_fn(|_| AtomicU64::new(0)),
            acknowledged_lifetimes: std::array::from_fn(|_| AtomicU64::new(0)),
            revoked_lifetimes: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl CurrentTimingAcknowledgements {
    /// The one callback writer publishes only the captured content incarnation.
    /// Revocation has its own lifetime tag, so initializing a later ACK cannot
    /// erase an already queued explicit revocation for that same new content.
    pub(crate) fn acknowledge_for_key(
        &self,
        ownership: &InputRuntimeOwnership,
        key: ContentKey,
        publication_epoch: u64,
    ) -> bool {
        if !ownership.content_key_current(key) || publication_epoch == 0 {
            return false;
        }
        let cell = key.cell().index();
        self.acknowledged_lifetimes[cell].store(0, Ordering::SeqCst);
        self.epochs[cell].store(publication_epoch, Ordering::SeqCst);
        self.acknowledged_lifetimes[cell].store(key.lifetime(), Ordering::SeqCst);
        ownership.content_key_current(key)
    }

    /// Fixed content lookup, independent of a pending placement publication.
    pub(crate) fn current_for_key(
        &self,
        ownership: &InputRuntimeOwnership,
        key: ContentKey,
    ) -> u64 {
        let cell = key.cell().index();
        if !ownership.content_key_current(key)
            || self.acknowledged_lifetimes[cell].load(Ordering::SeqCst) != key.lifetime()
        {
            return 0;
        }
        let epoch = self.epochs[cell].load(Ordering::SeqCst);
        let revoked_lifetime = self.revoked_lifetimes[cell].load(Ordering::SeqCst);
        let through = self.revoked_through[cell].load(Ordering::SeqCst);
        if self.acknowledged_lifetimes[cell].load(Ordering::SeqCst) != key.lifetime()
            || self.revoked_lifetimes[cell].load(Ordering::SeqCst) != revoked_lifetime
            || !ownership.content_key_current(key)
            || (revoked_lifetime == key.lifetime() && epoch <= through)
        {
            0
        } else {
            epoch
        }
    }

    pub(crate) fn current_for_binding(
        &self,
        ownership: &InputRuntimeOwnership,
        id: usize,
        binding: InputPadBinding,
    ) -> u64 {
        if let Some(key) = binding
            .placement
            .and_then(|snapshot| snapshot.content_key())
        {
            self.current_for_key(ownership, key)
        } else if !ownership.is_source_tracked() {
            self.current_epoch(id)
        } else {
            0
        }
    }

    /// Control routing is resolved afresh; an odd layout publication grants no ACK.
    pub(crate) fn current_for_slot(&self, ownership: &InputRuntimeOwnership, id: usize) -> u64 {
        if !ownership.is_source_tracked() {
            return self.current_epoch(id);
        }
        ownership
            .slot_snapshot(id)
            .and_then(|snapshot| snapshot.content_key())
            .map_or(0, |key| self.current_for_key(ownership, key))
    }

    /// Callback clear of one captured incarnation, never a late numeric slot.
    pub(crate) fn clear_for_key(&self, ownership: &InputRuntimeOwnership, key: ContentKey) -> bool {
        if !ownership.content_key_current(key) {
            return false;
        }
        let cell = key.cell().index();
        if self.acknowledged_lifetimes[cell].load(Ordering::SeqCst) == key.lifetime() {
            self.epochs[cell].store(0, Ordering::SeqCst);
        }
        ownership.content_key_current(key)
    }

    /// Called under the existing request owner, which serializes source/lifetime
    /// changes and explicit revocation. The callback writes only ACK fields.
    #[cfg(test)]
    pub(crate) fn revoke_for_key(
        &self,
        ownership: &InputRuntimeOwnership,
        key: ContentKey,
        through_epoch: u64,
    ) -> bool {
        if !ownership.content_key_current(key) {
            return false;
        }
        self.revoke_captured_key(key, through_epoch);
        ownership.content_key_current(key)
    }

    /// Fixed stores after the caller has claimed this exact captured incarnation.
    /// The shared mutation marker prevents a lifetime replacement until publication.
    fn revoke_captured_key(&self, key: ContentKey, through_epoch: u64) {
        let cell = key.cell().index();
        let through = if self.revoked_lifetimes[cell].load(Ordering::SeqCst) == key.lifetime() {
            through_epoch.max(self.revoked_through[cell].load(Ordering::SeqCst))
        } else {
            through_epoch
        };
        self.revoked_lifetimes[cell].store(0, Ordering::SeqCst);
        self.revoked_through[cell].store(through, Ordering::SeqCst);
        self.revoked_lifetimes[cell].store(key.lifetime(), Ordering::SeqCst);
    }

    // Raw cell methods support untracked standalone mixers and their fixtures.
    // Real engine readers/writers use the lifetime-bound methods above.
    pub(crate) fn acknowledge(&self, id: usize, publication_epoch: u64) {
        self.epochs[id].store(publication_epoch, Ordering::Release);
    }

    pub(crate) fn clear(&self, id: usize) {
        self.epochs[id].store(0, Ordering::Release);
    }

    pub(crate) fn current_epoch(&self, id: usize) -> u64 {
        let epoch = self.epochs[id].load(Ordering::Acquire);
        if epoch <= self.revoked_through[id].load(Ordering::Acquire) {
            0
        } else {
            epoch
        }
    }

    pub(super) fn clear_all(&self) {
        for id in 0..NUM_SAMPLES {
            self.clear(id);
        }
    }
}

/// Small non-realtime record with no PCM allocation owner. The address is only
/// compared, never dereferenced; current monotonic generation/digest/rate and
/// extent checks prevent a recycled address from authorizing another source.
/// The complete immutable accepted evidence is retained independently of tickets.
#[derive(Clone)]
pub(super) struct CurrentConstantTimingRecord {
    pub(super) content_key: Option<ContentKey>,
    ownership: Arc<InputRuntimeOwnership>,
    source_address: usize,
    sample_count: usize,
    channels: usize,
    binding: PcmBindingMetadata,
    revision: String,
    raw_revision: String,
    period_seconds: f64,
    origin: IndependentTimingOrigin,
    decision: TimingAcceptanceDecision,
    publication_epoch: u64,
    publication: PreparedSourcePermit,
    pub(super) projection: AcceptedTimingProjection,
    // Complete accepted evidence must survive callers dropping opaque tickets.
    // This owner is retained/destroyed exclusively outside realtime processing.
    accepted: Arc<AcceptedConstantTiming>,
    pcm_budget: PcmBudget,
}

impl CurrentConstantTimingRecord {
    fn new(
        timing: &AcceptedConstantTiming,
        ticket: &ConstantTimingTicket,
        publication_epoch: u64,
        publication: PreparedSourcePermit,
        projection: AcceptedTimingProjection,
    ) -> Self {
        Self {
            content_key: ticket.placement.and_then(|snapshot| snapshot.content_key()),
            ownership: ticket.ownership.clone(),
            source_address: ticket.sample.source_address(),
            sample_count: ticket.sample.source_sample_count(),
            channels: ticket.sample.channels,
            binding: ticket.binding.clone(),
            revision: timing.revision().to_owned(),
            raw_revision: timing.evidence().source_identity().raw_revision.clone(),
            period_seconds: timing.period_seconds_per_quarter(),
            origin: timing.origin().clone(),
            decision: timing.decision().clone(),
            publication_epoch,
            publication,
            projection,
            accepted: Arc::new(timing.clone()),
            pcm_budget: ticket.pcm_budget,
        }
    }

    fn acknowledged_epoch(&self, engine: &AudioEngine) -> u64 {
        if !Arc::ptr_eq(&self.ownership, &engine.input_runtime_ownership) {
            return 0;
        }
        self.content_key.map_or(0, |key| {
            engine
                .current_timing_acknowledgements
                .current_for_key(&self.ownership, key)
        })
    }

    fn occupies_slot(&self, engine: &AudioEngine, id: usize) -> bool {
        Arc::ptr_eq(&self.ownership, &engine.input_runtime_ownership)
            && self.content_key.is_some_and(|key| {
                engine
                    .input_runtime_ownership
                    .slot_snapshot(id)
                    .and_then(|snapshot| snapshot.content_key())
                    == Some(key)
            })
    }

    /// Project current placement without rewriting the immutable accepted evidence.
    pub(super) fn metadata_for_slot(&self, py: Python<'_>, id: usize) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("pad_id", id)?;
        dict.set_item("pcm_limit_bytes", self.pcm_budget.limit_bytes())?;
        dict.set_item(
            "source_id",
            format!("loaded-{id}-{}", self.binding.job.source_generation),
        )?;
        dict.set_item("source_generation", self.binding.job.source_generation)?;
        dict.set_item("accepted_request_id", self.binding.job.request_id)?;
        dict.set_item("publication_epoch", self.publication_epoch)?;
        dict.set_item("source_sha256", &self.binding.source_sha256)?;
        dict.set_item("source_provenance", &self.binding.source_provenance)?;
        dict.set_item("pcm_sha256", &self.binding.pcm_sha256)?;
        dict.set_item("sample_rate_hz", self.binding.sample_rate_hz)?;
        dict.set_item("frame_count", self.binding.frame_count)?;
        dict.set_item("source_zero_seconds", self.binding.origin_seconds)?;
        dict.set_item("mono_revision", &self.binding.mono_revision)?;
        dict.set_item("raw_revision", &self.raw_revision)?;
        dict.set_item("revision", &self.revision)?;
        dict.set_item("period_seconds_per_quarter", self.period_seconds)?;
        dict.set_item("origin_seconds", self.origin.seconds)?;
        dict.set_item("origin_provenance", &self.origin.provenance)?;
        dict.set_item("acceptance_policy_version", &self.decision.policy_version)?;
        dict.set_item("acceptance_provenance", &self.decision.provenance)?;
        Ok(dict.into_any().unbind())
    }
}

/// Terminal status is acquired before reading current acknowledgement: the mixer
/// publishes its epoch before marking accepted, so a terminal non-current record
/// cannot become current later. Pending records survive this off-RT retirement.
pub(super) fn retire_old_current_records(
    engine: &AudioEngine,
    records: &mut Vec<CurrentConstantTimingRecord>,
) {
    records.retain(|record| {
        let status = record.publication.status();
        status == "pending"
            || (status == "accepted"
                && record.acknowledged_epoch(engine) == record.publication_epoch)
    });
}

/// Resolve actual current source and callback acknowledgement, never a caller
/// snapshot or the historical accepted flag. New preparation requests do not
/// revoke the effective projection; source and explicit authority edits do.
pub(super) fn current_metadata(
    engine: &AudioEngine,
    py: Python<'_>,
    id: usize,
) -> PyResult<Option<Py<PyAny>>> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let intents = engine
        .timing_intents
        .lock()
        .map_err(|_| PyRuntimeError::new_err("timing intent lock poisoned"))?;
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let generations = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
    let digests = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
    let mut all_records = engine
        .current_constant_timing
        .lock()
        .map_err(|_| PyRuntimeError::new_err("current timing lock poisoned"))?;
    let records = &mut all_records[id];
    retire_old_current_records(engine, records);
    if intents[id] != TimingIntent::Automatic {
        return Ok(None);
    }
    let Some(source) = cache[id].as_ref() else {
        return Ok(None);
    };
    // A changed callback epoch during lookup yields no accepted claim from this
    // poll. The next poll observes the new acknowledged record; audio progresses
    // independently. No spin/retry or callback lock is required.
    let acknowledged = engine
        .current_timing_acknowledgements
        .current_for_slot(&engine.input_runtime_ownership, id);
    let Some(record) = current_record_for_source(
        engine,
        id,
        source,
        generations[id],
        digests[id].as_deref(),
        records,
    ) else {
        return Ok(None);
    };
    let metadata = record.metadata_for_slot(py, id)?;
    if engine
        .current_timing_acknowledgements
        .current_for_slot(&engine.input_runtime_ownership, id)
        != acknowledged
    {
        return Ok(None);
    }
    Ok(Some(metadata))
}

/// Shared authoritative current source/acknowledgement predicate for control consumers.
pub(super) fn current_record_for_source<'a>(
    engine: &AudioEngine,
    id: usize,
    source: &SampleBuffer,
    generation_and_rate: (u64, u32),
    digest: Option<&str>,
    records: &'a [CurrentConstantTimingRecord],
) -> Option<&'a CurrentConstantTimingRecord> {
    let acknowledged = engine
        .current_timing_acknowledgements
        .current_for_slot(&engine.input_runtime_ownership, id);
    records.iter().find(|record| {
        record.occupies_slot(engine, id)
            && record.publication_epoch == acknowledged
            && record.channels == source.channels
            && record.source_address == source.source_address()
            && record.sample_count == source.source_sample_count()
            && generation_and_rate
                == (
                    record.binding.job.source_generation,
                    record.binding.sample_rate_hz,
                )
            && digest == Some(record.binding.source_sha256.as_str())
    })
}

/// Fixed complete accepted revision and precise scalar projection; no evidence enters RT.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AcceptedTimingProjection {
    pub(crate) revision: [u8; 32],
    pub(crate) period_seconds: f64,
    pub(crate) origin_seconds: f64,
    pub(crate) sample_rate_hz: u32,
    pub(crate) publication_epoch: u64,
}

impl PartialEq for AcceptedTimingProjection {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.period_seconds.to_bits() == other.period_seconds.to_bits()
            && self.origin_seconds.to_bits() == other.origin_seconds.to_bits()
            && self.sample_rate_hz == other.sample_rate_hz
            && self.publication_epoch == other.publication_epoch
    }
}

/// Whole transient source pin/permit retires off the callback after bounded adoption.
#[derive(Debug, Clone)]
pub(crate) struct PreparedConstantTiming {
    pub(crate) reference: SampleBuffer,
    pub(crate) publication: PreparedSourcePermit,
    pub(crate) projection: AcceptedTimingProjection,
    pub(crate) content_key: Option<ContentKey>,
}

/// Opaque native analysis and adoption ticket. Python cannot replace its evidence or identity.
#[pyclass(frozen)]
pub struct ConstantTimingTicket {
    id: usize,
    placement: Option<SlotSnapshot>,
    ownership: Arc<InputRuntimeOwnership>,
    request_id: u64,
    request_incarnation: Option<u64>,
    source_generation: u64,
    sample_rate_hz: u32,
    source_digest: String,
    sample: SampleBuffer,
    epoch: Arc<AtomicU64>,
    captured_epoch: u64,
    evidence: BoundTempoEvidence,
    binding: PcmBindingMetadata,
    guard: Mutex<TimingAdoptionGuard>,
    adoption_ticket: TimingAdoptionTicket,
    publication: Mutex<Option<PreparedSourcePermit>>,
    pcm_budget: PcmBudget,
}

#[pymethods]
impl ConstantTimingTicket {
    /// A bounded wait observation for this retained original lifetime. It grants
    /// no source or publication authority and never treats retirement as Pending.
    pub fn placement_pending(&self) -> bool {
        self.placement
            .and_then(|snapshot| snapshot.content_key())
            .is_some_and(|key| {
                self.ownership.content_key_current(key)
                    && matches!(
                        self.ownership.observe_control_key(key),
                        PlacementObservation::Pending
                    )
                    && self.ownership.content_key_current(key)
            })
    }

    /// Original Native source placement, never metadata-pad-id permission.
    pub fn current_control_slot(&self) -> Option<usize> {
        if !self.ownership.is_source_tracked() {
            return Some(self.id);
        }
        let key = self.placement?.content_key()?;
        match self.ownership.observe_control_key(key) {
            super::content_placement::PlacementObservation::Current(snapshot) => {
                Some(snapshot.slot())
            }
            _ => None,
        }
    }
    /// Complete native source/PCM/timebase and original QM arrays, outside realtime work.
    pub fn metadata(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let binding = &self.binding;
        let dict = PyDict::new(py);
        dict.set_item("pad_id", self.id)?;
        dict.set_item("pcm_limit_bytes", self.pcm_budget.limit_bytes())?;
        dict.set_item("request_id", self.request_id)?;
        dict.set_item("source_id", &binding.job.source_id)?;
        dict.set_item("source_generation", self.source_generation)?;
        dict.set_item("source_sha256", &binding.source_sha256)?;
        dict.set_item("source_provenance", &binding.source_provenance)?;
        dict.set_item("pcm_sha256", &binding.pcm_sha256)?;
        dict.set_item("sample_rate_hz", self.sample_rate_hz)?;
        dict.set_item("frame_count", binding.frame_count)?;
        dict.set_item("origin_seconds", binding.origin_seconds)?;
        dict.set_item("mono_revision", &binding.mono_revision)?;
        dict.set_item(
            "raw_revision",
            &self.evidence.source_identity().raw_revision,
        )?;
        dict.set_item("beat_seconds", self.evidence.beat_seconds())?;
        dict.set_item(
            "timing_error_halfwidth_seconds",
            self.evidence.timing_bound().halfwidth_seconds,
        )?;
        dict.set_item(
            "timing_error_provenance",
            &self.evidence.timing_bound().provenance,
        )?;
        if let BackendEvidence::Qm { raw, input } = self.evidence.backend() {
            dict.set_item("beat_frames", raw.beat_frames())?;
            dict.set_item("downbeat_raw_indices", raw.downbeat_raw_indices())?;
            dict.set_item(
                "downbeat_seconds",
                raw.downbeat_seconds().collect::<Vec<_>>(),
            )?;
            dict.set_item("analyzer_sample_rate_hz", input.sample_rate_hz)?;
            dict.set_item("analyzer_frame_count", input.frame_count)?;
            dict.set_item("analyzer_input_sha256", &input.input_sha256)?;
            dict.set_item("odf_hop_samples", raw.odf_hop_samples())?;
            let configuration = PyDict::new(py);
            let config = raw.configuration();
            configuration.set_item("step_secs", config.step_secs)?;
            configuration.set_item("max_bin_hz", config.max_bin_hz)?;
            configuration.set_item("input_tempo", config.input_tempo)?;
            configuration.set_item("alpha", config.alpha)?;
            configuration.set_item("tightness", config.tightness)?;
            configuration.set_item("viterbi_sigma", config.viterbi_sigma)?;
            configuration.set_item("window_length", config.window_length)?;
            configuration.set_item("hop_size", config.hop_size)?;
            dict.set_item("requested_configuration", configuration)?;
            let transform = PyDict::new(py);
            match &input.transform {
                QmInputTransform::Identity => transform.set_item("kind", "identity")?,
                QmInputTransform::Rubato44100 {
                    revision,
                    provenance,
                } => {
                    transform.set_item("kind", "rubato44100")?;
                    transform.set_item("revision", revision)?;
                    transform.set_item("provenance", provenance)?;
                }
            }
            dict.set_item("input_transform", transform)?;
        }
        Ok(dict.into_any().unbind())
    }

    /// Historical callback acknowledgement, independent of Python polling cadence.
    pub fn publication_status(&self) -> PyResult<&'static str> {
        let publication = self
            .publication
            .lock()
            .map_err(|_| PyRuntimeError::new_err("timing publication lock poisoned"))?;
        Ok(publication
            .as_ref()
            .map_or("captured", PreparedSourcePermit::status))
    }

    /// Return this record only after actual callback acceptance; this is historical evidence.
    pub fn accepted_metadata(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        if self.publication_status()? != "accepted" {
            return Ok(None);
        }
        let guard = self
            .guard
            .lock()
            .map_err(|_| PyRuntimeError::new_err("timing guard lock poisoned"))?;
        let timing = guard
            .accepted()
            .ok_or_else(|| PyRuntimeError::new_err("accepted timing record unavailable"))?;
        let dict = PyDict::new(py);
        dict.set_item("revision", timing.revision())?;
        dict.set_item(
            "period_seconds_per_quarter",
            timing.period_seconds_per_quarter(),
        )?;
        dict.set_item("origin_seconds", timing.origin().seconds)?;
        dict.set_item("origin_provenance", &timing.origin().provenance)?;
        dict.set_item(
            "acceptance_policy_version",
            &timing.decision().policy_version,
        )?;
        dict.set_item("acceptance_provenance", &timing.decision().provenance)?;
        Ok(Some(dict.into_any().unbind()))
    }
}

struct PreparationLease(Arc<AtomicBool>);
impl PreparationLease {
    fn capture(engine: &AudioEngine) -> Result<Self, String> {
        if engine
            .constant_timing_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("constant timing preparation busy".into());
        }
        Ok(Self(engine.constant_timing_busy.clone()))
    }
}
impl Drop for PreparationLease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Full-source PCM is a bounded temporary analysis lease. Opaque tickets keep
/// only their captured resident reference; accepting evidence never pins a
/// complete playback allocation merely because this verifier needed it.
fn timing_resident_reference_bytes(reference: &SampleBuffer) -> Result<usize, String> {
    if reference.resident_start() == 0 && reference.resident_end() == reference.frame_count() {
        Ok(0)
    } else {
        reference
            .samples
            .len()
            .checked_mul(size_of::<f32>())
            .ok_or_else(|| "timing resident-reference byte count overflow".into())
    }
}

fn check_timing_pcm_geometry(
    reference: &SampleBuffer,
    rate: u32,
    budget: PcmBudget,
) -> Result<(), String> {
    let retained = timing_resident_reference_bytes(reference)?;
    let available = budget
        .limit_bytes()
        .checked_sub(retained)
        .filter(|bytes| *bytes > 0)
        .ok_or("constant timing PCM byte limit exceeded")?;
    PcmBudget::new(available)?.check_loaded_geometry(
        reference.source_sample_count(),
        reference.channels,
        rate,
    )?;
    if retained != 0
        && reference
            .source_sample_count()
            .checked_mul(2 * size_of::<f32>())
            .is_none_or(|bytes| bytes > available)
    {
        return Err("constant timing PCM byte limit exceeded".into());
    }
    Ok(())
}

fn complete_timing_sample(
    work: &super::native_owned_work::CompleteSampleWork,
    reference: &SampleBuffer,
    budget: PcmBudget,
) -> Result<(SampleBuffer, usize), String> {
    let retained_reference_bytes = timing_resident_reference_bytes(reference)?;
    let complete = work.materialize(budget.limit_bytes())?;
    if !complete.same_source(reference)
        || complete.resident_start() != 0
        || complete.resident_end() != reference.frame_count()
    {
        return Err("complete timing PCM lease differs from captured source".into());
    }
    Ok((complete, retained_reference_bytes))
}

pub(super) fn parse_intent(value: &str) -> PyResult<TimingIntent> {
    match value {
        "automatic" => Ok(TimingIntent::Automatic),
        "manual" => Ok(TimingIntent::Manual),
        "tap" => Ok(TimingIntent::Tap),
        "legacy" => Ok(TimingIntent::Legacy),
        _ => Err(PyValueError::new_err(
            "intent must be automatic, manual, tap or legacy",
        )),
    }
}

/// Opaque synchronous source/request capture for off-thread native preparation.
#[pyclass(frozen)]
#[derive(Clone)]
pub struct CapturedConstantTiming {
    id: usize,
    placement: Option<SlotSnapshot>,
    ownership: Arc<InputRuntimeOwnership>,
    sample: SampleBuffer,
    source_generation: u64,
    sample_rate_hz: u32,
    source_digest: String,
    request_id: u64,
    request_incarnation: Option<u64>,
    captured_epoch: u64,
    epoch: Arc<AtomicU64>,
    timing_bound: TimingBound,
    pcm_budget: PcmBudget,
}

#[cfg(test)]
pub(super) fn prepare(
    engine: &AudioEngine,
    id: usize,
    timing_bound: TimingBound,
) -> Result<ConstantTimingTicket, String> {
    ConstantTimingPreparationWork::capture(engine, id, timing_bound)?
        .prepare()?
        .finish(engine)
}

/// Capture all actual native owners under their common request publication mutex.
pub(super) fn capture_preparation(
    engine: &AudioEngine,
    id: usize,
    timing_bound: TimingBound,
    expected: Option<&super::input_runtime_binding::InputRuntimePadBinding>,
) -> Result<CapturedConstantTiming, String> {
    capture_preparation_with_limit(engine, id, timing_bound, expected, MAX_PCM_BYTES)
}

pub(super) fn capture_preparation_with_limit(
    engine: &AudioEngine,
    id: usize,
    timing_bound: TimingBound,
    expected: Option<&super::input_runtime_binding::InputRuntimePadBinding>,
    pcm_limit_bytes: usize,
) -> Result<CapturedConstantTiming, String> {
    let pcm_budget = PcmBudget::new(pcm_limit_bytes)?;
    if id >= NUM_SAMPLES {
        return Err("id out of range".into());
    }
    if !timing_bound.halfwidth_seconds.is_finite()
        || timing_bound.halfwidth_seconds < 0.0
        || timing_bound.provenance.trim().is_empty()
        || timing_bound.provenance.len() > 4096
    {
        return Err("invalid explicit timing error declaration".into());
    }
    let (
        sample,
        source_generation,
        sample_rate_hz,
        source_digest,
        request_id,
        captured_epoch,
        placement,
        epoch,
    ) = {
        let mut requests = engine
            .pad_request_ids
            .lock()
            .map_err(|_| "request lock poisoned")?;
        if let Some(expected) = expected {
            let current = super::input_runtime_binding::capture_under_request_lock(engine, id)
                .map_err(|error| error.to_string())?
                .ok_or("constant timing preparation source unavailable")?;
            if expected.id != id
                || !Arc::ptr_eq(&expected.ownership, &engine.input_runtime_ownership)
                || !expected.current()
                || !expected.binding.same_projection(&current.binding)
                || !current.current()
            {
                return Err("stale or foreign constant timing preparation binding".into());
            }
        }
        let intents = engine
            .timing_intents
            .lock()
            .map_err(|_| "timing intent lock poisoned")?;
        if intents[id] != TimingIntent::Automatic {
            return Err("current timing intent is not automatic".into());
        }
        if engine
            .loading_sample_ids
            .lock()
            .map_err(|_| "loading lock poisoned")?
            .contains(&id)
        {
            return Err("sample is currently loading".into());
        }
        if super::has_active_task_for_id(
            &*engine
                .active_tasks
                .lock()
                .map_err(|_| "task lock poisoned")?,
            engine.active_task_cell(id)?,
        ) {
            return Err("sample task already running".into());
        }
        let cache = engine
            .sample_cache
            .lock()
            .map_err(|_| "sample cache lock poisoned")?;
        let sample = cache[id].clone().ok_or("sample is not loaded")?;
        let generations = engine
            .loaded_source_generations
            .lock()
            .map_err(|_| "source generation lock poisoned")?;
        let (source_generation, sample_rate_hz) = generations[id];
        let digests = engine
            .loaded_source_digests
            .lock()
            .map_err(|_| "source digest lock poisoned")?;
        let digest = digests[id]
            .clone()
            .ok_or("loaded source digest unavailable")?;
        if source_generation == 0 || sample_rate_hz == 0 {
            return Err("loaded source identity unavailable".into());
        }
        let frames = sample.frame_count();
        check_timing_pcm_geometry(&sample, sample_rate_hz, pcm_budget)?;
        // Geometry admission uses the complete source extent. Reading complete
        // PCM belongs to the detached preparation worker, after this capture.
        if timing_bound.halfwidth_seconds > frames as f64 / f64::from(sample_rate_hz) {
            return Err("timing error exceeds source duration".into());
        }
        super::complete_context::CompleteSourceGuard::capture(
            engine,
            id,
            &sample,
            source_generation,
            sample_rate_hz,
        )?;
        let placement = engine.input_runtime_ownership.slot_snapshot(id);
        if engine.input_runtime_ownership.is_source_tracked()
            && placement
                .and_then(|snapshot| snapshot.content_key())
                .is_none()
        {
            return Err("constant timing source has no current content lifetime".into());
        }
        let epoch = engine.prepared_source_epoch(id)?.clone();
        let advance = PadRequestAdvance::prepare(&mut requests[id], &epoch)?;
        let logical_advance = engine
            .input_runtime_ownership
            .prepare_source_request_advance(id)?;
        logical_advance.commit();
        let request_id = advance.commit();
        (
            sample,
            source_generation,
            sample_rate_hz,
            digest,
            request_id,
            epoch.load(Ordering::Acquire),
            placement,
            epoch,
        )
    };
    engine.offline_jobs.cancel(Some(id));
    Ok(CapturedConstantTiming {
        id,
        placement,
        ownership: engine.input_runtime_ownership.clone(),
        sample,
        source_generation,
        sample_rate_hz,
        source_digest,
        request_id,
        request_incarnation: timing_request_incarnation(&engine.input_runtime_ownership, placement),
        captured_epoch,
        epoch,
        timing_bound,
        pcm_budget,
    })
}

#[cfg(test)]
pub(super) fn prepare_captured(
    engine: &AudioEngine,
    captured: &CapturedConstantTiming,
) -> Result<ConstantTimingTicket, String> {
    ConstantTimingPreparationWork::capture_existing(engine, captured)?
        .prepare()?
        .finish(engine)
}

pub(super) struct ConstantTimingPreparationWork {
    captured: CapturedConstantTiming,
    source: super::native_owned_work::CompleteSampleWork,
    lease: PreparationLease,
}

pub(super) struct PreparedTimingEvidence {
    ticket: ConstantTimingTicket,
    _lease: PreparationLease,
}

impl ConstantTimingPreparationWork {
    pub(super) fn capture(
        engine: &AudioEngine,
        id: usize,
        timing_bound: TimingBound,
    ) -> Result<Self, String> {
        let lease = PreparationLease::capture(engine)?;
        let captured = capture_preparation(engine, id, timing_bound, None)?;
        let source =
            super::native_owned_work::CompleteSampleWork::capture(engine, id, &captured.sample)?;
        Ok(Self {
            captured,
            source,
            lease,
        })
    }

    pub(super) fn capture_existing(
        engine: &AudioEngine,
        captured: &CapturedConstantTiming,
    ) -> Result<Self, String> {
        let (id, _) =
            resolve_timing_owner(engine, captured.placement, &captured.ownership, captured.id)?;
        if !Arc::ptr_eq(&captured.ownership, &engine.input_runtime_ownership)
            || !Arc::ptr_eq(&captured.epoch, engine.prepared_source_epoch(id)?)
            || captured.epoch.load(Ordering::Acquire) != captured.captured_epoch
            || timing_request_incarnation(&captured.ownership, captured.placement)
                != captured.request_incarnation
        {
            return Err("stale or foreign captured constant timing preparation".into());
        }
        let lease = PreparationLease::capture(engine)?;
        let source =
            super::native_owned_work::CompleteSampleWork::capture(engine, id, &captured.sample)?;
        Ok(Self {
            captured: captured.clone(),
            source,
            lease,
        })
    }

    pub(super) fn prepare(self) -> Result<PreparedTimingEvidence, String> {
        let ticket = prepare_captured_inner(&self.captured, &self.source)?;
        Ok(PreparedTimingEvidence {
            ticket,
            _lease: self.lease,
        })
    }
}

impl PreparedTimingEvidence {
    pub(super) fn finish(self, engine: &AudioEngine) -> Result<ConstantTimingTicket, String> {
        let requests = engine
            .pad_request_ids
            .lock()
            .map_err(|_| "request lock poisoned")?;
        validate_evidence_admission(engine, &self.ticket, &requests)?;
        Ok(self.ticket)
    }
}

fn prepare_captured_inner(
    captured: &CapturedConstantTiming,
    source: &super::native_owned_work::CompleteSampleWork,
) -> Result<ConstantTimingTicket, String> {
    let id = captured.id;
    let sample = captured.sample.clone();
    let source_generation = captured.source_generation;
    let sample_rate_hz = captured.sample_rate_hz;
    let source_digest = captured.source_digest.clone();
    let request_id = captured.request_id;
    let captured_epoch = captured.captured_epoch;
    let timing_bound = captured.timing_bound.clone();
    let epoch = captured.epoch.clone();
    let cancelled = || epoch.load(Ordering::Acquire) != captured_epoch;
    let pcm_limit_bytes = captured.pcm_budget.limit_bytes();
    if cancelled() {
        return Err("constant timing preparation cancelled".into());
    }
    let (complete, retained_reference_bytes) =
        complete_timing_sample(source, &sample, captured.pcm_budget)?;
    #[cfg(test)]
    super::native_owned_work::observe(super::native_owned_work::WorkPoint::TimingPcm);
    if cancelled() {
        return Err("constant timing preparation cancelled".into());
    }
    let snapshot = LoadedPcmSnapshot::new(
        complete,
        sample_rate_hz,
        PcmIdentity {
            pad_id: id,
            request_id,
            source_id: format!("loaded-{id}-{source_generation}"),
            source_generation,
        },
        pcm_limit_bytes,
    )
    .map_err(|e| e.to_string())?;
    let mono_budget = pcm_limit_bytes
        .checked_sub(snapshot.retained_bytes())
        .and_then(|bytes| bytes.checked_sub(retained_reference_bytes))
        .ok_or("constant timing PCM byte limit exceeded")?;
    let mono = snapshot
        .prepare_complete_mono(mono_budget, &cancelled)
        .map_err(|e| e.to_string())?;
    let retained_and_mono = snapshot
        .retained_bytes()
        .checked_add(retained_reference_bytes)
        .and_then(|bytes| bytes.checked_add(mono.capacity() * size_of::<f32>()))
        .ok_or("timing PCM byte count overflow")?;
    let converter_budget = pcm_limit_bytes
        .checked_sub(retained_and_mono)
        .ok_or("constant timing PCM byte limit exceeded")?;
    if mono
        .len()
        .checked_mul(size_of::<f32>())
        .is_none_or(|bytes| bytes > converter_budget)
    {
        return Err("constant timing PCM byte limit exceeded".into());
    }
    let binding = PcmBinding::verify(
        &mono,
        PcmBindingMetadata {
            job: JobIdentity {
                pad_id: id as u64,
                request_id,
                source_id: format!("loaded-{id}-{source_generation}"),
                source_generation,
            },
            source_sha256: source_digest.clone(),
            source_provenance: SOURCE_PROVENANCE.into(),
            pcm_sha256: f32_pcm_sha256(&mono),
            sample_rate_hz,
            frame_count: mono.len() as u64,
            origin_seconds: 0.0,
            mono_revision: MONO_REVISION.into(),
        },
    )
    .map_err(|e| e.to_string())?;
    let converted = resample_mono_cancellable(
        mono.clone(),
        sample_rate_hz,
        44_100,
        converter_budget,
        &cancelled,
    )
    .map_err(|e| e.to_string())?;
    let conversion_peak = retained_and_mono
        .checked_add(converted.capacity() * size_of::<f32>())
        .and_then(|bytes| bytes.checked_add(converted.len() * size_of::<f64>()))
        .ok_or("timing PCM byte count overflow")?;
    if conversion_peak > pcm_limit_bytes {
        return Err("constant timing PCM byte limit exceeded".into());
    }
    let analyzer: Vec<f64> = converted.into_iter().map(f64::from).collect();
    if cancelled() {
        return Err("constant timing preparation cancelled".into());
    }
    let raw = analyze_bpm_raw(&analyzer, 44_100, &AnalysisConfig::default())?;
    let input = QmInputDescriptor {
        job: binding.metadata().job.clone(),
        pcm_sha256: binding.metadata().pcm_sha256.clone(),
        input_sha256: f64_input_sha256(&analyzer),
        sample_rate_hz: 44_100,
        frame_count: analyzer.len() as u64,
        origin_seconds: 0.0,
        transform: if sample_rate_hz == 44_100 {
            QmInputTransform::Identity
        } else {
            QmInputTransform::Rubato44100 { revision: "rubato-fft-1.0-44100-delay-trim-tail-flush-v1".into(), provenance: "native resample_mono_cancellable executed on this ticket's verified complete loaded mono".into() }
        },
    };
    let evidence = BoundTempoEvidence::from_qm(&binding, raw, &analyzer, input, timing_bound, 0.0)
        .map_err(|e| e.to_string())?;
    let mut guard =
        TimingAdoptionGuard::new(&binding, TimingIntent::Automatic).map_err(|e| e.to_string())?;
    let adoption_ticket = guard.issue_ticket().map_err(|e| e.to_string())?;
    let ticket = ConstantTimingTicket {
        id,
        placement: captured.placement,
        ownership: captured.ownership.clone(),
        request_id,
        request_incarnation: captured.request_incarnation,
        source_generation,
        sample_rate_hz,
        source_digest,
        sample,
        epoch,
        captured_epoch,
        binding: binding.metadata().clone(),
        evidence,
        guard: Mutex::new(guard),
        adoption_ticket,
        publication: Mutex::new(None),
        pcm_budget: captured.pcm_budget,
    };
    Ok(ticket)
}

fn timing_request_incarnation(
    ownership: &InputRuntimeOwnership,
    placement: Option<SlotSnapshot>,
) -> Option<u64> {
    placement
        .and_then(|snapshot| snapshot.content_key())
        .and_then(|key| ownership.key_source_request_incarnation(key))
}

fn resolve_timing_owner(
    engine: &AudioEngine,
    original: Option<SlotSnapshot>,
    ownership: &Arc<InputRuntimeOwnership>,
    diagnostic_id: usize,
) -> Result<(usize, bool), String> {
    if !Arc::ptr_eq(ownership, &engine.input_runtime_ownership) {
        return Err("stale or foreign constant timing ticket".into());
    }
    if !ownership.is_source_tracked() {
        return Ok((diagnostic_id, true));
    }
    let original = original.ok_or("stale or foreign constant timing ticket")?;
    let key = original
        .content_key()
        .ok_or("stale or foreign constant timing ticket")?;
    match ownership.observe_control_key(key) {
        super::content_placement::PlacementObservation::Current(current) => {
            Ok((current.slot(), original.same_projection(current)))
        }
        super::content_placement::PlacementObservation::Pending => {
            Err("constant timing content placement pending".into())
        }
        _ => Err("stale or foreign constant timing ticket".into()),
    }
}

fn validate_current(
    engine: &AudioEngine,
    ticket: &ConstantTimingTicket,
    requests: &[u64],
) -> Result<usize, String> {
    let (id, original_slot) =
        resolve_timing_owner(engine, ticket.placement, &ticket.ownership, ticket.id)?;
    if !Arc::ptr_eq(&ticket.ownership, &engine.input_runtime_ownership)
        || (engine.input_runtime_ownership.is_source_tracked() && ticket.placement.is_none())
        || !Arc::ptr_eq(engine.prepared_source_epoch(id)?, &ticket.epoch)
        || (original_slot && requests[id] != ticket.request_id)
        || timing_request_incarnation(&ticket.ownership, ticket.placement)
            != ticket.request_incarnation
        || ticket.epoch.load(Ordering::Acquire) != ticket.captured_epoch
    {
        return Err("stale or foreign constant timing ticket".into());
    }
    if engine
        .timing_intents
        .lock()
        .map_err(|_| "timing intent lock poisoned")?[id]
        != TimingIntent::Automatic
    {
        return Err("current timing intent is not automatic".into());
    }
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| "sample cache lock poisoned")?;
    let current = cache[id].as_ref().ok_or("sample is not loaded")?;
    if !current.same_source(&ticket.sample) {
        return Err("current loaded source owner changed".into());
    }
    if engine
        .loaded_source_generations
        .lock()
        .map_err(|_| "source generation lock poisoned")?[id]
        != (ticket.source_generation, ticket.sample_rate_hz)
        || engine
            .loaded_source_digests
            .lock()
            .map_err(|_| "source digest lock poisoned")?[id]
            .as_ref()
            != Some(&ticket.source_digest)
    {
        return Err("current source digest, generation or rate changed".into());
    }
    Ok(id)
}

enum TimingEvidenceAdmission {
    Current(usize),
    Pending,
}

/// Original immutable work can finish during a placement gap. This grants no
/// publication: source, request, epoch and intent are checked at their captured
/// stable cell, without recapturing permission from a current numeric slot.
fn validate_intrinsic_timing_identity(
    engine: &AudioEngine,
    ticket: &ConstantTimingTicket,
) -> Result<(), &'static str> {
    if !Arc::ptr_eq(&ticket.ownership, &engine.input_runtime_ownership) {
        return Err("stale or foreign constant timing ticket");
    }
    if !ticket.ownership.is_source_tracked() {
        return if engine
            .prepared_source_epochs
            .get(ticket.id)
            .is_some_and(|epoch| Arc::ptr_eq(epoch, &ticket.epoch))
            && ticket.epoch.load(Ordering::Acquire) == ticket.captured_epoch
        {
            Ok(())
        } else {
            Err("stale or foreign constant timing ticket")
        };
    }
    let original = ticket
        .placement
        .ok_or("stale or foreign constant timing ticket")?;
    let key = original
        .content_key()
        .filter(|key| ticket.ownership.content_key_current(*key))
        .ok_or("stale or foreign constant timing ticket")?;
    if !engine
        .prepared_source_epochs
        .get(key.cell().index())
        .is_some_and(|epoch| Arc::ptr_eq(epoch, &ticket.epoch))
        || ticket.epoch.load(Ordering::Acquire) != ticket.captured_epoch
        || ticket.request_incarnation.is_none()
        || ticket.ownership.key_source_request_incarnation(key) != ticket.request_incarnation
    {
        return Err("stale or foreign constant timing ticket");
    }
    let binding = InputPadBinding {
        placement: Some(original),
        resident: ticket.sample.resident_binding(),
        source_address: ticket.sample.source_address(),
        sample_count: ticket.sample.source_sample_count(),
        channels: ticket.sample.channels,
        sample_rate_hz: ticket.sample_rate_hz,
        authority_revision: 0,
        runtime_revision: 0,
        accepted: None,
    };
    if ticket
        .ownership
        .binding_source_generation(ticket.id, binding)
        != Some(ticket.source_generation)
    {
        return Err("current loaded source owner changed");
    }
    if ticket.ownership.binding_timing_intent(ticket.id, binding) != Some(1) {
        return Err("current timing intent is not automatic");
    }
    if !ticket.ownership.content_key_current(key)
        || ticket.ownership.key_source_request_incarnation(key) != ticket.request_incarnation
        || ticket.epoch.load(Ordering::Acquire) != ticket.captured_epoch
    {
        return Err("stale or foreign constant timing ticket");
    }
    Ok(())
}

fn validate_evidence_admission(
    engine: &AudioEngine,
    ticket: &ConstantTimingTicket,
    requests: &[u64],
) -> Result<TimingEvidenceAdmission, String> {
    if !Arc::ptr_eq(&ticket.ownership, &engine.input_runtime_ownership) {
        return Err("stale or foreign constant timing ticket".into());
    }
    if !ticket.ownership.is_source_tracked() {
        return validate_current(engine, ticket, requests).map(TimingEvidenceAdmission::Current);
    }
    let key = ticket
        .placement
        .and_then(|snapshot| snapshot.content_key())
        .ok_or("stale or foreign constant timing ticket")?;
    match ticket.ownership.observe_control_key(key) {
        PlacementObservation::Current(_) => match validate_current(engine, ticket, requests) {
            Ok(id) => Ok(TimingEvidenceAdmission::Current(id)),
            Err(_)
                if matches!(
                    ticket.ownership.observe_control_key(key),
                    PlacementObservation::Pending
                ) =>
            {
                validate_pending_timing_evidence(engine, ticket, key)
            }
            Err(error) => Err(error),
        },
        PlacementObservation::Pending => validate_pending_timing_evidence(engine, ticket, key),
        PlacementObservation::Stale | PlacementObservation::Retired => {
            Err("stale or foreign constant timing ticket".into())
        }
    }
}

fn validate_pending_timing_evidence(
    engine: &AudioEngine,
    ticket: &ConstantTimingTicket,
    key: ContentKey,
) -> Result<TimingEvidenceAdmission, String> {
    validate_intrinsic_timing_identity(engine, ticket)?;
    // The request owner serializes the complete control mirror. Its current
    // custody row validates retained evidence during the gap, but cannot grant
    // publication at that old physical placement.
    let id = ticket.ownership.control_slot_for_cell(key.cell());
    if engine
        .timing_intents
        .lock()
        .map_err(|_| "timing intent lock poisoned")?
        .get(id)
        != Some(&TimingIntent::Automatic)
    {
        return Err("current timing intent is not automatic".into());
    }
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| "sample cache lock poisoned")?;
    if !cache
        .get(id)
        .and_then(Option::as_ref)
        .is_some_and(|source| source.same_source(&ticket.sample))
    {
        return Err("current loaded source owner changed".into());
    }
    if engine
        .loaded_source_generations
        .lock()
        .map_err(|_| "source generation lock poisoned")?
        .get(id)
        != Some(&(ticket.source_generation, ticket.sample_rate_hz))
        || engine
            .loaded_source_digests
            .lock()
            .map_err(|_| "source digest lock poisoned")?
            .get(id)
            .and_then(Option::as_ref)
            != Some(&ticket.source_digest)
    {
        return Err("current source digest, generation or rate changed".into());
    }
    validate_intrinsic_timing_identity(engine, ticket)?;
    Ok(TimingEvidenceAdmission::Pending)
}

fn timing_publication_placement_error(
    engine: &AudioEngine,
    ticket: &ConstantTimingTicket,
    error: PlacementError,
) -> PyErr {
    if error == PlacementError::Pending {
        if let Err(error) = validate_intrinsic_timing_identity(engine, ticket) {
            return PyValueError::new_err(error);
        }
        PyBlockingIOError::new_err("constant timing content placement pending")
    } else {
        PyValueError::new_err(error.to_string())
    }
}

struct OwnedHypothesis {
    id: String,
    provenance: String,
    verification: QuarterNoteVerification,
    denominator: u32,
    counts: Vec<Option<i64>>,
}

fn parse_hypotheses(json: &str) -> Result<Vec<OwnedHypothesis>, String> {
    if json.len() > MAX_HYPOTHESIS_JSON_BYTES {
        return Err("timing hypotheses byte limit exceeded".into());
    }
    let value: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let values = value
        .as_array()
        .filter(|items| !items.is_empty() && items.len() <= MAX_HYPOTHESES)
        .ok_or("invalid timing hypotheses")?;
    values
        .iter()
        .map(|value| {
            let fields = value
                .as_object()
                .filter(|fields| fields.len() == 5)
                .ok_or("invalid timing hypothesis fields")?;
            let text = |key: &str| {
                fields
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty() && text.len() <= 4096)
                    .map(str::to_owned)
                    .ok_or_else(|| format!("invalid hypothesis {key}"))
            };
            let verification = match value["verification"].as_str() {
                Some("verified") => QuarterNoteVerification::Verified,
                Some("unverified") => QuarterNoteVerification::Unverified,
                _ => return Err("invalid quarter verification".into()),
            };
            let denominator = value["quarter_note_denominator"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or("invalid count denominator")?;
            let values = value["quarter_counts"]
                .as_array()
                .filter(|v| v.len() <= flitzis_looper_analysis::tempo_summary::MAX_RAW_POSITIONS)
                .ok_or("invalid quarter counts")?;
            let counts = values
                .iter()
                .map(|v| {
                    if v.is_null() {
                        Ok(None)
                    } else {
                        v.as_i64().map(Some).ok_or("invalid quarter numerator")
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(OwnedHypothesis {
                id: text("id")?,
                provenance: text("provenance")?,
                verification,
                denominator,
                counts,
            })
        })
        .collect()
}

fn projection(
    timing: &AcceptedConstantTiming,
    rate: u32,
    epoch: u64,
) -> Result<AcceptedTimingProjection, String> {
    let hex = timing
        .revision()
        .strip_prefix("accepted-constant-timing-v1:")
        .filter(|hex| hex.len() == 64)
        .ok_or("invalid complete accepted revision")?;
    let mut revision = [0_u8; 32];
    for (index, byte) in revision.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .map_err(|_| "invalid complete accepted revision")?;
    }
    Ok(AcceptedTimingProjection {
        revision,
        period_seconds: timing.period_seconds_per_quarter(),
        origin_seconds: timing.origin().seconds,
        sample_rate_hz: rate,
        publication_epoch: epoch,
    })
}

#[cfg(test)]
pub(super) fn publish(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ticket: &ConstantTimingTicket,
    hypotheses_json: &str,
    origin: IndependentTimingOrigin,
    decision: TimingAcceptanceDecision,
) -> PyResult<()> {
    let timing = prepare_publication(ticket, hypotheses_json, origin, decision)?;
    publish_record(engine, producer, ticket, timing, false)
}

pub(super) fn prepare_publication(
    ticket: &ConstantTimingTicket,
    hypotheses_json: &str,
    origin: IndependentTimingOrigin,
    decision: TimingAcceptanceDecision,
) -> PyResult<AcceptedConstantTiming> {
    let hypotheses = parse_hypotheses(hypotheses_json).map_err(PyValueError::new_err)?;
    let borrowed: Vec<_> = hypotheses
        .iter()
        .map(|h| QuarterNoteHypothesis {
            id: &h.id,
            provenance: &h.provenance,
            verification: h.verification,
            quarter_note_denominator: h.denominator,
            quarter_counts: &h.counts,
        })
        .collect();
    let timing =
        AcceptedConstantTiming::from_raw(ticket.evidence.clone(), &borrowed, origin, decision)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
    #[cfg(test)]
    super::native_owned_work::observe(super::native_owned_work::WorkPoint::AcceptedTiming);
    Ok(timing)
}

pub(super) fn publish_record(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ticket: &ConstantTimingTicket,
    timing: AcceptedConstantTiming,
    source_verified_restore: bool,
) -> PyResult<()> {
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let id = match validate_evidence_admission(engine, ticket, &requests)
        .map_err(PyValueError::new_err)?
    {
        TimingEvidenceAdmission::Current(id) => id,
        TimingEvidenceAdmission::Pending => {
            return Err(PyBlockingIOError::new_err(
                "constant timing content placement pending",
            ));
        }
    };
    let mut guard = ticket
        .guard
        .lock()
        .map_err(|_| PyRuntimeError::new_err("timing guard lock poisoned"))?;
    let mut publication = ticket
        .publication
        .lock()
        .map_err(|_| PyRuntimeError::new_err("timing publication lock poisoned"))?;
    if publication.is_some() {
        return Err(PyValueError::new_err(
            "constant timing ticket already published",
        ));
    }
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    let next = next_epoch(&ticket.epoch).map_err(PyRuntimeError::new_err)?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "Failed to send PublishConstantTiming - buffer may be full",
        ));
    }
    let projected =
        projection(&timing, ticket.sample_rate_hz, next).map_err(PyValueError::new_err)?;
    let permit = PreparedSourcePermit::new(ticket.epoch.clone(), next);
    let mut current_records = engine
        .current_constant_timing
        .lock()
        .map_err(|_| PyRuntimeError::new_err("current timing lock poisoned"))?;
    current_records[id]
        .try_reserve(1)
        .map_err(|_| PyRuntimeError::new_err("current timing record allocation failed"))?;
    let record = CurrentConstantTimingRecord::new(&timing, ticket, next, permit.clone(), projected);
    permit.mark_pending().map_err(PyRuntimeError::new_err)?;
    let adoption = if source_verified_restore {
        guard.prepare_source_verified_adoption(&ticket.adoption_ticket, timing)
    } else {
        guard.prepare_adoption(&ticket.adoption_ticket, timing)
    }
    .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let message = ControlMessage::PublishConstantTiming {
        id,
        timing: PreparedConstantTiming {
            reference: ticket.sample.clone(),
            publication: permit.clone(),
            projection: projected,
            content_key: ticket.placement.and_then(|snapshot| snapshot.content_key()),
        },
    };
    let ownership = &ticket.ownership;
    let placement = ownership.slot_snapshot(id).ok_or_else(|| {
        timing_publication_placement_error(engine, ticket, PlacementError::Pending)
    })?;
    let prepared = ownership
        .prepare_slot_write_fixed(placement, placement.lifetime())
        .map_err(|error| timing_publication_placement_error(engine, ticket, error))?;
    #[cfg(test)]
    completion_tests::before_publication_claim();
    let mut displaced = None;
    ownership
        .commit_slot_write_fixed(
            &prepared,
            || {
                ownership.slot_snapshot_fields_current(placement)
                    && ownership.control_slot_for_cell(placement.cell()) == id
                    && validate_intrinsic_timing_identity(engine, ticket).is_ok()
            },
            |_| {
                displaced = adoption.commit();
                current_records[id].push(record);
                ticket.epoch.store(next, Ordering::Release);
                *publication = Some(permit);
                // Capacity was checked under this same sole-producer mutex;
                // the consumer can only make more space before this fixed push.
                producer
                    .push(message)
                    .unwrap_or_else(|_| unreachable!("reserved single-producer capacity lost"));
            },
        )
        .map_err(|error| timing_publication_placement_error(engine, ticket, error))?;
    // Neither a displaced guard record nor obsolete registry evidence may have
    // its final destructor run inside the shared publication marker.
    drop(displaced);
    retire_old_current_records(engine, &mut current_records[id]);
    Ok(())
}

/// All fallible admission, locks and queue capacity checks precede this bounded mutation.
/// A callback source/window writer and a pair writer use the same marker. Once claimed,
/// only the captured cell is touched and the returned snapshot names the complete edit.
fn commit_timing_intent_under_request_lock(
    engine: &AudioEngine,
    placement: SlotSnapshot,
    epoch: &AtomicU64,
    next: u64,
    intents: &mut [TimingIntent],
    intent: TimingIntent,
) -> PyResult<SlotSnapshot> {
    let ownership = &engine.input_runtime_ownership;
    let prepared = ownership
        .prepare_slot_write_fixed(placement, placement.lifetime())
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    let authority = ownership.authority_for_cell(placement.cell());
    let captured_authority = authority.load(Ordering::Acquire);
    let next_authority = next_epoch(authority).map_err(PyRuntimeError::new_err)?;
    let declared_intent = intents
        .get_mut(placement.slot())
        .ok_or_else(|| PyValueError::new_err("Invalid sample id"))?;
    ownership
        .commit_slot_write_fixed(
            &prepared,
            || {
                authority.load(Ordering::Acquire) == captured_authority
                    && epoch.load(Ordering::Acquire) == next - 1
            },
            |cell| {
                if let Some(key) = placement.content_key() {
                    engine
                        .current_timing_acknowledgements
                        .revoke_captured_key(key, next);
                }
                authority.store(next_authority, Ordering::Release);
                epoch.store(next, Ordering::Release);
                *declared_intent = intent;
                ownership.set_timing_intent_for_cell(cell, intent);
            },
        )
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))
}

fn capture_timing_edit_placement(engine: &AudioEngine, id: usize) -> PyResult<SlotSnapshot> {
    engine
        .input_runtime_ownership
        .slot_snapshot(id)
        .ok_or_else(|| PyRuntimeError::new_err("current timing placement is pending or invalid"))
}

/// Publish authority and an ordered clear transaction; every edit invalidates old tickets.
#[cfg(test)]
pub(super) fn set_intent(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    id: usize,
    intent: TimingIntent,
) -> PyResult<()> {
    set_intent_with_source(engine, producer, id, intent, None)
}

pub(super) fn set_intent_with_source(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    id: usize,
    intent: TimingIntent,
    expected_source_generation: Option<u64>,
) -> PyResult<()> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let placement = capture_timing_edit_placement(engine, id)?;
    super::input_runtime_binding::validate_expected_source_generation_under_request_lock(
        engine,
        id,
        expected_source_generation,
    )?;
    let mut intents = engine
        .timing_intents
        .lock()
        .map_err(|_| PyRuntimeError::new_err("timing intent lock poisoned"))?;
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    let epoch = &engine.prepared_source_epochs[placement.cell().index()];
    let next = next_epoch(epoch).map_err(PyRuntimeError::new_err)?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "Failed to send timing intent - buffer may be full",
        ));
    }
    let placement = commit_timing_intent_under_request_lock(
        engine,
        placement,
        epoch,
        next,
        &mut intents,
        intent,
    )?;
    producer
        .push(ControlMessage::ClearPadConstantTiming {
            id,
            placement: Some(placement),
            through_epoch: next,
            expected_source_generation,
        })
        .map_err(|_| PyRuntimeError::new_err("reserved single-producer capacity lost"))
}

#[cfg(test)]
pub(super) fn publish_legacy_bpm(
    engine: &AudioEngine,
    parameters: &Arc<Mutex<Producer<ControlParameterMessage>>>,
    id: usize,
    bpm: Option<f64>,
) -> PyResult<()> {
    publish_legacy_bpm_with_source(engine, parameters, id, bpm, None)
}

pub(super) fn publish_legacy_bpm_with_source(
    engine: &AudioEngine,
    parameters: &Arc<Mutex<Producer<ControlParameterMessage>>>,
    id: usize,
    bpm: Option<f64>,
    expected_source_generation: Option<u64>,
) -> PyResult<()> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let placement = capture_timing_edit_placement(engine, id)?;
    super::input_runtime_binding::validate_expected_source_generation_under_request_lock(
        engine,
        id,
        expected_source_generation,
    )?;
    let mut intents = engine
        .timing_intents
        .lock()
        .map_err(|_| PyRuntimeError::new_err("timing intent lock poisoned"))?;
    let mut parameters = parameters
        .lock()
        .map_err(|_| PyRuntimeError::new_err("parameter producer lock poisoned"))?;
    let epoch = &engine.prepared_source_epochs[placement.cell().index()];
    let next = next_epoch(epoch).map_err(PyRuntimeError::new_err)?;
    if parameters.is_full() {
        return Err(PyRuntimeError::new_err(
            "Failed to send SetPadBpm - buffer may be full",
        ));
    }
    let placement = commit_timing_intent_under_request_lock(
        engine,
        placement,
        epoch,
        next,
        &mut intents,
        TimingIntent::Legacy,
    )?;
    parameters
        .push(ControlParameterMessage::SetLegacyPadBpm {
            id,
            placement: Some(placement),
            bpm,
            through_epoch: next,
            expected_source_generation,
        })
        .map_err(|_| PyRuntimeError::new_err("reserved parameter capacity lost"))
}

#[cfg(test)]
pub(super) fn publish_legacy_origin(
    engine: &AudioEngine,
    ordered: &Arc<Mutex<Producer<ControlMessage>>>,
    id: usize,
    phase_anchor_s: f64,
) -> PyResult<()> {
    publish_legacy_origin_with_source(engine, ordered, id, phase_anchor_s, None)
}

pub(super) fn publish_legacy_origin_with_source(
    engine: &AudioEngine,
    ordered: &Arc<Mutex<Producer<ControlMessage>>>,
    id: usize,
    phase_anchor_s: f64,
    expected_source_generation: Option<u64>,
) -> PyResult<()> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let placement = capture_timing_edit_placement(engine, id)?;
    super::input_runtime_binding::validate_expected_source_generation_under_request_lock(
        engine,
        id,
        expected_source_generation,
    )?;
    let mut intents = engine
        .timing_intents
        .lock()
        .map_err(|_| PyRuntimeError::new_err("timing intent lock poisoned"))?;
    let mut ordered = ordered
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    let epoch = &engine.prepared_source_epochs[placement.cell().index()];
    let next = next_epoch(epoch).map_err(PyRuntimeError::new_err)?;
    if ordered.is_full() {
        return Err(PyRuntimeError::new_err(
            "Failed to send SetPadTimingMetadata - buffer may be full",
        ));
    }
    let placement = commit_timing_intent_under_request_lock(
        engine,
        placement,
        epoch,
        next,
        &mut intents,
        TimingIntent::Legacy,
    )?;
    ordered
        .push(ControlMessage::SetLegacyPadTimingMetadata {
            id,
            placement: Some(placement),
            metadata: crate::messages::PadTimingMetadata { phase_anchor_s },
            through_epoch: next,
            expected_source_generation,
        })
        .map_err(|_| PyRuntimeError::new_err("reserved ordered capacity lost"))
}

/// Observe immutable evidence sharing without granting or altering current authority.
#[cfg(test)]
pub(super) fn copy_records_share_accepted_for_test(
    engine: &AudioEngine,
    source: usize,
    target: usize,
) -> bool {
    let records = engine
        .current_constant_timing
        .lock()
        .expect("timing test lock");
    records
        .get(source)
        .and_then(|items| items.last())
        .zip(records.get(target).and_then(|items| items.last()))
        .is_some_and(|(source, target)| Arc::ptr_eq(&source.accepted, &target.accepted))
}

#[cfg(test)]
#[path = "constant_timing_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "constant_timing/content_ack_tests.rs"]
mod content_ack_tests;

#[cfg(test)]
#[path = "constant_timing/content_epoch_tests.rs"]
mod content_epoch_tests;

#[cfg(test)]
#[path = "constant_timing/completion_tests.rs"]
mod completion_tests;

#[cfg(test)]
#[path = "constant_timing/control_write_tests.rs"]
mod control_write_tests;

#[cfg(all(test, windows))]
#[path = "native_borrow_tests.rs"]
mod native_borrow_tests;

#[cfg(test)]
#[path = "l3_constant_timing_fixture.rs"]
pub(crate) mod l3_constant_timing_fixture;

#[cfg(test)]
#[path = "constant_timing_pcm_budget_tests.rs"]
mod pcm_budget_tests;

#[cfg(test)]
#[path = "musical_loop_proof_tests.rs"]
mod musical_loop_proof_tests;

#[cfg(test)]
#[path = "musical_loop_private_probe.rs"]
mod musical_loop_private_probe;
