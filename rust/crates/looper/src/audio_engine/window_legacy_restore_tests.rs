//! Retain current source authority across the original legacy and paired refusals.
use super::*;
use crate::audio_engine::native_owned_work::CompleteSampleWork;

fn capture_current_source(p: &Productive) -> impl Fn(&Productive) + use<> {
    let source = p.h.ticket(0);
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
    let assignment = p.h.engine.cold_leases.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .assignment_id();
    let finite = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    let finite_reader = Arc::downgrade(&finite.samples);
    let finite_pcm = finite.samples.to_vec();
    let set = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    let set_identity = Arc::downgrade(&set.complete_set_identity);
    let components = set
        .stems
        .each_ref()
        .map(|stem| (Arc::downgrade(&stem.samples), stem.samples.to_vec()));
    move |p: &Productive| {
        let after = p.h.ticket(0);
        assert!(source.is_current() && after.is_current());
        assert!(source.same_preparation_context(&after));
        assert_eq!(source.source_version, after.source_version);
        assert_eq!(after.source_version, p.h.version);
        assert_eq!(
            source.admission.source_generation,
            after.admission.source_generation
        );
        assert!(
            source
                .admission
                .binding
                .same_projection(&after.admission.binding)
        );
        assert_eq!(
            after.admission.binding.placement.unwrap().content_key(),
            Some(key)
        );
        assert!(p.h.engine.input_runtime_ownership.content_key_current(key));
        let rows = p.h.engine.cold_pcm_history.lock().unwrap();
        assert!(Arc::ptr_eq(&rows[0].identity, &history));
        assert!(rows[0].iter().any(|pcm| pcm.ptr_eq(&finite_reader)));
        drop(rows);
        let leases = p.h.engine.cold_leases.lock().unwrap();
        let lease = leases[0].as_ref().unwrap();
        assert_eq!(lease.assignment_id(), assignment);
        assert!(!lease.assignment_retired());
        assert_eq!(lease.original_path, files.original.path);
        assert_eq!(lease.cache_path, files.cache[0].path);
        assert!(lease.original_identity_matches(&files.original.identity));
        drop(leases);
        files.verify();
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
            (LOOP_START, LOOP_END, 4096, 1500, 2)
        );
        assert!(finite_reader.ptr_eq(&Arc::downgrade(&finite.samples)));
        assert_eq!(finite.samples.as_ref(), finite_pcm);
        assert_eq!(
            finite_pcm,
            p.h.material.sample.samples[LOOP_START * 2..LOOP_END * 2]
        );
        let set = p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(set_identity.ptr_eq(&Arc::downgrade(&set.complete_set_identity)));
        for ((reader, pcm), stem) in components.iter().zip(&set.stems) {
            assert!(reader.ptr_eq(&Arc::downgrade(&stem.samples)));
            assert_eq!(stem.samples.as_ref(), pcm);
        }
    }
}

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn capture_legacy_currentness(
    p: &Productive,
) -> impl Fn(&Productive) + use<> {
    let verify_source = capture_current_source(p);
    let verify = move |p: &Productive| {
        verify_source(p);
        let owners = p.h.engine.resident_stem_cache.lock().unwrap();
        assert!(owners[0].accepted.as_ref().unwrap().pair.is_none());
    };
    verify(p);
    verify
}

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn capture_paired_currentness(
    p: &Productive,
) -> impl Fn(&Productive) + use<> {
    let verify_source = capture_current_source(p);
    let (reader, descriptor, root, reference, assignment, original, cache, pair_files) = {
        let owners = p.h.engine.resident_stem_cache.lock().unwrap();
        let pair = owners[0].accepted.as_ref().unwrap().pair.as_ref().unwrap();
        let descriptor_path = crate::audio_engine::material_paths::resolve(
            &pair.root,
            Path::new(&pair.descriptor_reference),
        )
        .unwrap()
        .path;
        let pcm_path = crate::audio_engine::material_paths::resolve(
            &pair.root,
            Path::new(&pair.reader.descriptor.pcm_generation),
        )
        .unwrap()
        .path;
        (
            Arc::downgrade(&pair.reader),
            pair.reader.descriptor.clone(),
            pair.root.clone(),
            pair.descriptor_reference.clone(),
            pair.source_lease.assignment_id(),
            ObjectWitness::capture(&pair.source_lease.original_path),
            ObjectWitness::capture(&pair.source_lease.cache_path),
            [tree(&descriptor_path), tree(&pcm_path)],
        )
    };
    let root_identity = ObjectWitness::capture(&root);
    let verify = move |p: &Productive| {
        verify_source(p);
        let owners = p.h.engine.resident_stem_cache.lock().unwrap();
        let pair = owners[0].accepted.as_ref().unwrap().pair.as_ref().unwrap();
        assert!(reader.ptr_eq(&Arc::downgrade(&pair.reader)));
        assert_eq!(pair.reader.descriptor, descriptor);
        assert_eq!(pair.root, root);
        assert_eq!(pair.descriptor_reference, reference);
        assert_eq!(pair.reader.descriptor_reference, reference);
        assert_eq!(
            capture_identity(pair.source_lease.samples_root())
                .unwrap()
                .unwrap(),
            root_identity.identity
        );
        assert_eq!(pair.source_lease.assignment_id(), assignment);
        assert!(!pair.source_lease.assignment_retired());
        assert_eq!(pair.source_lease.original_path, original.path);
        assert_eq!(pair.source_lease.cache_path, cache.path);
        assert!(
            pair.source_lease
                .original_identity_matches(&original.identity)
        );
        let leases = p.h.engine.cold_leases.lock().unwrap();
        assert_eq!(leases[0].as_ref().unwrap().assignment_id(), assignment);
        drop((leases, owners));
        root_identity.verify();
        original.verify();
        cache.verify();
        for files in &pair_files {
            assert_eq!(tree(&files[0].path), *files);
        }
    };
    verify(p);
    verify
}

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn assert_current_complete_source(
    p: &Productive,
    verify: &impl Fn(&Productive),
) {
    verify(p);
    // File custody alone does not authorize a complete read for this current owner.
    let complete_reader = {
        let work = {
            let cache = p.h.engine.sample_cache.lock().unwrap();
            CompleteSampleWork::capture(&p.h.engine, 0, cache[0].as_ref().unwrap()).unwrap()
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
        assert_eq!(
            complete.samples.as_ref(),
            p.h.material.sample.samples.as_ref()
        );
        Arc::downgrade(&complete.samples)
    };
    assert_eq!(complete_reader.strong_count(), 0);
    verify(p);
}

pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) fn finish_legacy_source_fixture(
    p: Productive,
    refuse: bool,
    verify: impl Fn(&Productive),
) {
    assert_current_complete_source(&p, &verify);
    let current = p.h.ticket(0);
    eprintln!(
        "legacy TRANSIENT continuing source {}",
        json!({"refuse":refuse,"current_binding":format!("{:?}",current.admission.binding),"source_version":current.source_version,"source_generation":current.admission.source_generation,"complete_exact_frames":4096,"complete_reader_end":true,"finite_range":[LOOP_START,LOOP_END],"finite_frames":1500,"four_components_unchanged":true})
    );
    // Only the probe's test input ends here, after its original assertions and reader.
    p.h.engine.resident_account.clear_host_memory_for_test();
    drop((current, verify));
    finish_cancelled_source_fixture(p);
}
