//! Immutable selected-pair capture and unpublished target ownership for Native Copy.
//! Only the Copy callback grants selection/source authority; these helpers do no enqueueing.

use super::{CommittedStemSetTicket, SelectedStemIdentity, SelectionOwner};
use crate::audio_engine::AudioEngine;
use crate::audio_engine::cold_store::CommittedColdLease;
use crate::audio_engine::constant_timing::AcceptedTimingProjection;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_runtime_binding::{
    InputPadBinding, InputRuntimeOwnership, InputRuntimePadBinding,
};
use crate::audio_engine::material_migration::PreparedMigrationMaterial;
use crate::audio_engine::material_paths;
use crate::audio_engine::pad_copy::CopySelectionFences;
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::project_assets::stem_readers::StemPairReaders;
use crate::audio_engine::resident_proposal::ResidentReadContext;
use crate::audio_engine::resident_relocation::{ResidentStemCache, StemOwner, StemPairOwner};
use crate::audio_engine::stem_cache::source_version_hash;
use crate::audio_engine::stem_pair::{VerifiedStemPair, open_verified_pair};
use crate::messages::{PreparedStemSet, STEM_COMPONENT_MASK, SampleBuffer, StemMixMode};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};

/// Detached preparation never looks the originating pad up again. Its source and
/// sealed reader owners may outlive that pad's original assignment and Native voice.
#[derive(Clone)]
pub(in crate::audio_engine) struct CapturedCopySelection {
    reference: SampleBuffer,
    source_lease: CommittedColdLease,
    source_version: String,
    root: PathBuf,
    descriptor_reference: String,
    pair: Option<Arc<VerifiedStemPair>>,
    pair_assignment_id: Option<u64>,
    identity: Option<Arc<[u8; 32]>>,
    components: Option<PreparedStemSet>,
}

/// Future bindings and permits all name the actual reserved target cells. They
/// are deliberately not current until the one Native Copy installation completes.
pub(in crate::audio_engine) struct CopySelectionTarget {
    pub id: usize,
    pub source_generation: u64,
    pub binding: InputPadBinding,
    pub source_epoch: Arc<AtomicU64>,
    pub future_epoch: u64,
    pub ownership: Arc<InputRuntimeOwnership>,
    pub selection_fences: CopySelectionFences,
    pub publication: PreparedSourcePermit,
    pub projection: Option<AcceptedTimingProjection>,
    pub mode: StemMixMode,
}

#[derive(Clone)]
pub(in crate::audio_engine) struct PreparedCopySelection {
    pub selection_owner: Arc<SelectionOwner>,
    pub stem_owner: Option<StemOwner>,
    pub selected: SelectedStemIdentity,
    pub selection_fences: CopySelectionFences,
    source_publication: PreparedSourcePermit,
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn check_cancelled(cancelled: &impl Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("copy selection preparation cancelled".into())
    } else {
        Ok(())
    }
}

fn sample_matches_binding(sample: &SampleBuffer, binding: InputPadBinding) -> bool {
    sample.source_address() == binding.source_address
        && sample.source_sample_count() == binding.sample_count
        && sample.channels == binding.channels
        && sample.resident_binding() == binding.resident
        && sample.valid_residency(binding.sample_rate_hz, binding.channels)
}

/// The caller holds the existing request gate. No request lock, publication,
/// proposal, selection epoch or source-authority advancement is performed here.
pub(in crate::audio_engine) fn capture_copy_selection(
    engine: &AudioEngine,
    binding: &InputRuntimePadBinding,
    expected_descriptor: Option<&str>,
) -> Result<Option<CapturedCopySelection>, String> {
    let Some(expected_descriptor) = expected_descriptor else {
        return Ok(None);
    };
    if expected_descriptor.is_empty() || expected_descriptor.len() > 4096 {
        return Err("copy selected pair reference is invalid".into());
    }
    let id = binding.id;
    if id >= NUM_SAMPLES
        || !Arc::ptr_eq(&binding.ownership, &engine.input_runtime_ownership)
        || !binding.available()
        || !binding.current()
    {
        return Err("copy selected pair source binding is stale".into());
    }
    let sample = engine
        .sample_cache
        .lock()
        .map_err(|_| "copy source cache lock poisoned")?
        .get(id)
        .and_then(Option::as_ref)
        .cloned()
        .ok_or("copy selected pair source is unloaded")?;
    let (generation, rate) = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| "copy source generation lock poisoned")?[id];
    if generation != binding.source_generation
        || rate != binding.binding.sample_rate_hz
        || engine.cold_lease_generations[id].load(Ordering::Acquire) != generation
        || !sample_matches_binding(&sample, binding.binding)
    {
        return Err("copy selected pair source generation/window changed".into());
    }
    let source_lease = engine
        .cold_leases
        .lock()
        .map_err(|_| "copy source custody lock poisoned")?[id]
        .clone()
        .ok_or("copy selected pair source custody missing")?;
    if source_lease.assignment_retired() || source_lease.material_id.is_none() {
        return Err("copy selected pair requires live canonical source custody".into());
    }
    source_lease
        .verify_reference(&sample)
        .map_err(|error| error.to_string())?;
    let root = source_lease.samples_root().to_path_buf();
    let relative = source_lease
        .original_path
        .strip_prefix(&root)
        .map_err(|_| "copy original lies outside samples root")?;
    let digest = &sample
        .residency
        .as_ref()
        .ok_or("copy complete source descriptor missing")?
        .source
        .original_sha256;
    let source_version = format!(
        "samples/{}|sha256-v1:{}",
        relative.to_string_lossy().replace('\\', "/"),
        hex(digest),
    );
    let owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| "copy selected pair ownership lock poisoned")?;
    let owner = &owners[id];
    let selection = owner.committed.accepted.as_ref().filter(|selection| {
        selection.id == id
            && selection.current()
            && selection.source_generation == generation
            && selection.source_version == source_version
            && selection.pair.descriptor_reference == expected_descriptor
            && selection.pair.root == root
            && selection.pair.reader.descriptor_reference == expected_descriptor
            && selection.pair.reader.descriptor.stem_set_identity == hex(&selection.identity)
    });
    let pair = selection.map(|selection| selection.pair.reader.clone());
    let pair_assignment_id = selection.map(|selection| selection.pair.source_lease.assignment_id());
    let identity = selection.map(|selection| selection.identity.clone());
    let components = selection.and_then(|selection| {
        owner
            .accepted
            .as_ref()
            .filter(|components| {
                components.source_version == source_version
                    && *components.set.complete_set_identity == *selection.identity
                    && components
                        .set
                        .stems
                        .iter()
                        .all(|stem| stem.same_source(&sample))
                    && components.pair.as_ref().is_some_and(|pair| {
                        pair.descriptor_reference == expected_descriptor && pair.root == root
                    })
            })
            .map(|components| components.set.clone())
    });
    if !binding.current() {
        return Err("copy selected pair source changed during capture".into());
    }
    // With no matching accepted Native owner the worker verifies the requested
    // canonical pair using these captured owners; no later origin lookup is needed.
    Ok(Some(CapturedCopySelection {
        reference: sample,
        source_lease,
        source_version,
        root,
        descriptor_reference: expected_descriptor.to_owned(),
        pair,
        pair_assignment_id,
        identity,
        components,
    }))
}

/// Allocate a future token number only. The actual target atoms remain unchanged.
pub(in crate::audio_engine) fn capture_copy_selection_fences(
    cache: &ResidentStemCache,
) -> Result<CopySelectionFences, String> {
    let selection = cache.committed.selection_epoch.clone();
    let registration = cache.committed.registration_epoch.clone();
    let expected = selection
        .load(Ordering::Acquire)
        .max(registration.load(Ordering::Acquire))
        .checked_add(1)
        .ok_or("copy selection epoch exhausted")?;
    Ok(CopySelectionFences {
        selection,
        publication: PreparedSourcePermit::new(registration.clone(), expected),
        registration,
        expected,
    })
}

/// Capture both actual source mutation fences under the caller's request gate.
/// Their identities and values remain usable even when no pair is selected.
pub(in crate::audio_engine) fn capture_copy_source_selection_fences(
    cache: &ResidentStemCache,
) -> [(Arc<AtomicU64>, u64); 2] {
    [
        cache.committed.selection_epoch.clone(),
        cache.intent_epoch.clone(),
    ]
    .map(|epoch| {
        let expected = epoch.load(Ordering::Acquire);
        (epoch, expected)
    })
}

fn validate_target(target: &SampleBuffer, future: &CopySelectionTarget) -> Result<(), String> {
    let fences = &future.selection_fences;
    if future.id >= NUM_SAMPLES
        || future.source_generation == 0
        || future.future_epoch == 0
        || future.binding.accepted != future.projection
        || !sample_matches_binding(target, future.binding)
        || !Arc::ptr_eq(&future.publication.epoch, &future.source_epoch)
        || future.publication.expected != future.future_epoch
        || future.source_epoch.load(Ordering::Acquire) >= future.future_epoch
        || Arc::ptr_eq(&future.source_epoch, &fences.selection)
        || Arc::ptr_eq(&future.source_epoch, &fences.registration)
        || !matches!(future.publication.status(), "captured" | "pending")
        || fences.expected == 0
        || fences.selection.load(Ordering::Acquire) >= fences.expected
        || fences.registration.load(Ordering::Acquire) >= fences.expected
        || !Arc::ptr_eq(&fences.publication.epoch, &fences.registration)
        || fences.publication.expected != fences.expected
        || !matches!(fences.publication.status(), "captured" | "pending")
    {
        return Err("copy selection target requires unpublished actual target permits".into());
    }
    if future.projection.is_some_and(|projection| {
        projection.publication_epoch != future.future_epoch
            || projection.sample_rate_hz != future.binding.sample_rate_hz
    }) {
        return Err("copy selection target timing projection is inconsistent".into());
    }
    Ok(())
}

fn verify_pair_source(
    pair: &VerifiedStemPair,
    captured: &CapturedCopySelection,
    target: &SampleBuffer,
    lease: &CommittedColdLease,
) -> Result<Arc<[u8; 32]>, String> {
    pair.descriptor.validate()?;
    let source = &target
        .residency
        .as_ref()
        .ok_or("copy pair target complete descriptor missing")?
        .source;
    let content = &pair.descriptor.content;
    let pair_source = &content.source;
    let original = &lease.manifest.descriptor["decoder"]["original"];
    if pair.descriptor_reference != captured.descriptor_reference
        || content.source_version != captured.source_version
        || Some(&content.material_id) != lease.material_id.as_ref()
        || pair_source.frame_count != source.frame_count as u64
        || usize::try_from(pair_source.channels).ok() != Some(source.channels)
        || pair_source.sample_rate_hz != source.sample_rate_hz
        || Some(pair_source.original_bytes) != original["bytes"].as_u64()
        || original["sha256"].as_str() != Some(hex(&source.original_sha256).as_str())
        || pair_source.original_sha256 != hex(&source.original_sha256)
        || pair_source.playback_sha256 != hex(&source.playback_sha256)
        || pair_source.mono_sha256 != hex(&source.mono_sha256)
        || pair_source.transform_sha256 != hex(&source.transform_sha256)
        || pair_source.source_zero_frame != 0
        || source.source_zero_frame != 0
    {
        return Err("copy selected pair differs from verified complete target source".into());
    }
    let mut decoded = [0_u8; 32];
    for (index, byte) in decoded.iter_mut().enumerate() {
        *byte = u8::from_str_radix(
            &pair.descriptor.stem_set_identity[index * 2..index * 2 + 2],
            16,
        )
        .map_err(|_| "copy selected pair identity is malformed")?;
    }
    if let Some(identity) = &captured.identity {
        if **identity != decoded {
            return Err("copy selected pair identity changed after capture".into());
        }
        Ok(identity.clone())
    } else {
        Ok(Arc::new(decoded))
    }
}

fn reusable_components(set: &PreparedStemSet, target: &SampleBuffer, identity: &[u8; 32]) -> bool {
    *set.complete_set_identity == *identity
        && set.sample_rate_hz == target.residency.as_ref().unwrap().source.sample_rate_hz
        && set.channels == target.channels
        && set.frame_count == target.frame_count()
        && set.available_mask == STEM_COMPONENT_MASK
        && set.stems.iter().all(|stem| {
            stem.same_source(target)
                && stem.resident_start() == target.resident_start()
                && stem.resident_end() == target.resident_end()
        })
}

fn rebind_components(
    set: &PreparedStemSet,
    target: &SampleBuffer,
) -> Result<[SampleBuffer; 4], String> {
    let context = target
        .residency
        .as_ref()
        .ok_or("copy resident descriptor missing")?
        .context;
    set.stems
        .clone()
        .map(|stem| {
            stem.window(
                target.resident_start(),
                target.resident_end(),
                target.window_revision(),
                context,
            )
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| "copy retained component count differs".into())
}

/// Worker-only preparation. The target lease must be a fresh verified subscriber,
/// never the origin's assignment clone. Same-range component allocations are shared;
/// other ranges are read directly from existing sealed PCM, without decode/separation.
pub(in crate::audio_engine) fn prepare_copy_selection(
    captured: &CapturedCopySelection,
    target: &SampleBuffer,
    target_lease: &CommittedColdLease,
    mut future: CopySelectionTarget,
    resident: &ResidentReadContext,
    cancelled: &impl Fn() -> bool,
) -> Result<PreparedCopySelection, String> {
    check_cancelled(cancelled)?;
    validate_target(target, &future)?;
    if !captured.reference.same_source(target)
        || target_lease.samples_root() != captured.root.as_path()
        || target_lease.assignment_id() == captured.source_lease.assignment_id()
        || captured.pair_assignment_id == Some(target_lease.assignment_id())
        || target_lease.assignment_retired()
    {
        return Err("copy selected pair requires its own live target source custody".into());
    }
    target_lease
        .verify_reference(target)
        .map_err(|error| error.to_string())?;
    let pair = if let Some(pair) = &captured.pair {
        pair.clone()
    } else {
        let material = PreparedMigrationMaterial::from_current(
            &captured.root,
            target.clone(),
            target_lease.clone(),
        )?;
        Arc::new(open_verified_pair(
            &captured.root,
            &captured.descriptor_reference,
            &material,
            cancelled,
        )?)
    };
    let identity = verify_pair_source(&pair, captured, target, target_lease)?;
    let generation_path =
        material_paths::resolve(&captured.root, Path::new(&pair.descriptor.wav_generation))
            .map_err(|error| error.to_string())?
            .path;
    let pair_owner = StemPairOwner {
        root: captured.root.clone(),
        descriptor_reference: captured.descriptor_reference.clone(),
        source_lease: target_lease.clone(),
        reader: pair.clone(),
    };
    future.selection_fences.publication = future
        .selection_fences
        .publication
        .clone()
        .with_source_epoch(future.source_epoch.clone(), future.future_epoch);
    let owner = Arc::new(SelectionOwner {
        id: future.id,
        binding: future.binding,
        source_generation: future.source_generation,
        source_version: captured.source_version.clone(),
        identity: identity.clone(),
        pair: pair_owner.clone(),
        generation_path: generation_path.clone(),
        epoch: future.selection_fences.selection.clone(),
        expected: future.selection_fences.expected,
        revoked: Arc::new(AtomicBool::new(false)),
        source_epoch: future.source_epoch.clone(),
        ownership: future.ownership.clone(),
    });
    let backing = if future.mode == StemMixMode::AllStems {
        let captured_backing = captured
            .components
            .as_ref()
            .filter(|set| reusable_components(set, target, &identity));
        let retained = if let Some(backing) = captured_backing {
            // Reserve the exact target owner views that will be committed. Reusing
            // the origin views here and rebinding them after admission is ambiguous
            // with registry backing, which already returns fresh target views.
            let mut rebound = backing.clone();
            rebound.stems = rebind_components(backing, target)?;
            Some(rebound)
        } else {
            resident
                .assets
                .shared_stems(
                    &generation_path,
                    target,
                    &captured.source_version,
                    future.binding.sample_rate_hz,
                )
                .map_err(|error| error.to_string())?
        };
        retained.filter(|set| reusable_components(set, target, &identity))
    } else {
        None
    };
    let component_count = if future.mode == StemMixMode::AllStems {
        4
    } else {
        0
    };
    let bytes = if component_count == 0 || backing.is_some() {
        0
    } else {
        target
            .samples
            .len()
            .checked_mul(16)
            .ok_or("copy component resident byte overflow")?
    };
    let mut reused = vec![target];
    if let Some(backing) = &backing {
        reused.extend(backing.stems.iter());
    }
    let mut reservation = resident
        .reserve(bytes, &reused, component_count + 1)
        .map_err(|error| error.to_string())?;
    reservation
        .reserve_reader_slots(1)
        .map_err(|error| error.to_string())?;
    check_cancelled(cancelled)?;
    let stem_owner = if component_count != 0 {
        let stems = if let Some(backing) = &backing {
            backing.stems.clone()
        } else {
            pair.prepare_component_views_cancellable(target, cancelled)?
        };
        let set = PreparedStemSet {
            complete_set_identity: identity.clone(),
            accepted_timing: future.projection,
            reference_samples: target.samples.clone(),
            reference_residency: target.residency.clone(),
            publication: future.publication.clone(),
            source_version_hash: source_version_hash(&captured.source_version),
            sample_rate_hz: future.binding.sample_rate_hz,
            channels: target.channels,
            frame_count: target.frame_count(),
            available_mask: STEM_COMPONENT_MASK,
            stems,
        };
        Some(StemOwner {
            set,
            source_version: captured.source_version.clone(),
            cache_dir: generation_path.clone(),
            generation_path: generation_path.clone(),
            pair: Some(pair_owner),
        })
    } else {
        None
    };
    let mut views = vec![target];
    if let Some(stems) = &stem_owner {
        views.extend(stems.set.stems.iter());
    }
    // Source-history admission/registration is owned by the caller's source-window
    // preparation. This component reservation covers all actual PCM and pair readers.
    reservation
        .commit(&views)
        .map_err(|error| error.to_string())?;
    check_cancelled(cancelled)?;
    resident
        .assets
        .retain_stem_pair_reserved(
            generation_path,
            StemPairReaders {
                reference: target,
                version: &captured.source_version,
                identity: &identity,
                stems: stem_owner.as_ref().map(|owner| &owner.set),
            },
            pair.clone_pins(
                captured
                    .root
                    .parent()
                    .ok_or("copy pair project root missing")?,
            )?,
            target_lease.clone(),
            &mut reservation,
        )
        .map_err(|error| error.to_string())?;
    check_cancelled(cancelled)?;
    validate_target(target, &future)?;
    Ok(PreparedCopySelection {
        selected: SelectedStemIdentity {
            identity: *identity,
            source_generation: owner.source_generation,
            epoch: owner.expected,
        },
        selection_owner: owner,
        stem_owner,
        selection_fences: future.selection_fences,
        source_publication: future.publication,
    })
}

/// Install only independently acknowledged target owners. Reservation remains held
/// through the caller's configuration commit; raw authority/window checks therefore
/// intentionally avoid authority_current(), which denies pending Copy targets.
pub(in crate::audio_engine) fn reconcile_copy_selection(
    cache: &mut ResidentStemCache,
    prepared: PreparedCopySelection,
    copy_phase: &AtomicU8,
) -> Result<CommittedStemSetTicket, String> {
    let owner = &prepared.selection_owner;
    let fences = &prepared.selection_fences;
    if copy_phase.load(Ordering::Acquire) != 2
        || prepared.source_publication.status() != "accepted"
        || !prepared.source_publication.current()
        || fences.publication.status() != "accepted"
        || !fences.publication.current()
        || !Arc::ptr_eq(&cache.committed.selection_epoch, &fences.selection)
        || !Arc::ptr_eq(&cache.committed.registration_epoch, &fences.registration)
        || fences.registration.load(Ordering::Acquire) != fences.expected
        || !owner.current()
        || owner.ownership.authority[owner.id].load(Ordering::Acquire)
            != owner.binding.authority_revision
        || !owner
            .ownership
            .binding_window_current(owner.id, owner.binding)
        || owner
            .ownership
            .binding_source_generation(owner.id, owner.binding)
            != Some(owner.source_generation)
        || prepared.selected
            != (SelectedStemIdentity {
                identity: *owner.identity,
                source_generation: owner.source_generation,
                epoch: owner.expected,
            })
        || prepared.stem_owner.as_ref().is_some_and(|stems| {
            stems.set.publication.status() != "accepted" || !stems.set.publication.current()
        })
    {
        return Err("copy selected pair lacks its own current Native Copy acknowledgement".into());
    }
    if cache.ordinary_publication_pending() || cache.committed.handover_pending() {
        return Err("copy selected pair target retains a pending predecessor".into());
    }
    cache.committed.accepted = Some(owner.clone());
    cache.committed.registration = None;
    cache.committed.policy_interest = false;
    cache.committed.explicit_interest = prepared.stem_owner.is_some();
    cache.accepted = prepared.stem_owner;
    Ok(CommittedStemSetTicket {
        owner: prepared.selection_owner,
        publication: prepared.selection_fences.publication,
        candidate: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::ResidentContext;

    #[test]
    fn copy_selection_fences_capture_actual_atoms_without_advancing_or_acknowledging() {
        let cache = ResidentStemCache::default();
        cache.committed.selection_epoch.store(7, Ordering::Release);
        cache
            .committed
            .registration_epoch
            .store(11, Ordering::Release);
        cache.intent_epoch.store(13, Ordering::Release);
        let fences = capture_copy_selection_fences(&cache).unwrap();
        assert_eq!(fences.expected, 12);
        assert!(Arc::ptr_eq(
            &fences.selection,
            &cache.committed.selection_epoch
        ));
        assert!(Arc::ptr_eq(
            &fences.registration,
            &cache.committed.registration_epoch
        ));
        assert!(Arc::ptr_eq(&fences.publication.epoch, &fences.registration));
        assert_eq!(fences.selection.load(Ordering::Acquire), 7);
        assert_eq!(fences.registration.load(Ordering::Acquire), 11);
        assert_eq!(fences.publication.status(), "captured");
        assert!(!fences.publication.current());
        let [(selection, selected), (intent, intended)] =
            capture_copy_source_selection_fences(&cache);
        assert!(Arc::ptr_eq(&selection, &cache.committed.selection_epoch));
        assert!(Arc::ptr_eq(&intent, &cache.intent_epoch));
        assert_eq!((selected, intended), (7, 13));
        cache
            .committed
            .selection_epoch
            .store(u64::MAX, Ordering::Release);
        assert!(capture_copy_selection_fences(&cache).is_err());
        assert_eq!(
            cache.committed.registration_epoch.load(Ordering::Acquire),
            11
        );
        assert_eq!(cache.intent_epoch.load(Ordering::Acquire), 13);
    }

    #[test]
    fn copy_component_rebind_shares_backing_with_fresh_target_window_context() {
        let complete = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.0; 16]),
            residency: None,
        }
        .with_complete_source(8_000);
        let origin = complete
            .window(2, 8, 3, ResidentContext::FiniteLoop)
            .unwrap();
        let target = complete
            .window(2, 8, 19, ResidentContext::KeyLockFiniteLoop)
            .unwrap();
        let origin_epoch = Arc::new(AtomicU64::new(3));
        let original = PreparedStemSet {
            complete_set_identity: Arc::new([8; 32]),
            accepted_timing: None,
            reference_samples: origin.samples.clone(),
            reference_residency: origin.residency.clone(),
            publication: PreparedSourcePermit::new(origin_epoch.clone(), 3),
            source_version_hash: 37,
            sample_rate_hz: 8_000,
            channels: 1,
            frame_count: 16,
            available_mask: STEM_COMPONENT_MASK,
            stems: std::array::from_fn(|_| SampleBuffer {
                channels: 1,
                samples: Arc::from([0.25; 6]),
                residency: origin.residency.clone(),
            }),
        };
        assert!(reusable_components(&original, &target, &[8; 32]));
        let rebound = rebind_components(&original, &target).unwrap();
        for (before, after) in original.stems.iter().zip(&rebound) {
            assert!(Arc::ptr_eq(&before.samples, &after.samples));
            assert!(!Arc::ptr_eq(
                before.residency.as_ref().unwrap(),
                after.residency.as_ref().unwrap()
            ));
            assert!(after.same_window(&target));
            assert_eq!(before.window_revision(), 3);
            assert_eq!(
                before.residency.as_ref().unwrap().context,
                ResidentContext::FiniteLoop
            );
        }
        assert_eq!(origin_epoch.load(Ordering::Acquire), 3);
        assert_eq!(original.publication.status(), "captured");
        assert!(!reusable_components(&original, &target, &[9; 32]));
        let different = complete
            .window(3, 8, 20, ResidentContext::FiniteLoop)
            .unwrap();
        assert!(!reusable_components(&original, &different, &[8; 32]));
    }

    #[test]
    fn copy_target_preparation_uses_future_permits_without_current_authority() {
        let sample = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.0; 8]),
            residency: None,
        }
        .with_complete_source(8_000);
        let cache = ResidentStemCache::default();
        let source_epoch = Arc::new(AtomicU64::new(9));
        let mut target = CopySelectionTarget {
            id: 36,
            source_generation: 5,
            binding: InputPadBinding {
                resident: sample.resident_binding(),
                source_address: sample.source_address(),
                sample_count: sample.source_sample_count(),
                channels: sample.channels,
                sample_rate_hz: 8_000,
                authority_revision: 17,
                runtime_revision: 0,
                accepted: None,
            },
            source_epoch: source_epoch.clone(),
            future_epoch: 11,
            ownership: Arc::default(),
            selection_fences: capture_copy_selection_fences(&cache).unwrap(),
            publication: PreparedSourcePermit::new(source_epoch.clone(), 11),
            projection: None,
            mode: StemMixMode::FullMix,
        };
        assert!(!target.publication.current());
        assert!(validate_target(&sample, &target).is_ok());
        assert_eq!(source_epoch.load(Ordering::Acquire), 9);
        assert_eq!(cache.committed.selection_epoch.load(Ordering::Acquire), 0);
        assert_eq!(
            cache.committed.registration_epoch.load(Ordering::Acquire),
            0
        );
        target.binding.channels = 2;
        assert!(validate_target(&sample, &target).is_err());
        target.binding.channels = 1;
        target.publication = PreparedSourcePermit::new(Arc::new(AtomicU64::new(9)), 11);
        assert!(validate_target(&sample, &target).is_err());
        assert_eq!(source_epoch.load(Ordering::Acquire), 9);
        assert_eq!(target.selection_fences.publication.status(), "captured");
    }
}
