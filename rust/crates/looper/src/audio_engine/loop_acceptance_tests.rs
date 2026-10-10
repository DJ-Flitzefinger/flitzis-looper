//! Synthetic source fixtures exercise actual native publication, render and current guards.
//! No device is opened and none of these observations constitute device acceptance.

use super::*;
use crate::audio_engine::loop_acceptance;

fn fixture() -> (AudioEngine, RtMixer) {
    let engine = test_engine();
    let (producer, mut consumer) = queue(2);
    let mut mixer = acknowledged_mixer(&engine);
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    publish(
        &engine,
        &producer,
        &synthetic_ticket(&engine),
        &hypotheses(),
        origin(),
        decision(),
    )
    .unwrap();
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    mixer.set_pad_loop_region(0, 0.0, Some(PERIOD / 16.0));
    mixer.set_speed(0.73);
    mixer.play_sample(0, 1.0);
    let mut output = [0.0; 512];
    let mut peaks = [0.0; NUM_SAMPLES];
    mixer.render_at_output_frame(0, &mut output, &mut peaks);
    (engine, mixer)
}

fn observe(engine: &AudioEngine, mixer: &RtMixer) -> u64 {
    let request = engine.loop_acceptance.request(0).unwrap();
    engine
        .loop_acceptance
        .observe(mixer, engine.input_clock.capture_ns(), 512, 512);
    request
}

fn assert_status(engine: &AudioEngine, request: u64, available: bool) {
    Python::attach(|py| {
        let response = loop_acceptance::metadata(engine, py, 0, request)
            .unwrap()
            .unwrap();
        let dict = response.bind(py).cast::<PyDict>().unwrap();
        assert_eq!(
            dict.get_item("current_acknowledged")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap(),
            available
        );
        assert_eq!(
            dict.get_item("status")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            if available {
                "available"
            } else {
                "unavailable"
            }
        );
    });
}

#[test]
fn effective_fractional_voice_snapshot_preserves_progression_and_full_current_ack() {
    let (engine, mixer) = fixture();
    let before = mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap()
        .source_playback;
    let request = observe(&engine, &mixer);
    assert_status(&engine, request, true);
    let after = mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap()
        .source_playback;
    assert!(before.matches_exact(&after));
    Python::attach(|py| {
        let response = loop_acceptance::metadata(&engine, py, 0, request)
            .unwrap()
            .unwrap();
        let dict = response.bind(py).cast::<PyDict>().unwrap();
        let get = |key| dict.get_item(key).unwrap().unwrap();
        assert_eq!(get("output_frame").extract::<u64>().unwrap(), 512);
        assert_eq!(
            get("applied_source_rate")
                .extract::<f64>()
                .unwrap()
                .to_bits(),
            0.73_f64.to_bits()
        );
        assert!(get("rate_settled").extract::<bool>().unwrap());
        assert_eq!(
            get("musical_loop_period_frames")
                .extract::<f64>()
                .unwrap()
                .to_bits(),
            before.loop_period().unwrap().to_bits()
        );
        assert_eq!(
            get("source_fraction").extract::<f64>().unwrap().to_bits(),
            before.position().fraction.to_bits()
        );
        let binding = get("current_binding");
        let accepted = binding
            .cast::<PyDict>()
            .unwrap()
            .get_item("accepted_timing")
            .unwrap()
            .unwrap();
        let revision = accepted
            .cast::<PyDict>()
            .unwrap()
            .get_item("revision")
            .unwrap()
            .unwrap()
            .extract::<String>()
            .unwrap();
        assert_eq!(
            get("effective_accepted_revision")
                .extract::<String>()
                .unwrap(),
            revision
        );
        assert_eq!(
            get("eq_applied_normalized").extract::<Vec<f64>>().unwrap(),
            vec![0.5; 3]
        );
    });
}

#[test]
fn effective_snapshot_rejects_authority_edit_and_source_replacement_without_relabelling() {
    let (engine, mut mixer) = fixture();
    let request = observe(&engine, &mixer);
    let authority = engine.input_runtime_ownership.next_authority(0).unwrap();
    engine.input_runtime_ownership.revoke(0, authority);
    assert_status(&engine, request, false);

    let replacement = SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from(vec![0.25; 8000]),
    };
    engine.sample_cache.lock().unwrap()[0] = Some(replacement.clone());
    engine.loaded_source_generations.lock().unwrap()[0] = (8, RATE);
    engine.loaded_source_digests.lock().unwrap()[0] = Some("b".repeat(64));
    engine
        .input_runtime_ownership
        .publish_source(0, &replacement, RATE, 8);
    mixer.load_sample(0, replacement);
    let request = observe(&engine, &mixer);
    assert_status(&engine, request, false);
    assert!(mixer.voices.iter().any(|voice| voice.active));
}

#[test]
fn effective_snapshot_rechecks_fence_generation_even_at_identical_pointer_shape_and_ack() {
    let (engine, mixer) = fixture();
    let request = observe(&engine, &mixer);
    let source = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    // Model the immediate source fence publishing before cache/record lookup catches up.
    // This is observer guard coverage, not C1 immutable decoder/copy-first ABA acceptance.
    engine
        .input_runtime_ownership
        .publish_source(0, &source, RATE, 8);
    assert_status(&engine, request, false);
}

#[test]
fn effective_snapshot_rejects_new_full_revision_even_when_period_is_unchanged() {
    let (engine, mut mixer) = fixture();
    let old_request = observe(&engine, &mixer);
    let (producer, mut consumer) = queue(2);
    let mut changed = decision();
    changed
        .provenance
        .push_str("; independently distinct decision identity");
    publish(
        &engine,
        &producer,
        &synthetic_ticket(&engine),
        &hypotheses(),
        origin(),
        changed,
    )
    .unwrap();
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    assert_status(&engine, old_request, false);
    let before_render_request = observe(&engine, &mixer);
    assert_status(&engine, before_render_request, false);
    mixer.render_at_output_frame(512, &mut [0.0; 512], &mut [0.0; NUM_SAMPLES]);
    let new_request = observe(&engine, &mixer);
    assert_status(&engine, new_request, true);
}

#[test]
fn effective_snapshot_reports_ambiguous_voice_and_stale_observation_explicitly() {
    let (engine, mut mixer) = fixture();
    let request = engine.loop_acceptance.request(0).unwrap();
    engine.loop_acceptance.observe(&mixer, u64::MAX, 512, 512);
    assert_status(&engine, request, false);
    // Defensive fixture: two actual slots retaining this source must never select one silently.
    let active = mixer.voices.iter().find(|voice| voice.active).unwrap();
    let config = crate::audio_engine::voice_slot::VoiceStartConfig {
        sample_id: 0,
        sample: active.sample.clone().unwrap(),
        initial_frame_pos: 0,
        volume: 1.0,
        initial_tempo_ratio: 0.73,
        start_output_frame: Some(512),
        source_timing: active.source_timing,
        source_admission: None,
    };
    mixer.voices[1].start_rt(config, &mut ImmediateAudioBufferRetirement);
    let request = observe(&engine, &mixer);
    Python::attach(|py| {
        let response = loop_acceptance::metadata(&engine, py, 0, request)
            .unwrap()
            .unwrap();
        let dict = response.bind(py).cast::<PyDict>().unwrap();
        assert_eq!(
            dict.get_item("reason")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "multiple effective voices for pad"
        );
    });
    assert_status(&engine, request, false);
}

#[test]
fn effective_snapshot_records_applied_rate_and_dsp_smoothing_separately_from_targets() {
    let (engine, mut mixer) = fixture();
    mixer.set_speed(1.8);
    mixer.set_pad_eq(0, -60.0, 0.0, 6.0);
    mixer.render_at_output_frame(512, &mut [0.0; 1], &mut [0.0; NUM_SAMPLES]);
    let request = observe(&engine, &mixer);
    Python::attach(|py| {
        let response = loop_acceptance::metadata(&engine, py, 0, request)
            .unwrap()
            .unwrap();
        let dict = response.bind(py).cast::<PyDict>().unwrap();
        let get = |key| dict.get_item(key).unwrap().unwrap();
        assert!(!get("rate_settled").extract::<bool>().unwrap());
        assert_ne!(
            get("applied_source_rate").extract::<f64>().unwrap(),
            get("target_source_rate").extract::<f64>().unwrap()
        );
        assert_ne!(
            get("eq_applied_normalized").extract::<Vec<f64>>().unwrap(),
            get("eq_target_normalized").extract::<Vec<f64>>().unwrap()
        );
    });
}
