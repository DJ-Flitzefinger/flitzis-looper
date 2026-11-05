//! Keep the actual moved window owner authoritative while its old slot is reused.
use super::window_cancel_restore::{restore_closed_source_fixture_with_storage, workers_end};
use super::window_copy_restore::verify_copy_saved_sources as verify_saved_sources;
use super::*;
use crate::audio_engine::native_owned_work::CompleteSampleWork;

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn finish_moved_source_fixture(
    mut p: Productive,
    mirror_while_held: bool,
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
    assert_eq!(
        (
            p.h.material.sample.frame_count(),
            p.h.material.sample.channels
        ),
        (4096, 2)
    );
    let moved = p.h.ticket(1);
    assert!(moved.is_current());
    assert_eq!(moved.source_version, p.h.version);
    let moved_key = moved
        .admission
        .binding
        .placement
        .unwrap()
        .content_key()
        .unwrap();
    let moved_history = p.h.engine.cold_pcm_history.lock().unwrap()[1]
        .identity
        .clone();
    let moved_assignment = {
        let leases = p.h.engine.cold_leases.lock().unwrap();
        let lease = leases[1].as_ref().unwrap();
        assert!(!lease.assignment_retired());
        assert!(current.assignments.contains(&lease.assignment_id()));
        assert_eq!(lease.original_path, current.original.path);
        assert_eq!(lease.cache_path, current.cache[0].path);
        assert!(lease.original_identity_matches(&current.original.identity));
        lease.assignment_id()
    };
    let finite_reader = Arc::downgrade(
        &p.h.callback.mixer.bank_for_measurement()[1]
            .as_ref()
            .unwrap()
            .samples,
    );
    let finite_pcm = full_pcm[(LOOP_START - 1) * 2..(LOOP_END + 1) * 2].to_vec();
    let verify_moved = |p: &Productive| {
        let after = p.h.ticket(1);
        assert!(moved.is_current() && after.is_current());
        assert!(moved.same_preparation_context(&after));
        assert_eq!(moved.source_version, after.source_version);
        assert_eq!(
            moved.admission.source_generation,
            after.admission.source_generation
        );
        assert!(
            moved
                .admission
                .binding
                .same_projection(&after.admission.binding)
        );
        assert_eq!(
            after.admission.binding.placement.unwrap().content_key(),
            Some(moved_key)
        );
        assert!(
            p.h.engine
                .input_runtime_ownership
                .content_key_current(moved_key)
        );
        let history = p.h.engine.cold_pcm_history.lock().unwrap();
        assert!(Arc::ptr_eq(&history[1].identity, &moved_history));
        assert!(
            history[1]
                .iter()
                .any(|reader| reader.ptr_eq(&finite_reader))
        );
        drop(history);
        let finite = p.h.callback.mixer.bank_for_measurement()[1]
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                finite.resident_start(),
                finite.resident_end(),
                finite.samples.len() / finite.channels,
                finite.frame_count(),
                finite.channels
            ),
            (LOOP_START - 1, LOOP_END + 1, 1502, 4096, 2)
        );
        assert_eq!(finite.samples.as_ref(), finite_pcm);
        assert!(finite_reader.ptr_eq(&Arc::downgrade(&finite.samples)));
        let leases = p.h.engine.cold_leases.lock().unwrap();
        let lease = leases[1].as_ref().unwrap();
        assert_eq!(lease.assignment_id(), moved_assignment);
        assert!(!lease.assignment_retired());
        assert_eq!(lease.original_path, current.original.path);
        assert_eq!(lease.cache_path, current.cache[0].path);
        assert!(lease.original_identity_matches(&current.original.identity));
        drop(leases);
        for source in &sources {
            source.verify();
        }
    };
    verify_moved(&p);
    assert!(p.h.callback.mixer.bank_for_measurement()[0].is_none());
    assert!(p.h.engine.sample_cache.lock().unwrap()[0].is_none());
    assert!(p.h.engine.cold_leases.lock().unwrap()[0].is_none());
    assert!(
        p.h.engine
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .is_none()
    );

    // Equal source bytes cannot authorize this new occupant of the vacated slot.
    let fresh = prepare_material(&p.h.root, &current.original.path, RATE, 2, &|| false).unwrap();
    assert!(fresh.lease.integrity.warm);
    assert_eq!(fresh.lease.original_path, current.original.path);
    assert_eq!(fresh.lease.cache_path, current.cache[0].path);
    assert_eq!(fresh.sample.samples.as_ref(), full_pcm);
    let fresh_reader = Arc::downgrade(&fresh.sample.samples);
    let preparation = control::prepared_for_test(&p.h.engine, fresh, &p.h.root).unwrap();
    let request = control::adopt_for_format(
        &p.h.engine,
        0,
        &preparation,
        p.h.producer.clone(),
        (2, RATE, p.h.root.clone()),
    )
    .unwrap();
    wait_until(|| p.h.consumer.peek().is_ok() && p.h.producer.try_lock().is_ok());
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    source_successes(&p.h.engine, &[request.request_id()]);
    assert!(request.is_current().unwrap());
    let replacement_request_id = request.request_id();
    let replacement = p.h.ticket(0);
    assert!(replacement.is_current());
    assert_eq!(replacement.source_version, moved.source_version);
    let replacement_key = replacement
        .admission
        .binding
        .placement
        .unwrap()
        .content_key()
        .unwrap();
    assert_ne!(replacement_key, moved_key);
    assert_ne!(replacement_key.lifetime(), moved_key.lifetime());
    let replacement_assignment = {
        let leases = p.h.engine.cold_leases.lock().unwrap();
        let lease = leases[0].as_ref().unwrap();
        assert!(!lease.assignment_retired());
        assert_ne!(lease.assignment_id(), moved_assignment);
        assert!(!current.assignments.contains(&lease.assignment_id()));
        assert_eq!(lease.original_path, current.original.path);
        assert_eq!(lease.cache_path, current.cache[0].path);
        assert!(lease.original_identity_matches(&current.original.identity));
        lease.assignment_id()
    };
    let admitted = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            admitted.resident_start(),
            admitted.resident_end(),
            admitted.frame_count(),
            admitted.samples.len() / admitted.channels,
            admitted.channels
        ),
        (0, 4096, 4096, 4096, 2)
    );
    assert_eq!(admitted.samples.as_ref(), full_pcm);
    verify_moved(&p);
    preparation.release_preparation().unwrap();
    drop((preparation, request));

    // Exercise this moved owner's own complete-source authority after old-slot reuse.
    let complete_reader = {
        let work = {
            let cache = p.h.engine.sample_cache.lock().unwrap();
            CompleteSampleWork::capture(&p.h.engine, 1, cache[1].as_ref().unwrap()).unwrap()
        };
        let complete = work
            .materialize(crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES)
            .unwrap();
        assert_eq!(
            (
                complete.resident_start(),
                complete.resident_end(),
                complete.frame_count(),
                complete.samples.len() / complete.channels,
                complete.channels
            ),
            (0, 4096, 4096, 4096, 2)
        );
        assert_eq!(complete.samples.as_ref(), full_pcm);
        Arc::downgrade(&complete.samples)
    };
    assert_eq!(complete_reader.strong_count(), 0);
    verify_moved(&p);
    p.h.engine.unload_sample(0).unwrap();
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    reconcile(&p.h.engine).unwrap();
    assert!(!replacement.is_current());
    assert!(p.h.callback.mixer.bank_for_measurement()[0].is_none());
    assert!(p.h.engine.sample_cache.lock().unwrap()[0].is_none());
    assert!(p.h.engine.cold_leases.lock().unwrap()[0].is_none());
    assert!(
        p.h.engine
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .is_none()
    );
    verify_moved(&p);
    eprintln!(
        "move VACATED SLOT REUSE {}",
        json!({"mirror_while_held":mirror_while_held,"moved_assignment_id":moved_assignment,"replacement_assignment_id":replacement_assignment,"replacement_source_request_id":replacement_request_id,"replacement_source_own_ack_current_before_unload":true,"moved_lifetime":moved_key.lifetime(),"replacement_lifetime":replacement_key.lifetime(),"moved_binding":format!("{:?}",moved.admission.binding),"source_version":moved.source_version,"source_generation":moved.admission.source_generation,"moved_current_unretired_history_unchanged":true,"post_reuse_complete_exact_frames":4096,"moved_finite_range":[LOOP_START-1,LOOP_END+1],"moved_finite_frames":1502,"sources":sources.iter().map(AssignmentWitness::evidence).collect::<Vec<_>>()})
    );
    drop((moved, replacement, moved_history));
    let before: Vec<_> = observed
        .workers
        .iter()
        .map(WorkerWitness::observation)
        .collect();
    assert!(before.iter().all(|worker| worker["wait_result"] == 258));
    let closed = close_productive_readers(p, &sources);
    assert_eq!(observed.engine_owner.as_ref().unwrap().strong_count(), 0);
    assert!(
        observed
            .readers
            .iter()
            .all(|reader| reader.strong_count() == 0)
    );
    assert_eq!(finite_reader.strong_count(), 0);
    assert_eq!(fresh_reader.strong_count(), 0);
    assert_eq!(complete_reader.strong_count(), 0);
    workers_end(
        observed.workers,
        6,
        "moved original engine and both mixer images",
    );
    verify_saved_sources(&sources);
    restore_closed_source_fixture_with_storage(
        closed,
        sources,
        full_pcm,
        finite_pcm,
        verify_saved_sources,
        Some((LOOP_START - 1, LOOP_END + 1)),
    );
    eprintln!(
        "move SAME source FINAL: mirror_while_held={mirror_while_held}, current moved owner after vacated-slot reuse/unload, own complete4096 and guarded1502 exact; original6/restored4 workers END, own claims0 and checked close"
    );
}
