//! Genuine mapped phase custody refuses stems, then the same selection recovers after OFF.

use super::*;
use crate::audio_engine::source_beatmap::editor;
use crate::audio_engine::{LoaderEventDelivery, OfflineAnalysisJob, SourceBeatmap};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::time::{Duration, Instant};

fn original_map_delivery(p: &Productive) -> (OfflineAnalysisJob, LoaderEventDelivery) {
    let manifest: serde_json::Value =
        serde_json::from_str(&p.h.engine.cold_source_manifest(0).unwrap().unwrap()).unwrap();
    let map = SourceBeatmap::manual(
        manifest["descriptor"]["decoder"].clone(),
        0.0,
        46.875 / f64::from(RATE),
    )
    .unwrap();
    let mut value: serde_json::Value = serde_json::from_str(map.canonical_record()).unwrap();
    let template = value["record"]["accepted"]["anchors"][0].clone();
    value["record"]["accepted"]["anchors"] = serde_json::Value::Array(
        [(0.0_f64, 0.0_f64), (1.0 / 16.0, 64.0)]
            .into_iter()
            .enumerate()
            .map(|(index, (seconds, beat))| {
                let mut anchor = template.clone();
                anchor["id"] = json!(format!("committed-mapped-phase-{index}"));
                anchor["source_seconds_bits"] = json!(format!("{:016x}", seconds.to_bits()));
                anchor["beat_bits"] = json!(format!("{:016x}", beat.to_bits()));
                anchor
            })
            .collect(),
    );
    let map = editor::seal(value).unwrap();
    let job =
        p.h.engine
            .begin_source_beatmap_store_async(0, map.canonical_record().into())
            .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let terminal = Python::attach(|py| {
            job.source_beatmap_async_status(py)
                .unwrap()
                .unwrap()
                .bind(py)
                .get_item("terminal")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        });
        if terminal {
            break;
        }
        assert!(Instant::now() < deadline, "original map worker timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
    let delivery = Python::attach(|py| {
        while let Some(event) = p.h.engine.poll_loader_events(py).unwrap() {
            let event = event.bind(py).cast::<PyDict>().unwrap();
            if event
                .get_item("type")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap()
                == "source_beatmap_completed"
            {
                let owner = event.get_item("_native_event_owner").unwrap().unwrap();
                let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
                assert!(
                    job.resolve_source_beatmap_artifact_completion(&delivery)
                        .unwrap()
                        .is_some()
                );
                return delivery.clone();
            }
        }
        panic!("original private map delivery required");
    });
    (job, delivery)
}

fn publish_mapped_policy(
    p: &Productive,
    delivery: &LoaderEventDelivery,
    sync: bool,
    revision: u64,
) {
    let bindings = (0..NUM_SAMPLES)
        .map(|id| {
            (id == 0).then(|| {
                p.h.engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    starts[0] = p.loop_region.start as f64 / f64::from(RATE);
    ends[0] = Some(p.loop_region.end as f64 / f64::from(RATE));
    let mut rows = vec![SourceMapRow::default(); NUM_SAMPLES];
    rows[0] = SourceMapRow {
        sync_requested: true,
        intent_revision: 7,
        delivery: Some(delivery.clone()),
        exact_quarters: Some(32.0),
    };
    p.h.engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_policy(
            true,
            (0..NUM_SAMPLES).map(|id| id == 0).collect(),
            starts,
            ends,
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                PitchPadProjection {
                    sync: true,
                    key_lock: false,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(rows),
            GlobalTimingPolicy::parse(Some(
                &json!({
                    "schema_version": 1, "sync": sync, "realtime_bpm": true,
                    "revision": revision,
                })
                .to_string(),
            ))
            .unwrap(),
        )
        .unwrap();
}

#[test]
fn correcting_mapped_phase_refuses_committed_stems_then_same_selection_progresses_after_off() {
    let mut p = Productive::new(false, 1.0);
    assert_eq!(RATE, 48_000);
    assert_eq!(p.loop_region.len(), 1500);
    assert_eq!(p.loop_region.start, 17);
    p.h.callback
        .transport
        .advance_by_rendered_frames(p.frame as usize);
    p.h.engine.test_output_format = Some((RATE, 2));
    p.h.engine.input_runtime = Some(InputRuntime::new_with_ownership(
        p.h.producer.clone(),
        InputClock::new(),
        p.h.engine.input_runtime_ownership.clone(),
    ));
    p.h.engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_render_format(RATE, 2)
        .unwrap();
    let (producer, mut parameters) = rtrb::RingBuffer::new(16);
    p.h.engine.test_parameter_producer = Some(Arc::new(Mutex::new(producer)));
    let period = 46.875 / f64::from(RATE);
    p.h.engine.set_pad_bpm(0, Some(60.0 / period)).unwrap();
    p.h.engine.set_pad_timing_metadata(0, 0.0).unwrap();
    {
        let callback = p.h.callback.as_mut();
        drain_parameter_messages(
            &mut parameters,
            &mut callback.mixer,
            &mut callback.transport,
        );
        callback.mixer.set_bpm_lock(true);
        callback.mixer.set_master_period(period);
        assert!(callback.transport.set_master_period(period));
        callback.transport.complete_bootstrap();
    }
    begin_qualified_phase_fixture(&mut p);
    let (_map_job, delivery) = original_map_delivery(&p);
    publish_mapped_policy(&p, &delivery, true, 2);
    let original = policy_effect(&mut p);
    assert_eq!(original.observation().unwrap().state, "applied");
    assert!(voice(&p.h.callback.mixer).source_playback.is_mapped());
    assert!(!voice(&p.h.callback.mixer).source_playback.is_mapped_phase());
    let phase = policy_effect(&mut p);
    assert!(Arc::ptr_eq(&original, &phase));
    assert_eq!(phase.observation().unwrap().state, "applied");
    let clock = voice(&p.h.callback.mixer).source_playback;
    assert!(clock.is_mapped_phase());
    assert!(
        !clock
            .mapped_phase_settled(
                voice(&p.h.callback.mixer)
                    .mapped_start
                    .as_ref()
                    .unwrap()
                    .mapped_geometry()
                    .unwrap(),
            )
            .unwrap()
    );
    let generation = voice(&p.h.callback.mixer).generation;
    let sample = voice(&p.h.callback.mixer).sample.clone().unwrap();
    let native = voice(&p.h.callback.mixer).stretch.native_state_address();
    let transition = p.h.callback.mixer.stem_transition_for_measurement(0);
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    let refused = p.request("all_stems", 0b1010);
    drain_actual(&mut p);
    wait_until(|| refused.error().is_some());
    assert_eq!(refused.residency_status(), "rejected");
    assert_eq!(refused.effective_mode_status(), "rejected");
    assert!(
        voice(&p.h.callback.mixer)
            .source_playback
            .matches_exact(&clock)
    );
    assert_eq!(voice(&p.h.callback.mixer).generation, generation);
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.native_state_address(),
        native
    );
    assert!(
        voice(&p.h.callback.mixer)
            .sample
            .as_ref()
            .unwrap()
            .same_window(&sample)
    );
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    assert!(
        p.h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    assert_eq!(
        p.h.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    let mut expected = clock;
    assert!(expected.advance(64));
    let output = render_actual(&mut p, 64);
    assert!(output.iter().any(|value| value.abs() > 0.04));
    assert!(
        voice(&p.h.callback.mixer)
            .source_playback
            .matches_exact(&expected)
    );
    publish_mapped_policy(&p, &delivery, false, 3);
    let off = policy_effect(&mut p);
    assert_eq!(off.observation().unwrap().state, "applied");
    assert!(!voice(&p.h.callback.mixer).source_playback.is_mapped());
    assert!(
        !voice(&p.h.callback.mixer)
            .source_playback
            .has_phase_program()
    );
    render_actual(&mut p, 1);
    let fresh = p.request("all_stems", 0b1010);
    capture_then_wait_at_actual(&mut p, &fresh);
    assert_eq!(drain_actual(&mut p), 1);
    assert_eq!(fresh.residency_status(), "accepted", "{:?}", fresh.error());
    assert!(fresh.is_current() && p.selected.is_current());
    assert_eq!(fresh.effective_mode_status(), "transitioning");
    render_actual(&mut p, 128);
    assert_eq!(
        fresh.effective_mode_status(),
        "effective",
        "{:?}",
        fresh.error()
    );
    assert_eq!(
        p.h.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::AllStems
    );
    assert_eq!(voice(&p.h.callback.mixer).generation, generation);
    let stems = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(stems.stems.iter().all(|stem| stem.same_window(&sample)));
    assert!(
        !p.h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
}
