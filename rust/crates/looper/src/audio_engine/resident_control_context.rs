//! Owned shared control views never own a stream or a Python Engine borrow.
use super::*;
#[derive(Clone)]
pub(super) struct ResidentControlContext {
    pub(super) pad_request_ids: Arc<Mutex<Vec<u64>>>,
    pub(super) sample_cache: Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    pub(super) resident_stem_cache: Arc<Mutex<Vec<resident_relocation::ResidentStemCache>>>,
    pub(super) input_runtime_ownership: Arc<input_runtime_binding::InputRuntimeOwnership>,
    pub(super) current_timing_acknowledgements: Arc<constant_timing::CurrentTimingAcknowledgements>,
    pub(super) timing_intents: Arc<Mutex<Vec<analysis::tempo_acceptance::TimingIntent>>>,
    pub(super) loaded_source_generations: Arc<Mutex<Vec<(u64, u32)>>>,
    pub(super) loaded_source_digests: Arc<Mutex<Vec<Option<String>>>>,
    pub(super) current_constant_timing:
        Arc<Mutex<Vec<Vec<constant_timing::CurrentConstantTimingRecord>>>>,
}
#[derive(Clone)]
pub(super) struct CommittedControlContext {
    control: ResidentControlContext,
    pub(super) prepared_source_epochs: Vec<Arc<AtomicU64>>,
    pub(super) resident_proposals: Arc<resident_proposal::ResidentProposals>,
    pub(super) resident_preparation_gate: Arc<Mutex<()>>,
    pub(super) global_timing_revision: Arc<AtomicU64>,
    pub(super) project_assets: Arc<project_assets::ProjectAssets>,
    pub(super) resident_account: Arc<resident_accounting::ResidentAccount>,
    pub(super) cold_cancelled: Arc<AtomicBool>,
    pub(super) cold_jobs: cold_jobs::ColdJobsHandle,
}
impl std::ops::Deref for CommittedControlContext {
    type Target = ResidentControlContext;
    fn deref(&self) -> &Self::Target {
        &self.control
    }
}
impl ResidentControlContext {
    pub(super) fn capture(engine: &AudioEngine) -> Self {
        Self {
            pad_request_ids: engine.pad_request_ids.clone(),
            sample_cache: engine.sample_cache.clone(),
            resident_stem_cache: engine.resident_stem_cache.clone(),
            input_runtime_ownership: engine.input_runtime_ownership.clone(),
            current_timing_acknowledgements: engine.current_timing_acknowledgements.clone(),
            timing_intents: engine.timing_intents.clone(),
            loaded_source_generations: engine.loaded_source_generations.clone(),
            loaded_source_digests: engine.loaded_source_digests.clone(),
            current_constant_timing: engine.current_constant_timing.clone(),
        }
    }
}
impl CommittedControlContext {
    pub(super) fn capture(engine: &AudioEngine) -> Self {
        Self {
            control: ResidentControlContext::capture(engine),
            prepared_source_epochs: engine.prepared_source_epochs.clone(),
            resident_proposals: engine.resident_proposals.clone(),
            resident_preparation_gate: engine.resident_preparation_gate.clone(),
            global_timing_revision: engine.global_timing_revision.clone(),
            project_assets: engine.project_assets.clone(),
            resident_account: engine.resident_account.clone(),
            cold_cancelled: engine.cold_cancelled.clone(),
            cold_jobs: engine.cold_jobs.handle(),
        }
    }
    pub(super) fn prepared_source_epoch(&self, id: usize) -> Result<&Arc<AtomicU64>, String> {
        let snapshot = self
            .input_runtime_ownership
            .slot_snapshot(id)
            .ok_or("content placement is pending or unavailable")?;
        self.prepared_source_epochs
            .get(snapshot.cell().index())
            .ok_or_else(|| "content epoch cell is unavailable".into())
    }
}
