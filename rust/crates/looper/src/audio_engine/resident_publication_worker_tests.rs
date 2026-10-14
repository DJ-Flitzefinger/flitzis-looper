//! Failure before actual reused-window control publication preserves its predecessor.
use super::*;
use crate::audio_engine::resident_relocation::control_publication_tests::{
    PendingWriter, with_before_commit,
};
use std::sync::atomic::AtomicU64;

fn same_window() -> WindowRequest {
    WindowRequest {
        loop_region: Some((32.0 / 48_000.0, Some(64.0 / 48_000.0))),
        key_lock: Some(false),
        ..WindowRequest::default()
    }
}

struct Before {
    placement: crate::audio_engine::content_placement::SlotSnapshot,
    epoch: Arc<AtomicU64>,
    intent: u64,
    control: u64,
    request: u64,
    sample: SampleBuffer,
}

impl Before {
    fn capture(loaded: &Loaded) -> Self {
        let ownership = &loaded.engine.input_runtime_ownership;
        let placement = ownership.slot_snapshot(0).unwrap();
        let epoch = loaded.engine.resident_stem_cache.lock().unwrap()[0]
            .intent_epoch
            .clone();
        Self {
            placement,
            intent: epoch.load(Ordering::Acquire),
            control: ownership
                .resident_control_for_cell(placement.cell())
                .load(Ordering::Acquire),
            request: ownership
                .key_lock_request_for_cell(placement.cell())
                .load(Ordering::Acquire),
            sample: loaded.sample(),
            epoch,
        }
    }

    fn unchanged(&self, loaded: &Loaded, predecessor: &ResidentWindowTicket) {
        let ownership = &loaded.engine.input_runtime_ownership;
        assert_eq!(self.epoch.load(Ordering::Acquire), self.intent);
        assert_eq!(
            ownership
                .resident_control_for_cell(self.placement.cell())
                .load(Ordering::Acquire),
            self.control
        );
        assert_eq!(
            ownership
                .key_lock_request_for_cell(self.placement.cell())
                .load(Ordering::Acquire),
            self.request
        );
        assert!(loaded.sample().same_window(&self.sample));
        assert_eq!(predecessor.publication_status(), "pending");
        let ControlMessage::RelocateResident(transaction) = loaded.consumer.peek().unwrap() else {
            panic!("failed admission must preserve the actual predecessor command");
        };
        assert_eq!(transaction.publication.expected, self.intent);
        assert_eq!(transaction.intent.key_lock_request_id, Some(self.request));
        assert!(transaction.sample.same_window(&self.sample));
    }
}

#[test]
fn real_reused_window_pending_after_preparation_preserves_predecessor_and_retry_ack() {
    pyo3::Python::initialize();
    let mut loaded = Loaded::new();
    let predecessor = loaded.prepare(same_window());
    let before = Before::capture(&loaded);
    let held = Arc::new(Mutex::new(None));
    let keep = held.clone();
    let ownership = loaded.engine.input_runtime_ownership.clone();
    let result = with_before_commit(
        move || *keep.lock().unwrap() = Some(PendingWriter::hold(ownership)),
        || prepare_window_with_producer(&loaded.engine, 0, same_window(), loaded.producer.clone()),
    );
    held.lock().unwrap().take().unwrap().complete();
    assert!(result.is_err());
    before.unchanged(&loaded, &predecessor);
    loaded.adopt(&predecessor);
    let retry = loaded.prepare(same_window());
    assert_eq!(
        retry.key_lock_request_id_for_test(),
        Some(before.request + 1)
    );
    loaded.adopt(&retry);
    assert!(loaded.consumer.is_empty());
    assert!(
        !loaded
            .engine
            .input_runtime_ownership
            .resident_control_pending(0)
    );
    assert!(loaded.sample().same_window(&before.sample));
}

#[test]
fn real_reused_window_rebound_after_preparation_preserves_old_cell_and_command() {
    pyo3::Python::initialize();
    let mut loaded = Loaded::new();
    let predecessor = loaded.prepare(same_window());
    let before = Before::capture(&loaded);
    let ownership = loaded.engine.input_runtime_ownership.clone();
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
        || prepare_window_with_producer(&loaded.engine, 0, same_window(), loaded.producer.clone()),
    );
    assert!(result.is_err());
    before.unchanged(&loaded, &predecessor);
    let ownership = &loaded.engine.input_runtime_ownership;
    assert_eq!(
        ownership.slot_snapshot(1).unwrap().content_key(),
        before.placement.content_key()
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
    // Authority-only Pair fixture: restore physical routing before real callback adoption.
    let pair = ownership
        .prepare_placement_pair(
            ownership.slot_snapshot(1).unwrap(),
            ownership.slot_snapshot(0).unwrap(),
        )
        .unwrap();
    ownership
        .commit_placement_pair(&pair, true, || true, || {})
        .unwrap();
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    assert!(loaded.consumer.is_empty());
    assert!(loaded.sample().same_window(&before.sample));
}

#[test]
fn real_reused_window_full_command_queue_keeps_predecessor_and_captured_requests() {
    pyo3::Python::initialize();
    let mut loaded = Loaded::new();
    let predecessor = loaded.prepare(same_window());
    let before = Before::capture(&loaded);
    while !loaded.producer.lock().unwrap().is_full() {
        loaded
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::Ping())
            .unwrap();
    }
    let result =
        prepare_window_with_producer(&loaded.engine, 0, same_window(), loaded.producer.clone());
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("resident command queue is full")
    );
    before.unchanged(&loaded, &predecessor);
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 8);
    assert_eq!(predecessor.publication_status(), "accepted");
    assert!(predecessor.is_current());
    reconcile(&loaded.engine).unwrap();
    assert!(loaded.consumer.is_empty());
    let retry = loaded.prepare(same_window());
    assert_eq!(
        retry.key_lock_request_id_for_test(),
        Some(before.request + 1)
    );
    loaded.adopt(&retry);
}
