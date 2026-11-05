//! Continue the actual held-window Copy under its own surviving assignment.
use super::window_cancel_restore::{restore_closed_source_fixture, workers_end};
use super::*;
use crate::audio_engine::native_owned_work::CompleteSampleWork;

fn verify_copy_saved_sources(sources: &[AssignmentWitness; 2]) {
    let rows = assignment_rows(sources);
    for source in sources {
        source.verify();
        assert!(source.pcm.iter().all(|reader| reader.strong_count() == 0));
        // Saved representatives may coalesce to the Copy's actual subscriber.
        // Preserve all real IDs in evidence; require a real unretired tuple.
        assert!(rows.iter().any(|row| {
            row["original"] == json!(source.original.path)
                && row["cache"] == json!(source.cache[0].path)
                && row["assignment_retired"] == false
        }));
    }
    eprintln!("copy SAME saved custody {}", json!({"claims": rows}));
}

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn finish_copied_source_fixture(
    mut p: Productive,
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
    let guarded = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            guarded.resident_start(),
            guarded.resident_end(),
            guarded.samples.len() / guarded.channels,
            guarded.frame_count(),
            guarded.channels
        ),
        (LOOP_START - 1, LOOP_END + 1, 1502, 4096, 2)
    );
    assert_eq!(
        guarded.samples.as_ref(),
        &full_pcm[(LOOP_START - 1) * 2..(LOOP_END + 1) * 2]
    );
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
        (LOOP_START, LOOP_END, 1500, 4096, 2)
    );
    let finite_pcm = finite.samples.to_vec();
    assert_eq!(finite_pcm, full_pcm[LOOP_START * 2..LOOP_END * 2]);
    let finite_reader = Arc::downgrade(&finite.samples);
    let copied = p.h.ticket(1);
    assert!(copied.is_current());
    assert_eq!(copied.source_version, p.h.version);
    let (source_assignment, copy_assignment) = {
        let leases = p.h.engine.cold_leases.lock().unwrap();
        let source = leases[0].as_ref().unwrap();
        let copy = leases[1].as_ref().unwrap();
        assert!(!source.assignment_retired());
        assert!(!copy.assignment_retired());
        assert!(current.assignments.contains(&source.assignment_id()));
        assert_eq!(copy.original_path, current.original.path);
        assert_eq!(copy.cache_path, current.cache[0].path);
        assert!(copy.original_identity_matches(&current.original.identity));
        assert_ne!(source.assignment_id(), copy.assignment_id());
        (source.assignment_id(), copy.assignment_id())
    };
    for source in &sources {
        source.verify();
    }
    // PAD1 is still wanted. Ordinary peer unload grants no shared-file retire.
    p.h.engine.unload_sample(0).unwrap();
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    reconcile(&p.h.engine).unwrap();
    assert!(p.h.callback.mixer.bank_for_measurement()[0].is_none());
    assert!(p.h.engine.sample_cache.lock().unwrap()[0].is_none());
    assert!(p.h.engine.cold_leases.lock().unwrap()[0].is_none());
    assert!(
        p.h.engine
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .is_none()
    );
    let after = p.h.ticket(1);
    assert!(copied.is_current() && after.is_current());
    assert!(copied.same_preparation_context(&after));
    assert_eq!(copied.source_version, after.source_version);
    // A peer publication advances the global layout revision, not this owner.
    assert!(
        copied
            .admission
            .binding
            .same_projection(&after.admission.binding)
    );
    assert_eq!(
        copied.admission.source_generation,
        after.admission.source_generation
    );
    {
        let leases = p.h.engine.cold_leases.lock().unwrap();
        let copy = leases[1].as_ref().unwrap();
        assert_eq!(copy.assignment_id(), copy_assignment);
        assert!(!copy.assignment_retired());
        assert_eq!(copy.original_path, current.original.path);
        assert_eq!(copy.cache_path, current.cache[0].path);
        assert!(copy.original_identity_matches(&current.original.identity));
    }
    // A saved file pin cannot stand in for the continuing Copy's own authority.
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
    let final_copy = p.h.ticket(1);
    assert!(final_copy.is_current() && copied.is_current());
    assert!(copied.same_preparation_context(&final_copy));
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
        (LOOP_START, LOOP_END, 1500, 4096, 2)
    );
    assert_eq!(finite.samples.as_ref(), finite_pcm);
    assert!(finite_reader.ptr_eq(&Arc::downgrade(&finite.samples)));
    {
        let leases = p.h.engine.cold_leases.lock().unwrap();
        let copy = leases[1].as_ref().unwrap();
        assert_eq!(copy.assignment_id(), copy_assignment);
        assert!(!copy.assignment_retired());
    }
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
        "copy PEER UNLOAD continuing owner {}",
        json!({
            "source_assignment_id": source_assignment, "copy_assignment_id": copy_assignment,
            "copy_current_binding": true, "source_version": final_copy.source_version,
            "source_generation": final_copy.admission.source_generation,
            "copy_unretired": true, "source0_guarded_frames": 1502,
            "copy_finite_range": [LOOP_START, LOOP_END], "copy_finite_frames": 1500,
            "post_peer_unload_complete_exact_frames": 4096,
            "sources": sources.iter().map(AssignmentWitness::evidence).collect::<Vec<_>>(), "workers": before
        })
    );
    drop((copied, after, final_copy));
    let closed = close_productive_readers(p, &sources);
    assert_eq!(observed.engine_owner.as_ref().unwrap().strong_count(), 0);
    assert!(
        observed
            .readers
            .iter()
            .all(|reader| reader.strong_count() == 0)
    );
    assert_eq!(finite_reader.strong_count(), 0);
    assert_eq!(complete_reader.strong_count(), 0);
    workers_end(
        observed.workers,
        6,
        "copied original engine and both mixer images",
    );
    verify_copy_saved_sources(&sources);
    restore_closed_source_fixture(
        closed,
        sources,
        full_pcm,
        finite_pcm,
        verify_copy_saved_sources,
    );
    eprintln!(
        "copy SAME source FINAL: continuing copied owner after peer unload, own complete4096 and finite1500 exact; original6/restored4 workers END, own claims0 and checked close"
    );
}
