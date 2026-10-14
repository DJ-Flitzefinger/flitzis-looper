//! Control timing publication shares the actual source/pair mutation marker.

use super::*;

#[test]
fn pending_mutation_rejects_all_timing_writers_without_partial_control_or_ack_changes() {
    let engine = super::tests::test_engine();
    let ownership = &engine.input_runtime_ownership;
    let original = ownership.slot_snapshot(0).unwrap();
    let key = original.content_key().unwrap();
    let epoch = engine.prepared_source_epoch(0).unwrap().clone();
    let before_epoch = epoch.load(Ordering::Acquire);
    let before_authority = ownership
        .authority_for_cell(original.cell())
        .load(Ordering::Acquire);
    assert!(engine.current_timing_acknowledgements.acknowledge_for_key(
        ownership,
        key,
        before_epoch,
    ));
    let (producer, mut consumer) = super::tests::queue(3);
    let (parameters, mut parameter_consumer) = rtrb::RingBuffer::new(1);
    let parameters = Arc::new(Mutex::new(parameters));
    let other = ownership.slot_snapshot(215).unwrap();
    let prepared = ownership
        .prepare_slot_write_fixed(other, other.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(
            &prepared,
            || true,
            |_| {
                assert!(set_intent(&engine, &producer, 0, TimingIntent::Manual).is_err());
                assert!(publish_legacy_bpm(&engine, &parameters, 0, Some(97.0)).is_err());
                assert!(publish_legacy_origin(&engine, &producer, 0, 0.125).is_err());
                // Exercise the race after capture/admission, rather than only early Pending capture.
                let mut intents = engine.timing_intents.lock().unwrap();
                assert!(
                    commit_timing_intent_under_request_lock(
                        &engine,
                        original,
                        &epoch,
                        before_epoch + 1,
                        &mut intents,
                        TimingIntent::Tap,
                    )
                    .is_err()
                );
                assert_eq!(intents[0], TimingIntent::Automatic);
                assert_eq!(epoch.load(Ordering::Acquire), before_epoch);
                assert_eq!(
                    ownership
                        .authority_for_cell(original.cell())
                        .load(Ordering::Acquire),
                    before_authority
                );
                assert_eq!(
                    engine
                        .current_timing_acknowledgements
                        .current_for_key(ownership, key),
                    before_epoch
                );
                assert!(consumer.pop().is_err());
                assert!(parameter_consumer.pop().is_err());
            },
        )
        .unwrap();
    let after = ownership.slot_snapshot(0).unwrap();
    assert_eq!(after.cell(), original.cell());
    assert_eq!(after.binding_epoch(), original.binding_epoch());
    assert_eq!(after.lifetime(), original.lifetime());
    assert_eq!(epoch.load(Ordering::Acquire), before_epoch);
    assert_eq!(
        engine.timing_intents.lock().unwrap()[0],
        TimingIntent::Automatic
    );
    assert_eq!(
        engine
            .current_timing_acknowledgements
            .current_for_key(ownership, key),
        before_epoch
    );
}

#[test]
fn successful_timing_writers_queue_the_complete_same_incarnation_snapshot() {
    for id in [0, 36] {
        for edit in 0..3 {
            let engine = super::tests::test_engine();
            let ownership = &engine.input_runtime_ownership;
            let original = ownership.slot_snapshot(id).unwrap();
            let epoch = engine.prepared_source_epoch(id).unwrap().clone();
            let before_epoch = epoch.load(Ordering::Acquire);
            let before_authority = ownership
                .authority_for_cell(original.cell())
                .load(Ordering::Acquire);
            if let Some(key) = original.content_key() {
                assert!(engine.current_timing_acknowledgements.acknowledge_for_key(
                    ownership,
                    key,
                    before_epoch
                ));
            }
            let (producer, mut consumer) = super::tests::queue(1);
            let (parameters, mut parameter_consumer) = rtrb::RingBuffer::new(1);
            let parameters = Arc::new(Mutex::new(parameters));
            let captured = match edit {
                0 => {
                    set_intent(&engine, &producer, id, TimingIntent::Tap).unwrap();
                    let ControlMessage::ClearPadConstantTiming {
                        placement,
                        through_epoch,
                        ..
                    } = consumer.pop().unwrap()
                    else {
                        panic!("wrong clear publication")
                    };
                    assert_eq!(through_epoch, before_epoch + 1);
                    placement.unwrap()
                }
                1 => {
                    publish_legacy_bpm(&engine, &parameters, id, Some(97.0)).unwrap();
                    let ControlParameterMessage::SetLegacyPadBpm {
                        placement,
                        through_epoch,
                        ..
                    } = parameter_consumer.pop().unwrap()
                    else {
                        panic!("wrong BPM publication")
                    };
                    assert_eq!(through_epoch, before_epoch + 1);
                    placement.unwrap()
                }
                _ => {
                    publish_legacy_origin(&engine, &producer, id, 0.125).unwrap();
                    let ControlMessage::SetLegacyPadTimingMetadata {
                        placement,
                        through_epoch,
                        ..
                    } = consumer.pop().unwrap()
                    else {
                        panic!("wrong origin publication")
                    };
                    assert_eq!(through_epoch, before_epoch + 1);
                    placement.unwrap()
                }
            };
            assert_eq!(captured, ownership.slot_snapshot(id).unwrap());
            assert_eq!(captured.cell(), original.cell());
            assert_eq!(captured.binding_epoch(), original.binding_epoch());
            assert_eq!(captured.lifetime(), original.lifetime());
            assert_eq!(epoch.load(Ordering::Acquire), before_epoch + 1);
            assert_eq!(
                ownership
                    .authority_for_cell(captured.cell())
                    .load(Ordering::Acquire),
                before_authority + 1
            );
            assert_eq!(
                engine.timing_intents.lock().unwrap()[id],
                if edit == 0 {
                    TimingIntent::Tap
                } else {
                    TimingIntent::Legacy
                }
            );
            assert_eq!(
                engine
                    .current_timing_acknowledgements
                    .current_for_slot(ownership, id),
                0
            );
            assert!(consumer.pop().is_err());
            assert!(parameter_consumer.pop().is_err());
        }
    }
}

#[test]
fn stale_control_capture_and_exhausted_authority_leave_accepted_timing_unchanged() {
    for invalidation in 0..3 {
        let engine = super::tests::test_engine();
        let ownership = &engine.input_runtime_ownership;
        let original = ownership.slot_snapshot(0).unwrap();
        let key = original.content_key().unwrap();
        let epoch = engine.prepared_source_epoch(0).unwrap().clone();
        let captured_epoch = epoch.load(Ordering::Acquire);
        assert!(engine.current_timing_acknowledgements.acknowledge_for_key(
            ownership,
            key,
            captured_epoch,
        ));
        match invalidation {
            0 => epoch.store(captured_epoch + 1, Ordering::Release),
            1 => {
                let pair = ownership
                    .prepare_placement_pair(original, ownership.slot_snapshot(36).unwrap())
                    .unwrap();
                ownership
                    .commit_placement_pair(&pair, true, || true, || {})
                    .unwrap();
            }
            _ => ownership
                .authority_for_cell(original.cell())
                .store(u64::MAX, Ordering::Release),
        }
        let expected_epoch = epoch.load(Ordering::Acquire);
        let expected_authority = ownership
            .authority_for_cell(original.cell())
            .load(Ordering::Acquire);
        let before = ownership.slot_snapshot(0).unwrap();
        let mut intents = engine.timing_intents.lock().unwrap();
        assert!(
            commit_timing_intent_under_request_lock(
                &engine,
                original,
                &epoch,
                captured_epoch + 1,
                &mut intents,
                TimingIntent::Manual,
            )
            .is_err()
        );
        assert_eq!(ownership.slot_snapshot(0).unwrap(), before);
        assert_eq!(intents[0], TimingIntent::Automatic);
        assert_eq!(epoch.load(Ordering::Acquire), expected_epoch);
        assert_eq!(
            ownership
                .authority_for_cell(original.cell())
                .load(Ordering::Acquire),
            expected_authority
        );
        assert_eq!(
            engine
                .current_timing_acknowledgements
                .current_for_key(ownership, key),
            captured_epoch
        );
    }
}
