//! Control-owned selected maps and immutable launch witnesses. No geometry grants authority.

use super::content_placement::ContentKey;
use super::input_runtime_binding::{
    InputPadBinding, InputRuntimeOwnership, InputRuntimePadBinding, SourceSyncWitness,
};
use super::loader_events::LoaderEventDelivery;
use super::loop_phase_reference::GridPhaseOrigin;
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
    pub(crate) ordinary_pending: bool,
    pub(crate) phase_origin_seconds: Option<f64>,
    pub(crate) operational_four_quarter_bars: bool,
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
    phase_origin: Option<GridPhaseOrigin>,
    operational_four_quarter_bars: bool,
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
                phase_origin: None,
                operational_four_quarter_bars: false,
            },
            phase_origin: None,
            operational_four_quarter_bars: false,
        })
    }

    fn with_requested_phase(
        mut self,
        row: &SourceMapRow,
        binding: &InputRuntimePadBinding,
    ) -> Result<Self, String> {
        if row.phase_origin_seconds.is_some() && !binding.current() {
            return Err("requested mapped grid origin binding is stale".into());
        }
        self.phase_origin = row
            .phase_origin_seconds
            .map(|seconds| {
                let frame = (seconds * f64::from(self.loaded_rate)).round();
                if !seconds.is_finite() || !frame.is_finite() || frame.abs() >= (1_u64 << 53) as f64
                {
                    return Err(
                        "requested mapped grid origin has no supported loaded-frame coordinate"
                            .to_owned(),
                    );
                }
                Ok(GridPhaseOrigin {
                    frame_bits: frame.to_bits(),
                    authority_revision: binding.binding.authority_revision,
                })
            })
            .transpose()?;
        self.operational_four_quarter_bars = row.operational_four_quarter_bars;
        self.witness.phase_origin = self.phase_origin;
        self.witness.operational_four_quarter_bars = self.operational_four_quarter_bars;
        Ok(self)
    }

    pub(crate) fn phase_origin(&self) -> Option<GridPhaseOrigin> {
        self.phase_origin
    }

    pub(crate) fn operational_four_quarter_bars(&self) -> bool {
        self.operational_four_quarter_bars
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
            && self.phase_origin.is_none_or(|origin| {
                binding.authority_revision == origin.authority_revision
                    && binding.placement.is_some_and(|placement| {
                        ownership.authority_current(placement.slot(), binding)
                    })
            })
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
            && self.phase_origin == other.phase_origin
            && self.operational_four_quarter_bars == other.operational_four_quarter_bars
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
    ordinary_pending: Option<OrdinaryPendingSource>,
}

/// Captured provenance only; admission still consults the existing source authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OrdinaryPendingSource {
    generation: u64,
    request_incarnation: u64,
}

impl OrdinaryPendingSource {
    fn current(self, ownership: &InputRuntimeOwnership, binding: InputPadBinding) -> bool {
        let Some(placement) = binding.placement else {
            return false;
        };
        ownership.admission_source_current(placement.slot(), binding)
            && ownership.binding_source_generation(placement.slot(), binding)
                == Some(self.generation)
            && ownership.binding_source_request_incarnation(placement.slot(), binding)
                == Some(self.request_incarnation)
    }

    /// Runtime publication owns the placement marker and carries a future runtime
    /// revision. Recheck the original scalar source fields without recapturing it.
    fn publication_fields_current(
        self,
        ownership: &InputRuntimeOwnership,
        binding: InputPadBinding,
    ) -> bool {
        let Some(placement) = binding.placement else {
            return false;
        };
        placement.content_key().is_some()
            && ownership.slot_snapshot_fields_current(placement)
            && ownership.copy_reservation(placement.slot()) == 0
            && ownership.pair_reservation(placement.slot()) == 0
            && ownership.authority_current(placement.slot(), binding)
            && ownership.binding_source_current(placement.slot(), binding)
            && ownership.binding_source_generation(placement.slot(), binding)
                == Some(self.generation)
            && ownership.binding_source_request_incarnation(placement.slot(), binding)
                == Some(self.request_incarnation)
    }
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
            ordinary_pending: None,
        }
    }

    pub(crate) fn qualify(
        row: &SourceMapRow,
        binding: Option<&InputRuntimePadBinding>,
    ) -> Result<Self, String> {
        if row.ordinary_pending && row.delivery.is_some() {
            return Err("ordinary pending permission requires no selected delivery".into());
        }
        let ordinary_pending = if row.ordinary_pending {
            let binding = binding
                .filter(|binding| binding.current())
                .ok_or("ordinary pending permission requires current loaded binding")?;
            let source = OrdinaryPendingSource {
                generation: binding.source_generation,
                request_incarnation: binding
                    .ownership
                    .binding_source_request_incarnation(binding.id, binding.binding)
                    .ok_or("ordinary pending source request is unavailable")?,
            };
            if !source.current(&binding.ownership, binding.binding) {
                return Err("ordinary pending source changed during qualification".into());
            }
            Some(source)
        } else {
            None
        };
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
                let binding =
                    binding.ok_or("selected source map requires current loaded binding")?;
                let mut selected = delivery
                    .qualify_source_map(binding, row.exact_quarters)?
                    .with_requested_phase(row, binding)?;
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
            ordinary_pending,
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
            && self.ordinary_pending == other.ordinary_pending
            && self.same_selected_projection(other)
    }

    /// Raw request publication retains an existing map only through its exact
    /// original delivery owner, source, geometry and immutable revision.
    pub(crate) fn same_selected_projection(&self, other: &Self) -> bool {
        if self.ordinary_pending != other.ordinary_pending {
            return false;
        }
        match (&self.selected, &other.selected) {
            (Some(left), Some(right)) => left.same_projection(right),
            (None, None) => true,
            _ => false,
        }
    }

    /// Data-only selection changes never invalidate ordinary unmapped work.
    pub(crate) fn same_launch_projection(&self, other: &Self) -> bool {
        if self.ordinary_pending != other.ordinary_pending {
            return false;
        }
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
        if self.ordinary_pending.is_some_and(|source| {
            !binding.is_some_and(|binding| source.publication_fields_current(ownership, binding))
        }) {
            return false;
        }
        self.selected.as_ref().is_none_or(|selected| {
            binding.is_some_and(|binding| selected.current_for(ownership, binding))
        })
    }

    pub(crate) fn permits_ordinary_pending(&self) -> bool {
        self.ordinary_pending.is_some()
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
        let Some(selected) = self.projection.selected.as_ref() else {
            let Some(source) = self.projection.ordinary_pending else {
                return Err(PitchActionReject::TimingUnavailable);
            };
            return if source.current(ownership, binding) {
                Ok(None)
            } else {
                Err(PitchActionReject::SourceChanged)
            };
        };
        if !selected.current_for(ownership, binding) {
            return Err(PitchActionReject::SourceChanged);
        }
        Ok(Some(selected.clone()))
    }
}
