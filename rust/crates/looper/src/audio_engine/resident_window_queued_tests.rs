//! Whole-window cancellation retires unclaimed readers without waiting for a worker.
use super::*;
use crate::audio_engine::cold_jobs::{QUEUED_JOBS, WORKERS};
use std::sync::atomic::AtomicUsize;

fn queued_window(close: bool) {
    Python::initialize();
    let mut fixture = PairWorkerFixture::new();
    let engine = &mut fixture.source.fixture.engine;
    wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    // Finish this fixture's original real load delivery before measuring the
    // independent window request. Drop its Python/native event owners here too.
    let original_request = engine.pad_request_ids.lock().unwrap()[0];
    Python::attach(|py| {
        let mut successes = 0;
        while let Some(event) = engine.poll_loader_events(py).unwrap() {
            let dict = event.bind(py).cast::<pyo3::types::PyDict>().unwrap();
            let field = |key| dict.get_item(key).unwrap().unwrap();
            assert_eq!(field("id").extract::<usize>().unwrap(), 0);
            assert_eq!(
                field("request_id").extract::<u64>().unwrap(),
                original_request
            );
            let kind = field("type").extract::<String>().unwrap();
            assert!(matches!(kind.as_str(), "started" | "progress" | "success"));
            successes += usize::from(kind == "success");
        }
        assert_eq!(successes, 1);
    });
    let samples = Arc::downgrade(
        &engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples,
    );
    let owners_before = samples.strong_count();
    let held = BlockedWorkers::hold(engine);
    let ticket = resident_relocation::prepare_window_with_producer(
        engine,
        0,
        resident_relocation::WindowRequest {
            loop_region: Some((512.0 / 48_000.0, Some(1024.0 / 48_000.0))),
            ..resident_relocation::WindowRequest::default()
        },
        fixture.producer.clone(),
    )
    .unwrap();
    let captured_owners = samples.strong_count();
    let peer_runs = Arc::new(AtomicUsize::new(0));
    for _ in 0..QUEUED_JOBS - 1 {
        let ran = peer_runs.clone();
        engine
            .cold_jobs
            .submit(engine.cold_jobs.reserve().unwrap(), move || {
                ran.fetch_add(1, Ordering::AcqRel);
            })
            .unwrap();
    }
    let admitted_before = engine.cold_jobs.counts_for_test();
    if close {
        // This separate positive control exercises the original unrun JobGuard.
        engine.cold_jobs.close_admission();
    } else {
        assert!(ticket.cancel());
    }
    Python::attach(|py| assert!(engine.poll_loader_events(py).unwrap().is_none()));
    let after_pump = engine.cold_jobs.counts_for_test();
    let owners_after_pump = samples.strong_count();
    let status = ticket.publication_status();
    let current = ticket.is_current();
    let selected_current = fixture.selection.is_current();
    let original = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let window_unchanged = original.resident_start() == 0
        && original.resident_end() == 4096
        && original.same_source(fixture.mixer.bank_for_measurement()[0].as_ref().unwrap());
    drop(original);
    let no_publication = fixture.consumer.is_empty();
    let peers_before_release = peer_runs.load(Ordering::Acquire);
    let replacement_runs = Arc::new(AtomicUsize::new(0));
    let replacement_admitted = if close {
        false
    } else {
        let ran = replacement_runs.clone();
        match engine.cold_jobs.reserve() {
            Ok(reservation) => {
                engine
                    .cold_jobs
                    .submit(reservation, move || {
                        ran.fetch_add(1, Ordering::AcqRel);
                    })
                    .unwrap();
                true
            }
            Err(_) => false,
        }
    };
    eprintln!(
        "queued window close={close}: before={admitted_before:?} after={after_pump:?} \
         PCMowners={owners_before}/{captured_owners}/{owners_after_pump} \
         status={status} current={current} selected={selected_current} \
         original={window_unchanged} no_publication={no_publication} \
         peer_before={peers_before_release} replacement={replacement_admitted}"
    );
    // Join the actual workers before evaluating the pre-release observations.
    drop(held);
    wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    engine.cold_jobs.shutdown();
    // Other legitimate owners still hold this backing. Only these two actual
    // WindowWork readers must end; this does not certify its final physical reader.
    assert_eq!(captured_owners, owners_before + 2);
    assert_eq!(
        owners_after_pump, owners_before,
        "queued old/bank readers end"
    );
    assert_eq!(
        admitted_before,
        (WORKERS, QUEUED_JOBS, WORKERS + QUEUED_JOBS)
    );
    let peers = if close { 0 } else { QUEUED_JOBS - 1 };
    assert_eq!(after_pump, (WORKERS, peers, WORKERS + peers));
    assert_eq!(status, "cancelled");
    assert!(!current);
    assert!(selected_current);
    assert!(window_unchanged);
    assert!(no_publication);
    assert_eq!(peers_before_release, 0);
    assert_eq!(replacement_admitted, !close);
    assert_eq!(peer_runs.load(Ordering::Acquire), peers);
    assert_eq!(
        replacement_runs.load(Ordering::Acquire),
        usize::from(!close)
    );
}

#[test]
fn cancelled_unclaimed_window_releases_old_bank_readers_and_full_lane_capacity() {
    queued_window(false);
}

#[test]
fn uncancelled_close_retires_queued_window_readers_and_settles_original_guard() {
    queued_window(true);
}

fn queued_window_after_newer_request(admit_newer: bool) {
    Python::initialize();
    let mut fixture = PairWorkerFixture::new();
    let engine = &fixture.source.fixture.engine;
    wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    let original_request = engine.pad_request_ids.lock().unwrap()[0];
    Python::attach(|py| {
        let mut successes = 0;
        while let Some(event) = engine.poll_loader_events(py).unwrap() {
            let dict = event.bind(py).cast::<pyo3::types::PyDict>().unwrap();
            let field = |key| dict.get_item(key).unwrap().unwrap();
            assert_eq!(field("id").extract::<usize>().unwrap(), 0);
            assert_eq!(
                field("request_id").extract::<u64>().unwrap(),
                original_request
            );
            let kind = field("type").extract::<String>().unwrap();
            assert!(matches!(kind.as_str(), "started" | "progress" | "success"));
            successes += usize::from(kind == "success");
        }
        assert_eq!(successes, 1);
    });
    let samples = Arc::downgrade(
        &engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples,
    );
    let owners_before = samples.strong_count();
    let held = BlockedWorkers::hold(engine);
    let request = |start, end| {
        engine.prepare_resident_control_with_source(
            0,
            Some(start as f64 / 48_000.0),
            Some(end as f64 / 48_000.0),
            None,
            None,
            None,
        )
    };
    let first = request(512, 1024).unwrap();
    let owners_first = samples.strong_count();
    let latest = admit_newer.then(|| request(1024, 1536).unwrap());
    let owners_latest = samples.strong_count();
    let peer_runs = Arc::new(AtomicUsize::new(0));
    let peers = QUEUED_JOBS - 1 - usize::from(admit_newer);
    for _ in 0..peers {
        let ran = peer_runs.clone();
        engine
            .cold_jobs
            .submit(engine.cold_jobs.reserve().unwrap(), move || {
                ran.fetch_add(1, Ordering::AcqRel);
            })
            .unwrap();
    }
    // A real failed capacity admission must not supersede the accepted first request.
    let refused = if admit_newer {
        None
    } else {
        Some(
            request(1024, 1536)
                .err()
                .expect("the actual full cold queue must refuse the newer request")
                .to_string(),
        )
    };
    let before = engine.cold_jobs.counts_for_test();
    let first_current_before = first.is_current();
    let first_status_before = first.publication_status();
    Python::attach(|py| assert!(engine.poll_loader_events(py).unwrap().is_none()));
    let after = engine.cold_jobs.counts_for_test();
    let owners_after = samples.strong_count();
    let first_current = first.is_current();
    let first_status = first.publication_status();
    let current = latest.as_ref().unwrap_or(&first);
    let latest_current = current.is_current();
    let latest_status = current.publication_status();
    let original = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let unchanged = original.resident_start() == 0
        && original.resident_end() == 4096
        && original.same_source(fixture.mixer.bank_for_measurement()[0].as_ref().unwrap())
        && fixture.selection.is_current();
    drop(original);
    let no_publication = fixture.consumer.is_empty();
    let peers_before_release = peer_runs.load(Ordering::Acquire);
    let replacement_runs = Arc::new(AtomicUsize::new(0));
    let replacement_admitted = match engine.cold_jobs.reserve() {
        Ok(reservation) => {
            let ran = replacement_runs.clone();
            engine
                .cold_jobs
                .submit(reservation, move || {
                    ran.fetch_add(1, Ordering::AcqRel);
                })
                .unwrap();
            true
        }
        Err(_) => false,
    };
    eprintln!(
        "queued public window newer={admit_newer}: before={before:?} after={after:?} \
         PCMowners={owners_before}/{owners_first}/{owners_latest}/{owners_after} \
         first={first_status_before}/{first_status} current={first_current_before}/{first_current} \
         latest={latest_status}/{latest_current} unchanged={unchanged} no_publication={no_publication} \
         peer_before={peers_before_release} replacement={replacement_admitted} refused={refused:?}"
    );
    // Finish every real worker and the sole surviving original callback before RED assertions.
    drop(held);
    wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    let message = fixture.consumer.pop().unwrap();
    let ControlMessage::RelocateResident(transaction) = &message else {
        panic!("actual current window publication required");
    };
    let expected_start = if admit_newer { 1024 } else { 512 };
    assert_eq!(transaction.sample.resident_start(), expected_start);
    assert_eq!(transaction.sample.resident_end(), expected_start + 512);
    assert!(fixture.consumer.is_empty());
    fixture.dispatch(message);
    let engine = &mut fixture.source.fixture.engine;
    resident_relocation::reconcile(engine).unwrap();
    let current = latest.as_ref().unwrap_or(&first);
    let accepted = current.publication_status() == "accepted" && current.is_current();
    let original = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let adopted = original.resident_start() == expected_start
        && original.resident_end() == expected_start + 512
        && original.same_source(fixture.mixer.bank_for_measurement()[0].as_ref().unwrap());
    drop(original);
    engine.cold_jobs.shutdown();
    assert_eq!(owners_first, owners_before + 2, "actual old/bank captures");
    assert_eq!(owners_latest, owners_first + 2 * usize::from(admit_newer));
    assert_eq!(
        owners_after, owners_first,
        "only the current queued readers remain"
    );
    assert_eq!(before, (WORKERS, QUEUED_JOBS, WORKERS + QUEUED_JOBS));
    let remaining = QUEUED_JOBS - usize::from(admit_newer);
    assert_eq!(after, (WORKERS, remaining, WORKERS + remaining));
    assert_eq!(first_status_before, "preparing");
    assert_eq!(first_current_before, !admit_newer);
    assert_eq!(
        first_status,
        if admit_newer {
            "cancelled"
        } else {
            "preparing"
        }
    );
    assert_eq!(first_current, !admit_newer);
    assert!(latest_current);
    assert_eq!(latest_status, "preparing");
    assert!(unchanged && no_publication);
    assert_eq!(peers_before_release, 0);
    assert_eq!(replacement_admitted, admit_newer);
    assert!(
        refused.is_none_or(
            |error| error.contains("cold source queue full (2 workers, 32 queued jobs)")
        )
    );
    assert_eq!(peer_runs.load(Ordering::Acquire), peers);
    assert_eq!(
        replacement_runs.load(Ordering::Acquire),
        usize::from(admit_newer)
    );
    assert!(accepted && adopted && fixture.selection.is_current());
}

#[test]
fn superseded_queued_window_releases_only_old_readers_before_latest_own_ack() {
    queued_window_after_newer_request(true);
}

#[test]
fn refused_new_window_preserves_queued_original_readers_and_own_ack() {
    queued_window_after_newer_request(false);
}

fn original_pending_stem_readers(
    engine: &AudioEngine,
    ticket: &ResidentStemTicket,
    samples: &std::sync::Weak<[f32]>,
    components: &[std::sync::Weak<[f32]>],
) -> bool {
    let owners = engine.resident_stem_cache.lock().unwrap();
    let committed = &owners[0].committed;
    assert!(committed.registration.is_none());
    assert!(committed.proposal.is_none());
    assert!(committed.policy_pending.is_none());
    assert!(committed.policy_proposal.is_none());
    let Some(pending) = committed.pending.as_ref() else {
        return false;
    };
    assert!(Arc::ptr_eq(&pending.status, &ticket.status));
    let set = pending.stems.as_ref().unwrap();
    assert!(Arc::downgrade(&set.reference_samples).ptr_eq(samples));
    assert_eq!(set.stems.len(), components.len());
    for (stem, component) in set.stems.iter().zip(components) {
        assert!(Arc::downgrade(&stem.samples).ptr_eq(component));
    }
    true
}

fn queued_window_after_full_mix(admit_full_mix: bool) {
    Python::initialize();
    let mut fixture = PairWorkerFixture::new();
    let engine = &fixture.source.fixture.engine;
    wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    Python::attach(|py| {
        let mut successes = 0;
        while let Some(event) = engine.poll_loader_events(py).unwrap() {
            let dict = event.bind(py).cast::<pyo3::types::PyDict>().unwrap();
            let kind = dict
                .get_item("type")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert!(matches!(kind.as_str(), "started" | "progress" | "success"));
            successes += usize::from(kind == "success");
        }
        assert_eq!(successes, 1);
    });
    // Start from a genuine selected disk pair and its actual AllStems adoption.
    // FullMix must change callback demand, not merely repeat its default value.
    let stems = engine
        .request_committed_stem_residency_with_interest_and_producer(
            &fixture.selection,
            "all_stems",
            STEM_COMPONENT_MASK,
            "explicit",
            &fixture.producer,
        )
        .unwrap();
    let capture = fixture.consumer.pop().unwrap();
    assert!(matches!(&capture, ControlMessage::CommittedStemWork(work)
        if matches!(work.as_ref(), CommittedStemRtWork::Capture(_))));
    fixture.dispatch(capture);
    wait_until(|| stems.status.control_published.load(Ordering::Acquire));
    wait_until(|| !fixture.consumer.is_empty());
    let adoption = fixture.consumer.pop().unwrap();
    assert!(matches!(&adoption, ControlMessage::CommittedStemWork(work)
        if matches!(work.as_ref(), CommittedStemRtWork::Adopt(_))));
    fixture.dispatch(adoption);
    resident_relocation::reconcile(&fixture.source.fixture.engine).unwrap();
    assert_eq!(stems.residency_status(), "accepted");
    assert!(stems.is_current());
    assert_eq!(
        fixture.mixer.stem_demand_for_measurement(0).0,
        crate::messages::StemMixMode::AllStems
    );
    let engine = &fixture.source.fixture.engine;
    wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    let samples = Arc::downgrade(
        &engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples,
    );
    let components = {
        let owners = engine.resident_stem_cache.lock().unwrap();
        let set = &owners[0].accepted.as_ref().unwrap().set;
        assert!(Arc::downgrade(&set.reference_samples).ptr_eq(&samples));
        set.stems
            .iter()
            .map(|stem| Arc::downgrade(&stem.samples))
            .collect::<Vec<_>>()
    };
    assert_eq!(components.len(), 4);
    for (index, component) in components.iter().enumerate() {
        assert!(!component.ptr_eq(&samples));
        assert!(
            components[..index]
                .iter()
                .all(|other| !component.ptr_eq(other))
        );
    }
    assert!(original_pending_stem_readers(
        engine,
        &stems,
        &samples,
        &components
    ));
    let owners_before = samples.strong_count();
    let components_before = components
        .iter()
        .map(std::sync::Weak::strong_count)
        .collect::<Vec<_>>();
    let held = BlockedWorkers::hold(engine);
    let first = engine
        .prepare_resident_control_with_source(
            0,
            Some(512.0 / 48_000.0),
            Some(1024.0 / 48_000.0),
            None,
            None,
            None,
        )
        .unwrap();
    let owners_captured = samples.strong_count();
    let components_captured = components
        .iter()
        .map(std::sync::Weak::strong_count)
        .collect::<Vec<_>>();
    assert!(
        resident_relocation::control_publication_tests::queued_window_has_no_prepared_cache_readers(
            engine, 0
        )
    );
    assert!(original_pending_stem_readers(
        engine,
        &stems,
        &samples,
        &components
    ));
    let peer_runs = Arc::new(AtomicUsize::new(0));
    for _ in 0..QUEUED_JOBS - 1 {
        let ran = peer_runs.clone();
        engine
            .cold_jobs
            .submit(engine.cold_jobs.reserve().unwrap(), move || {
                ran.fetch_add(1, Ordering::AcqRel);
            })
            .unwrap();
    }
    let blocked_controls = if admit_full_mix {
        0
    } else {
        let mut producer = fixture.producer.lock().unwrap();
        let mut filled = 0;
        while !producer.is_full() {
            producer.push(ControlMessage::Ping()).unwrap();
            filled += 1;
        }
        filled
    };
    let intent = engine.resident_stem_cache.lock().unwrap()[0]
        .intent_epoch
        .clone();
    let intent_before = intent.load(Ordering::Acquire);
    // This exact production admission helper is also used by the public FullMix
    // API. The fixture supplies its real producer without opening an audio device.
    let full_mix =
        resident_relocation::set_stem_pair_full_mix_with_producer(engine, 0, &fixture.producer);
    let full_mix_error = full_mix.err().map(|error| error.to_string());
    let intent_after = intent.load(Ordering::Acquire);
    let owners_after_admission = samples.strong_count();
    let components_after_admission = components
        .iter()
        .map(std::sync::Weak::strong_count)
        .collect::<Vec<_>>();
    assert!(
        resident_relocation::control_publication_tests::queued_window_has_no_prepared_cache_readers(
            engine, 0
        )
    );
    assert!(original_pending_stem_readers(
        engine,
        &stems,
        &samples,
        &components
    ));
    let accepted_owner_retained = engine.resident_stem_cache.lock().unwrap()[0]
        .accepted
        .is_some();
    let before = engine.cold_jobs.counts_for_test();
    Python::attach(|py| assert!(engine.poll_loader_events(py).unwrap().is_none()));
    let after = engine.cold_jobs.counts_for_test();
    let owners_after = samples.strong_count();
    let components_after = components
        .iter()
        .map(std::sync::Weak::strong_count)
        .collect::<Vec<_>>();
    let pending_after = original_pending_stem_readers(engine, &stems, &samples, &components);
    let status = first.publication_status();
    let current = first.is_current();
    let original = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let unchanged = original.resident_start() == 0
        && original.resident_end() == 4096
        && original.same_source(fixture.mixer.bank_for_measurement()[0].as_ref().unwrap())
        && fixture.selection.is_current();
    drop(original);
    let demand_before_callback = fixture.mixer.stem_demand_for_measurement(0).0;
    let peers_before_release = peer_runs.load(Ordering::Acquire);
    let replacement_runs = Arc::new(AtomicUsize::new(0));
    let replacement_admitted = match engine.cold_jobs.reserve() {
        Ok(reservation) => {
            let ran = replacement_runs.clone();
            engine
                .cold_jobs
                .submit(reservation, move || {
                    ran.fetch_add(1, Ordering::AcqRel);
                })
                .unwrap();
            true
        }
        Err(_) => false,
    };
    eprintln!(
        "queued window FullMix={admit_full_mix}: before={before:?} after={after:?} \
         PCMowners={owners_before}/{owners_captured}/{owners_after_admission}/{owners_after} \
         components={components_before:?}/{components_captured:?}/{components_after_admission:?}/{components_after:?} \
         original_pending_after={pending_after} \
         intent={intent_before}/{intent_after} status={status} current={current} \
         unchanged={unchanged} peers={peers_before_release} replacement={replacement_admitted} \
         control_error={full_mix_error:?}"
    );
    if admit_full_mix {
        let message = fixture.consumer.pop().unwrap();
        assert!(matches!(
            &message,
            ControlMessage::SetStemPairFullMix { id: 0, .. }
        ));
        assert!(
            fixture.consumer.is_empty(),
            "obsolete window never publishes"
        );
        fixture.dispatch(message);
        assert_eq!(
            fixture.mixer.stem_demand_for_measurement(0).0,
            crate::messages::StemMixMode::FullMix
        );
    } else {
        assert!(blocked_controls > 0);
        for _ in 0..blocked_controls {
            let message = fixture.consumer.pop().unwrap();
            assert!(matches!(&message, ControlMessage::Ping()));
            fixture.dispatch(message);
        }
        assert!(fixture.consumer.is_empty());
    }
    drop(held);
    wait_until(|| fixture.source.fixture.engine.cold_jobs.counts_for_test() == (0, 0, 0));
    // A fresh valid window after FullMix must still prepare and reach its OWN ACK.
    // The refused FullMix instead preserves the original queued window's OWN ACK.
    let fresh = if admit_full_mix {
        assert!(
            fixture.consumer.is_empty(),
            "obsolete window never publishes"
        );
        Some(
            fixture
                .source
                .fixture
                .engine
                .prepare_resident_control_with_source(
                    0,
                    Some(1024.0 / 48_000.0),
                    Some(1536.0 / 48_000.0),
                    None,
                    None,
                    None,
                )
                .unwrap(),
        )
    } else {
        None
    };
    let surviving = fresh.as_ref().unwrap_or(&first);
    wait_until(|| !fixture.consumer.is_empty());
    let message = fixture.consumer.pop().unwrap();
    let ControlMessage::RelocateResident(transaction) = &message else {
        panic!("actual surviving window publication required");
    };
    let expected_start = if admit_full_mix { 1024 } else { 512 };
    assert_eq!(transaction.sample.resident_start(), expected_start);
    assert_eq!(transaction.sample.resident_end(), expected_start + 512);
    assert!(fixture.consumer.is_empty());
    fixture.dispatch(message);
    let engine = &mut fixture.source.fixture.engine;
    resident_relocation::reconcile(engine).unwrap();
    let accepted = surviving.publication_status() == "accepted" && surviving.is_current();
    let original = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let adopted = original.resident_start() == expected_start
        && original.resident_end() == expected_start + 512
        && original.same_source(fixture.mixer.bank_for_measurement()[0].as_ref().unwrap());
    drop(original);
    engine.cold_jobs.shutdown();
    // WindowWork owns old, bank and the cloned stem reference plus one reader of
    // each component. The separately identified original Pending mirror also
    // ends on FullMix polling; it is not one of those queued captures.
    assert!(accepted && adopted && fixture.selection.is_current());
    eprintln!("FullMix={admit_full_mix} surviving window ownACK={accepted} adopted={adopted}");
    assert_eq!(owners_captured, owners_before + 3);
    assert_eq!(owners_after_admission, owners_captured);
    assert_eq!(
        owners_after,
        owners_before - usize::from(admit_full_mix) + 3 * usize::from(!admit_full_mix)
    );
    assert_eq!(components_after_admission, components_captured);
    for ((baseline, captured), after) in components_before
        .iter()
        .zip(&components_captured)
        .zip(&components_after)
    {
        assert_eq!(*captured, baseline + 1);
        assert_eq!(
            *after,
            baseline - usize::from(admit_full_mix) + usize::from(!admit_full_mix)
        );
    }
    assert_eq!(pending_after, !admit_full_mix);
    assert_eq!(before, (WORKERS, QUEUED_JOBS, WORKERS + QUEUED_JOBS));
    let remaining = QUEUED_JOBS - usize::from(admit_full_mix);
    assert_eq!(after, (WORKERS, remaining, WORKERS + remaining));
    assert_eq!(intent_after, intent_before + u64::from(admit_full_mix));
    assert_eq!(accepted_owner_retained, !admit_full_mix);
    assert_eq!(
        status,
        if admit_full_mix {
            "cancelled"
        } else {
            "preparing"
        }
    );
    assert_eq!(current, !admit_full_mix);
    assert_eq!(
        demand_before_callback,
        crate::messages::StemMixMode::AllStems
    );
    assert!(unchanged);
    assert_eq!(peers_before_release, 0);
    assert_eq!(replacement_admitted, admit_full_mix);
    assert_eq!(peer_runs.load(Ordering::Acquire), QUEUED_JOBS - 1);
    assert_eq!(
        replacement_runs.load(Ordering::Acquire),
        usize::from(admit_full_mix)
    );
    assert_eq!(full_mix_error.is_none(), admit_full_mix);
    assert!(full_mix_error.is_none_or(|error| error.contains("stem full-mix queue is full")));
}

#[test]
fn full_mix_retires_queued_window_readers_before_its_callback_and_preserves_fresh_own_ack() {
    queued_window_after_full_mix(true);
}

#[test]
fn refused_full_mix_preserves_queued_window_readers_and_original_own_ack() {
    queued_window_after_full_mix(false);
}
