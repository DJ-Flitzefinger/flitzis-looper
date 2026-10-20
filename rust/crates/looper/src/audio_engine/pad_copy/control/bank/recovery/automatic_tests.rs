//! Actual QM saved evidence and callback adoption, using only owned generated PCM.
use super::*;
use crate::audio_engine::constant_timing;
use crate::audio_engine::native_owned_work::{PreparationObservationScope, WorkPoint};
use flitzis_looper_analysis::tempo_acceptance::{
    IndependentTimingOrigin, TimingAcceptanceDecision,
};
use std::sync::atomic::AtomicUsize;

const AUTOMATIC_SLOT: usize = 36;
const PULSE_PERIOD: f64 = 60.0 / 119.999;

fn write_automatic_pulses(path: &Path) -> usize {
    let frames = (64.0 * PULSE_PERIOD * f64::from(RATE)).ceil() as usize;
    let mut pcm = vec![0_i16; frames * 2];
    // The four-sample 8-kHz pulse used by the established QM fixture, held
    // for six samples per value at this fixture's actual 48-kHz stereo rate.
    assert_eq!(RATE, 48_000);
    for quarter in 0..64 {
        let start = (quarter as f64 * PULSE_PERIOD * f64::from(RATE)).round() as usize;
        for offset in 0..24 {
            let value = [16_384_i16, -8_192, 4_096, -2_048][offset / 6];
            for channel in 0..2 {
                pcm[(start + offset) * 2 + channel] = value;
            }
        }
    }
    let data_bytes = u32::try_from(pcm.len() * 2).unwrap();
    let mut encoded = Vec::with_capacity(44 + data_bytes as usize);
    encoded.extend_from_slice(b"RIFF");
    encoded.extend_from_slice(&(36_u32 + data_bytes).to_le_bytes());
    encoded.extend_from_slice(b"WAVEfmt ");
    encoded.extend_from_slice(&16_u32.to_le_bytes());
    encoded.extend_from_slice(&1_u16.to_le_bytes());
    encoded.extend_from_slice(&2_u16.to_le_bytes());
    encoded.extend_from_slice(&RATE.to_le_bytes());
    encoded.extend_from_slice(&(RATE * 4).to_le_bytes());
    encoded.extend_from_slice(&4_u16.to_le_bytes());
    encoded.extend_from_slice(&16_u16.to_le_bytes());
    encoded.extend_from_slice(b"data");
    encoded.extend_from_slice(&data_bytes.to_le_bytes());
    for value in pcm {
        encoded.extend_from_slice(&value.to_le_bytes());
    }
    fs::write(path, encoded).unwrap();
    frames
}

fn restored_automatic_image(fixture: &mut Fixture) -> String {
    let root = fixture.directory.path().join("samples");
    let original = root.join("recovered-automatic-pulses.wav");
    let frames = write_automatic_pulses(&original);
    let material =
        prepare_material_complete_for_test(&root, &original, RATE, 2, &|| false).unwrap();
    assert_eq!(material.sample.frame_count(), frames);
    let metadata = material.metadata();
    let prepared = fixture.with_engine(|engine| {
        prepared_for_test(
            engine,
            PreparedMigrationMaterial::from_current(
                &root,
                material.sample.clone(),
                material.lease.clone(),
            )
            .unwrap(),
            &root,
        )
        .unwrap()
    });
    let loaded = fixture.with_engine(|engine| {
        adopt_for_format(
            engine,
            AUTOMATIC_SLOT,
            &prepared,
            fixture.producer.clone(),
            (2, RATE, root.clone()),
        )
        .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        fixture.callback.drain();
        if fixture
            .with_engine(|engine| engine.cold_loading[AUTOMATIC_SLOT].load(Ordering::Acquire) == 0)
        {
            break;
        }
        assert!(Instant::now() < deadline, "owned Automatic source cold ACK");
        std::thread::sleep(Duration::from_millis(1));
    }
    fixture.with_engine(|engine| {
        let mut success = false;
        while let Ok(event) = engine
            .loader_rx
            .lock()
            .unwrap()
            .try_recv()
            .map(LoaderEvent::into_test_payload)
        {
            match event {
                LoaderEvent::Success { request_id, .. } if request_id == loaded.request_id() => {
                    success = true;
                }
                LoaderEvent::Error { error, .. } => panic!("owned Automatic source: {error:?}"),
                _ => {}
            }
        }
        assert!(success && loaded.is_current().unwrap());
        prepared.release_preparation().unwrap();
        engine
            .set_pad_loop_region(
                AUTOMATIC_SLOT,
                START as f64 / f64::from(RATE),
                Some(END as f64 / f64::from(RATE)),
            )
            .unwrap();
        engine.set_pad_gain(AUTOMATIC_SLOT, -7.0).unwrap();
        engine.set_pad_eq(AUTOMATIC_SLOT, 3.0, -4.0, 1.5).unwrap();
        engine.set_pad_key_lock(AUTOMATIC_SLOT, false).unwrap();
        engine
            .set_pad_timing_intent(AUTOMATIC_SLOT, "automatic")
            .unwrap();
    });
    fixture.callback.drain();
    let ticket = Python::attach(|py| {
        AudioEngine::prepare_constant_timing(
            fixture.engine.bind(py),
            py,
            AUTOMATIC_SLOT,
            0.05,
            "owned generated quarter-pulse matching tolerance".into(),
        )
        .unwrap()
    });
    let beats: Vec<f64> = Python::attach(|py| {
        let metadata = ticket.metadata(py).unwrap();
        metadata
            .bind(py)
            .get_item("beat_seconds")
            .unwrap()
            .extract()
            .unwrap()
    });
    assert!(
        beats.len() >= 16,
        "actual QM must retain real detected pulse evidence"
    );
    let counts: Vec<Option<i64>> = beats
        .iter()
        .map(|seconds| Some((seconds / PULSE_PERIOD).round() as i64))
        .collect();
    let hypotheses = json!([{
        "id":"owned-recovered-bank-pulse-quarters",
        "provenance":"known independently generated pulse period; nearest quarter association",
        "verification":"verified", "quarter_note_denominator":1, "quarter_counts":counts,
    }])
    .to_string();
    fixture.with_engine(|engine| {
        constant_timing::publish(
            engine,
            &fixture.producer,
            &ticket,
            &hypotheses,
            IndependentTimingOrigin {
                seconds: -0.125_012_3,
                provenance: "independently chosen signed generated-fixture origin".into(),
            },
            TimingAcceptanceDecision {
                policy_version: "explicit-owned-recovered-bank-fixture-v1".into(),
                provenance: "generated quarter truth; no device or musical acceptance".into(),
            },
        )
        .unwrap();
    });
    assert_eq!(ticket.publication_status().unwrap(), "pending");
    assert_eq!(fixture.callback.drain(), 1);
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    fixture.with_engine(|engine| {
        let binding = engine
            .current_input_runtime_pad_binding(AUTOMATIC_SLOT)
            .unwrap()
            .unwrap();
        assert!(binding.is_current() && binding.available());
        assert!(
            engine
                .input_runtime_ownership
                .binding_source_adoption_accepted(AUTOMATIC_SLOT, binding.binding,)
        );
        assert_ne!(
            engine
                .current_timing_acknowledgements
                .current_for_slot(&engine.input_runtime_ownership, AUTOMATIC_SLOT,),
            0
        );
    });
    // Export exactly once from the genuine current verifier after its real
    // callback ACK. The recovery query below receives this fixed saved codec.
    let saved = Python::attach(|py| {
        AudioEngine::export_current_constant_timing(
            fixture.engine.bind(py),
            py,
            AUTOMATIC_SLOT,
            material.lease.original_path.to_string_lossy().into_owned(),
        )
        .unwrap()
        .unwrap()
    });
    let saved: Value = serde_json::from_str(&saved).unwrap();
    assert!(saved["record"]["evidence"]["qm"].is_object());
    assert_eq!(
        saved["record"]["evidence"]["binding"]["frame_count"],
        json!(frames)
    );
    let period = f64::from_bits(
        u64::from_str_radix(saved["record"]["period_bits"].as_str().unwrap(), 16).unwrap(),
    );
    let mut envelope: Value = serde_json::from_str(&fixture.envelope()).unwrap();
    envelope["snapshot"]["source_path"] = metadata["new_reference"].clone();
    envelope["snapshot"]["source_identity"]["material_id"] = metadata["material_id"].clone();
    envelope["snapshot"]["source_identity"]["material_creation"] =
        metadata["material_creation"].clone();
    envelope["snapshot"]["duration"] = json!(frames as f64 / f64::from(RATE));
    envelope["snapshot"]["analysis"] = json!({
        "bpm":60.0 / period, "key":"", "beat_grid":{"beats":[], "downbeats":[], "bars":[]},
        "accepted_timing":saved,
    });
    envelope["snapshot"]["manual_bpm"] = Value::Null;
    envelope["snapshot"]["timing_intent"] = json!("automatic");
    envelope["snapshot"]["stem_intent"]["enabled_mask"] = json!(15);
    envelope.to_string()
}

#[test]
fn actual_qm_saved_automatic_image_accepts_and_changed_saved_period_rejects_before_reservation() {
    isolated(
        "bank_public_tests::recovered_bank_tests::automatic_tests::actual_qm_saved_automatic_image_accepts_and_changed_saved_period_rejects_before_reservation",
        || {
            let mut fixture = recovery_fixture(false, true);
            let envelope = restored_automatic_image(&mut fixture);
            let original = view(&fixture, 1);
            let before = snapshots(&fixture);
            let requests =
                fixture.with_engine(|engine| engine.pad_request_ids.lock().unwrap().clone());
            let lease = fixture.with_engine(|engine| {
                engine.cold_leases.lock().unwrap()[AUTOMATIC_SLOT]
                    .as_ref()
                    .unwrap()
                    .clone()
            });
            let assignments = lease.cache_assignment_count();
            let outside = voice(&fixture, 0);
            let frame = fixture.callback.frame;
            assert!(fixture.callback.controls.is_empty());
            all_unreserved(&fixture);
            let work = Arc::new(AtomicUsize::new(0));
            let observed = work.clone();
            let _scope = PreparationObservationScope::install(Some(Box::new(move |point| {
                if matches!(
                    point,
                    WorkPoint::TimingPcm | WorkPoint::SavedTimingSource | WorkPoint::AcceptedTiming
                ) {
                    observed.fetch_add(1, Ordering::AcqRel);
                }
            })));
            let mut modified: Value = serde_json::from_str(&envelope).unwrap();
            let bits = u64::from_str_radix(
                modified["snapshot"]["analysis"]["accepted_timing"]["record"]["period_bits"]
                    .as_str()
                    .unwrap(),
                16,
            )
            .unwrap();
            modified["snapshot"]["analysis"]["accepted_timing"]["record"]["period_bits"] =
                json!(format!("{:016x}", (f64::from_bits(bits) + 0.01).to_bits()));
            let mut rejected = vec![None; 36];
            rejected[0] = Some(modified.to_string());
            fixture.with_engine(|engine| {
                let error = engine
                    .capture_recovered_bank(&original, rejected)
                    .err()
                    .unwrap();
                assert!(error.to_string().contains("Automatic evidence differs"));
                assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests);
            });
            all_unreserved(&fixture);
            unchanged(&fixture, &before);
            assert!(fixture.callback.controls.is_empty());
            assert_eq!(lease.cache_assignment_count(), assignments);
            assert_eq!(work.load(Ordering::Acquire), 0);
            let mut matching = vec![None; 36];
            matching[0] = Some(envelope);
            let acceptance = accept(&fixture, &original, matching);
            assert_eq!(status(&acceptance)["status"], json!("pending"));
            assert_eq!(fixture.callback.controls.slots(), 1);
            fixture.with_engine(|engine| {
                assert!(!engine.validate_recovered_bank(&acceptance).unwrap())
            });
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(status(&acceptance)["status"], json!("accepted"));
            assert_eq!(status(&acceptance)["accepted_nonce"], json!(1));
            assert_eq!(status(&acceptance)["accepted_output_frame"], json!(frame));
            fixture.with_engine(|engine| {
                assert!(engine.validate_recovered_bank(&acceptance).unwrap());
                assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests);
            });
            assert_eq!(
                work.load(Ordering::Acquire),
                0,
                "query cannot prepare PCM or QM evidence"
            );
            unchanged(&fixture, &before);
            assert_eq!(voice(&fixture, 0), outside);
            assert_eq!(fixture.callback.frame, frame);
            release(&fixture, &acceptance);
        },
    );
}
