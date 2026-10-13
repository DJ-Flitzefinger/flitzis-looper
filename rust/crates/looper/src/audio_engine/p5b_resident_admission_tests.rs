//! Actual native proposal and policy transactions on the existing productive harness.
use super::*;

fn policy(p: &mut Productive, interest: &str) -> ResidentStemTicket {
    p.h.engine
        .request_committed_stem_residency_with_interest_and_producer(
            &p.selected,
            "full_mix",
            15,
            interest,
            &p.h.producer,
        )
        .unwrap()
}

fn settle_policy(p: &mut Productive, receipt: &ResidentStemTicket) {
    assert_eq!(receipt.effective_mode_status(), "not_requested");
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    wait_until(|| p.h.consumer.peek().is_ok() || receipt.error().is_some());
    assert_eq!(
        receipt.residency_status(),
        "pending",
        "{:?}",
        receipt.error()
    );
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(
        receipt.residency_status(),
        "accepted",
        "{:?}",
        receipt.error()
    );
    assert!(receipt.is_current());
    assert_eq!(receipt.effective_mode_status(), "not_requested");
    reconcile(&p.h.engine).unwrap();
}

#[test]
fn native_preload_ack_preserves_full_mix_and_withdraw_keeps_explicit_components() {
    for key_lock in [false, true] {
        let mut p = Productive::new(key_lock, 0.73);
        let before = voice(&p.h.callback.mixer).source_playback;
        let generation = voice(&p.h.callback.mixer).generation;
        let native = voice(&p.h.callback.mixer).stretch.native_state_address();
        let demand = p.h.callback.mixer.stem_demand_for_measurement(0);
        let epoch = p.h.engine.resident_stem_cache.lock().unwrap()[0]
            .intent_epoch
            .load(Ordering::Acquire);
        let preload = policy(&mut p, "preload");
        assert_eq!(
            p.h.engine.resident_stem_cache.lock().unwrap()[0]
                .intent_epoch
                .load(Ordering::Acquire),
            epoch
        );
        settle_policy(&mut p, &preload);
        assert_eq!(p.h.callback.mixer.stem_demand_for_measurement(0), demand);
        assert!(
            voice(&p.h.callback.mixer)
                .source_playback
                .matches_exact(&before)
        );
        assert_eq!(voice(&p.h.callback.mixer).generation, generation);
        assert_eq!(
            voice(&p.h.callback.mixer).stretch.native_state_address(),
            native
        );
        let components = p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .stems
            .clone()
            .map(|sample| sample.samples);
        p.render(37, true);

        let explicit = p.request("all_stems", 15);
        p.capture_then_wait(&explicit);
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        assert_eq!(explicit.residency_status(), "accepted");
        let current = p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap();
        for (old, current) in components.iter().zip(&current.stems) {
            assert!(Arc::ptr_eq(old, &current.samples));
        }
        p.select_reference("all_stems", 15);
        p.render(128, true);
        // Policy waits for the genuine explicit ramp/Native acknowledgement,
        // including the slower source-frame rate of this 0.73 fixture.
        p.render(256, false);
        if p.key_lock {
            p.render(777, false);
            p.render(13, true);
            p.adopt_native();
        }
        assert_eq!(explicit.effective_mode_status(), "effective");
        assert!(
            preload.is_current(),
            "unchanged ready backing survives an explicit same-window start"
        );

        let demand = p.h.callback.mixer.stem_demand_for_measurement(0);
        let withdrawn = policy(&mut p, "withdraw_preload");
        settle_policy(&mut p, &withdrawn);
        assert!(!preload.is_current());
        assert!(
            explicit.is_current(),
            "policy withdrawal preserves explicit receipt authority"
        );
        assert_eq!(p.h.callback.mixer.stem_demand_for_measurement(0), demand);
        let current = p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap();
        for (old, current) in components.iter().zip(&current.stems) {
            assert!(Arc::ptr_eq(old, &current.samples));
        }
        p.render(37, true);
    }
}

#[test]
fn explicit_preparation_rejects_busy_policy_without_supersession_then_policy_keeps_own_ack() {
    let mut p = Productive::new(false, 0.73);
    let explicit = p.request("all_stems", 15);
    let intent = p.h.engine.resident_stem_cache.lock().unwrap()[0]
        .intent_epoch
        .load(Ordering::Acquire);
    let busy =
        p.h.engine
            .request_committed_stem_residency_with_interest_and_producer(
                &p.selected,
                "full_mix",
                15,
                "preload",
                &p.h.producer,
            )
            .err()
            .expect("policy must defer behind the explicit own acknowledgement");
    assert!(busy.to_string().contains("[resident_stem_interest_busy]"));
    assert!(explicit.is_current());
    assert_eq!(
        p.h.engine.resident_stem_cache.lock().unwrap()[0]
            .intent_epoch
            .load(Ordering::Acquire),
        intent
    );
    p.capture_then_wait(&explicit);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(explicit.residency_status(), "accepted");
    assert!(explicit.is_current());
    p.select_reference("all_stems", 15);
    p.render(256, true);
    assert_eq!(explicit.effective_mode_status(), "effective");
    let audible_epoch = p.h.engine.resident_stem_cache.lock().unwrap()[0]
        .intent_epoch
        .load(Ordering::Acquire);
    let demand = p.h.callback.mixer.stem_demand_for_measurement(0);
    let preload = policy(&mut p, "preload");
    settle_policy(&mut p, &preload);
    assert!(explicit.is_current());
    assert_eq!(
        p.h.engine.resident_stem_cache.lock().unwrap()[0]
            .intent_epoch
            .load(Ordering::Acquire),
        audible_epoch
    );
    assert_eq!(p.h.callback.mixer.stem_demand_for_measurement(0), demand);
    let withdrawn = policy(&mut p, "withdraw_preload");
    settle_policy(&mut p, &withdrawn);
    assert!(explicit.is_current());
    assert_eq!(
        p.h.engine.resident_stem_cache.lock().unwrap()[0]
            .intent_epoch
            .load(Ordering::Acquire),
        audible_epoch
    );
    assert_eq!(p.h.callback.mixer.stem_demand_for_measurement(0), demand);
    p.render(37, true);
}

fn wait_phase(phase: &str, id: usize, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "P5b native phase {phase} timed out at pad {id}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait_source_enqueue(
    h: &mut Harness,
    request: &crate::audio_engine::MaterialMigrationSourceTicket,
) {
    let id = request.sample_id();
    wait_phase("Source enqueue or rejection", id, || {
        h.consumer.peek().is_ok() || request.phase().unwrap() == "rejected"
    });
    if request.phase().unwrap() == "rejected" {
        let mut failure = None;
        wait_phase("Source rejection error delivery", id, || {
            while let Ok(event) = h.engine.loader_rx.lock().unwrap().try_recv() {
                if let LoaderEvent::Error {
                    id: actual,
                    request_id,
                    error,
                } = event
                    && actual == id
                    && request_id == request.request_id()
                {
                    failure = Some(error);
                }
            }
            failure.is_some()
        });
        panic!(
            "P5b Source enqueue rejected at pad {id}, request {}: {}",
            request.request_id(),
            failure.unwrap()
        );
    }
}

fn current_material_preparation(h: &Harness) -> crate::audio_engine::MaterialMigrationPreparation {
    let source = h.ticket(0).sample;
    let lease = h.engine.cold_leases.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .subscribe_verified()
        .unwrap();
    // long_harness has already released its original preparation assignment.
    // A fresh independent verified subscriber supplies this new preparation;
    // releasing it cannot retire the actual current source assignment.
    control::prepared_for_test(
        &h.engine,
        PreparedMigrationMaterial::from_current(&h.root, source, lease).unwrap(),
        &h.root,
    )
    .unwrap()
}

#[test]
fn source_success_public_poll_uses_armed_epoch_and_later_timing_edit_is_stale() {
    let mut h = long_harness();
    let preparation = current_material_preparation(&h);
    for later_edit in [false, true] {
        let request = control::adopt_for_format(
            &h.engine,
            0,
            &preparation,
            h.producer.clone(),
            (2, RATE, h.root.clone()),
        )
        .unwrap();
        wait_source_enqueue(&mut h, &request);
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        wait_phase("public Source Success production", 0, || {
            h.engine.cold_loading[0].load(Ordering::Acquire) == 0
        });
        if later_edit {
            crate::audio_engine::constant_timing::set_intent(
                &h.engine,
                &h.producer,
                0,
                TimingIntent::Manual,
            )
            .unwrap();
        }
        Python::attach(|py| {
            let mut matched = false;
            wait_phase("public Source Success poll", 0, || {
                let Some(event) = h.engine.poll_loader_events(py).unwrap() else {
                    return false;
                };
                let event = event.bind(py).cast::<PyDict>().unwrap();
                let kind = event
                    .get_item("type")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap();
                let actual_request = event
                    .get_item("request_id")
                    .unwrap()
                    .unwrap()
                    .extract::<u64>()
                    .unwrap();
                if actual_request == request.request_id() && kind == "error" {
                    panic!(
                        "actual source adoption failed: {:?}",
                        event.get_item("error").unwrap()
                    );
                }
                if actual_request == request.request_id() && kind == "success" {
                    assert_eq!(
                        event.get_item("timing_stale").unwrap().is_some(),
                        later_edit
                    );
                    matched = true;
                }
                matched
            });
        });
        if later_edit {
            assert_eq!(h.callback.drain(&mut h.consumer), 1);
        }
    }
    preparation.release_preparation().unwrap();
}

#[test]
fn capture_only_seek_budget_failure_preserves_current_authority_and_audio() {
    let mut p = Productive::new(false, 0.73);
    p.h.engine
        .project_assets
        .set_resident_budget_mib(&p.h.engine.resident_account, 128)
        .unwrap();
    let status =
        p.h.engine
            .project_assets
            .resident_budget_status(&p.h.engine.resident_account)
            .unwrap();
    let _pending_other_job =
        p.h.engine
            .project_assets
            .reserve_resident(
                &p.h.engine.resident_account,
                status.available_bytes - 16,
                &[],
                0,
            )
            .unwrap();
    let before = voice(&p.h.callback.mixer).source_playback;
    let generation = voice(&p.h.callback.mixer).generation;
    let request = p.h.engine.pad_request_ids.lock().unwrap()[0];
    let source_epoch = p.h.engine.prepared_source_epochs[0].load(Ordering::Acquire);
    let intent = p.h.engine.resident_stem_cache.lock().unwrap()[0]
        .intent_epoch
        .load(Ordering::Acquire);
    let bank = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap()
        .samples
        .clone();
    let ticket = prepare_window_with_producer(
        &p.h.engine,
        0,
        WindowRequest {
            seek_position_s: Some(0.0),
            ..WindowRequest::default()
        },
        p.h.producer.clone(),
    )
    .unwrap();
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert!(
        voice(&p.h.callback.mixer)
            .source_playback
            .matches_exact(&before),
        "capture cannot seek before admission"
    );
    wait_until(|| ticket.error().unwrap().is_some());
    assert!(
        ticket
            .error()
            .unwrap()
            .unwrap()
            .starts_with("[resident_pcm_budget_exceeded]")
    );
    assert_eq!(p.h.engine.pad_request_ids.lock().unwrap()[0], request);
    assert_eq!(
        p.h.engine.prepared_source_epochs[0].load(Ordering::Acquire),
        source_epoch
    );
    assert_eq!(
        p.h.engine.resident_stem_cache.lock().unwrap()[0]
            .intent_epoch
            .load(Ordering::Acquire),
        intent
    );
    assert_eq!(voice(&p.h.callback.mixer).generation, generation);
    assert!(Arc::ptr_eq(
        &bank,
        &p.h.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    assert!(p.h.consumer.peek().is_err());
    p.render(37, true);
}

fn wait_source_ack(h: &mut Harness, id: usize, request: u64) {
    let mut accepted = false;
    wait_phase("Source own ACK", id, || {
        while let Ok(event) = h.engine.loader_rx.lock().unwrap().try_recv() {
            match event {
                LoaderEvent::Success {
                    id: actual,
                    request_id,
                    ..
                } if actual == id && request_id == request => accepted = true,
                LoaderEvent::Error {
                    id: actual,
                    request_id,
                    error,
                    ..
                } if actual == id && request_id == request => panic!("source {id}: {error}"),
                _ => {}
            }
        }
        accepted && h.engine.cold_loading[id].load(Ordering::Acquire) == 0
    });
}

fn actual_bank_pcm(h: &Harness) -> (usize, usize) {
    let mut actual = std::collections::HashMap::new();
    for id in 0..NUM_SAMPLES {
        let source = h.callback.mixer.bank_for_measurement()[id]
            .as_ref()
            .unwrap();
        actual.insert(source.samples.as_ptr() as usize, source.samples.len() * 4);
        if let Some(set) = &h.callback.mixer.stems_for_measurement()[id] {
            actual.insert(
                set.reference_samples.as_ptr() as usize,
                set.reference_samples.len() * 4,
            );
            for component in &set.stems {
                actual.insert(
                    component.samples.as_ptr() as usize,
                    component.samples.len() * 4,
                );
            }
        }
    }
    (actual.values().sum(), actual.len())
}

fn actual_bank_reader_probes(h: &Harness) -> Vec<std::sync::Weak<[f32]>> {
    let mut actual = std::collections::HashMap::new();
    for id in 0..NUM_SAMPLES {
        let source = h.callback.mixer.bank_for_measurement()[id]
            .as_ref()
            .unwrap();
        actual.insert(
            source.samples.as_ptr() as usize,
            Arc::downgrade(&source.samples),
        );
        if let Some(set) = &h.callback.mixer.stems_for_measurement()[id] {
            actual.insert(
                set.reference_samples.as_ptr() as usize,
                Arc::downgrade(&set.reference_samples),
            );
            for component in &set.stems {
                actual.insert(
                    component.samples.as_ptr() as usize,
                    Arc::downgrade(&component.samples),
                );
            }
        }
    }
    actual.into_values().collect()
}

#[test]
fn all_216_stopped_pads_ack_real_preload_and_repeat_window_registry_retirement() {
    // One genuine verified source and component set, retained by all 216 final
    // banks. The real 2/32 lane and callback consume each subscriber's own ACK;
    // no voice or RubberBand pool limit is raised for this stopped-pad case.
    eprintln!("P5b 216 phase: initialize productive harness");
    let mut h = long_harness();
    // The generic Harness collector intentionally pins retired payloads. Move
    // those real initial owners into the same bounded FIFO/worker used by the
    // productive public Generate test, then use it for every later ACK.
    let initial_retired: Vec<_> = h
        .callback
        .retirement
        .retired
        .iter()
        .filter_map(|buffer| match buffer {
            crate::audio_engine::buffer_retirement::RetiredAudioBuffer::Sample(sample)
                if sample.residency.is_some() =>
            {
                Some(Arc::downgrade(&sample.samples))
            }
            _ => None,
        })
        .collect();
    let (actual_retirement, retirement_worker) =
        crate::audio_engine::buffer_retirement::create_audio_buffer_retirement();
    h.callback.retirement.actual = Some(actual_retirement);
    for buffer in std::mem::take(&mut h.callback.retirement.retired) {
        h.callback.retirement.push(buffer);
    }
    wait_phase("initial actual retirement worker", 0, || {
        initial_retired.iter().all(|pcm| pcm.strong_count() == 0)
    });
    eprintln!("P5b 216 phase: admit actual source banks");
    h.engine
        .project_assets
        .set_resident_budget_mib(&h.engine.resident_account, 128)
        .unwrap();
    let preparation = current_material_preparation(&h);
    for id in 0..NUM_SAMPLES {
        let request = control::adopt_for_format(
            &h.engine,
            id,
            &preparation,
            h.producer.clone(),
            (2, RATE, h.root.clone()),
        )
        .unwrap();
        wait_source_enqueue(&mut h, &request);
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        wait_source_ack(&mut h, id, request.request_id());
        assert!(request.is_current().unwrap());
    }
    preparation.release_preparation().unwrap();
    drop(preparation);

    let mut previous_receipts: Vec<ResidentStemTicket> = Vec::new();
    let mut previous_selections: Vec<Option<CommittedStemSetTicket>> =
        (0..NUM_SAMPLES).map(|_| None).collect();
    for round in 0..3 {
        eprintln!("P5b 216 phase: round {round} window/register/preload");
        if round > 0 {
            let (start, end) = if round == 1 { (32, 3072) } else { (64, 2048) };
            for id in 0..NUM_SAMPLES {
                let window = prepare_window_with_producer(
                    &h.engine,
                    id,
                    WindowRequest {
                        loop_region: Some((
                            f64::from(start) / f64::from(RATE),
                            Some(f64::from(end) / f64::from(RATE)),
                        )),
                        key_lock: Some(false),
                        ..WindowRequest::default()
                    },
                    h.producer.clone(),
                )
                .unwrap();
                wait_phase("216 window enqueue", id, || {
                    h.consumer.peek().is_ok() || window.error().unwrap().is_some()
                });
                assert_eq!(
                    h.callback.drain(&mut h.consumer),
                    1,
                    "{:?}",
                    window.error().unwrap()
                );
                assert_eq!(
                    window.publication_status(),
                    "accepted",
                    "{:?}",
                    window.error().unwrap()
                );
                assert!(window.is_current());
                reconcile(&h.engine).unwrap();
            }
            assert!(
                previous_receipts
                    .iter()
                    .all(|receipt| !receipt.is_current())
            );
            previous_receipts.clear();
        }
        let pair = h.prepare(0, &h.ticket(0), false, None);
        let _saved = h.save(&pair);
        let mut selections = Vec::new();
        let mut registration_acks = 0;
        let mut selection_reuses = 0;
        for (id, previous_selection) in previous_selections.iter().enumerate() {
            let binding_before = crate::audio_engine::input_runtime_binding::capture(&h.engine, id)
                .unwrap()
                .unwrap();
            let identity_before = h.callback.mixer.stems_for_measurement()[id]
                .as_ref()
                .map(|set| set.complete_set_identity.clone());
            let source = h.ticket(id);
            let selected = h
                .engine
                .register_committed_stem_pair_with_producer(&pair, &source, &h.producer)
                .unwrap();
            if selected.publication_status() == "accepted" {
                assert!(
                    round > 0,
                    "initial selected set requires its own Register ACK"
                );
                assert!(
                    previous_selection
                        .as_ref()
                        .expect("later round retains its existing selection")
                        .is_current()
                );
                assert!(
                    h.consumer.peek().is_err(),
                    "same-set reuse enqueued no new Register"
                );
                let held = identity_before
                    .as_ref()
                    .expect("window retained actual same-set readers");
                assert_eq!(**held, *pair._identity);
                let current = h.callback.mixer.stems_for_measurement()[id]
                    .as_ref()
                    .unwrap();
                assert!(
                    Arc::ptr_eq(held, &current.complete_set_identity),
                    "reuse preserves the actual accepted Native set identity"
                );
                let binding_after =
                    crate::audio_engine::input_runtime_binding::capture(&h.engine, id)
                        .unwrap()
                        .unwrap();
                assert_eq!(binding_after.binding, binding_before.binding);
                assert_eq!(
                    binding_after.source_generation,
                    binding_before.source_generation
                );
                selection_reuses += 1;
            } else {
                assert_eq!(selected.publication_status(), "pending");
                assert_eq!(h.callback.drain(&mut h.consumer), 1);
                registration_acks += 1;
            }
            assert_eq!(selected.publication_status(), "accepted");
            assert!(selected.is_current());
            selections.push(selected);
        }
        if round == 0 {
            assert_eq!(registration_acks, NUM_SAMPLES);
        } else {
            assert_eq!(selection_reuses, NUM_SAMPLES);
        }
        eprintln!(
            "P5b 216 round {round}: fresh Register ACKs={registration_acks}, existing selection reuses={selection_reuses}"
        );
        drop(pair);
        for (id, selected) in selections.iter().enumerate() {
            let receipt = h
                .engine
                .request_committed_stem_residency_with_interest_and_producer(
                    selected,
                    "full_mix",
                    15,
                    "preload",
                    &h.producer,
                )
                .unwrap();
            assert_eq!(h.callback.drain(&mut h.consumer), 1);
            wait_phase("216 policy enqueue", id, || {
                h.consumer.peek().is_ok() || receipt.error().is_some()
            });
            assert_eq!(
                h.callback.drain(&mut h.consumer),
                1,
                "pad {id}: {:?}",
                receipt.error()
            );
            assert_eq!(
                receipt.residency_status(),
                "accepted",
                "pad {id}: {:?}",
                receipt.error()
            );
            assert!(receipt.is_current());
            assert_eq!(receipt.effective_mode_status(), "not_requested");
            assert!(h.callback.mixer.stems_for_measurement()[id].is_some());
            assert_eq!(
                h.callback.mixer.stem_demand_for_measurement(id).0,
                StemMixMode::FullMix
            );
            previous_receipts.push(receipt);
            reconcile(&h.engine).unwrap();
        }
        assert_eq!(previous_receipts.len(), NUM_SAMPLES);
        assert!(previous_receipts.iter().all(ResidentStemTicket::is_current));
        let (bytes, count) = actual_bank_pcm(&h);
        let deadline = Instant::now() + Duration::from_secs(10);
        wait_phase("216 final exact account snapshot", round, || {
            let status = h
                .engine
                .project_assets
                .resident_budget_status(&h.engine.resident_account)
                .unwrap();
            if Instant::now() >= deadline {
                panic!(
                    "round {round}: actual banks bytes={bytes} count={count}; ledger bytes={} count={} pending_bytes={} pending_count={}; material PCM address={} bytes={}",
                    status.held_unique_pcm_bytes,
                    status.unique_backing_count,
                    status.pending_resident_bytes,
                    status.pending_reservation_count,
                    h.material.sample.samples.as_ptr() as usize,
                    h.material.sample.samples.len() * 4
                );
            }
            status.pending_reservation_count == 0
                && status.held_unique_pcm_bytes == bytes
                && status.unique_backing_count == count
        });
        let status = h
            .engine
            .project_assets
            .resident_budget_status(&h.engine.resident_account)
            .unwrap();
        assert_eq!(status.admitted_total_bytes, bytes);
        assert!(!status.over_budget);
        if round == 0 {
            assert!(
                count < NUM_SAMPLES * 5,
                "actual shared backing is charged once"
            );
        } else {
            assert!(
                count >= NUM_SAMPLES,
                "actual distinct finite source allocations stay charged"
            );
        }
        previous_selections = selections.into_iter().map(Some).collect();
    }
    let status = h
        .engine
        .project_assets
        .resident_budget_status(&h.engine.resident_account)
        .unwrap();
    let _pressure = h
        .engine
        .project_assets
        .reserve_resident(
            &h.engine.resident_account,
            status.available_bytes - 16,
            &[],
            0,
        )
        .unwrap();
    let last = NUM_SAMPLES - 1;
    let before = h.callback.mixer.bank_for_measurement()[last]
        .as_ref()
        .unwrap()
        .samples
        .clone();
    let rejected = prepare_window_with_producer(
        &h.engine,
        last,
        WindowRequest {
            loop_region: Some((96.0 / f64::from(RATE), Some(1024.0 / f64::from(RATE)))),
            key_lock: Some(false),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    wait_until(|| rejected.error().unwrap().is_some());
    assert!(
        rejected
            .error()
            .unwrap()
            .unwrap()
            .starts_with("[resident_pcm_budget_exceeded]")
    );
    assert!(h.consumer.peek().is_err());
    assert!(Arc::ptr_eq(
        &before,
        &h.callback.mixer.bank_for_measurement()[last]
            .as_ref()
            .unwrap()
            .samples
    ));
    assert!(previous_receipts.iter().all(ResidentStemTicket::is_current));
    drop(before);
    drop(_pressure);

    // Final bank readiness was measured with all 216 owners retained. Now use
    // the real ordered unload/reconcile/FIFO path to prove the final readers
    // and their scoped accounting end together; weak probes grant no ownership.
    let final_readers = actual_bank_reader_probes(&h);
    for id in 0..NUM_SAMPLES {
        crate::audio_engine::cold_load::unload_for_producer(&h.engine, id, &h.producer).unwrap();
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert!(h.callback.mixer.bank_for_measurement()[id].is_none());
        assert!(h.callback.mixer.stems_for_measurement()[id].is_none());
        reconcile(&h.engine).unwrap();
    }
    assert!(
        previous_receipts
            .iter()
            .all(|receipt| !receipt.is_current())
    );
    previous_receipts.clear();
    wait_phase(
        "final actual readers and scoped account retirement",
        NUM_SAMPLES - 1,
        || {
            let status = h
                .engine
                .project_assets
                .resident_budget_status(&h.engine.resident_account)
                .unwrap();
            final_readers.iter().all(|pcm| pcm.strong_count() == 0)
                && status.held_unique_pcm_bytes == 0
                && status.pending_resident_bytes == 0
                && status.unique_backing_count == 0
                && status.pending_reservation_count == 0
        },
    );
    h.callback.retirement.actual.take();
    drop(retirement_worker);
}
