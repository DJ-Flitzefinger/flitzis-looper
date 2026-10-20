//! Isolated registry/stopped-Native-bank resource proof, without NN inference or CPAL.
//!
//! Register as a child of `stem_readers::paired_owner_tests` to reuse its actual
//! material/pair fixture foundations. Run this ignored test alone in a fresh
//! Windows test process, with `--exact --ignored --nocapture --test-threads=1`.
//! C3 counters cover that whole test process; the exact census covers only this
//! real isolated ProjectAssets registry. Neither establishes a full NN-process
//! residual-handle explanation, source/callback ACK acceptance or device behavior.

use super::*;
use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::stem_pair::open_verified_pair;
use crate::messages::{STEM_COMPONENT_MASK, StemMixMode};
use std::collections::BTreeSet;
use std::sync::atomic::AtomicU64;

const CYCLES: usize = 4;
// One fixed allowance above the FIRST equal fully settled cycle, never an
// allowance per iteration. The exact ownership census permits no accumulation.
// This accommodates process/harness/thread runtime handle bookkeeping outside
// the isolated registry; it is not a production handle or latency budget.
const SETTLED_HANDLE_TOLERANCE: u64 = 16;

/// File pins counted here are the actual SharedStemReaders File vector only.
/// Cold leases also own sealed handles privately; C3 measures those handles,
/// while this census records their actual reader/assignment/path ownership.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct Census {
    readers: usize,
    cold_readers: usize,
    paired_readers: usize,
    other_readers: usize,
    native_readers: usize,
    pending_assignments: usize,
    saved_cold_claims: usize,
    pcm_weak_slots: usize,
    live_pcm_allocations: usize,
    live_set_identities: usize,
    pair_file_pins: usize,
    owners: usize,
    saved_owners: usize,
    transient_pins: usize,
    original_paths: BTreeSet<PathBuf>,
    cache_paths: BTreeSet<PathBuf>,
    pair_paths: BTreeSet<PathBuf>,
    owner_paths: BTreeSet<PathBuf>,
    cold_cache_assignment_counts: BTreeSet<usize>,
    paired_retired_source_assignments: usize,
    retirements: usize,
    deleted: u64,
    errors: Vec<String>,
}

fn census(assets: &ProjectAssets) -> Census {
    let state = assets.state.lock().unwrap();
    let mut value = Census {
        readers: state.readers.len(),
        cold_readers: 0,
        paired_readers: 0,
        other_readers: 0,
        native_readers: 0,
        pending_assignments: 0,
        saved_cold_claims: 0,
        pcm_weak_slots: 0,
        live_pcm_allocations: 0,
        live_set_identities: 0,
        pair_file_pins: 0,
        owners: 0,
        saved_owners: 0,
        transient_pins: 0,
        original_paths: BTreeSet::new(),
        cache_paths: BTreeSet::new(),
        pair_paths: BTreeSet::new(),
        owner_paths: BTreeSet::new(),
        cold_cache_assignment_counts: BTreeSet::new(),
        paired_retired_source_assignments: 0,
        retirements: state.retiring.len(),
        deleted: state.deleted,
        errors: state.errors.clone(),
    };
    let mut live_pcm = BTreeSet::new();
    for reader in &state.readers {
        value.pending_assignments += usize::from(reader.pending_assignment);
        value.native_readers += usize::from(
            reader.pcm.iter().any(|pcm| pcm.strong_count() > 0) || reader.shared_native_live(),
        );
        value.pcm_weak_slots += reader.pcm.len();
        for pcm in &reader.pcm {
            if pcm.strong_count() > 0 {
                live_pcm.insert(pcm.as_ptr() as *const f32 as usize);
            }
        }
        if let Some(lease) = &reader.cold {
            value.cold_readers += 1;
            value.saved_cold_claims += usize::from(reader.saved_claim);
            value.original_paths.insert(lease.original_path.clone());
            value.cache_paths.insert(lease.cache_path.clone());
            value
                .cold_cache_assignment_counts
                .insert(lease.cache_assignment_count());
        }
        if let Some(shared) = &reader.shared_stems {
            value.live_set_identities += usize::from(shared.identity.strong_count() > 0);
            value.pair_file_pins += shared._files.len();
            if let Some(pair) = &shared.pair {
                value.paired_readers += 1;
                value.pair_paths.extend(pair.paths.iter().cloned());
                value
                    .original_paths
                    .insert(pair.source_lease.original_path.clone());
                value
                    .cache_paths
                    .insert(pair.source_lease.cache_path.clone());
                value.paired_retired_source_assignments +=
                    usize::from(pair.source_lease.assignment_retired());
            }
        }
        if reader.cold.is_none()
            && reader
                .shared_stems
                .as_ref()
                .is_none_or(|shared| shared.pair.is_none())
        {
            value.other_readers += 1;
        }
    }
    value.live_pcm_allocations = live_pcm.len();
    for owner in state.owners.iter().filter_map(Weak::upgrade) {
        value.owners += 1;
        let saved = owner.saved_assignment.load(Ordering::Acquire);
        value.saved_owners += usize::from(saved);
        value.transient_pins += usize::from(!saved);
        value.owner_paths.insert(owner.path.clone());
    }
    value
}

fn stage(assets: &ProjectAssets, cycle: usize, name: &str) -> (Census, serde_json::Value) {
    let census = census(assets);
    // Census owns only scalars/paths, and its temporary registry mutex and owner
    // upgrades have already been released before this whole-process snapshot.
    let cold_cleanup = crate::audio_engine::cold_store::cleanup_status().unwrap();
    let cold_admission = crate::audio_engine::cold_store::cleanup_admission_status();
    let process = crate::audio_engine::c3_process_metrics::snapshot();
    eprintln!(
        "{}",
        json!({"proof":"same-material-project-assets-plateau", "cycle":cycle,
            "stage":name, "registry":census, "process":process,
            "process_wide_cold_cleanup":cold_cleanup,
            "process_wide_cold_cleanup_admission":cold_admission})
    );
    (census, process)
}

/// The existing paired-owner fixture with an explicitly owned workspace prefix.
/// WAV construction, material preparation and complete-pair verification remain
/// the existing fixture/productive implementations, rather than a new registry.
fn owned_fixture() -> Fixture {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let artifacts = workspace.join("test-artifacts/b1-stem-batch-20261020");
    fs::create_dir_all(&artifacts).unwrap();
    let temp = tempfile::Builder::new()
        .prefix("project-assets-resource-plateau-")
        .tempdir_in(artifacts)
        .unwrap();
    fs::write(
        temp.path().join("OWNER.md"),
        "Owned disposable same-material ProjectAssets resource proof. No Human inputs, model, App or installation.\n",
    )
    .unwrap();
    let root = temp.path().join("samples");
    fs::create_dir(&root).unwrap();
    let old = root.join("old.wav");
    let encoded = wav();
    fs::write(&old, &encoded).unwrap();
    let material = prepare_material(&root, &old, RATE, 1, &|| false).unwrap();
    let metadata = material.metadata();
    let digest = metadata["original"]["sha256"].as_str().unwrap();
    let canonical_version = format!(
        "{}|sha256-v1:{digest}",
        metadata["new_reference"].as_str().unwrap()
    );
    let material_reference =
        crate::audio_engine::material_paths::canonical_original_root_reference(
            Path::new(metadata["new_reference"].as_str().unwrap()),
            metadata["material_id"].as_str().unwrap(),
        )
        .unwrap();
    let wav_reference = format!(
        "{}/stems/.ready-{GENERATION}",
        material_reference.to_string_lossy().replace('\\', "/")
    );
    let wav_path = root.parent().unwrap().join(&wav_reference);
    fs::create_dir_all(&wav_path).unwrap();
    let mut hashes = serde_json::Map::new();
    for name in STEM_FILE_NAMES {
        fs::write(wav_path.join(format!("{name}.wav")), &encoded).unwrap();
        hashes.insert(
            name.to_owned(),
            json!(format!("{:x}", Sha256::digest(&encoded))),
        );
    }
    fs::write(
        wav_path.join(".complete.json"),
        serde_json::to_vec(&json!({"schema":"stem-set-sha256-v1",
            "source_version":canonical_version,"stems":hashes}))
        .unwrap(),
    )
    .unwrap();
    let (root, wav_path) = owned_path(&root, &wav_path).unwrap();
    Fixture {
        _temp: temp,
        root,
        material,
        canonical_version,
        legacy_version: format!("samples/old.wav|sha256-v1:{digest}"),
        wav_reference,
        wav_path,
    }
}

struct NativeCycle {
    mixer: Box<RtMixer>,
    ownership: Weak<InputRuntimeOwnership>,
    pcm: Vec<Weak<[f32]>>,
    identities: Vec<Weak<[u8; 32]>>,
}

fn load_cycle(
    assets: &ProjectAssets,
    root: &Path,
    original: &Path,
    wav_path: &Path,
    version: &str,
    pair_reference: &str,
    original_owner: &ProjectAssetLease,
) -> NativeCycle {
    let mut mixer = Box::new(RtMixer::new(1, RATE as f32));
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    let epochs: Vec<_> = (0..NUM_SAMPLES)
        .map(|_| Arc::new(AtomicU64::new(1)))
        .collect();
    mixer.set_input_runtime_ownership(ownership.clone());
    mixer.set_current_timing_acknowledgements(acknowledgements.clone());
    mixer.set_prepared_source_epochs(epochs.clone());
    let mut pcm = Vec::with_capacity(NUM_SAMPLES * (STEM_BUFFER_COUNT + 1));
    let mut identities = Vec::with_capacity(NUM_SAMPLES);
    for id in 0..NUM_SAMPLES {
        // Fresh actual original/cache assignment and checked pair readers for
        // every subscriber, all using the SAME small immutable disk material.
        let material = prepare_material(root, original, RATE, 1, &|| false).unwrap();
        assert_eq!(material.sample.frame_count(), 64);
        assert_eq!(material.lease.original_path, original);
        let pair = open_verified_pair(root, pair_reference, &material, &|| false).unwrap();
        let identity = Arc::new(std::array::from_fn(|index| {
            u8::from_str_radix(
                &pair.descriptor.stem_set_identity[index * 2..index * 2 + 2],
                16,
            )
            .unwrap()
        }));
        ownership.publish_test_source(id, &material.sample, RATE, 1);
        mixer.load_sample(id, material.sample.clone());
        assert!(ownership.source_current(id, &material.sample, RATE));
        let placement = ownership.slot_snapshot(id).unwrap();
        let binding = InputPadBinding {
            placement: Some(placement),
            resident: material.sample.resident_binding(),
            source_address: material.sample.source_address(),
            sample_count: material.sample.source_sample_count(),
            channels: 1,
            sample_rate_hz: RATE,
            authority_revision: ownership.authority_revision(id),
            runtime_revision: ownership.runtime_revision(id),
            accepted: None,
        };
        let publication = PreparedSourcePermit::for_native_binding_for_test(
            id,
            binding,
            ownership.clone(),
            acknowledgements.clone(),
            epochs[placement.cell().index()].clone(),
        )
        .unwrap();
        let stems = PreparedStemSet {
            complete_set_identity: identity.clone(),
            accepted_timing: None,
            reference_samples: material.sample.samples.clone(),
            reference_residency: material.sample.residency.clone(),
            publication,
            source_version_hash: source_version_hash(version),
            sample_rate_hz: RATE,
            channels: 1,
            frame_count: 64,
            available_mask: STEM_COMPONENT_MASK,
            stems: pair.prepare_component_views(&material.sample).unwrap(),
        };
        assets
            .retain_cold(material.lease.clone(), &material.sample, Weak::new())
            .unwrap();
        assets
            .retain_stem_pair(
                wav_path.to_owned(),
                &material.sample,
                version,
                &identity,
                Some(&stems),
                pair.clone_pins(root.parent().unwrap()).unwrap(),
                material.lease.clone(),
            )
            .unwrap();
        pcm.push(Arc::downgrade(&material.sample.samples));
        pcm.extend(stems.stems.iter().map(|stem| Arc::downgrade(&stem.samples)));
        identities.push(Arc::downgrade(&identity));
        // Same real guarded mixer publication as the existing prepared-stem
        // tests. This test does not label its control-side call a callback ACK.
        assert!(mixer.publish_prepared_stems(id, stems));
        assert!(mixer.set_stem_mix_mode(id, StemMixMode::AllStems, source_version_hash(version)));
        // Acknowledge the existing saved file claim only after actual bank
        // adoption. This does not grant source/timing publication authority.
        assets
            .acknowledge_delivered(original_owner.owner.as_ref().unwrap())
            .unwrap();
    }
    assert_eq!(
        mixer.bank_for_measurement().iter().flatten().count(),
        NUM_SAMPLES
    );
    assert_eq!(
        mixer.stems_for_measurement().iter().flatten().count(),
        NUM_SAMPLES
    );
    for id in 0..NUM_SAMPLES {
        assert_eq!(
            mixer.stems_for_measurement()[id]
                .as_ref()
                .unwrap()
                .publication
                .status(),
            "accepted"
        );
        assert!(mixer.transport_state_for_test(id).is_none());
    }
    NativeCycle {
        mixer,
        ownership: Arc::downgrade(&ownership),
        pcm,
        identities,
    }
}

fn assert_files_unchanged(files: &[(PathBuf, Vec<u8>)]) {
    for (path, bytes) in files {
        assert_eq!(
            fs::read(path).unwrap(),
            *bytes,
            "saved/unknown bytes changed: {path:?}"
        );
    }
}

#[test]
#[ignore = "isolated Windows C3 resource profile; run this exact test alone in a fresh process"]
fn actual_216_same_material_native_release_and_collect_reaches_a_handle_plateau() {
    assert_eq!(NUM_SAMPLES, 216);
    let fixture = owned_fixture();
    let assets = ProjectAssets::isolated();
    let registered = fixture.register(&assets, false, &fixture.canonical_version);
    let original = fixture.material.lease.original_path.clone();
    let cache = fixture.material.lease.cache_path.clone();
    let pair_reference = registered
        .descriptor_path
        .strip_prefix(fixture.root.parent().unwrap())
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let mut original_owner = assets.acquire(&fixture.root, &original).unwrap();
    let mut stem_owner = assets.acquire(&fixture.root, &fixture.wav_path).unwrap();
    assets
        .retain_cold(
            fixture.material.lease.clone(),
            &fixture.material.sample,
            Weak::new(),
        )
        .unwrap();
    assets
        .acknowledge_delivered(original_owner.owner.as_ref().unwrap())
        .unwrap();
    let unknown = fixture.root.join("unknown.keep");
    fs::write(
        &unknown,
        b"unknown file must remain unchanged; never a retirement target",
    )
    .unwrap();
    let mut paths = vec![original.clone(), unknown, fixture.root.join("old.wav")];
    paths.extend(["decoder.f32le", "playback.f32le", "manifest.json"].map(|name| cache.join(name)));
    paths.extend(STEM_FILES.map(|name| fixture.wav_path.join(name)));
    paths.extend(STEM_PCM_FILES.map(|name| registered.pcm_path.join(name)));
    paths.push(registered.descriptor_path.clone());
    let preserved: Vec<_> = paths
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    let expected_pair_paths: BTreeSet<_> = [
        fixture.wav_path.clone(),
        registered.pcm_path.clone(),
        registered.descriptor_path.clone(),
    ]
    .into_iter()
    .collect();
    drop(registered);
    let Fixture {
        _temp: temp,
        root,
        material,
        canonical_version: version,
        wav_path,
        ..
    } = fixture;
    // No preparation fixture keeps a hidden complete PCM/native owner alive.
    let preparation_pcm = Arc::downgrade(&material.sample.samples);
    drop(material);
    assert_eq!(preparation_pcm.strong_count(), 0);
    drop(preparation_pcm);
    assets.collect_for_test();
    let (initial, _) = stage(&assets, 0, "initial-saved-file-representatives");
    assert_eq!((initial.cold_readers, initial.paired_readers), (1, 1));
    assert_eq!(initial.native_readers, 0);
    assert_eq!(initial.pending_assignments, 0);
    assert_files_unchanged(&preserved);

    let mut settled_baseline = None;
    let mut process_baseline: Option<serde_json::Value> = None;
    for cycle in 1..=CYCLES {
        let NativeCycle {
            mixer,
            ownership,
            pcm,
            identities,
        } = load_cycle(
            &assets,
            &root,
            &original,
            &wav_path,
            &version,
            &pair_reference,
            &original_owner,
        );
        let (loaded, loaded_process) = stage(&assets, cycle, "216-native-banks-and-pinned-readers");
        assert_eq!(loaded.cold_readers, NUM_SAMPLES + 1);
        assert_eq!(loaded.paired_readers, NUM_SAMPLES);
        assert_eq!(loaded.native_readers, NUM_SAMPLES * 2);
        assert_eq!(
            loaded.live_pcm_allocations,
            NUM_SAMPLES * (STEM_BUFFER_COUNT + 1)
        );
        assert_eq!(loaded.live_set_identities, NUM_SAMPLES);
        assert_eq!(loaded.pending_assignments, 0);
        assert_eq!(loaded.pair_paths, expected_pair_paths);
        assert!(loaded.pair_file_pins >= NUM_SAMPLES * 11);
        // Real RtMixer Drop joins its preparation workers. No forced registry
        // clear, fabricated release flag, retirement or deletion is involved.
        drop(mixer);
        assert_eq!(ownership.strong_count(), 0);
        assert!(pcm.iter().all(|reader| reader.strong_count() == 0));
        assert!(identities.iter().all(|reader| reader.strong_count() == 0));
        drop((ownership, pcm, identities));
        let (uncollected, _) = stage(&assets, cycle, "actual-native-release-before-collection");
        assert_eq!(uncollected.readers, loaded.readers);
        assert_eq!(uncollected.native_readers, 0);
        assert_eq!(uncollected.live_pcm_allocations, 0);
        assets.collect_for_test();
        let (settled, process) = stage(
            &assets,
            cycle,
            "off-rt-collection-complete-saved-claims-held",
        );
        assert_eq!(settled.readers, 2);
        assert_eq!(
            (
                settled.cold_readers,
                settled.paired_readers,
                settled.other_readers
            ),
            (1, 1, 0)
        );
        assert_eq!(
            (settled.owners, settled.saved_owners, settled.transient_pins),
            (2, 2, 0)
        );
        assert_eq!(
            (
                settled.native_readers,
                settled.live_set_identities,
                settled.pcm_weak_slots
            ),
            (0, 0, 0)
        );
        assert_eq!(settled.saved_cold_claims, 1);
        assert_eq!(settled.pending_assignments, 0);
        assert_eq!(settled.original_paths, BTreeSet::from([original.clone()]));
        assert_eq!(settled.cache_paths, BTreeSet::from([cache.clone()]));
        assert_eq!(settled.pair_paths, expected_pair_paths);
        assert_eq!(settled.cold_cache_assignment_counts, BTreeSet::from([1]));
        assert_eq!(loaded.pair_file_pins, settled.pair_file_pins * NUM_SAMPLES);
        assert_eq!((settled.retirements, settled.deleted), (0, 0));
        assert!(settled.errors.is_empty(), "{settled:?}");
        let released_pair_pins = (loaded.pair_file_pins - settled.pair_file_pins) as u64;
        let loaded_handles = loaded_process["handle_count"].as_u64().unwrap();
        let settled_handles = process["handle_count"].as_u64().unwrap();
        assert!(
            settled_handles + released_pair_pins <= loaded_handles + SETTLED_HANDLE_TOLERANCE,
            "actual handle reduction must cover closed paired File pins: cycle={cycle}, closed_pair_pins={released_pair_pins}, loaded={loaded_process}, settled={process}"
        );
        assert_files_unchanged(&preserved);
        if let Some(expected) = &settled_baseline {
            assert_eq!(
                &settled, expected,
                "logical ownership grew after equal settled cycle {cycle}"
            );
        } else {
            settled_baseline = Some(settled);
        }
        if let Some(before) = &process_baseline {
            assert_eq!(process["process_id"], before["process_id"]);
            assert_eq!(
                process["process_creation_ticks"],
                before["process_creation_ticks"]
            );
            let actual = process["handle_count"].as_u64().unwrap();
            let baseline = before["handle_count"].as_u64().unwrap();
            assert!(
                actual <= baseline + SETTLED_HANDLE_TOLERANCE,
                "post-settlement handle growth: cycle={cycle}, first={baseline}, actual={actual}, fixed_tolerance={SETTLED_HANDLE_TOLERANCE}; before={before}; after={process}"
            );
        } else {
            process_baseline = Some(process);
        }
    }
    original_owner.release();
    stem_owner.release();
    assets.collect_for_test();
    let (released, _) = stage(
        &assets,
        CYCLES + 1,
        "assignment-handles-released-saved-original-retained",
    );
    assert_eq!(
        (
            released.readers,
            released.cold_readers,
            released.paired_readers
        ),
        (1, 1, 0)
    );
    assert_eq!(released.saved_cold_claims, 1);
    assert_eq!(
        (
            released.owners,
            released.pair_file_pins,
            released.retirements,
            released.deleted
        ),
        (0, 0, 0, 0)
    );
    assert!(released.errors.is_empty());
    assert_files_unchanged(&preserved);
    drop(assets);
    // Closing this isolated service is a process-lifetime analogue, not the
    // production shared singleton's shutdown contract. Saved files still exist.
    assert_files_unchanged(&preserved);
    eprintln!(
        "{}",
        json!({"proof":"same-material-project-assets-plateau",
        "stage":"isolated-registry-dropped-files-preserved",
        "process":crate::audio_engine::c3_process_metrics::snapshot(),
        "cycles":CYCLES,"subscribers_per_cycle":NUM_SAMPLES,
        "scope":"actual disk/cold/pair/component/stopped Native bank ownership; isolated registry; no NN/FFmpeg/Python observer/CPAL/callback ACK or full-process leak acceptance"})
    );
    drop(temp);
}
