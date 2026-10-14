//! Captured resident intent publication and external terminal settlement.
use super::*;
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use std::cell::{Cell, RefCell};
use std::sync::mpsc;

thread_local! {
    static BEFORE_COMMIT: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

pub(super) fn before_commit() {
    let hook = BEFORE_COMMIT.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

pub(in crate::audio_engine) fn with_before_commit<T>(
    hook: impl FnOnce() + 'static,
    action: impl FnOnce() -> T,
) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            BEFORE_COMMIT.with(|slot| *slot.borrow_mut() = None);
        }
    }
    BEFORE_COMMIT.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    let _reset = Reset;
    action()
}

/// Hold an actual unrelated writer's odd marker. Drop releases it even on failed assertions.
pub(in crate::audio_engine) struct PendingWriter {
    release: Option<mpsc::Sender<()>>,
    writer: Option<std::thread::JoinHandle<Result<SlotSnapshot, PlacementError>>>,
}

impl PendingWriter {
    pub(in crate::audio_engine) fn hold(ownership: Arc<InputRuntimeOwnership>) -> Self {
        let other = ownership.slot_snapshot(NUM_SAMPLES - 1).unwrap();
        let write = ownership
            .prepare_slot_write_fixed(other, other.lifetime())
            .unwrap();
        let (entered, observed) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            ownership.commit_slot_write_fixed(
                &write,
                || true,
                |_| {
                    let _ = entered.send(());
                    let _ = resume.recv();
                },
            )
        });
        let held = Self {
            release: Some(release),
            writer: Some(writer),
        };
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        held
    }

    pub(in crate::audio_engine) fn complete(mut self) {
        self.release.take().unwrap().send(()).unwrap();
        self.writer.take().unwrap().join().unwrap().unwrap();
    }
}

impl Drop for PendingWriter {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

struct Fixture {
    engine: AudioEngine,
    binding: InputRuntimePadBinding,
    epoch: AtomicU64,
    placement: SlotSnapshot,
}

impl Fixture {
    fn new() -> Self {
        pyo3::Python::initialize();
        let engine = AudioEngine::new().unwrap();
        let sample = SampleBuffer {
            samples: Arc::from([0.25_f32; 64]),
            channels: 1,
            residency: None,
        }
        .with_complete_source(48_000);
        let ownership = &engine.input_runtime_ownership;
        let captured = ownership.slot_snapshot(0).unwrap();
        let lifetime = super::super::content_lifetime::reserve().unwrap();
        let write = ownership.prepare_slot_write(captured, lifetime).unwrap();
        let publication = ownership
            .publish_source_write(
                &write,
                &sample,
                48_000,
                7,
                ownership.next_authority(0).unwrap(),
                Some(TimingIntent::Manual),
                false,
                || true,
            )
            .unwrap();
        assert!(publication.current(&sample, 48_000));
        engine.sample_cache.lock().unwrap()[0] = Some(sample);
        engine.pad_request_ids.lock().unwrap()[0] = 7;
        engine.loaded_source_generations.lock().unwrap()[0] = (7, 48_000);
        engine.loaded_source_digests.lock().unwrap()[0] = Some(format!("{:064x}", 7));
        engine.timing_intents.lock().unwrap()[0] = TimingIntent::Manual;
        engine
            .prepared_source_epoch(0)
            .unwrap()
            .store(7, Ordering::Release);
        let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
        assert!(binding.current());
        let placement = binding.binding.placement.unwrap();
        ownership
            .resident_control_for_cell(placement.cell())
            .store(6, Ordering::Release);
        ownership
            .key_lock_request_for_cell(placement.cell())
            .store(4, Ordering::Release);
        Self {
            engine,
            binding,
            epoch: AtomicU64::new(9),
            placement,
        }
    }

    fn prepared(&self) -> ResidentControlWrite {
        ResidentControlWrite::capture(&self.binding, 9, 10, true).unwrap()
    }

    fn counters(&self) -> (u64, u64, u64) {
        let ownership = &self.engine.input_runtime_ownership;
        (
            self.epoch.load(Ordering::Acquire),
            ownership
                .resident_control_for_cell(self.placement.cell())
                .load(Ordering::Acquire),
            ownership
                .key_lock_request_for_cell(self.placement.cell())
                .load(Ordering::Acquire),
        )
    }
}

#[test]
fn resident_control_success_stores_only_prepared_counters_inside_captured_cell() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let applied = Cell::new(false);
    prepared
        .commit(
            &fixture.binding,
            &fixture.epoch,
            || true,
            || {
                assert!(fixture.engine.input_runtime_ownership.placement_pending());
                assert_eq!(fixture.counters(), (10, 10, 5));
                applied.set(true);
            },
        )
        .unwrap();
    assert!(applied.get());
    let after = fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap();
    assert_eq!(after.content_key(), fixture.placement.content_key());
    assert_eq!(after.binding_epoch(), fixture.placement.binding_epoch());
    assert_eq!(fixture.counters(), (10, 10, 5));
}

#[test]
fn resident_control_pending_after_preparation_preserves_all_counters_and_retries() {
    let fixture = Fixture::new();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let held = Arc::new(Mutex::new(None));
    let keep = held.clone();
    let applied = Cell::new(false);
    let prepared = fixture.prepared();
    let result = with_before_commit(
        move || *keep.lock().unwrap() = Some(PendingWriter::hold(ownership)),
        || {
            prepared.commit(
                &fixture.binding,
                &fixture.epoch,
                || true,
                || applied.set(true),
            )
        },
    );
    let while_pending = fixture.counters();
    held.lock().unwrap().take().unwrap().complete();
    assert!(result.is_err());
    assert!(!applied.get());
    assert_eq!(while_pending, (9, 6, 4));
    assert_eq!(fixture.counters(), (9, 6, 4));
    prepared
        .commit(&fixture.binding, &fixture.epoch, || true, || {})
        .unwrap();
    assert_eq!(fixture.counters(), (10, 10, 5));
}

#[test]
fn resident_control_rebound_after_preparation_cannot_route_to_replacement_slot() {
    let fixture = Fixture::new();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let prepared = fixture.prepared();
    let applied = Cell::new(false);
    let result = with_before_commit(
        move || {
            let pair = ownership
                .prepare_placement_pair(
                    ownership.slot_snapshot(0).unwrap(),
                    ownership.slot_snapshot(1).unwrap(),
                )
                .unwrap();
            ownership
                .commit_placement_pair(&pair, true, || true, || {})
                .unwrap();
        },
        || {
            prepared.commit(
                &fixture.binding,
                &fixture.epoch,
                || true,
                || applied.set(true),
            )
        },
    );
    assert!(result.is_err());
    assert!(!applied.get());
    assert_eq!(fixture.counters(), (9, 6, 4));
    let ownership = &fixture.engine.input_runtime_ownership;
    assert_eq!(
        ownership.slot_snapshot(1).unwrap().content_key(),
        fixture.placement.content_key()
    );
    let replacement = ownership.slot_snapshot(0).unwrap().cell();
    assert_eq!(
        ownership
            .resident_control_for_cell(replacement)
            .load(Ordering::Acquire),
        0
    );
    assert_eq!(
        ownership
            .key_lock_request_for_cell(replacement)
            .load(Ordering::Acquire),
        0
    );
}

#[test]
fn resident_control_original_request_control_or_intent_change_rejects_before_apply() {
    for change in 0..3 {
        let fixture = Fixture::new();
        let prepared = fixture.prepared();
        let claimed = Cell::new(false);
        match change {
            0 => fixture.epoch.store(20, Ordering::Release),
            1 => fixture
                .engine
                .input_runtime_ownership
                .key_lock_request_for_cell(fixture.placement.cell())
                .store(21, Ordering::Release),
            _ => fixture
                .engine
                .input_runtime_ownership
                .resident_control_for_cell(fixture.placement.cell())
                .store(22, Ordering::Release),
        }
        let before = fixture.counters();
        let result = prepared.commit(
            &fixture.binding,
            &fixture.epoch,
            || {
                claimed.set(true);
                true
            },
            || panic!("stale captured counters cannot apply"),
        );
        assert!(result.is_err());
        assert!(!claimed.get());
        assert_eq!(fixture.counters(), before);
    }
}

#[test]
fn resident_control_rejected_final_claim_preserves_cell_intent_and_layout() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared();
    let result = prepared.commit(
        &fixture.binding,
        &fixture.epoch,
        || false,
        || panic!("rejected final claim cannot publish"),
    );
    assert!(result.is_err());
    assert_eq!(fixture.counters(), (9, 6, 4));
    assert_eq!(
        fixture.engine.input_runtime_ownership.slot_snapshot(0),
        Some(fixture.placement)
    );
}

#[test]
fn resident_failure_pending_retains_terminal_until_retry_preserving_replacement_owner() {
    for replacement in [false, true] {
        let fixture = Fixture::new();
        let ownership = &fixture.engine.input_runtime_ownership;
        let state =
            WindowState::for_placement(fixture.placement, fixture.binding.source_generation);
        state.control_published.store(true, Ordering::Release);
        let held = PendingWriter::hold(ownership.clone());
        state.fail(ownership, 0, 6, "injected worker failure".into(), false);
        let before_retry = (state.terminal.load(Ordering::Acquire), fixture.counters());
        held.complete();
        if replacement {
            ownership
                .resident_control_for_cell(fixture.placement.cell())
                .store(23, Ordering::Release);
        }
        state.retry_failure(ownership);
        assert_eq!(before_retry, (0, (9, 6, 4)));
        assert_eq!(state.terminal.load(Ordering::Acquire), 1);
        assert!(state.failed_settlement.lock().unwrap().is_none());
        assert_eq!(fixture.counters().1, if replacement { 23 } else { 0 });
    }
}

#[test]
fn resident_failure_old_lifetime_and_unpublished_proposal_preserve_same_counter_owner() {
    for published in [false, true] {
        let fixture = Fixture::new();
        let ownership = &fixture.engine.input_runtime_ownership;
        let state =
            WindowState::for_placement(fixture.placement, fixture.binding.source_generation);
        state.control_published.store(published, Ordering::Release);
        if published {
            let replacement = super::super::content_lifetime::reserve().unwrap();
            let write = ownership
                .prepare_slot_write_fixed(fixture.placement, replacement)
                .unwrap();
            ownership
                .commit_slot_write_fixed(
                    &write,
                    || true,
                    |cell| {
                        ownership
                            .resident_control_for_cell(cell)
                            .store(6, Ordering::Release);
                    },
                )
                .unwrap();
            assert!(!ownership.content_key_current(fixture.placement.content_key().unwrap()));
        }
        state.fail(ownership, 0, 6, "superseded worker".into(), true);
        assert_eq!(state.terminal.load(Ordering::Acquire), 2);
        assert_eq!(
            fixture.counters().1,
            6,
            "same numeric counter grants no replacement authority"
        );
    }
}
