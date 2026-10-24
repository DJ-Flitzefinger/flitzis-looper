//! Original ordinary-start ownership, allocated before residency can replace a window.
//!
//! The owner contains no requested pitch or grid calculation. The existing callback
//! freezes the actual mixer tuple and ordinary launch target into its separate work.

use super::AudioEngine;
use super::constant_timing::CurrentTimingAcknowledgements;
use super::content_placement::{ContentKey, PlacementObservation};
use super::input_runtime_binding::{
    InputPadBinding, InputRuntimeOwnership, InputRuntimePadBinding,
};
use super::key_lock_start_policy::{KeyLockStartBehavior, KeyLockStartLease};
use super::mapped_source_clock::MappedLoopGeometry;
use super::ordinary_start::OrdinaryStartWork;
use super::pitch_actions::PitchActionReject;
use super::resident_relocation::{ResidentLaunchGuard, ResidentWindowTicket};
use super::source_map_admission::{
    QualifiedSourceMap, RuntimeMapClass, RuntimeMapPreparation, SourceMapLaunchSnapshot,
};
use super::source_reader::{FrameRange, effective_loop_region};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};

type InputOwners = Option<(Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicBool>)>;

#[derive(Clone)]
struct PublishedBinding {
    binding: InputPadBinding,
    resident_guard: Option<ResidentLaunchGuard>,
}

pub(crate) struct MappedStartSource {
    qualified: QualifiedSourceMap,
    geometry: MappedLoopGeometry,
}

/// One voice/retirement payload retains its effective immutable map and any genuine launch.
/// Runtime conversion creates no launch. Pending start work uses only its own original geometry.
pub(crate) struct PadStartCustody {
    original: Option<CapturedPadStart>,
    effective: Option<Arc<MappedStartSource>>,
}

impl PadStartCustody {
    /// Called only after the existing complete mapped launch claim succeeds.
    pub(crate) fn for_mapped_voice(original: &CapturedPadStart) -> Self {
        Self {
            original: Some(original.clone()),
            effective: Some(
                original
                    .mapped_source_owner()
                    .expect("claimed mapped launch retains original immutable map"),
            ),
        }
    }

    pub(crate) fn original_launch(&self) -> Option<&CapturedPadStart> {
        self.original.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn original(&self) -> &CapturedPadStart {
        self.original_launch()
            .expect("original mapped launch fixture retains its genuine launch")
    }

    /// The actual policy claim already owns the snapshot's original prepared map.
    /// A living scalar birth gains immutable map custody without inventing a launch action.
    pub(crate) fn for_runtime_map(source: &Arc<MappedStartSource>) -> Self {
        Self {
            original: None,
            effective: Some(source.clone()),
        }
    }

    /// Keep the genuine original launch and move only a displaced effective map to retirement.
    /// The caller reserves that existing worker-retirement slot before its atomic effect.
    pub(crate) fn replace_effective_map(
        &mut self,
        source: &Arc<MappedStartSource>,
    ) -> Option<Self> {
        if self
            .effective
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, source))
        {
            return None;
        }
        self.effective
            .replace(source.clone())
            .map(|effective| Self {
                original: None,
                effective: Some(effective),
            })
    }

    pub(crate) fn effective_source(&self) -> Option<&Arc<MappedStartSource>> {
        self.effective.as_ref()
    }

    pub(crate) fn mapped_geometry(&self) -> Option<&MappedLoopGeometry> {
        self.effective_source().map(|source| source.geometry())
    }

    pub(crate) fn source_sync_witness(
        &self,
        key: ContentKey,
        source_generation: u64,
    ) -> Option<super::input_runtime_binding::SourceSyncWitness> {
        self.effective_source()?
            .qualified
            .sync_witness_for(key, source_generation)
    }
}

impl From<CapturedPadStart> for PadStartCustody {
    /// A raw target has no effective voice map, but still retires through the same one slot.
    fn from(original: CapturedPadStart) -> Self {
        Self {
            original: Some(original),
            effective: None,
        }
    }
}

/// Scalar pending-admission witness. It holds no Work and cannot create a map owner.
pub(crate) struct MappedStartPendingGuard {
    status: Arc<AtomicU8>,
    ownership: Arc<InputRuntimeOwnership>,
    binding: InputPadBinding,
    key: ContentKey,
    launch_cancel_revision: u64,
}

impl MappedStartPendingGuard {
    pub(crate) fn current(&self, source: &MappedStartSource) -> bool {
        self.status.load(Ordering::Acquire) != REJECTED
            && self
                .ownership
                .launch_revision_for_cell(self.key.cell())
                .load(Ordering::Acquire)
                == self.launch_cancel_revision
            && source.qualified.current_for(&self.ownership, self.binding)
    }
}

impl MappedStartSource {
    pub(crate) fn geometry(&self) -> &MappedLoopGeometry {
        &self.geometry
    }

    pub(crate) fn sync_witness(&self) -> super::input_runtime_binding::SourceSyncWitness {
        self.qualified.sync_witness()
    }

    pub(crate) fn belongs_to(&self, ownership: &Arc<InputRuntimeOwnership>) -> bool {
        self.qualified.belongs_to(ownership)
    }

    pub(crate) fn current_for(
        &self,
        ownership: &Arc<InputRuntimeOwnership>,
        binding: InputPadBinding,
    ) -> bool {
        self.qualified.current_for(ownership, binding)
    }

    pub(crate) fn same_source(&self, other: &Self) -> bool {
        self.qualified.same_projection(&other.qualified)
            && self.geometry.domain() == other.geometry.domain()
    }

    /// Prepare one optional live-policy derivative off realtime. Legacy ordinary
    /// launch construction keeps its preceding geometry and availability contract.
    pub(crate) fn prepare_runtime_map(
        qualified: &QualifiedSourceMap,
        binding: InputPadBinding,
        start_s: f64,
        end_s: Option<f64>,
    ) -> (RuntimeMapPreparation, Option<Arc<Self>>) {
        let mut preparation = RuntimeMapPreparation {
            class: RuntimeMapClass::Unsupported,
            domain: None,
            witness: Some(qualified.sync_witness()),
        };
        if let Ok(mut source) = Self::prepare(qualified.clone(), binding, start_s, end_s) {
            preparation.domain = Some(source.geometry.domain().into());
            if let Ok(geometry) = source.geometry.clone().qualify_exact_continuation()
                && geometry.has_compact_continuation()
            {
                source.geometry = geometry;
                source.geometry.prepare_common_phase();
                preparation.class = RuntimeMapClass::Ready;
                return (preparation, Some(Arc::new(source)));
            }
        }
        // The old displayed inverse and rounded endpoint are not a prerequisite
        // of this separate exact original-map class. A failed legacy attempt must
        // not narrow either general extent class.
        let Ok((rate, sample_frames, region)) =
            Self::loaded_region(qualified, binding, start_s, end_s)
        else {
            return (preparation, None);
        };
        let geometry = match qualified.exact_quarters() {
            Some(quarters) => MappedLoopGeometry::new_general_declared(
                qualified.map().clone(),
                sample_frames,
                rate,
                region,
                quarters,
                true,
            ),
            // Manual physical intent retains its existing meter contract and
            // never inherits saved bars or a displayed inverse difference.
            None => MappedLoopGeometry::new_general_physical(
                qualified.map().clone(),
                sample_frames,
                rate,
                region,
                false,
            ),
        };
        let Ok(mut geometry) = geometry else {
            return (preparation, None);
        };
        preparation.domain = Some(geometry.domain().into());
        geometry.prepare_common_phase();
        preparation.class = RuntimeMapClass::Ready;
        let source = Self {
            qualified: qualified.clone(),
            geometry,
        };
        (preparation, Some(Arc::new(source)))
    }

    /// Reuse the same persisted integer-marker interpretation for both classes.
    fn loaded_region(
        qualified: &QualifiedSourceMap,
        binding: InputPadBinding,
        start_s: f64,
        end_s: Option<f64>,
    ) -> Result<(u32, usize, FrameRange), PitchActionReject> {
        let rate = qualified.loaded_rate();
        if rate == 0
            || binding.sample_rate_hz != rate
            || binding.channels == 0
            || !binding.sample_count.is_multiple_of(binding.channels)
        {
            return Err(PitchActionReject::SourceChanged);
        }
        let sample_frames = binding.sample_count / binding.channels;
        let start = start_s * f64::from(rate);
        let end = end_s.map(|end| end * f64::from(rate));
        if !start.is_finite() || end.is_some_and(|end| !end.is_finite()) {
            return Err(PitchActionReject::InvalidAction);
        }
        let region = effective_loop_region(
            start.round() as usize,
            end.map(|end| end.round() as usize),
            sample_frames,
        )
        .ok_or(PitchActionReject::InvalidAction)?;
        Ok((rate, sample_frames, region))
    }

    /// Control/MIDI capture only. Coverage scans and owning geometry never originate on RT.
    fn prepare(
        qualified: QualifiedSourceMap,
        binding: InputPadBinding,
        start_s: f64,
        end_s: Option<f64>,
    ) -> Result<Self, PitchActionReject> {
        let (rate, sample_frames, region) =
            Self::loaded_region(&qualified, binding, start_s, end_s)?;
        let exact = qualified.exact_quarters();
        let quarters = match exact {
            Some(quarters) => quarters,
            None => {
                let start = qualified
                    .map()
                    .canonical_beat_at_source(region.start as f64 / f64::from(rate));
                let end = qualified
                    .map()
                    .canonical_beat_at_source(region.end as f64 / f64::from(rate));
                end.zip(start)
                    .map(|(end, start)| end - start)
                    .ok_or(PitchActionReject::TimingUnavailable)?
            }
        };
        let geometry = MappedLoopGeometry::new(
            qualified.map().clone(),
            sample_frames,
            rate,
            region,
            quarters,
            exact.is_some(),
        )
        .map_err(|_| PitchActionReject::TimingUnavailable)?;
        Ok(Self {
            qualified,
            geometry,
        })
    }
}

struct PadStartOwner {
    start_policy: KeyLockStartLease,
    key: ContentKey,
    binding: InputPadBinding,
    ownership: Arc<InputRuntimeOwnership>,
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
    source_generation: u64,
    source_request_incarnation: u64,
    timing_intent: u8,
    launch_cancel_revision: u64,
    loop_start_s: f64,
    loop_end_s: Option<f64>,
    volume: f32,
    exclusive: bool,
    received_at_ns: u64,
    sequence: u64,
    mapped: Option<Arc<MappedStartSource>>,
    sync_request: super::source_sync_policy::SourceSyncRequest,
    input_owners: InputOwners,
    work: Mutex<Option<Box<OrdinaryStartWork>>>,
    publication: OnceLock<PublishedBinding>,
    window_prepared: AtomicBool,
    published: AtomicBool,
    admitted: AtomicBool,
    status: Arc<AtomicU8>,
    reason: AtomicU8,
    effective_frame: AtomicU64,
    target_captured: AtomicBool,
    frames_state: AtomicU8,
    original_frame: AtomicU64,
    source_control_revision: AtomicU64,
    execution_frame: AtomicU64,
}

const CAPTURED: u8 = 0;
const PREPARING: u8 = 1;
const PREPARED: u8 = 2;
const SCHEDULED: u8 = 3;
const APPLYING: u8 = 4;
const APPLIED: u8 = 5;
const REJECTED: u8 = 6;

/// One original GUI/MIDI/global attack. Python fields are observations only.
#[pyclass(frozen)]
#[derive(Clone)]
pub struct CapturedPadStart {
    owner: Arc<PadStartOwner>,
}

impl std::fmt::Debug for CapturedPadStart {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapturedPadStart")
            .field("key", &self.owner.key)
            .field("sequence", &self.owner.sequence)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PadStartGuard {
    pub(crate) id: usize,
    pub(crate) binding: InputPadBinding,
}

/// Fixed Native source/ACK ownership used only to share physical readiness.
/// It retains neither an attack, its Work, nor a Native adapter candidate.
pub(crate) struct PadStartReadinessSeal {
    key: ContentKey,
    binding: InputPadBinding,
    ownership: Arc<InputRuntimeOwnership>,
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
    source_generation: u64,
    source_request_incarnation: u64,
    timing_intent: u8,
    launch_cancel_revision: u64,
}

impl PadStartReadinessSeal {
    pub(crate) fn capture(binding: &InputRuntimePadBinding) -> Result<Self, PitchActionReject> {
        let ownership = &binding.ownership;
        let key = binding
            .binding
            .placement
            .and_then(|placement| placement.content_key())
            .ok_or(PitchActionReject::StaleContent)?;
        let mut original = binding.binding;
        original.runtime_revision = ownership
            .runtime_for_cell(key.cell())
            .load(Ordering::Acquire);
        Ok(Self {
            key,
            binding: original,
            ownership: ownership.clone(),
            acknowledgements: binding.acknowledgements.clone(),
            source_generation: ownership
                .binding_source_generation(binding.id, original)
                .ok_or(PitchActionReject::SourceChanged)?,
            source_request_incarnation: ownership
                .binding_source_request_incarnation(binding.id, original)
                .ok_or(PitchActionReject::SourceChanged)?,
            timing_intent: ownership
                .binding_timing_intent(binding.id, original)
                .ok_or(PitchActionReject::TimingChanged)?,
            launch_cancel_revision: ownership
                .launch_revision_for_cell(key.cell())
                .load(Ordering::Acquire),
        })
    }
}

impl CapturedPadStart {
    /// A real retained request may admit a new member only for its original
    /// envelope or its own genuinely adopted binding. Equal Python fields grant
    /// no membership; the caller must hold that exact Native readiness record.
    pub(crate) fn matches_readiness_seal(
        &self,
        seal: &PadStartReadinessSeal,
        adopted: Option<InputPadBinding>,
    ) -> bool {
        let owner = &self.owner;
        if owner.key != seal.key
            || !Arc::ptr_eq(&owner.ownership, &seal.ownership)
            || !Arc::ptr_eq(&owner.acknowledgements, &seal.acknowledgements)
            || owner.source_generation != seal.source_generation
            || owner.source_request_incarnation != seal.source_request_incarnation
            || owner.timing_intent != seal.timing_intent
            || owner.launch_cancel_revision != seal.launch_cancel_revision
        {
            return false;
        }
        let placement = match owner.ownership.observe_control_key(owner.key) {
            PlacementObservation::Current(placement) => placement,
            _ => return false,
        };
        let mut original = owner.binding;
        original.placement = Some(placement);
        let matches = |mut candidate: InputPadBinding| {
            candidate.placement = original.placement;
            candidate.runtime_revision = original.runtime_revision;
            original.same_projection(&candidate)
        };
        // A physical source/ACK seal is reusable after benign RuntimeState
        // projections. Each member still validates its OWN captured runtime and
        // input owners below; another member's older revision grants no authority.
        (matches(seal.binding) || adopted.is_some_and(matches))
            && self
                .scalar_guard(placement.slot(), original, true, false)
                .is_ok()
    }
    // The complete source/intent envelope is deliberately supplied in one capture.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn capture_from_binding(
        binding: &InputRuntimePadBinding,
        start_s: f64,
        end_s: Option<f64>,
        volume: f32,
        exclusive: bool,
        received_at_ns: u64,
        render_format: (u32, usize),
        input_owners: InputOwners,
    ) -> Result<Self, PitchActionReject> {
        Self::capture_parts(
            binding.binding,
            binding.ownership.clone(),
            binding.acknowledgements.clone(),
            start_s,
            end_s,
            volume,
            exclusive,
            received_at_ns,
            render_format,
            input_owners,
            binding.source_map_launch_snapshot(),
        )
    }

    // MIDI already owns these exact native fields; private source metadata is not rebuilt.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn capture_parts(
        mut binding: InputPadBinding,
        ownership: Arc<InputRuntimeOwnership>,
        acknowledgements: Arc<CurrentTimingAcknowledgements>,
        start_s: f64,
        end_s: Option<f64>,
        volume: f32,
        exclusive: bool,
        received_at_ns: u64,
        render_format: (u32, usize),
        input_owners: InputOwners,
        source_map: SourceMapLaunchSnapshot,
    ) -> Result<Self, PitchActionReject> {
        if !start_s.is_finite()
            || start_s < 0.0
            || end_s.is_some_and(|end| !end.is_finite() || end < 0.0)
            || !volume.is_finite()
            || !(super::constants::VOLUME_MIN..=super::constants::VOLUME_MAX).contains(&volume)
        {
            return Err(PitchActionReject::InvalidAction);
        }
        let placement = binding.placement.ok_or(PitchActionReject::StaleContent)?;
        let key = placement
            .content_key()
            .ok_or(PitchActionReject::StaleContent)?;
        binding.runtime_revision = ownership
            .runtime_for_cell(key.cell())
            .load(Ordering::Acquire);
        if !ownership.admission_current(placement.slot(), binding) {
            return Err(PitchActionReject::StaleContent);
        }
        let source_generation = ownership
            .binding_source_generation(placement.slot(), binding)
            .ok_or(PitchActionReject::SourceChanged)?;
        let source_request_incarnation = ownership
            .binding_source_request_incarnation(placement.slot(), binding)
            .ok_or(PitchActionReject::SourceChanged)?;
        let timing_intent = ownership
            .binding_timing_intent(placement.slot(), binding)
            .ok_or(PitchActionReject::TimingChanged)?;
        let launch_cancel_revision = ownership
            .launch_revision_for_cell(key.cell())
            .load(Ordering::Acquire);
        let sync_request = source_map.request();
        let mapped = source_map
            .admit(&ownership, binding)?
            .map(|qualified| {
                MappedStartSource::prepare(qualified, binding, start_s, end_s).map(Arc::new)
            })
            .transpose()?;
        let start_policy = ownership
            .register_key_lock_start_owner()
            .map_err(PitchActionReject::from_start_policy_admission)?;
        let work = OrdinaryStartWork::new(render_format.0, render_format.1)
            .map_err(|_| PitchActionReject::PreparationFailed)?;
        let sequence = work.sequence();
        let start = Self {
            owner: Arc::new(PadStartOwner {
                start_policy,
                key,
                binding,
                ownership,
                acknowledgements,
                source_generation,
                source_request_incarnation,
                timing_intent,
                launch_cancel_revision,
                loop_start_s: start_s,
                loop_end_s: end_s,
                volume,
                exclusive,
                received_at_ns,
                sequence,
                mapped,
                sync_request,
                input_owners,
                work: Mutex::new(Some(work)),
                publication: OnceLock::new(),
                window_prepared: AtomicBool::new(false),
                published: AtomicBool::new(false),
                admitted: AtomicBool::new(false),
                status: Arc::new(AtomicU8::new(CAPTURED)),
                reason: AtomicU8::new(0),
                effective_frame: AtomicU64::new(0),
                target_captured: AtomicBool::new(false),
                frames_state: AtomicU8::new(0),
                original_frame: AtomicU64::new(0),
                source_control_revision: AtomicU64::new(u64::MAX),
                execution_frame: AtomicU64::new(0),
            }),
        };
        #[cfg(test)]
        capture_pending_tests::after_owner();
        // Capture already proved this original source/request/timing envelope.
        // A later odd marker grants no publication authority, but must not erase
        // the owner: the existing readiness path waits on its original key.
        match start.control_guard() {
            Ok(_) | Err(PitchActionReject::PlacementPending) => {}
            Err(reason) => return Err(reason),
        }
        Ok(start)
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.owner.sequence
    }
    /// Borrow immutable admitted geometry; its original owner retains all source/artifact pins.
    pub(crate) fn mapped_geometry(&self) -> Option<&MappedLoopGeometry> {
        self.owner.mapped.as_ref().map(|mapped| &mapped.geometry)
    }

    pub(crate) fn sync_request(&self) -> super::source_sync_policy::SourceSyncRequest {
        self.owner.sync_request
    }

    /// Share only the original immutable qualified map, never this owner's mutable Work.
    pub(crate) fn mapped_source_owner(&self) -> Option<Arc<MappedStartSource>> {
        self.owner.mapped.clone()
    }

    pub(crate) fn mapped_pending_guard(
        &self,
    ) -> Result<MappedStartPendingGuard, PitchActionReject> {
        let guard = self.callback_guard()?;
        Ok(MappedStartPendingGuard {
            status: self.owner.status.clone(),
            ownership: self.owner.ownership.clone(),
            binding: guard.binding,
            key: self.owner.key,
            launch_cancel_revision: self.owner.launch_cancel_revision,
        })
    }

    /// Actual immutable launch witness; later requested runtime/map changes do not rewrite it.
    #[cfg(test)]
    pub(crate) fn source_sync_witness(
        &self,
        key: ContentKey,
        source_generation: u64,
    ) -> Option<super::input_runtime_binding::SourceSyncWitness> {
        (self.owner.key == key && self.owner.source_generation == source_generation)
            .then(|| {
                self.owner
                    .mapped
                    .as_ref()
                    .and_then(|mapped| mapped.qualified.sync_witness_for(key, source_generation))
            })
            .flatten()
    }

    /// Admission/claim only. An already effective reader never polls the old job token.
    pub(crate) fn mapped_source_current(&self, binding: InputPadBinding) -> bool {
        self.owner
            .mapped
            .as_ref()
            .is_none_or(|mapped| mapped.qualified.current_for(&self.owner.ownership, binding))
    }
    pub(crate) fn start_behavior(&self) -> KeyLockStartBehavior {
        self.owner.start_policy.behavior()
    }
    pub(crate) fn ownership(&self) -> &Arc<InputRuntimeOwnership> {
        &self.owner.ownership
    }
    pub(crate) fn acknowledgements(&self) -> &Arc<CurrentTimingAcknowledgements> {
        &self.owner.acknowledgements
    }
    pub(crate) fn content_key(&self) -> ContentKey {
        self.owner.key
    }
    pub(crate) fn volume(&self) -> f32 {
        self.owner.volume
    }
    pub(crate) fn exclusive(&self) -> bool {
        self.owner.exclusive
    }
    pub(crate) fn loop_region(&self) -> (f64, Option<f64>) {
        (self.owner.loop_start_s, self.owner.loop_end_s)
    }
    pub(crate) fn received_timestamp(&self) -> u64 {
        self.owner.received_at_ns
    }
    pub(crate) fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &other.owner)
    }
    pub(crate) fn belongs_to(&self, ownership: &Arc<InputRuntimeOwnership>) -> bool {
        Arc::ptr_eq(&self.owner.ownership, ownership)
    }
    pub(crate) fn is_published(&self) -> bool {
        self.owner.published.load(Ordering::Acquire)
    }

    fn input_current(&self) -> bool {
        self.is_published()
            || self
                .owner
                .input_owners
                .as_ref()
                .is_none_or(|(mapping, enabled, learn)| {
                    mapping.load(Ordering::Acquire)
                        && enabled.load(Ordering::Acquire)
                        && !learn.load(Ordering::Acquire)
                })
    }

    fn scalar_guard(
        &self,
        id: usize,
        mut binding: InputPadBinding,
        own_window: bool,
        require_window: bool,
    ) -> Result<PadStartGuard, PitchActionReject> {
        let owner = &self.owner;
        let ownership = &owner.ownership;
        let original = owner.binding;
        if owner.status.load(Ordering::Acquire) == REJECTED {
            return Err(PitchActionReject::StaleContent);
        }
        // A settled historical effect keeps its observation contract after a stopped edit.
        if owner.status.load(Ordering::Acquire) != APPLIED
            && self.start_behavior() != ownership.key_lock_start_behavior()
        {
            return Err(PitchActionReject::PreparedMismatch);
        }
        if !self.input_current() {
            return Err(PitchActionReject::InputDisabled);
        }
        if owner.status.load(Ordering::Acquire) != APPLIED && !self.mapped_source_current(binding) {
            return Err(PitchActionReject::SourceChanged);
        }
        if !ownership.content_key_current(owner.key)
            || binding
                .placement
                .and_then(|placement| placement.content_key())
                != Some(owner.key)
        {
            return Err(PitchActionReject::StaleContent);
        }
        if ownership
            .launch_revision_for_cell(owner.key.cell())
            .load(Ordering::Acquire)
            != owner.launch_cancel_revision
        {
            return Err(PitchActionReject::LaunchCancelled);
        }
        if binding.source_address != original.source_address
            || binding.sample_count != original.sample_count
            || binding.channels != original.channels
            || binding.sample_rate_hz != original.sample_rate_hz
            || binding.authority_revision != original.authority_revision
            || !ownership.authority_current(id, binding)
            || ownership.binding_source_generation(id, binding) != Some(owner.source_generation)
            || ownership.key_source_request_incarnation(owner.key)
                != Some(owner.source_request_incarnation)
        {
            return Err(PitchActionReject::SourceChanged);
        }
        if binding.accepted != original.accepted
            || ownership.binding_timing_intent(id, binding) != Some(owner.timing_intent)
            || owner
                .acknowledgements
                .current_for_binding(ownership, id, binding)
                != original
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
            || !ownership.source_timing_available_for_binding(id, binding, &owner.acknowledgements)
        {
            return Err(PitchActionReject::TimingChanged);
        }
        let runtime_revision = ownership
            .runtime_for_cell(owner.key.cell())
            .load(Ordering::Acquire);
        if !self.is_published()
            && runtime_revision != original.runtime_revision
            && !(own_window && Some(runtime_revision) == original.runtime_revision.checked_add(1))
        {
            return Err(PitchActionReject::StaleContent);
        }
        if require_window && !ownership.binding_window_current(id, binding) {
            // Publication already owns an accepted exact window. A later
            // replacement cannot make that old window ready again at this E.
            return Err(if self.is_published() {
                PitchActionReject::SourceChanged
            } else {
                PitchActionReject::WindowPending
            });
        }
        if let Some(publication) = owner.publication.get()
            && publication
                .resident_guard
                .as_ref()
                .is_some_and(|guard| !guard.current())
        {
            return Err(PitchActionReject::SourceChanged);
        }
        binding.runtime_revision = runtime_revision;
        Ok(PadStartGuard { id, binding })
    }

    /// Validate the retained worker inputs through their original stable content cell.
    /// The historical placement selects binding-aware scalars only; it grants no slot claim.
    pub(crate) fn preparation_guard(&self) -> Result<(), PitchActionReject> {
        let placement = self
            .owner
            .binding
            .placement
            .ok_or(PitchActionReject::StaleContent)?;
        let publication = self.owner.publication.get();
        let mut binding = publication.map_or(self.owner.binding, |value| value.binding);
        binding.placement = Some(placement);
        self.scalar_guard(
            placement.slot(),
            binding,
            publication.is_some_and(|value| value.resident_guard.is_some()),
            true,
        )
        .map(|_| ())
    }

    pub(crate) fn control_guard(&self) -> Result<PadStartGuard, PitchActionReject> {
        let placement = match self.owner.ownership.observe_control_key(self.owner.key) {
            PlacementObservation::Current(placement) => placement,
            PlacementObservation::Pending => return Err(PitchActionReject::PlacementPending),
            PlacementObservation::Stale | PlacementObservation::Retired => {
                return Err(PitchActionReject::StaleContent);
            }
        };
        let publication = self.owner.publication.get();
        let mut binding = publication.map_or(self.owner.binding, |value| value.binding);
        binding.placement = Some(placement);
        self.scalar_guard(
            placement.slot(),
            binding,
            publication.is_some_and(|value| value.resident_guard.is_some()),
            true,
        )
    }

    pub(crate) fn callback_guard(&self) -> Result<PadStartGuard, PitchActionReject> {
        let ownership = &self.owner.ownership;
        let id = ownership.callback_key_slot(self.owner.key).ok_or_else(|| {
            if ownership.content_key_current(self.owner.key) {
                PitchActionReject::PlacementPending
            } else {
                PitchActionReject::StaleContent
            }
        })?;
        let placement = ownership
            .slot_snapshot(id)
            .ok_or(PitchActionReject::PlacementPending)?;
        let publication = self.owner.publication.get();
        let mut binding = publication.map_or(self.owner.binding, |value| value.binding);
        binding.placement = Some(placement);
        self.scalar_guard(
            id,
            binding,
            publication.is_some_and(|value| value.resident_guard.is_some()),
            true,
        )
    }

    /// Recheck inside the existing complete mutation marker; never acquires a lock.
    pub(crate) fn claim_current(&self, guard: PadStartGuard) -> bool {
        let ownership = &self.owner.ownership;
        guard.binding.placement.is_some_and(|placement| {
            placement.slot() == guard.id && ownership.slot_snapshot_fields_current(placement)
        }) && ownership.callback_key_slot(self.owner.key) == Some(guard.id)
            && self
                .scalar_guard(
                    guard.id,
                    guard.binding,
                    self.owner
                        .publication
                        .get()
                        .is_some_and(|value| value.resident_guard.is_some()),
                    true,
                )
                .is_ok()
    }

    pub(crate) fn preparation_slot(&self, engine: &AudioEngine) -> PyResult<usize> {
        if !self.belongs_to(&engine.input_runtime_ownership) {
            return Err(PyValueError::new_err("foreign captured pad start"));
        }
        self.control_guard()
            .map(|guard| guard.id)
            .map_err(|reason| PyValueError::new_err(reason.as_str()))
    }

    pub(crate) fn validates_preparation_binding(
        &self,
        id: usize,
        mut binding: InputPadBinding,
    ) -> bool {
        let Some(placement) = self.owner.ownership.slot_snapshot(id) else {
            return false;
        };
        let mut original = self.owner.binding;
        original.placement = Some(placement);
        binding.placement = Some(placement);
        binding.runtime_revision = original.runtime_revision;
        original.same_projection(&binding) && self.scalar_guard(id, binding, false, true).is_ok()
    }

    pub(crate) fn note_window_prepared(&self) {
        self.owner.window_prepared.store(true, Ordering::Release);
    }

    /// The ticket itself proves its exact retained owner; numeric fields grant no association.
    pub(crate) fn take_work(
        &self,
        window: Option<&ResidentWindowTicket>,
    ) -> PyResult<Option<Box<OrdinaryStartWork>>> {
        let (binding, resident_guard) = match window {
            Some(window) => {
                let Some((binding, guard)) = window.capture_pad_start_launch_guard(self)? else {
                    return Ok(None);
                };
                (binding, Some(guard))
            }
            None => (self.owner.binding, None),
        };
        self.take_work_with_binding(binding, resident_guard)
            .map_err(|reason| PyValueError::new_err(reason.as_str()))
    }

    /// Stage the exact Work; only the caller's successful complete claim publishes it.
    /// MIDI/global callers have already validated their original ticket association.
    pub(crate) fn preflight_work_with_binding(
        &self,
        mut binding: InputPadBinding,
        resident_guard: Option<&ResidentLaunchGuard>,
    ) -> Result<PadStartGuard, PitchActionReject> {
        if self.is_published() {
            return Err(PitchActionReject::AlreadyPublished);
        }
        let placement = match self.owner.ownership.observe_control_key(self.owner.key) {
            PlacementObservation::Current(placement) => placement,
            PlacementObservation::Pending => return Err(PitchActionReject::PlacementPending),
            _ => return Err(PitchActionReject::StaleContent),
        };
        if resident_guard.is_some_and(|guard| !guard.current())
            || (resident_guard.is_none() && binding.resident != self.owner.binding.resident)
        {
            return Err(PitchActionReject::WindowPending);
        }
        binding.placement = Some(placement);
        let guard = self.scalar_guard(placement.slot(), binding, resident_guard.is_some(), true)?;
        if let Some(previous) = self.owner.publication.get() {
            let mut previous_binding = previous.binding;
            previous_binding.placement = guard.binding.placement;
            previous_binding.runtime_revision = guard.binding.runtime_revision;
            if previous.resident_guard.as_ref() != resident_guard
                || !previous_binding.same_projection(&guard.binding)
            {
                return Err(PitchActionReject::PreparedMismatch);
            }
        }
        Ok(guard)
    }

    pub(crate) fn take_work_with_binding(
        &self,
        binding: InputPadBinding,
        resident_guard: Option<ResidentLaunchGuard>,
    ) -> Result<Option<Box<OrdinaryStartWork>>, PitchActionReject> {
        let mut retained = self
            .owner
            .work
            .lock()
            .map_err(|_| PitchActionReject::PreparationFailed)?;
        if self.is_published() {
            return Ok(None);
        }
        let guard = self.preflight_work_with_binding(binding, resident_guard.as_ref())?;
        if let Some(previous) = self.owner.publication.get() {
            let mut previous_binding = previous.binding;
            // Placement follows the original ContentKey. A benign projection may
            // refresh its runtime scalar, but it grants no new source/window owner.
            previous_binding.placement = guard.binding.placement;
            previous_binding.runtime_revision = guard.binding.runtime_revision;
            if previous.resident_guard != resident_guard
                || !previous_binding.same_projection(&guard.binding)
            {
                return Err(PitchActionReject::PreparedMismatch);
            }
        } else {
            self.owner
                .publication
                .set(PublishedBinding {
                    binding: guard.binding,
                    resident_guard,
                })
                .map_err(|_| PitchActionReject::PreparedMismatch)?;
        }
        self.control_guard()?;
        let Some(work) = retained.as_mut() else {
            return Err(PitchActionReject::AlreadyPublished);
        };
        work.bind_origin(self.clone())?;
        Ok(retained.take())
    }

    /// Control-side rollback retains the same unused Work after a failed publication
    /// claim. Invalid/foreign or already-published Work stays with the caller.
    pub(crate) fn restore_unpublished_work(
        &self,
        mut work: Box<OrdinaryStartWork>,
    ) -> Result<(), Box<OrdinaryStartWork>> {
        let Ok(mut retained) = self.owner.work.lock() else {
            return Err(work);
        };
        if self.is_published() || retained.is_some() {
            return Err(work);
        }
        if work.detach_unpublished_origin(self).is_err() {
            return Err(work);
        }
        *retained = Some(work);
        Ok(())
    }

    /// Scalar-only final operation inside an already validated complete Native
    /// publication claim. It never observes control placement or acquires a lock.
    pub(crate) fn mark_published_claimed(&self) -> bool {
        self.owner
            .published
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Publication freezes the input owners after complete source/ticket preflight.
    pub(crate) fn mark_published(&self) -> Result<(), PitchActionReject> {
        self.control_guard()?;
        if self.mark_published_claimed() {
            Ok(())
        } else {
            Err(PitchActionReject::AlreadyPublished)
        }
    }

    /// The existing ordinary scheduler supplies T and E exactly once; no grid lives here.
    pub(crate) fn freeze_frames(
        &self,
        original: u64,
        execution: u64,
    ) -> Result<(), PitchActionReject> {
        if execution < original {
            return Err(PitchActionReject::PreparedMismatch);
        }
        if self.target_frame() != Some(original) {
            return Err(PitchActionReject::PreparedMismatch);
        }
        self.owner
            .frames_state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PitchActionReject::PreparedMismatch)?;
        self.owner
            .execution_frame
            .store(execution, Ordering::Release);
        self.owner.frames_state.store(2, Ordering::Release);
        Ok(())
    }

    /// The single callback may retain this same original for another real worker endpoint.
    /// Only E changes: T, source/action ownership and cancellation guards stay untouched.
    pub(crate) fn replan_execution_frame(
        &self,
        expected: u64,
        execution: u64,
    ) -> Result<(), PitchActionReject> {
        self.preparation_guard()?;
        if self.owner.frames_state.load(Ordering::Acquire) != 2
            || self
                .target_frame()
                .is_none_or(|original| original > expected)
            || self.owner.execution_frame.load(Ordering::Acquire) != expected
            || execution <= expected
        {
            return Err(PitchActionReject::PreparedMismatch);
        }
        let status = self.owner.status.load(Ordering::Acquire);
        if !matches!(status, PREPARING | PREPARED | SCHEDULED) {
            return Err(PitchActionReject::StaleContent);
        }
        self.owner
            .status
            .compare_exchange(status, PREPARING, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PitchActionReject::StaleContent)?;
        self.owner
            .execution_frame
            .compare_exchange(expected, execution, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PitchActionReject::PreparedMismatch)?;
        Ok(())
    }

    /// Called only by the single callback, before any own residency publication.
    pub(crate) fn capture_target(
        &self,
        original: u64,
        source_control_revision: u64,
    ) -> Result<(), PitchActionReject> {
        if self.owner.target_captured.load(Ordering::Acquire) {
            return Err(PitchActionReject::PreparedMismatch);
        }
        if source_control_revision == u64::MAX {
            return Err(PitchActionReject::SequenceExhausted);
        }
        self.owner.original_frame.store(original, Ordering::Relaxed);
        self.owner
            .source_control_revision
            .store(source_control_revision, Ordering::Relaxed);
        self.owner.target_captured.store(true, Ordering::Release);
        Ok(())
    }

    pub(crate) fn source_control_revision(&self) -> Option<u64> {
        self.owner
            .target_captured
            .load(Ordering::Acquire)
            .then(|| self.owner.source_control_revision.load(Ordering::Relaxed))
    }

    /// Preserve T's immutable fence. Only its exact retained dependency may
    /// account for the intervening successful loop commit; later edits still fail.
    pub(crate) fn accepts_source_control_revision(&self, current: u64) -> bool {
        let Some(original) = self.source_control_revision() else {
            return false;
        };
        original != u64::MAX
            && current != u64::MAX
            && (original == current
                || self
                    .owner
                    .publication
                    .get()
                    .and_then(|publication| publication.resident_guard.as_ref())
                    .is_some_and(|guard| guard.owns_source_control_transition(original, current)))
    }

    pub(crate) fn target_frame(&self) -> Option<u64> {
        self.owner
            .target_captured
            .load(Ordering::Acquire)
            .then(|| self.owner.original_frame.load(Ordering::Relaxed))
    }

    /// A group may undo its claims only before its atomic audio effect has begun.
    pub(crate) fn abort_claim(&self, reason: PitchActionReject) {
        self.owner.reason.store(reason as u8, Ordering::Release);
        if self
            .owner
            .status
            .compare_exchange(APPLYING, REJECTED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.owner.start_policy.finish();
        }
    }

    pub(crate) fn mark_preparing(&self) -> bool {
        self.owner
            .status
            .compare_exchange(CAPTURED, PREPARING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    pub(crate) fn mark_prepared(&self) -> bool {
        let status = self.owner.status.load(Ordering::Acquire);
        matches!(status, CAPTURED | PREPARING | SCHEDULED)
            && self
                .owner
                .status
                .compare_exchange(status, PREPARED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }
    pub(crate) fn mark_scheduled(&self) -> bool {
        let status = self.owner.status.load(Ordering::Acquire);
        let admitted = matches!(status, CAPTURED | PREPARING | PREPARED)
            && self
                .owner
                .status
                .compare_exchange(status, SCHEDULED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok();
        if admitted {
            self.owner.admitted.store(true, Ordering::Release);
        }
        admitted
    }
    pub(crate) fn claim_effect(&self) -> bool {
        let status = self.owner.status.load(Ordering::Acquire);
        matches!(status, PREPARED | SCHEDULED)
            && self
                .owner
                .status
                .compare_exchange(status, APPLYING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }
    pub(crate) fn mark_effective(&self, output_frame: u64) -> bool {
        if self.owner.status.load(Ordering::Acquire) != APPLYING {
            return false;
        }
        self.owner
            .effective_frame
            .store(output_frame, Ordering::Release);
        self.owner.reason.store(0, Ordering::Release);
        let applied = self
            .owner
            .status
            .compare_exchange(APPLYING, APPLIED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if applied {
            self.owner.start_policy.finish();
        }
        applied
    }
    pub(crate) fn reject(&self, reason: PitchActionReject) -> bool {
        let status = self.owner.status.load(Ordering::Acquire);
        if matches!(status, APPLYING | APPLIED | REJECTED) {
            return false;
        }
        if self
            .owner
            .status
            .compare_exchange(status, REJECTED, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        self.owner.reason.store(reason as u8, Ordering::Release);
        self.owner.start_policy.finish();
        true
    }
}

#[pymethods]
impl CapturedPadStart {
    #[pyo3(name = "same_owner")]
    pub fn python_same_owner(&self, other: &CapturedPadStart) -> bool {
        self.same_owner(other)
    }
    #[getter]
    pub fn diagnostic_slot(&self) -> Option<usize> {
        match self.owner.ownership.observe_control_key(self.owner.key) {
            PlacementObservation::Current(placement) => Some(placement.slot()),
            _ => None,
        }
    }
    pub fn is_current(&self) -> bool {
        self.control_guard().is_ok()
    }
    pub fn original_owner_status(&self) -> &'static str {
        if self.owner.status.load(Ordering::Acquire) == REJECTED {
            return "superseded";
        }
        if self.is_published() {
            return "published";
        }
        let placement = match self.owner.ownership.observe_control_key(self.owner.key) {
            PlacementObservation::Current(placement) => placement,
            PlacementObservation::Pending => return "pending",
            _ => return "retired",
        };
        let mut binding = self.owner.binding;
        binding.placement = Some(placement);
        if self
            .scalar_guard(
                placement.slot(),
                binding,
                self.owner.window_prepared.load(Ordering::Acquire),
                false,
            )
            .is_ok()
        {
            "current"
        } else {
            "superseded"
        }
    }
    pub fn state(&self) -> &'static str {
        match self.owner.status.load(Ordering::Acquire) {
            CAPTURED => "captured",
            PREPARING => "preparing",
            PREPARED => "prepared",
            SCHEDULED => "scheduled",
            APPLYING => "applying",
            APPLIED => "applied",
            _ => "rejected",
        }
    }
    pub fn reason(&self) -> Option<&'static str> {
        match self.owner.reason.load(Ordering::Acquire) {
            1 => Some("invalid_action"),
            2 => Some("no_selected_content"),
            3 => Some("stale_content"),
            4 => Some("source_changed"),
            5 => Some("timing_changed"),
            6 => Some("timing_unavailable"),
            7 => Some("unknown_source_key"),
            8 => Some("mode_mismatch"),
            9 => Some("unsupported_pitch"),
            10 => Some("input_disabled"),
            11 => Some("placement_pending"),
            12 => Some("window_pending"),
            13 => Some("capacity"),
            14 => Some("preparation_failed"),
            15 => Some("prepared_mismatch"),
            16 => Some("already_published"),
            17 => Some("sequence_exhausted"),
            18 => Some("launch_cancelled"),
            19 => Some("mapped_timing_unavailable"),
            _ => None,
        }
    }
    pub fn accepted(&self) -> bool {
        self.owner.admitted.load(Ordering::Acquire)
            && self.owner.status.load(Ordering::Acquire) != REJECTED
    }
    pub fn effective_frame(&self) -> Option<u64> {
        (self.owner.status.load(Ordering::Acquire) == APPLIED)
            .then(|| self.owner.effective_frame.load(Ordering::Acquire))
    }
    pub fn original_frame(&self) -> Option<u64> {
        self.target_frame()
    }
    pub fn execution_frame(&self) -> Option<u64> {
        (self.owner.frames_state.load(Ordering::Acquire) == 2)
            .then(|| self.owner.execution_frame.load(Ordering::Acquire))
    }
    /// Cancel only this original attack; applying/completed effects remain intact.
    pub fn cancel(&self) -> bool {
        self.reject(PitchActionReject::LaunchCancelled)
    }
}

#[cfg(test)]
#[path = "pad_start_capture_pending_tests.rs"]
mod capture_pending_tests;

#[cfg(test)]
mod staging_tests {
    use super::*;
    use crate::audio_engine::input_mapping::launch_admission_tests::RATE;
    use crate::audio_engine::pitch_runtime_tests::{Fixture, LOOP_END, LOOP_START};

    fn capture(fixture: &Fixture) -> CapturedPadStart {
        fixture
            .control
            .engine
            .capture_pad_start(
                0,
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap()
    }

    #[test]
    fn failed_publication_restores_same_unpublished_work_without_an_owner_arc_cycle() {
        let fixture = Fixture::new(false);
        let start = capture(&fixture);
        let original_owner_count = Arc::strong_count(&start.owner);
        let work = start.take_work(None).unwrap().unwrap();
        let work_address = (&*work) as *const OrdinaryStartWork;
        assert!(
            work.candidate.is_none(),
            "capture retains only the original skeleton"
        );
        assert!(
            !start.is_published(),
            "staging cannot claim producer publication"
        );
        assert_eq!(Arc::strong_count(&start.owner), original_owner_count + 1);
        assert!(start.restore_unpublished_work(work).is_ok());
        assert_eq!(Arc::strong_count(&start.owner), original_owner_count);

        let work = start.take_work(None).unwrap().unwrap();
        assert_eq!((&*work) as *const OrdinaryStartWork, work_address);
        assert!(work.candidate.is_none());
        assert_eq!(work.sequence(), start.sequence());
        assert!(!start.is_published());
        assert!(start.mark_published_claimed());
        assert!(
            !start.mark_published_claimed(),
            "only one complete claim publishes"
        );
        let Err(work) = start.restore_unpublished_work(work) else {
            panic!("published work cannot be restored into the captured owner");
        };
        assert_eq!((&*work) as *const OrdinaryStartWork, work_address);
    }

    #[test]
    fn foreign_staged_work_cannot_replace_another_original_owner_on_rollback() {
        let fixture = Fixture::new(false);
        let first = capture(&fixture);
        let second = capture(&fixture);
        let first_work = first.take_work(None).unwrap().unwrap();
        let second_work = second.take_work(None).unwrap().unwrap();
        let address = (&*first_work) as *const OrdinaryStartWork;
        let Err(first_work) = second.restore_unpublished_work(first_work) else {
            panic!("foreign work acquired another original capture");
        };
        assert_eq!((&*first_work) as *const OrdinaryStartWork, address);
        assert!(first.restore_unpublished_work(first_work).is_ok());
        assert!(second.restore_unpublished_work(second_work).is_ok());
        let first_work = first.take_work(None).unwrap().unwrap();
        assert_eq!((&*first_work) as *const OrdinaryStartWork, address);
        assert!(first.restore_unpublished_work(first_work).is_ok());
        assert!(!first.is_published() && !second.is_published());
    }
}
