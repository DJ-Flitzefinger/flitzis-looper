//! Continue the original cancelled reader with its SAME saved source, not a proxy.
use super::*;

fn verify_saved_sources(sources: &[AssignmentWitness; 2]) {
    for source in sources {
        source.verify();
    }
    let rows = assignment_rows(sources);
    for source in sources {
        assert!(source.pcm.iter().all(|reader| reader.strong_count() == 0));
        assert!(rows.iter().any(|row| {
            source
                .assignments
                .iter()
                .any(|id| row["assignment_id"] == *id)
                && row["original"] == json!(source.original.path)
                && row["cache"] == json!(source.cache[0].path)
                && row["assignment_retired"] == false
        }));
    }
    eprintln!(
        "cancel SAME source saved custody {}",
        json!({"claims": rows})
    );
}

pub(super) fn workers_end(workers: Vec<WorkerWitness>, expected: usize, phase: &str) {
    assert_eq!(workers.len(), expected);
    assert_eq!(
        workers
            .iter()
            .map(|worker| (worker.id, worker.birth))
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        expected
    );
    let observations: Vec<_> = workers.iter().map(WorkerWitness::observation).collect();
    eprintln!(
        "cancel SAME source worker END {}",
        json!({
            "phase": phase, "workers": observations,
            "nested_worker_join_panic_results": "UNKNOWN: nested Drop discards results"
        })
    );
    assert!(observations.iter().all(|worker| worker["wait_result"] == 0));
    for worker in workers {
        worker.close();
    }
}

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn finish_cancelled_source_fixture(
    p: Productive,
) {
    let mut observed = OBSERVATION.with_borrow_mut(|slot| slot.take().unwrap());
    assert_eq!(observed.sources.len(), 2);
    let sources: [AssignmentWitness; 2] = std::mem::take(&mut observed.sources)
        .try_into()
        .unwrap_or_else(|_| panic!("both real construction generations"));
    assert_ne!(sources[0].original.path, sources[1].original.path);
    let current = &sources[1];
    assert_eq!(current.original.path, p.h.material.lease.original_path);
    assert_eq!(current.cache[0].path, p.h.material.lease.cache_path);
    let full_pcm = p.h.material.sample.samples.to_vec();
    let full_frames = p.h.material.sample.frame_count();
    let channels = p.h.material.sample.channels;
    assert_eq!((full_frames, channels), (4096, 2));
    let finite = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    // frame_count() is the complete source, independent of resident PCM length.
    assert_eq!(finite.frame_count(), full_frames);
    assert_eq!(
        (
            finite.resident_start(),
            finite.resident_end(),
            finite.samples.len() / finite.channels
        ),
        (LOOP_START, LOOP_END, LOOP_END - LOOP_START)
    );
    assert_eq!(finite.channels, channels);
    let finite_pcm = finite.samples.to_vec();
    assert_eq!(
        finite_pcm,
        full_pcm[LOOP_START * channels..LOOP_END * channels]
    );
    for source in &sources {
        source.verify();
    }
    let before: Vec<_> = observed
        .workers
        .iter()
        .map(WorkerWitness::observation)
        .collect();
    assert!(before.iter().all(|worker| worker["wait_result"] == 258));
    eprintln!(
        "cancel SAME source BEFORE SHUTDOWN {}",
        json!({
            "sources": sources.iter().map(AssignmentWitness::evidence).collect::<Vec<_>>(),
            "workers": before, "full_frames": full_frames,
            "finite_start": LOOP_START, "finite_end": LOOP_END, "finite_frames": 1500
        })
    );
    let closed = close_productive_readers(p, &sources);
    assert_eq!(observed.engine_owner.as_ref().unwrap().strong_count(), 0);
    assert!(
        observed
            .readers
            .iter()
            .all(|reader| reader.strong_count() == 0)
    );
    workers_end(observed.workers, 6, "original engine and both mixer images");
    verify_saved_sources(&sources);

    restore_closed_source_fixture(closed, sources, full_pcm, finite_pcm, verify_saved_sources);
}

// The warm Source/Window acceptance is identical after either original leaf's
// own readers end. Their different live-bank oracles stay in their wrappers.
pub(super) fn restore_closed_source_fixture(
    closed: ClosedFixture,
    sources: [AssignmentWitness; 2],
    full_pcm: Vec<f32>,
    finite_pcm: Vec<f32>,
    verify_saved: fn(&[AssignmentWitness; 2]),
) {
    let current = &sources[1];
    let (full_frames, channels) = (4096, 2);
    assert_eq!(full_pcm.len(), full_frames * channels);
    assert_eq!(
        finite_pcm,
        full_pcm[LOOP_START * channels..LOOP_END * channels]
    );
    verify_saved(&sources);
    let restored =
        prepare_material(&closed.root, &current.original.path, RATE, 2, &|| false).unwrap();
    assert!(restored.lease.integrity.warm);
    assert_eq!(restored.lease.original_path, current.original.path);
    assert_eq!(restored.lease.cache_path, current.cache[0].path);
    assert_eq!(
        (restored.sample.frame_count(), restored.sample.channels),
        (full_frames, channels)
    );
    assert_eq!(restored.sample.samples.as_ref(), full_pcm);
    let restored_pcm = Arc::downgrade(&restored.sample.samples);
    // The fresh consumer uses the existing in-memory previous-source fixture,
    // then adopts the SAME saved files through the ordinary source path.
    let engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    let previous_pcm = Arc::downgrade(&previous.samples);
    let mut callback = Box::new(Callback::new(&engine, &previous));
    drop(previous);
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    let mut engine = crate::audio_engine::constant_timing::with_control_producer(engine, &producer);
    let workers: Vec<_> = engine
        .cold_jobs
        .thread_handles_for_test()
        .into_iter()
        .chain(callback.mixer.thread_handles_for_test())
        .map(WorkerWitness::capture)
        .collect();
    let restore_before: Vec<_> = workers.iter().map(WorkerWitness::observation).collect();
    assert!(
        restore_before
            .iter()
            .all(|worker| worker["wait_result"] == 258)
    );
    let engine_owner = Arc::downgrade(&engine.input_runtime_ownership);
    let preparation = control::prepared_for_test(&engine, restored, &closed.root).unwrap();
    let request = control::adopt_for_format(
        &engine,
        0,
        &preparation,
        producer.clone(),
        (2, RATE, closed.root.clone()),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok() && producer.try_lock().is_ok());
    assert_eq!(callback.drain(&mut consumer), 1);
    source_successes(&engine, &[request.request_id()]);
    assert!(request.is_current().unwrap());
    let admitted = callback.mixer.bank_for_measurement()[0].as_ref().unwrap();
    assert_eq!(
        (
            admitted.resident_start(),
            admitted.resident_end(),
            admitted.frame_count(),
            admitted.channels
        ),
        (0, full_frames, full_frames, channels)
    );
    assert_eq!(admitted.samples.as_ref(), full_pcm);
    let admitted_pcm = Arc::downgrade(&admitted.samples);
    let fresh_assignment = {
        let leases = engine.cold_leases.lock().unwrap();
        let lease = leases[0].as_ref().unwrap();
        assert_eq!(lease.original_path, current.original.path);
        assert_eq!(lease.cache_path, current.cache[0].path);
        assert!(lease.original_identity_matches(&current.original.identity));
        assert!(!current.assignments.contains(&lease.assignment_id()));
        lease.assignment_id()
    };
    verify_saved(&sources);
    preparation.release_preparation().unwrap();
    drop((preparation, request));

    // The original finite bank has no fresh_window guard band. Request its
    // exact original range and require this new window's own current ACK.
    let window = prepare_window_with_producer(
        &engine,
        0,
        WindowRequest {
            key_lock: Some(false),
            loop_region: Some((
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
            )),
            ..WindowRequest::default()
        },
        producer.clone(),
    )
    .unwrap();
    wait_until(|| {
        (consumer.peek().is_ok() || window.error().unwrap().is_some())
            && producer.try_lock().is_ok()
    });
    assert_eq!(
        window.publication_status(),
        "pending",
        "{:?}",
        window.error()
    );
    assert_eq!(callback.drain(&mut consumer), 1);
    assert_eq!(window.publication_status(), "accepted");
    assert!(window.is_current());
    reconcile(&engine).unwrap();
    let finite = callback.mixer.bank_for_measurement()[0].as_ref().unwrap();
    // frame_count() is the complete source, independent of resident PCM length.
    assert_eq!(finite.frame_count(), full_frames);
    assert_eq!(
        (
            finite.resident_start(),
            finite.resident_end(),
            finite.samples.len() / finite.channels,
            finite.channels
        ),
        (LOOP_START, LOOP_END, LOOP_END - LOOP_START, channels)
    );
    assert_eq!(finite.samples.as_ref(), finite_pcm);
    let finite_reader = Arc::downgrade(&finite.samples);
    verify_saved(&sources);
    eprintln!(
        "SAME saved source RESTORED {}",
        json!({
            "source_request_id": engine.pad_request_ids.lock().unwrap()[0],
            "fresh_assignment_id": fresh_assignment, "source_own_ack_current": true,
            "window_own_ack_current": window.is_current(), "full_pcm_exact_frames": full_frames,
            "finite_pcm_exact_frames": LOOP_END - LOOP_START,
            "finite_range": [LOOP_START, LOOP_END], "workers_before": restore_before
        })
    );
    drop(window);
    engine.shut_down().unwrap();
    assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
    drop((producer, consumer, engine));
    let (owner, worker) = create_audio_buffer_retirement_with_callback(|actual| {
        callback.retirement.actual = Some(actual);
        callback
    })
    .unwrap();
    close_callback_and_join(owner, worker).unwrap();
    assert_eq!(engine_owner.strong_count(), 0);
    wait_phase("same source restored full and finite PCM END", 0, || {
        [&previous_pcm, &restored_pcm, &admitted_pcm, &finite_reader]
            .iter()
            .all(|pcm| pcm.strong_count() == 0)
    });
    workers_end(workers, 4, "restored engine and callback");
    verify_saved(&sources);
    // Only now has this saved source reached its explicitly declared OWN end.
    retire_closed_fixture(closed, &sources, None, None);
    wait_phase(
        "cancel same source exact own assignment claims END",
        0,
        || assignment_rows(&sources).is_empty(),
    );
    eprintln!(
        "SAME saved source FINAL: full4096/finite1500 exact, own current ACKs; original6/restored4 workers END, own claims0 and checked close"
    );
}
