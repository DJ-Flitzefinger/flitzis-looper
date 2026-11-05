//! Follow a real window ACK and successful SAME-selection retry with its source.
use super::window_cancel_restore::{
    restore_closed_source_fixture_with_storage, verify_saved_sources, workers_end,
};
use super::*;

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn capture_window_ack_source(
    p: &Productive,
) -> impl Fn(&Productive) + use<> {
    let source = p.h.ticket(0);
    assert!(source.is_current());
    let key = source
        .admission
        .binding
        .placement
        .unwrap()
        .content_key()
        .unwrap();
    let history = p.h.engine.cold_pcm_history.lock().unwrap()[0]
        .identity
        .clone();
    let files = AssignmentWitness::capture(&p.h);
    let pair = &p.pair;
    let reader = Arc::downgrade(&pair.pair_reader);
    let descriptor = pair.pair_reader.descriptor.clone();
    let root = ObjectWitness::capture(&pair.root);
    let reference = pair.descriptor_reference.clone();
    let assignment = pair.source_lease.assignment_id();
    let descriptor_path =
        crate::audio_engine::material_paths::resolve(&pair.root, Path::new(&reference))
            .unwrap()
            .path;
    let pcm_path = crate::audio_engine::material_paths::resolve(
        &pair.root,
        Path::new(&descriptor.pcm_generation),
    )
    .unwrap()
    .path;
    let pair_files = [tree(&descriptor_path), tree(&pcm_path)];
    let verify = move |p: &Productive| {
        let after = p.h.ticket(0);
        // Source lineage survives the intentionally replaced preparation/window.
        assert!(source.same_source_request(&after));
        assert!(after.is_current());
        assert_eq!(after.source_version, p.h.version);
        assert_eq!(
            after.admission.source_generation,
            source.admission.source_generation
        );
        assert_eq!(
            after.admission.binding.placement.unwrap().content_key(),
            Some(key)
        );
        assert!(p.h.engine.input_runtime_ownership.content_key_current(key));
        assert!(Arc::ptr_eq(
            &p.h.engine.cold_pcm_history.lock().unwrap()[0].identity,
            &history
        ));
        assert!(p.selected.is_current());
        assert_eq!(p.selected.publication_status(), "accepted");
        let prepared = &p.pair;
        let selected = p.selected.pair_for_test();
        assert!(reader.ptr_eq(&Arc::downgrade(&prepared.pair_reader)));
        assert!(reader.ptr_eq(&Arc::downgrade(&selected.reader)));
        assert_eq!(prepared.source_version, p.h.version);
        assert_eq!(prepared.pair_reader.descriptor, descriptor);
        assert_eq!(selected.reader.descriptor, descriptor);
        assert_eq!(prepared.root, root.path);
        assert_eq!(selected.root, root.path);
        assert_eq!(prepared.descriptor_reference, reference);
        assert_eq!(selected.descriptor_reference, reference);
        assert_eq!(prepared.pair_reader.descriptor_reference, reference);
        assert_eq!(selected.reader.descriptor_reference, reference);
        let leases = p.h.engine.cold_leases.lock().unwrap();
        let current = leases[0].as_ref().unwrap();
        for lease in [&prepared.source_lease, &selected.source_lease, current] {
            assert_eq!(
                capture_identity(lease.samples_root()).unwrap().unwrap(),
                root.identity
            );
            assert_eq!(lease.assignment_id(), assignment);
            assert!(!lease.assignment_retired());
            assert_eq!(lease.original_path, files.original.path);
            assert_eq!(lease.cache_path, files.cache[0].path);
            assert!(lease.original_identity_matches(&files.original.identity));
        }
        drop(leases);
        // The accepted pair is checked when the real retry publishes components.
        let owners = p.h.engine.resident_stem_cache.lock().unwrap();
        if let Some(accepted) = owners[0].accepted.as_ref() {
            let pair = accepted.pair.as_ref().unwrap();
            assert!(reader.ptr_eq(&Arc::downgrade(&pair.reader)));
            assert_eq!(pair.reader.descriptor, descriptor);
            assert_eq!(pair.root, root.path);
            assert_eq!(pair.descriptor_reference, reference);
            assert_eq!(pair.reader.descriptor_reference, reference);
            assert_eq!(
                capture_identity(pair.source_lease.samples_root())
                    .unwrap()
                    .unwrap(),
                root.identity
            );
            assert_eq!(pair.source_lease.assignment_id(), assignment);
            assert!(!pair.source_lease.assignment_retired());
            assert_eq!(pair.source_lease.original_path, files.original.path);
            assert_eq!(pair.source_lease.cache_path, files.cache[0].path);
            assert!(
                pair.source_lease
                    .original_identity_matches(&files.original.identity)
            );
        }
        drop(owners);
        root.verify();
        files.verify();
        for files in &pair_files {
            assert_eq!(tree(&files[0].path), *files);
        }
    };
    verify(p);
    verify
}

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn capture_window_ack_currentness<
    F: Fn(&Productive),
>(
    p: &Productive,
    verify_source: F,
) -> impl Fn(&Productive) + use<F> {
    verify_source(p);
    let source = p.h.ticket(0);
    assert!(source.is_current());
    let finite = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    let finite_reader = Arc::downgrade(&finite.samples);
    let finite_pcm = finite.samples.to_vec();
    let resident = finite.resident_binding();
    let set = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    let set_identity = Arc::downgrade(&set.complete_set_identity);
    let reference = Arc::downgrade(&set.reference_samples);
    let components = set
        .stems
        .each_ref()
        .map(|stem| (Arc::downgrade(&stem.samples), stem.samples.to_vec()));
    let verify = move |p: &Productive| {
        verify_source(p);
        let after = p.h.ticket(0);
        assert!(source.is_current() && after.is_current());
        assert!(source.same_preparation_context(&after));
        assert!(
            source
                .admission
                .binding
                .same_projection(&after.admission.binding)
        );
        let finite = p.h.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                finite.resident_start(),
                finite.resident_end(),
                finite.frame_count(),
                finite.samples.len() / finite.channels,
                finite.channels
            ),
            (LOOP_START - 1, LOOP_END + 1, 4096, 1502, 2)
        );
        assert_eq!(finite.resident_binding(), resident);
        assert!(finite_reader.ptr_eq(&Arc::downgrade(&finite.samples)));
        assert_eq!(finite.samples.as_ref(), finite_pcm);
        assert_eq!(
            finite_pcm,
            p.h.material.sample.samples[(LOOP_START - 1) * 2..(LOOP_END + 1) * 2]
        );
        assert!(
            p.h.engine.cold_pcm_history.lock().unwrap()[0]
                .iter()
                .any(|pcm| pcm.ptr_eq(&finite_reader))
        );
        let owners = p.h.engine.resident_stem_cache.lock().unwrap();
        let accepted = owners[0].accepted.as_ref().unwrap();
        assert!(accepted.pair.is_some());
        assert_eq!(accepted.source_version, p.h.version);
        assert!(set_identity.ptr_eq(&Arc::downgrade(&accepted.set.complete_set_identity)));
        assert!(reference.ptr_eq(&Arc::downgrade(&accepted.set.reference_samples)));
        for ((reader, pcm), stem) in components.iter().zip(&accepted.set.stems) {
            assert!(reader.ptr_eq(&Arc::downgrade(&stem.samples)));
            assert_eq!(stem.samples.as_ref(), pcm);
        }
        drop(owners);
        let set = p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(set_identity.ptr_eq(&Arc::downgrade(&set.complete_set_identity)));
        assert!(reference.ptr_eq(&Arc::downgrade(&set.reference_samples)));
        for ((reader, pcm), stem) in components.iter().zip(&set.stems) {
            assert!(reader.ptr_eq(&Arc::downgrade(&stem.samples)));
            assert_eq!(stem.samples.as_ref(), pcm);
        }
    };
    verify(p);
    verify
}

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn finish_window_ack_source_fixture(
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
    assert_eq!(
        (
            p.h.material.sample.frame_count(),
            p.h.material.sample.channels
        ),
        (4096, 2)
    );
    let finite = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            finite.resident_start(),
            finite.resident_end(),
            finite.frame_count(),
            finite.samples.len() / finite.channels,
            finite.channels
        ),
        (LOOP_START - 1, LOOP_END + 1, 4096, 1502, 2)
    );
    let finite_pcm = finite.samples.to_vec();
    assert_eq!(
        finite_pcm,
        full_pcm[(LOOP_START - 1) * 2..(LOOP_END + 1) * 2]
    );
    let before: Vec<_> = observed
        .workers
        .iter()
        .map(WorkerWitness::observation)
        .collect();
    assert!(before.iter().all(|worker| worker["wait_result"] == 258));
    eprintln!(
        "window ACK SAME source BEFORE SHUTDOWN {}",
        json!({"sources":sources.iter().map(AssignmentWitness::evidence).collect::<Vec<_>>(),"workers":before,"full_frames":4096,"finite_range":[LOOP_START-1,LOOP_END+1],"finite_frames":1502})
    );
    let closed = close_productive_readers(p, &sources);
    assert_eq!(observed.engine_owner.as_ref().unwrap().strong_count(), 0);
    assert!(
        observed
            .readers
            .iter()
            .all(|reader| reader.strong_count() == 0)
    );
    workers_end(
        observed.workers,
        6,
        "window ACK original engine and both mixer images",
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
}
