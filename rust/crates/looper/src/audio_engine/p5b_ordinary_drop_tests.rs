//! End only the actual original fixture, preserving independent saved sources.
use super::*;
use std::cell::RefCell;
use std::os::windows::io::{AsRawHandle, IntoRawHandle, OwnedHandle};
use std::rc::Rc;

#[path = "window_cancel_restore_tests.rs"]
mod window_cancel_restore;
pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) use window_cancel_restore::finish_cancelled_source_fixture;

#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetThreadId(thread: *mut std::ffi::c_void) -> u32;
    fn GetThreadTimes(
        thread: *mut std::ffi::c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
}

struct WorkerWitness {
    handle: OwnedHandle,
    id: u32,
    birth: u64,
}

impl WorkerWitness {
    fn capture(handle: OwnedHandle) -> Self {
        let (mut creation, mut exit, mut kernel, mut user) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        // The duplicated kernel handle retains no Rust worker or PCM owner.
        let id = unsafe { GetThreadId(handle.as_raw_handle()) };
        assert_ne!(id, 0);
        assert_ne!(
            unsafe {
                GetThreadTimes(
                    handle.as_raw_handle(),
                    &mut creation,
                    &mut exit,
                    &mut kernel,
                    &mut user,
                )
            },
            0
        );
        Self {
            handle,
            id,
            birth: (u64::from(creation.high) << 32) | u64::from(creation.low),
        }
    }

    fn observation(&self) -> Value {
        let wait = unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) };
        json!({"thread_id": self.id, "birth_file_time": self.birth, "wait_result": wait})
    }
    fn close(self) {
        assert_ne!(unsafe { CloseHandle(self.handle.into_raw_handle()) }, 0);
    }
}

#[derive(Default)]
struct Observation {
    sources: Vec<AssignmentWitness>,
    workers: Vec<WorkerWitness>,
    readers: Vec<std::sync::Weak<[f32]>>,
    engine_owner: Option<std::sync::Weak<InputRuntimeOwnership>>,
}

thread_local! {
    static OBSERVATION: RefCell<Option<Observation>> = const { RefCell::new(None) };
    // This separate file custody is deliberately longer-lived than the original fixture.
    static SAVED_CONTEXT: RefCell<Option<Rc<SavedSource>>> = const { RefCell::new(None) };
}

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn begin_ordinary_fixture()
 {
    OBSERVATION.with_borrow_mut(|slot| {
        assert!(slot.is_none());
        *slot = Some(Observation::default());
    });
}

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests) fn observe_ordinary_harness(
    h: &Harness,
) {
    OBSERVATION.with_borrow_mut(|slot| {
        let Some(observation) = slot else { return };
        if observation.sources.is_empty() {
            observation.engine_owner = Some(Arc::downgrade(&h.engine.input_runtime_ownership));
            observation.workers.extend(
                h.engine
                    .cold_jobs
                    .thread_handles_for_test()
                    .into_iter()
                    .map(WorkerWitness::capture),
            );
            observation.workers.extend(
                h.callback
                    .mixer
                    .thread_handles_for_test()
                    .into_iter()
                    .map(WorkerWitness::capture),
            );
        }
        observation.sources.push(AssignmentWitness::capture(h));
    });
}

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn observe_ordinary_productive(
    p: &Productive,
) {
    OBSERVATION.with_borrow_mut(|slot| {
        let Some(observation) = slot else { return };
        observation.workers.extend(
            p.reference
                .thread_handles_for_test()
                .into_iter()
                .map(WorkerWitness::capture),
        );
        observation.readers.extend(pcm(&p.reference));
    });
}

fn assignment_rows(sources: &[AssignmentWitness]) -> Vec<Value> {
    let assets = ProjectAssets::shared();
    let state = assets.state.lock().unwrap();
    let mut rows = Vec::new();
    for reader in &state.readers {
        if let Some(lease) = &reader.cold
            && sources.iter().any(|source| {
                source.assignments.contains(&lease.assignment_id())
                    || (source.original.path == lease.original_path
                        && source.cache[0].path == lease.cache_path)
            })
        {
            rows.push(json!({
                "assignment_id": lease.assignment_id(), "original": lease.original_path,
                "cache": lease.cache_path, "assignment_retired": lease.assignment_retired(),
                "pcm_strong_counts": reader.pcm.iter().map(std::sync::Weak::strong_count).collect::<Vec<_>>()
            }));
        }
    }
    rows
}

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn finish_ordinary_fixture(
    p: Productive,
) {
    let mut observation = OBSERVATION.with_borrow_mut(|slot| slot.take().unwrap());
    assert_eq!(observation.sources.len(), 2);
    assert_eq!(observation.workers.len(), 6);
    assert_eq!(
        observation
            .workers
            .iter()
            .map(|worker| (worker.id, worker.birth))
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        6
    );
    assert_ne!(
        observation.sources[0].original.path,
        observation.sources[1].original.path
    );
    assert_ne!(
        observation.sources[0].cache[0].path,
        observation.sources[1].cache[0].path
    );
    observation.readers.extend(pcm(&p.h.callback.mixer));
    observation
        .readers
        .push(Arc::downgrade(&p.h.material.sample.samples));
    let sources: [AssignmentWitness; 2] = std::mem::take(&mut observation.sources)
        .try_into()
        .unwrap_or_else(|_| panic!("both actual construction generations"));
    let saved = SAVED_CONTEXT.with_borrow(Clone::clone);
    let trace_native = saved.is_some();
    retire_original_fixture(p, &sources, saved.as_deref(), move || {
        let workers: Vec<_> = observation
            .workers
            .iter()
            .map(WorkerWitness::observation)
            .collect();
        let pcm_counts: Vec<_> = observation
            .readers
            .iter()
            .map(std::sync::Weak::strong_count)
            .collect();
        eprintln!(
            "ordinary original fixture terminal RAW END {}",
            json!({
                "workers": workers, "pcm_strong_counts": pcm_counts,
                "engine_owners": observation.engine_owner.as_ref().unwrap().strong_count(),
                "nested_worker_join_panic_results": "UNKNOWN: nested Drop discards results"
            })
        );
        assert_eq!(observation.engine_owner.as_ref().unwrap().strong_count(), 0);
        assert!(pcm_counts.iter().all(|count| *count == 0));
        assert!(workers.iter().all(|worker| worker["wait_result"] == 0));
        for worker in observation.workers.drain(..) {
            worker.close();
        }
        eprintln!(
            "ordinary original fixture terminal: six actual workers END and duplicate handles closed"
        );
        // Only the isolated changed-entry context enables Native observation.
        // Normal ordinary execution uses the same checked close/worker barriers.
        trace_native.then(native_end)
    });
    wait_phase("original fixture exact assignment claims end", 0, || {
        assignment_rows(&sources).is_empty()
    });
    eprintln!(
        "ordinary original fixture terminal: both exact source/cache assignment claims ended"
    );
}

#[test]
#[ignore = "actual changed ordinary entry and saved restore; fresh process Native observation"]
fn actual_ordinary_ordered_fixture_end_preserves_saved_restore() {
    native::enable();
    let saved = Rc::new(SavedSource::after_engine_shutdown());
    let saved_temp = ObjectWitness::capture(saved._temp.path());
    let saved_root = ObjectWitness::capture(&saved.root);
    saved.verify_saved_descriptor();
    SAVED_CONTEXT.with_borrow_mut(|slot| {
        assert!(slot.is_none());
        *slot = Some(saved.clone());
    });
    // This calls the actual original leaf, including Productive::new and every
    // original body operation/assertion, followed by its corrected terminal end.
    ordered_legacy_full_mix_and_selection_cancel_never_repromote_accepted_typed_components();
    let context = SAVED_CONTEXT.with_borrow_mut(Option::take).unwrap();
    assert!(Rc::ptr_eq(&context, &saved));
    drop(context);
    saved_temp.verify();
    saved_root.verify();
    saved.verify_saved_descriptor();
    saved.restore_then_retire();
    eprintln!(
        "ordinary original fixture terminal FINAL: independent warm own ACK/exact PCM; {} actual Native lifetimes deleted",
        native_end()
    );
}
