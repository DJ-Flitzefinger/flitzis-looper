//! Control epoch routing through the real placement owner, without claiming a live Mixer remap.

use super::*;

#[test]
fn preparation_accepts_same_source_binding_after_unrelated_publication_revision() {
    let engine = super::tests::test_engine();
    let binding = engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap();
    let ownership = &engine.input_runtime_ownership;
    let other = ownership.slot_snapshot(215).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(other, other.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(&write, || true, |_| {})
        .unwrap();
    let fresh = engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap();
    assert!(binding.is_current());
    assert_ne!(binding.binding, fresh.binding);
    assert!(binding.binding.same_projection(&fresh.binding));
    let captured = capture_preparation(
        &engine,
        0,
        TimingBound {
            halfwidth_seconds: 0.001,
            provenance: "unrelated publication regression".into(),
        },
        Some(&binding),
    )
    .unwrap();
    assert_eq!(
        captured.placement.unwrap().content_key(),
        binding.binding.placement.unwrap().content_key()
    );
    assert_eq!(captured.source_generation, binding.source_generation);
}

fn move_authority(engine: &AudioEngine) {
    let ownership = &engine.input_runtime_ownership;
    let prepared = ownership
        .prepare_placement_pair(
            ownership.slot_snapshot(0).unwrap(),
            ownership.slot_snapshot(36).unwrap(),
        )
        .unwrap();
    ownership
        .commit_placement_pair(&prepared, true, || true, || {})
        .unwrap();
}

#[test]
fn moved_content_keeps_its_epoch_arc_and_vacated_slot_mutates_only_the_empty_cell() {
    let engine = super::tests::test_engine();
    let original = engine.prepared_source_epoch(0).unwrap().clone();
    let empty = engine.prepared_source_epoch(36).unwrap().clone();
    let unrelated = engine.prepared_source_epoch(215).unwrap().clone();
    let unrelated_before = unrelated.load(Ordering::Acquire);
    original.store(7, Ordering::Release);
    empty.store(19, Ordering::Release);
    move_authority(&engine);

    assert!(Arc::ptr_eq(
        engine.prepared_source_epoch(36).unwrap(),
        &original
    ));
    assert!(Arc::ptr_eq(
        engine.prepared_source_epoch(0).unwrap(),
        &empty
    ));
    assert!(Arc::ptr_eq(
        engine.prepared_source_epoch(215).unwrap(),
        &unrelated
    ));
    assert_eq!(
        next_epoch(engine.prepared_source_epoch(36).unwrap()).unwrap(),
        8
    );
    let next_empty = next_epoch(engine.prepared_source_epoch(0).unwrap()).unwrap();
    assert_eq!(next_empty, 20);
    // Reusing the vacated physical slot cannot mutate the moved content's owner.
    engine
        .prepared_source_epoch(0)
        .unwrap()
        .store(next_empty, Ordering::Release);
    assert_eq!(original.load(Ordering::Acquire), 7);
    assert_eq!(unrelated.load(Ordering::Acquire), unrelated_before);
}

#[test]
fn unadmitted_timing_capture_does_not_follow_a_moved_content_into_new_slot_admission() {
    let engine = super::tests::test_engine();
    let captured = capture_preparation(
        &engine,
        0,
        TimingBound {
            halfwidth_seconds: 0.001,
            provenance: "explicit epoch routing fixture bound".into(),
        },
        None,
    )
    .unwrap();
    let original = captured.epoch.clone();
    let key = captured.placement.unwrap().content_key().unwrap();
    move_authority(&engine);
    assert!(engine.input_runtime_ownership.content_key_current(key));
    assert!(Arc::ptr_eq(
        engine.prepared_source_epoch(36).unwrap(),
        &original
    ));
    assert!(ConstantTimingPreparationWork::capture_existing(&engine, &captured).is_err());
    assert_eq!(original.load(Ordering::Acquire), captured.captured_epoch);
}

#[test]
fn identical_source_and_ack_scalars_cannot_promote_another_contents_registry_record() {
    let engine = super::tests::test_engine();
    let ticket = super::tests::synthetic_ticket(&engine);
    let (producer, mut consumer) = super::tests::queue(1);
    publish(
        &engine,
        &producer,
        &ticket,
        &super::tests::hypotheses(),
        super::tests::origin(),
        super::tests::decision(),
    )
    .unwrap();
    let mut mixer = super::tests::acknowledged_mixer(&engine);
    assert!(super::tests::accept_message(
        &mut mixer,
        consumer.pop().unwrap()
    ));
    let record = engine.current_constant_timing.lock().unwrap()[0][0].clone();
    let source = engine.sample_cache.lock().unwrap()[0].clone().unwrap();

    // A separate actual Native incarnation deliberately shares every PCM/generation scalar.
    let owner = &engine.input_runtime_ownership;
    owner.publish_test_source(36, &source, super::tests::RATE, 7);
    owner.set_timing_intent(36, TimingIntent::Automatic);
    engine.sample_cache.lock().unwrap()[36] = Some(source.clone());
    engine.loaded_source_generations.lock().unwrap()[36] = (7, super::tests::RATE);
    engine.loaded_source_digests.lock().unwrap()[36] = Some("a".repeat(64));
    engine.timing_intents.lock().unwrap()[36] = TimingIntent::Automatic;
    let key = owner.slot_snapshot(36).unwrap().content_key().unwrap();
    assert_ne!(Some(key), record.content_key);
    let epoch = engine.prepared_source_epoch(36).unwrap().clone();
    let publication = PreparedSourcePermit::new(epoch.clone(), record.publication_epoch);
    publication.mark_pending().unwrap();
    epoch.store(record.publication_epoch, Ordering::Release);
    mixer.load_sample(36, source.clone());
    assert!(mixer.publish_constant_timing_rt(
        36,
        PreparedConstantTiming {
            content_key: Some(key),
            reference: source.clone(),
            publication,
            projection: record.projection,
        },
        &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement,
    ));
    assert_eq!(
        engine
            .current_timing_acknowledgements
            .current_for_slot(owner, 36),
        record.publication_epoch,
    );
    // Only A's genuine record is present; B's own audio ACK cannot certify that record.
    engine.current_constant_timing.lock().unwrap()[36].push(record.clone());
    Python::attach(|py| assert!(current_metadata(&engine, py, 36).unwrap().is_none()));

    // During the mirror gap A remains retained by its own key, even at the old numeric slot.
    move_authority(&engine);
    let next = next_epoch(&epoch).unwrap();
    let replacement = PreparedSourcePermit::new(epoch.clone(), next);
    replacement.mark_pending().unwrap();
    epoch.store(next, Ordering::Release);
    assert!(mixer.publish_constant_timing_rt(
        36,
        PreparedConstantTiming {
            content_key: Some(key),
            reference: source,
            publication: replacement,
            projection: AcceptedTimingProjection {
                publication_epoch: next,
                ..record.projection
            },
        },
        &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement,
    ));
    assert_ne!(
        engine
            .current_timing_acknowledgements
            .current_for_slot(owner, 0),
        record.publication_epoch
    );
    Python::attach(|py| {
        assert!(current_metadata(&engine, py, 0).unwrap().is_none());
        assert_eq!(engine.current_constant_timing.lock().unwrap()[0].len(), 1);
        let metadata = current_metadata(&engine, py, 36).unwrap().unwrap();
        let dict = metadata.bind(py).cast::<PyDict>().unwrap();
        assert_eq!(
            dict.get_item("pad_id")
                .unwrap()
                .unwrap()
                .extract::<usize>()
                .unwrap(),
            36
        );
        assert_eq!(
            dict.get_item("source_id")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "loaded-36-7"
        );
    });
    assert_eq!(record.binding.job.pad_id, 0);
    assert_eq!(record.binding.job.source_id, "loaded-0-7");
    assert!(Arc::ptr_eq(
        &engine.current_constant_timing.lock().unwrap()[0][0].accepted,
        &record.accepted,
    ));
}
