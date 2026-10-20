//! Stable committed admission reads over actual fixed-slot publication markers.
//! This file is a child of resident_pair_worker_tests, reusing its genuine cold
//! source/registration/callback fixture. Small helper tests use its declared
//! source-control fixture or an empty engine and make no loaded-source ACK claim.

use super::*;

const PENDING_CODE: &str = "[resident_stem_placement_pending]";

fn pending_error<T>(result: PyResult<T>) {
    let error = match result {
        Ok(_) => panic!("actual placement contention must defer committed admission"),
        Err(error) => error,
    };
    assert!(error.to_string().contains(PENDING_CODE), "{error}");
    Python::attach(|py| assert!(error.is_instance_of::<PyRuntimeError>(py)));
}

fn advance_unrelated_slot(engine: &AudioEngine) {
    let ownership = &engine.input_runtime_ownership;
    let before = ownership.slot_snapshot(215).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(before, before.lifetime())
        .unwrap();
    let after = ownership
        .commit_slot_write_fixed(&write, || true, |_| {})
        .unwrap();
    assert_eq!(after.content_key(), before.content_key());
    assert_eq!(after.cell(), before.cell());
    assert_eq!(after.binding_epoch(), before.binding_epoch());
}

fn proposal_epochs(engine: &AudioEngine, placement: SlotSnapshot) -> [u64; 4] {
    engine
        .resident_proposals
        .capture_pair_fences_for_cell(placement.cell())
        .unwrap()
        .map(|(epoch, _)| epoch.load(Ordering::Acquire))
}

fn committed_epochs(engine: &AudioEngine) -> [u64; 5] {
    let owners = engine.resident_stem_cache.lock().unwrap();
    let owner = &owners[0];
    [
        owner.intent_epoch.load(Ordering::Acquire),
        owner.committed.registration_epoch.load(Ordering::Acquire),
        owner.committed.selection_epoch.load(Ordering::Acquire),
        owner.committed.policy_epoch.load(Ordering::Acquire),
        owner
            .committed
            .policy_operation_epoch
            .load(Ordering::Acquire),
    ]
}

#[test]
fn odd_unrelated_slot_marker_defers_before_calling_the_committed_read() {
    Python::initialize();
    let engine = AudioEngine::new().unwrap();
    let own = engine.input_runtime_ownership.slot_snapshot(0).unwrap();
    let own_epoch = engine.prepared_source_epoch(0).unwrap().clone();
    let before_epoch = own_epoch.load(Ordering::Acquire);
    let before_proposals = proposal_epochs(&engine, own);
    let before_committed = committed_epochs(&engine);
    let before_requests = engine.pad_request_ids.lock().unwrap().clone();
    let before_cold = engine.cold_jobs.counts_for_test();
    let called = Cell::new(false);
    let held = PendingWriter::hold(engine.input_runtime_ownership.clone());
    assert_eq!(engine.input_runtime_ownership.placement_read_token(), None);
    assert!(engine.input_runtime_ownership.slot_snapshot(0).is_none());

    pending_error(engine.stable_committed_placement_read(|| {
        called.set(true);
        Ok(own_epoch.clone())
    }));

    assert!(!called.get());
    assert_eq!(own_epoch.load(Ordering::Acquire), before_epoch);
    assert_eq!(proposal_epochs(&engine, own), before_proposals);
    assert_eq!(committed_epochs(&engine), before_committed);
    assert_eq!(*engine.pad_request_ids.lock().unwrap(), before_requests);
    assert_eq!(engine.cold_jobs.counts_for_test(), before_cold);
    assert!(engine.active_tasks.lock().unwrap().is_empty());
    held.complete();
    assert!(engine.input_runtime_ownership.slot_snapshot_current(own));
}

#[test]
fn completed_mid_read_publication_invalidates_success_and_ambiguous_failure() {
    for returns_error in [false, true] {
        Python::initialize();
        let engine = AudioEngine::new().unwrap();
        let own = engine.input_runtime_ownership.slot_snapshot(0).unwrap();
        let token = engine
            .input_runtime_ownership
            .placement_read_token()
            .unwrap();
        let own_epoch = engine.prepared_source_epoch(0).unwrap().clone();
        let called = Cell::new(0);

        let result = engine.stable_committed_placement_read(|| {
            called.set(called.get() + 1);
            advance_unrelated_slot(&engine);
            if returns_error {
                Err(PyValueError::new_err("current source/timing unavailable"))
            } else {
                Ok(own_epoch.clone())
            }
        });

        pending_error(result);
        assert_eq!(
            called.get(),
            1,
            "the helper does not retry the protected read"
        );
        assert_ne!(
            engine.input_runtime_ownership.placement_read_token(),
            Some(token)
        );
        assert!(engine.input_runtime_ownership.slot_snapshot_current(own));
        assert!(Arc::ptr_eq(
            engine.prepared_source_epoch(0).unwrap(),
            &own_epoch
        ));
        assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
    }
}

#[test]
fn still_odd_at_read_end_reclassifies_the_actual_ambiguous_epoch_error() {
    Python::initialize();
    let engine = AudioEngine::new().unwrap();
    let own = engine.input_runtime_ownership.slot_snapshot(0).unwrap();
    let own_epoch = engine.prepared_source_epoch(0).unwrap().clone();
    let mut held = None;
    let called = Cell::new(0);

    let result = engine.stable_committed_placement_read(|| {
        called.set(called.get() + 1);
        held = Some(PendingWriter::hold(engine.input_runtime_ownership.clone()));
        engine
            .prepared_source_epoch(0)
            .cloned()
            .map_err(PyRuntimeError::new_err)
    });

    pending_error(result);
    assert_eq!(called.get(), 1);
    assert_eq!(engine.input_runtime_ownership.placement_read_token(), None);
    assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
    held.take().unwrap().complete();
    assert!(engine.input_runtime_ownership.slot_snapshot_current(own));
    assert!(Arc::ptr_eq(
        engine.prepared_source_epoch(0).unwrap(),
        &own_epoch
    ));
}

#[test]
fn stable_read_returns_the_original_epoch_arc_and_control_guards_without_publishing() {
    let fixture = Fixture::new();
    let engine = &fixture.engine;
    assert!(fixture.binding.current());
    assert!(fixture.binding.available());
    let own_epoch = engine.prepared_source_epoch(0).unwrap().clone();
    let other_epoch = engine.prepared_source_epoch(215).unwrap().clone();
    let token = engine.input_runtime_ownership.placement_read_token();
    let before_counters = fixture.counters();
    let before_proposals = proposal_epochs(engine, fixture.placement);
    let before_committed = committed_epochs(engine);
    let before_requests = engine.pad_request_ids.lock().unwrap().clone();

    let (epoch, control) = engine
        .stable_committed_placement_read(|| {
            let epoch = engine
                .prepared_source_epoch(0)
                .cloned()
                .map_err(PyRuntimeError::new_err)?;
            let control = CommittedControlWrite::capture(&fixture.binding, 9, Some(10))
                .map_err(PyRuntimeError::new_err)?;
            Ok((epoch, control))
        })
        .unwrap();

    assert!(Arc::ptr_eq(&epoch, &own_epoch));
    assert!(!Arc::ptr_eq(&epoch, &other_epoch));
    assert_eq!(control.placement, fixture.placement);
    assert_eq!(control.original_intent, 9);
    assert_eq!(control.original_control, 6);
    assert_eq!(control.original_request, 4);
    assert_eq!(control.next_intent, Some(10));
    assert_eq!(engine.input_runtime_ownership.placement_read_token(), token);
    assert_eq!(fixture.counters(), before_counters);
    assert_eq!(proposal_epochs(engine, fixture.placement), before_proposals);
    assert_eq!(committed_epochs(engine), before_committed);
    assert_eq!(*engine.pad_request_ids.lock().unwrap(), before_requests);
    assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
}

#[test]
fn stable_missing_source_and_invalid_epoch_remain_original_terminal_errors() {
    Python::initialize();
    let engine = AudioEngine::new().unwrap();
    let token = engine.input_runtime_ownership.placement_read_token();
    let missing = engine.stable_committed_placement_read(|| {
        input_runtime_binding::capture(&engine, 0)?
            .filter(|binding| binding.current() && binding.available())
            .ok_or_else(|| PyValueError::new_err("current source/timing unavailable"))
    });
    let Err(error) = missing else {
        panic!("an empty engine has no loaded source");
    };
    Python::attach(|py| assert!(error.is_instance_of::<PyValueError>(py)));
    assert!(
        error
            .to_string()
            .contains("current source/timing unavailable")
    );
    assert!(!error.to_string().contains(PENDING_CODE));

    // This actual resolver's historical message contains 'pending', but a
    // stable invalid index is terminal; the helper must not classify by text.
    let invalid = engine.stable_committed_placement_read(|| {
        engine
            .prepared_source_epoch(216)
            .map_err(PyRuntimeError::new_err)
    });
    let error = invalid.expect_err("invalid slot must remain an error");
    assert!(
        error
            .to_string()
            .contains("content placement is pending or unavailable")
    );
    assert!(!error.to_string().contains(PENDING_CODE));
    assert_eq!(engine.input_runtime_ownership.placement_read_token(), token);
    assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
}

#[test]
fn stable_stale_control_capture_remains_terminal_without_acquiring_authority() {
    let fixture = Fixture::new();
    let engine = &fixture.engine;
    let ownership = &engine.input_runtime_ownership;
    let replacement = crate::audio_engine::content_lifetime::reserve().unwrap();
    let write = ownership
        .prepare_slot_write_fixed(fixture.placement, replacement)
        .unwrap();
    ownership
        .commit_slot_write_fixed(&write, || true, |_| {})
        .unwrap();
    assert!(!ownership.slot_snapshot_current(fixture.placement));
    let token = ownership.placement_read_token();
    let before_counters = fixture.counters();
    let before_committed = committed_epochs(engine);

    let result = engine.stable_committed_placement_read(|| {
        CommittedControlWrite::capture(&fixture.binding, 9, Some(10))
            .map_err(PyRuntimeError::new_err)
    });

    let Err(error) = result else {
        panic!("the original content lifetime is stale");
    };
    assert!(
        error
            .to_string()
            .contains("committed control content placement changed or is pending")
    );
    assert!(!error.to_string().contains(PENDING_CODE));
    assert_eq!(ownership.placement_read_token(), token);
    assert_eq!(fixture.counters(), before_counters);
    assert_eq!(committed_epochs(engine), before_committed);
    assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
}

#[test]
fn stable_arbitrary_terminal_error_preserves_the_original_python_exception_object() {
    Python::initialize();
    let engine = AudioEngine::new().unwrap();
    for original in [
        PyValueError::new_err("committed source generation changed"),
        PyRuntimeError::new_err("resident ownership lock poisoned"),
        placement_admission_error(PlacementError::ObservationGenerationExhausted),
    ] {
        let expected = Python::attach(|py| original.clone_ref(py));
        let result: PyResult<()> = engine.stable_committed_placement_read(|| Err(original));
        let returned = result.expect_err("a stable terminal error must survive");
        Python::attach(|py| assert!(returned.value(py).is(expected.value(py))));
        assert!(!returned.to_string().contains(PENDING_CODE));
    }
    // Actual observation-counter exhaustion lives in ContentPlacement's existing
    // tests; this child has no access to that private counter and injects no hook.
    assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
}

#[test]
fn real_committed_request_defers_without_admission_then_retries_to_its_own_native_ack() {
    for (interest, mode) in [("explicit", "all_stems"), ("preload", "full_mix")] {
        let mut fixture = PairWorkerFixture::new();
        wait_until(|| fixture.source.fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
        let engine = &fixture.source.fixture.engine;
        assert_eq!(fixture.selection.publication_status(), "accepted");
        assert!(fixture.selection.is_current());
        assert!(fixture.source.fixture.binding.current());
        assert!(fixture.source.fixture.binding.available());
        assert!(fixture.consumer.is_empty());
        let own = engine.input_runtime_ownership.slot_snapshot(0).unwrap();
        let before_proposals = proposal_epochs(engine, own);
        let before_committed = committed_epochs(engine);
        let before_counters = fixture.source.fixture.counters();
        let before_requests = engine.pad_request_ids.lock().unwrap().clone();
        let before_source_epoch = engine
            .prepared_source_epoch(0)
            .unwrap()
            .load(Ordering::Acquire);
        let before_queue = fixture.producer.lock().unwrap().slots();
        let before_cold = engine.cold_jobs.counts_for_test();
        let held = PendingWriter::hold(engine.input_runtime_ownership.clone());

        pending_error(
            engine.request_committed_stem_residency_with_interest_and_producer(
                &fixture.selection,
                mode,
                STEM_COMPONENT_MASK,
                interest,
                &fixture.producer,
            ),
        );

        assert!(fixture.consumer.is_empty());
        assert_eq!(fixture.producer.lock().unwrap().slots(), before_queue);
        assert_eq!(proposal_epochs(engine, own), before_proposals);
        assert_eq!(committed_epochs(engine), before_committed);
        assert_eq!(fixture.source.fixture.counters(), before_counters);
        assert_eq!(*engine.pad_request_ids.lock().unwrap(), before_requests);
        assert_eq!(
            fixture.source.owner.source_epoch.load(Ordering::Acquire),
            before_source_epoch
        );
        assert_eq!(engine.cold_jobs.counts_for_test(), before_cold);
        assert!(fixture.selection.is_current());
        assert_eq!(fixture.selection.publication_status(), "accepted");
        held.complete();
        assert!(engine.input_runtime_ownership.slot_snapshot_current(own));

        let ticket = engine
            .request_committed_stem_residency_with_interest_and_producer(
                &fixture.selection,
                mode,
                STEM_COMPONENT_MASK,
                interest,
                &fixture.producer,
            )
            .unwrap();
        let capture = fixture.consumer.pop().unwrap();
        let ControlMessage::CommittedStemWork(work) = &capture else {
            panic!("retry must use the actual committed capture queue");
        };
        let CommittedStemRtWork::Capture(capture_owner) = work.as_ref() else {
            panic!("retry must capture its own current Native voice");
        };
        assert!(Arc::ptr_eq(&capture_owner.status, &ticket.status));
        fixture.dispatch(capture);
        wait_until(|| !fixture.consumer.is_empty());
        let adopt = fixture.consumer.pop().unwrap();
        let ControlMessage::CommittedStemWork(work) = &adopt else {
            panic!("real resident preparation must publish through the committed queue");
        };
        let CommittedStemRtWork::Adopt(transaction) = work.as_ref() else {
            panic!("real resident preparation must return its own adoption");
        };
        assert!(Arc::ptr_eq(&transaction.status, &ticket.status));
        assert_eq!(
            transaction.binding.placement.unwrap().content_key(),
            own.content_key()
        );
        fixture.dispatch(adopt);
        assert_eq!(ticket.residency_status(), "accepted");
        assert!(ticket.is_current());
        assert!(ticket.error().is_none());
        assert_eq!(fixture.selection.publication_status(), "accepted");
        resident_relocation::reconcile(&fixture.source.fixture.engine).unwrap();
        wait_until(|| fixture.source.fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    }
}
