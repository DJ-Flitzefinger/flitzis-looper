//! One original fixture's declared end, beside an independently saved source.
//! Run this ignored leaf alone in a fresh process: Native observation has no reset.
use super::*;
use crate::audio_engine::buffer_retirement::{
    close_callback_and_join, create_audio_buffer_retirement_with_callback,
};
use crate::audio_engine::native_lifecycle_observation::{self as native, NativeEventKind};
use std::collections::BTreeMap;
use std::os::windows::fs::MetadataExt;

#[path = "p5b_ordinary_drop_tests.rs"]
mod ordinary_drop;
pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests) use ordinary_drop::observe_ordinary_harness;
pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) use ordinary_drop::{begin_ordinary_fixture, assert_current_complete_source, capture_component_cancel_currentness, capture_legacy_currentness, capture_paired_currentness, finish_legacy_source_fixture, finish_cancelled_source_fixture, finish_copied_source_fixture, finish_moved_source_fixture, finish_ordinary_fixture, observe_ordinary_productive};

#[derive(Debug, PartialEq)]
struct ObjectWitness {
    path: PathBuf,
    identity: FileIdentity,
    content: Option<(u64, String)>,
}

impl ObjectWitness {
    fn capture(path: &Path) -> Self {
        let metadata = fs::symlink_metadata(path).unwrap();
        assert!(!metadata.file_type().is_symlink());
        assert_eq!(
            metadata.file_attributes() & 0x400,
            0,
            "reparse object preserved"
        );
        assert!(metadata.is_file() || metadata.is_dir());
        Self {
            path: path.to_owned(),
            identity: capture_identity(path).unwrap().unwrap(),
            content: metadata.is_file().then(|| {
                let bytes = fs::read(path).unwrap();
                (bytes.len() as u64, format!("{:x}", Sha256::digest(&bytes)))
            }),
        }
    }

    fn verify(&self) {
        assert_eq!(Self::capture(&self.path), *self);
    }

    fn evidence(&self) -> Value {
        json!({"path": self.path, "file_id": self.identity, "content": self.content})
    }
}

fn tree(path: &Path) -> Vec<ObjectWitness> {
    let mut result = vec![ObjectWitness::capture(path)];
    if path.is_dir() {
        let mut children: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        children.sort();
        for child in children {
            result.extend(tree(&child));
        }
    }
    result
}

struct AssignmentWitness {
    original: ObjectWitness,
    cache: Vec<ObjectWitness>,
    wav: Vec<ObjectWitness>,
    assignments: [u64; 2],
    pcm: Vec<std::sync::Weak<[f32]>>,
}

fn pcm(mixer: &RtMixer) -> Vec<std::sync::Weak<[f32]>> {
    let mut result: Vec<_> = mixer
        .bank_for_measurement()
        .iter()
        .flatten()
        .map(|sample| Arc::downgrade(&sample.samples))
        .collect();
    for stems in mixer.stems_for_measurement().iter().flatten() {
        result.push(Arc::downgrade(&stems.reference_samples));
        result.extend(stems.stems.iter().map(|stem| Arc::downgrade(&stem.samples)));
    }
    result
}

impl AssignmentWitness {
    fn capture(h: &Harness) -> Self {
        // Capture at the actual construction seam, before long_harness replaces
        // the first material. Containment and a registry census grant no rights.
        let original = ObjectWitness::capture(&h.material.lease.original_path);
        let cache = tree(&h.material.lease.cache_path);
        let wav = tree(&h.wav_path);
        let assignments = {
            let leases = h.engine.cold_leases.lock().unwrap();
            [0, NUM_SAMPLES - 1].map(|id| {
                let lease = leases[id].as_ref().unwrap();
                assert_eq!(lease.original_path, original.path);
                assert_eq!(lease.cache_path, cache[0].path);
                assert!(lease.original_identity_matches(&original.identity));
                lease.assignment_id()
            })
        };
        let mut pcm = pcm(&h.callback.mixer);
        pcm.push(Arc::downgrade(&h.material.sample.samples));
        Self {
            original,
            cache,
            wav,
            assignments,
            pcm,
        }
    }

    fn verify(&self) {
        self.original.verify();
        assert_eq!(tree(&self.cache[0].path), self.cache);
        assert_eq!(tree(&self.wav[0].path), self.wav);
    }

    fn evidence(&self) -> Value {
        json!({
            "original": self.original.evidence(),
            "cache": self.cache.iter().map(ObjectWitness::evidence).collect::<Vec<_>>(),
            "wav": self.wav.iter().map(ObjectWitness::evidence).collect::<Vec<_>>(),
            "source_ack_assignment_ids": self.assignments,
            "pcm_weak_witnesses": self.pcm.len(),
        })
    }
}

// Once installed at terminal consumption, a failure preserves this exact
// test-owned TempDir. Earlier construction/body failures use the original Drop;
// a consumed close error is reported by TempDir itself.
struct KeepOnFailure(Option<tempfile::TempDir>);
impl Drop for KeepOnFailure {
    fn drop(&mut self) {
        if let Some(temp) = self.0.take() {
            eprintln!("fixture lifetime failure: preserved {:?}", temp.keep());
        }
    }
}

fn native_end() -> usize {
    let snapshot = native::snapshot();
    assert!(!snapshot.overflowed);
    assert_eq!(snapshot.incomplete, 0);
    assert_eq!(snapshot.reserved, snapshot.events.len());
    let mut created = BTreeMap::new();
    let mut deleted = BTreeMap::new();
    for event in snapshot.events {
        match event.kind {
            NativeEventKind::Created => {
                assert!(
                    created
                        .insert(event.native_id, event.opaque_handle)
                        .is_none()
                );
            }
            NativeEventKind::DeleteCompleted => {
                assert_eq!(created.get(&event.native_id), Some(&event.opaque_handle));
                assert!(
                    deleted
                        .insert(event.native_id, event.opaque_handle)
                        .is_none()
                );
            }
        }
    }
    assert!(!created.is_empty());
    assert_eq!(
        created, deleted,
        "actual C deletion must return for every Native lifetime"
    );
    created.len()
}

struct ClosedFixture {
    temp: KeepOnFailure,
    root: PathBuf,
    assets: Arc<ProjectAssets>,
    directory: ObjectWitness,
    inventory: Vec<ObjectWitness>,
    paired: [PathBuf; 3],
    saved_owner: crate::audio_engine::project_assets::ProjectAssetLease,
    original_owner: crate::audio_engine::project_assets::ProjectAssetLease,
    reader_count: usize,
}

// Separate checked reader END from explicit file retirement so this SAME saved
// source can outlive its original engine and be restored before its declared end.
fn close_productive_readers(p: Productive, sources: &[AssignmentWitness; 2]) -> ClosedFixture {
    for source in sources {
        source.verify();
    }
    let inventory = tree(p.h._temp.path());
    let directory = ObjectWitness::capture(&p.h.root);
    let assets = p.h.engine.project_assets.clone();
    let engine_owner = Arc::downgrade(&p.h.engine.input_runtime_ownership);
    let mut readers = pcm(&p.h.callback.mixer);
    readers.extend(pcm(&p.reference));
    readers.push(Arc::downgrade(&p.h.material.sample.samples));
    for source in sources {
        readers.extend(source.pcm.iter().cloned());
    }
    let selected = selection(&p.pair);
    let paired = ["wav_generation", "pcm_generation", "descriptor_reference"]
        .map(|field| selected_path(&p.h.root, &selected, field));
    let Productive {
        h,
        _saved,
        pair,
        selected,
        reference,
        reference_ownership,
        ..
    } = p;
    let Harness {
        _temp,
        root,
        mut engine,
        mut callback,
        producer,
        consumer,
        material,
        _original_owner,
        ..
    } = h;
    let temp = KeepOnFailure(Some(_temp));
    // This test explicitly ends its TWO witnessed ephemeral source assignments.
    // The independently saved TempDir/assignments have a longer declared lifetime.
    engine.shut_down().unwrap();
    assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
    drop((
        selected,
        pair,
        producer,
        consumer,
        material,
        reference_ownership,
        engine,
    ));
    // Use the existing productive close barrier for BOTH real mixer images.
    // Their nested Native workers synchronously join in Drop. Their discarded
    // panic results are not independently observed successful-join receipts.
    let (owner, worker) = create_audio_buffer_retirement_with_callback(|actual| {
        callback.retirement.actual = Some(actual);
        Box::new((callback, reference))
    })
    .unwrap();
    close_callback_and_join(owner, worker).unwrap();
    assert_eq!(engine_owner.strong_count(), 0);
    wait_phase("original ordered fixture actual PCM reader END", 0, || {
        readers.iter().all(|reader| reader.strong_count() == 0)
    });
    ClosedFixture {
        temp,
        root,
        assets,
        directory,
        inventory,
        paired,
        saved_owner: _saved,
        original_owner: _original_owner,
        reader_count: readers.len(),
    }
}

fn retire_original_fixture(
    p: Productive,
    sources: &[AssignmentWitness; 2],
    saved: Option<&SavedSource>,
    after_reader_end: impl FnOnce() -> Option<usize>,
) {
    if let Some(saved) = saved {
        assert_ne!(p.h._temp.path(), saved._temp.path());
    }
    let closed = close_productive_readers(p, sources);
    let native_lifetimes = after_reader_end();
    retire_closed_fixture(closed, sources, saved, native_lifetimes);
}

fn retire_closed_fixture(
    closed: ClosedFixture,
    sources: &[AssignmentWitness; 2],
    saved: Option<&SavedSource>,
    native_lifetimes: Option<usize>,
) {
    let ClosedFixture {
        mut temp,
        root,
        assets,
        directory,
        inventory,
        paired,
        saved_owner: _saved,
        original_owner: _original_owner,
        reader_count,
    } = closed;
    directory.verify();
    assert_eq!(tree(temp.0.as_ref().unwrap().path()), inventory);
    for source in sources {
        source.verify();
    }
    if let Some(saved) = saved {
        saved.verify_saved_descriptor();
    }
    eprintln!(
        "original fixture lifetime BEFORE RETIRE {}",
        json!({
            "temp_dir": inventory[0].evidence(), "root": directory.evidence(),
            "inventory": inventory.iter().map(ObjectWitness::evidence).collect::<Vec<_>>(),
            "sources": sources.iter().map(AssignmentWitness::evidence).collect::<Vec<_>>(),
            "cold_workers_checked_end": true, "callback_and_reference_close_join": true,
            "nested_native_worker_panic_results": "unobserved", "pcm_weak_end": reader_count,
            "native_delete_completed_lifetimes": native_lifetimes,
            "authority": "ONLY this original generated fixture's explicit assignment end",
            "independent_saved_context_verified": saved.is_some(),
        })
    );
    // Existing sealed paired retirement resolves only this accepted selection.
    // Original retirement also retires its matching cold cache through the
    // existing reader-aware worker. No collector/prune/limit change is used.
    assets.retire(&root, &paired[0], true).unwrap();
    for source in sources {
        assets.retire(&root, &source.original.path, false).unwrap();
    }
    drop((_saved, _original_owner));
    wait_phase(
        "original fixture exact own paired and source retirement",
        0,
        || {
            paired.iter().all(|path| !path.exists())
                && sources
                    .iter()
                    .all(|source| !source.original.path.exists() && !source.cache[0].path.exists())
        },
    );
    if let Some(saved) = saved {
        saved.verify_saved_descriptor();
    }
    // Physical cleanup is confined to this original TempDir after witnessed END
    // and exact identity-gated retirement; its normal checked close is explicit.
    // Both existing parent-pruners use this admission gate. Keep the final
    // custody snapshot and close serialized with them; queue/path absence alone
    // does not prove that their later empty-parent pass has finished.
    let cleanup_gate = assets.state.lock().unwrap();
    let path = temp.0.as_ref().unwrap().path().to_owned();
    inventory[0].verify();
    directory.verify();
    let remaining = tree(&path);
    for actual in &remaining {
        assert_eq!(
            inventory.iter().find(|prior| prior.path == actual.path),
            Some(actual),
            "new, replaced or changed residual object must be preserved"
        );
    }
    eprintln!(
        "original fixture lifetime RESIDUAL {}",
        json!({
            "inventory": remaining.iter().map(ObjectWitness::evidence).collect::<Vec<_>>(),
            "all_surviving_objects_exactly_original": true,
        })
    );
    temp.0.take().unwrap().close().unwrap();
    assert!(!path.exists());
    drop(cleanup_gate);
    eprintln!("original fixture final residual and close serialized with asset admission gate");
    if let Some(saved) = saved {
        saved.verify_saved_descriptor();
    }
    eprintln!(
        "original fixture lifetime AFTER CLOSE: exact own TempDir ended; independent saved context verified={}",
        saved.is_some()
    );
}

#[test]
#[ignore = "original fixture ownership and saved restore context; fresh process Native observation"]
fn original_ordered_fixture_ends_only_its_own_assignments_before_saved_restore() {
    native::enable();
    let saved = SavedSource::after_engine_shutdown();
    let saved_temp = ObjectWitness::capture(saved._temp.path());
    let saved_root = ObjectWitness::capture(&saved.root);
    saved.verify_saved_descriptor();
    let h = Harness::new();
    let first = AssignmentWitness::capture(&h);
    let h = long_harness_from(h, 4096);
    let second = AssignmentWitness::capture(&h);
    assert_ne!(first.original.path, second.original.path);
    assert_ne!(first.cache[0].path, second.cache[0].path);
    let p = Productive::with_harness(h, false, 0.75, false);
    let p = ordered_legacy_fixture(p);
    retire_original_fixture(p, &[first, second], Some(&saved), || Some(native_end()));
    saved_temp.verify();
    saved_root.verify();
    saved.verify_saved_descriptor();
    saved.restore_then_retire();
    let native_lifetimes = native_end();
    eprintln!(
        "original fixture lifetime FINAL: warm restore own ACK/PCM exact; all {native_lifetimes} Native lifetimes deleted"
    );
}
