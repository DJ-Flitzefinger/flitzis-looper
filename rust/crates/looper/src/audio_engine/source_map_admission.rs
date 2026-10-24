//! Control-owned selected maps and immutable launch witnesses. No geometry grants authority.

use super::content_placement::ContentKey;
use super::input_runtime_binding::{
    InputPadBinding, InputRuntimeOwnership, InputRuntimePadBinding, SourceSyncWitness,
};
use super::loader_events::LoaderEventDelivery;
use super::pitch_actions::PitchActionReject;
use super::source_beatmap::SourceBeatmap;
use super::source_beatmap_artifact::SourceBeatmapArtifact;
use super::source_reader::{FrameRange, SourceLoopDomain};
use super::source_reader_period::SourceReaderPeriod;
use super::source_sync_policy::{GlobalTimingPolicy, SourceSyncMode, SourceSyncRequest};
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// Strict additive durable intent, independent of selected map data and live effect.
pub(crate) fn validate_source_sync_intent(value: &serde_json::Value) -> Result<(), String> {
    if value.as_object().is_none_or(|object| {
        object.len() != 2 || !object.contains_key("requested") || !object.contains_key("revision")
    }) || value["requested"].as_bool().is_none()
        || value["revision"].as_u64().is_none()
    {
        return Err(
            "source SYNC intent requires exactly boolean requested and u64 revision".into(),
        );
    }
    Ok(())
}

/// A complete control projection, carried by the existing input-runtime publication.
/// A selected map remains data-only until explicit SYNC intent is present.
#[derive(Clone, Debug, Default)]
pub(crate) struct SourceMapRow {
    pub(crate) sync_requested: bool,
    pub(crate) intent_revision: u64,
    pub(crate) delivery: Option<LoaderEventDelivery>,
    pub(crate) exact_quarters: Option<f64>,
}

/// Stable off-realtime preparation class, never actual-E or source-adoption permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeMapClass {
    Unavailable,
    Unsupported,
    Ready,
}

/// Exact immutable geometry fingerprint used by the original policy retry observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeMapDomain {
    pub(crate) sample_frames: usize,
    pub(crate) region: FrameRange,
    pub(crate) period: SourceReaderPeriod,
}

impl From<SourceLoopDomain> for RuntimeMapDomain {
    fn from(domain: SourceLoopDomain) -> Self {
        Self {
            sample_frames: domain.sample_frames,
            region: domain.region,
            period: domain.musical_period,
        }
    }
}

/// Copy evidence describes only the original snapshot's prepared map and domain.
/// Cursor, raw fraction and actual-master horizon eligibility must be checked at actual E.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeMapPreparation {
    pub(crate) class: RuntimeMapClass,
    pub(crate) domain: Option<RuntimeMapDomain>,
    pub(crate) witness: Option<SourceSyncWitness>,
}

impl RuntimeMapPreparation {
    pub(crate) fn unavailable(witness: Option<SourceSyncWitness>) -> Self {
        Self {
            class: RuntimeMapClass::Unavailable,
            domain: None,
            witness,
        }
    }
}

/// The genuine original worker delivery owns the source qualification and artifact pin.
/// New admission/claim checks freshness; effective voices only retain the immutable pin.
#[derive(Clone, Debug)]
pub(crate) struct QualifiedSourceMap {
    delivery: LoaderEventDelivery,
    map: SourceBeatmap,
    _artifact: SourceBeatmapArtifact,
    key: ContentKey,
    generation: u64,
    loaded_rate: u32,
    exact_quarters: Option<f64>,
    witness: SourceSyncWitness,
}

impl QualifiedSourceMap {
    pub(super) fn new(
        delivery: LoaderEventDelivery,
        map: SourceBeatmap,
        artifact: SourceBeatmapArtifact,
        binding: &InputRuntimePadBinding,
        exact_quarters: Option<f64>,
    ) -> Result<Self, String> {
        let key = binding
            .binding
            .placement
            .and_then(|placement| placement.content_key())
            .ok_or("source map requires current content")?;
        let mut revision = [0; 4];
        for (word, text) in revision
            .iter_mut()
            .zip(map.map_revision().as_bytes().chunks_exact(16))
        {
            *word = u64::from_str_radix(
                std::str::from_utf8(text).map_err(|_| "source map revision is not hexadecimal")?,
                16,
            )
            .map_err(|_| "source map revision is not hexadecimal")?;
        }
        Ok(Self {
            delivery,
            map,
            _artifact: artifact,
            key,
            generation: binding.source_generation,
            loaded_rate: binding.binding.sample_rate_hz,
            exact_quarters,
            witness: SourceSyncWitness {
                intent_revision: 0,
                map_revision: revision,
            },
        })
    }

    pub(crate) fn map(&self) -> &SourceBeatmap {
        &self.map
    }

    pub(crate) fn loaded_rate(&self) -> u32 {
        self.loaded_rate
    }

    pub(crate) fn exact_quarters(&self) -> Option<f64> {
        self.exact_quarters
    }

    pub(crate) fn sync_witness(&self) -> SourceSyncWitness {
        self.witness
    }

    /// Retained effective provenance only. Pending admission separately checks `current_for`.
    pub(crate) fn belongs_to(&self, ownership: &Arc<InputRuntimeOwnership>) -> bool {
        self.delivery.source_map_belongs_to(ownership)
    }

    pub(crate) fn sync_witness_for(
        &self,
        key: ContentKey,
        source_generation: u64,
    ) -> Option<SourceSyncWitness> {
        (self.key == key && self.generation == source_generation).then(|| self.sync_witness())
    }

    /// Bounded scalar freshness for pending admission, including the original job token.
    pub(crate) fn current_for(
        &self,
        ownership: &Arc<InputRuntimeOwnership>,
        binding: InputPadBinding,
    ) -> bool {
        binding
            .placement
            .and_then(|placement| placement.content_key())
            == Some(self.key)
            && binding.sample_rate_hz == self.loaded_rate
            && ownership.binding_source_generation(
                binding
                    .placement
                    .map_or(usize::MAX, |placement| placement.slot()),
                binding,
            ) == Some(self.generation)
            && self.delivery.source_map_binding_current(ownership, binding)
    }

    pub(super) fn same_projection(&self, other: &Self) -> bool {
        self.key == other.key
            && self.generation == other.generation
            && self.loaded_rate == other.loaded_rate
            && self.exact_quarters == other.exact_quarters
            && self.delivery.same_owner(&other.delivery)
            && self.map.map_revision() == other.map.map_revision()
    }
}

/// Immutable data in one existing RuntimePadState, never a second selected-map registry.
#[derive(Clone, Debug, Default)]
pub(crate) struct SourceMapProjection {
    sync_requested: bool,
    intent_revision: u64,
    selected: Option<QualifiedSourceMap>,
    policy: GlobalTimingPolicy,
    multi_loop: bool,
    bpm_requested: bool,
}

impl SourceMapProjection {
    pub(crate) fn selected_sync_witness(&self) -> Option<SourceSyncWitness> {
        self.selected.as_ref().map(QualifiedSourceMap::sync_witness)
    }

    /// Borrow only the selected original row; no fresh capture or proxy can supply its custody.
    /// Runtime policy owns its publication revision and repeats this source check at admission.
    pub(crate) fn runtime_map(
        &self,
        ownership: &Arc<InputRuntimeOwnership>,
        binding: InputPadBinding,
    ) -> Option<&QualifiedSourceMap> {
        if self.request().mode != SourceSyncMode::Mapped {
            return None;
        }
        self.selected
            .as_ref()
            .filter(|selected| selected.current_for(ownership, binding))
    }

    #[cfg(test)]
    pub(crate) fn intent_revision_for_test(&self) -> u64 {
        self.intent_revision
    }

    pub(crate) fn without_selection(&self) -> Self {
        Self {
            sync_requested: self.sync_requested,
            intent_revision: self.intent_revision,
            selected: None,
            policy: self.policy,
            multi_loop: self.multi_loop,
            bpm_requested: self.bpm_requested,
        }
    }

    pub(crate) fn qualify(
        row: &SourceMapRow,
        binding: Option<&InputRuntimePadBinding>,
    ) -> Result<Self, String> {
        if row
            .exact_quarters
            .is_some_and(|quarters| !quarters.is_finite() || quarters <= 0.0)
        {
            return Err("mapped exact quarter-note extent must be finite and positive".into());
        }
        let selected = row
            .delivery
            .as_ref()
            .map(|delivery| {
                let mut selected = delivery.qualify_source_map(
                    binding.ok_or("selected source map requires current loaded binding")?,
                    row.exact_quarters,
                )?;
                selected.witness.intent_revision = row.intent_revision;
                Ok::<_, String>(selected)
            })
            .transpose()?;
        Ok(Self {
            sync_requested: row.sync_requested,
            intent_revision: row.intent_revision,
            selected,
            policy: GlobalTimingPolicy::default(),
            multi_loop: false,
            bpm_requested: false,
        })
    }

    pub(crate) fn with_policy(mut self, policy: GlobalTimingPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub(crate) fn policy(&self) -> GlobalTimingPolicy {
        self.policy
    }

    /// Requested parents are captured after the final pitch-row reconciliation.
    pub(crate) fn remask_parents(&mut self, multi_loop: bool, bpm_requested: bool) {
        self.multi_loop = multi_loop;
        self.bpm_requested = bpm_requested;
    }

    pub(crate) fn same_hierarchy(&self, other: &Self) -> bool {
        self.policy == other.policy
            && self.multi_loop == other.multi_loop
            && self.bpm_requested == other.bpm_requested
    }

    pub(crate) fn request(&self) -> SourceSyncRequest {
        let mut request = self
            .policy
            .request(self.sync_requested, self.intent_revision);
        if !self.multi_loop || !self.bpm_requested || request.mode == SourceSyncMode::Off {
            request.mode = SourceSyncMode::Legacy;
        }
        request
    }

    pub(crate) fn same_projection(&self, other: &Self) -> bool {
        self.sync_requested == other.sync_requested
            && self.intent_revision == other.intent_revision
            && self.policy == other.policy
            && self.multi_loop == other.multi_loop
            && self.bpm_requested == other.bpm_requested
            && self.same_selected_projection(other)
    }

    /// Raw request publication retains an existing map only through its exact
    /// original delivery owner, source, geometry and immutable revision.
    pub(crate) fn same_selected_projection(&self, other: &Self) -> bool {
        match (&self.selected, &other.selected) {
            (Some(left), Some(right)) => left.same_projection(right),
            (None, None) => true,
            _ => false,
        }
    }

    /// Data-only selection changes never invalidate ordinary unmapped work.
    pub(crate) fn same_launch_projection(&self, other: &Self) -> bool {
        if self.request().mode == SourceSyncMode::Legacy
            && other.request().mode == SourceSyncMode::Legacy
            && self.request().global_revision.is_none()
            && other.request().global_revision.is_none()
            && !self.sync_requested
            && !other.sync_requested
        {
            return true;
        }
        self.request() == other.request()
            && self.sync_requested == other.sync_requested
            && ((!self.sync_requested && !other.sync_requested)
                || self.same_selected_projection(other))
    }

    pub(crate) fn publication_current(
        &self,
        ownership: &Arc<InputRuntimeOwnership>,
        binding: Option<InputPadBinding>,
    ) -> bool {
        self.selected.as_ref().is_none_or(|selected| {
            binding.is_some_and(|binding| selected.current_for(ownership, binding))
        })
    }

    pub(crate) fn snapshot(
        &self,
        ownership: &Arc<InputRuntimeOwnership>,
        binding: InputPadBinding,
    ) -> SourceMapLaunchSnapshot {
        let revision = binding
            .placement
            .map(|placement| {
                ownership
                    .launch_revision_for_cell(placement.cell())
                    .load(Ordering::Acquire)
            })
            .unwrap_or(u64::MAX);
        SourceMapLaunchSnapshot {
            projection: self.clone(),
            launch_revision: revision,
        }
    }
}

/// The projection and launch revision are captured together, before any later CLEAR.
#[derive(Clone, Debug)]
pub(crate) struct SourceMapLaunchSnapshot {
    projection: SourceMapProjection,
    launch_revision: u64,
}

impl SourceMapLaunchSnapshot {
    pub(crate) fn launch_revision(&self) -> u64 {
        self.launch_revision
    }
    pub(crate) fn request(&self) -> SourceSyncRequest {
        self.projection.request()
    }
    pub(crate) fn unavailable() -> Self {
        Self {
            projection: SourceMapProjection::default(),
            launch_revision: u64::MAX,
        }
    }

    pub(crate) fn scalar(ownership: &Arc<InputRuntimeOwnership>, binding: InputPadBinding) -> Self {
        SourceMapProjection::default().snapshot(ownership, binding)
    }

    /// A stale old-map binding cannot inherit a newer cancellation revision.
    pub(crate) fn admit(
        &self,
        ownership: &Arc<InputRuntimeOwnership>,
        binding: InputPadBinding,
    ) -> Result<Option<QualifiedSourceMap>, PitchActionReject> {
        let placement = binding.placement.ok_or(PitchActionReject::StaleContent)?;
        if self.launch_revision == u64::MAX
            || ownership
                .launch_revision_for_cell(placement.cell())
                .load(Ordering::Acquire)
                != self.launch_revision
        {
            return Err(PitchActionReject::LaunchCancelled);
        }
        if self.request().mode != SourceSyncMode::Mapped {
            return Ok(None);
        }
        let selected = self
            .projection
            .selected
            .as_ref()
            .ok_or(PitchActionReject::TimingUnavailable)?;
        if !selected.current_for(ownership, binding) {
            return Err(PitchActionReject::SourceChanged);
        }
        Ok(Some(selected.clone()))
    }
}
