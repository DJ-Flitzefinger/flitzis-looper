//! Existing prepared-stem custody refuses static history and recovers after OFF.

#[path = "committed_stem_variable_phase_tests.rs"]
mod variable_phase_tests;

use super::*;
use crate::audio_engine::audio_stream::{
    drain_control_messages, drain_parameter_messages, render_scheduled_audio,
};
use crate::audio_engine::input_mapping::{InputRuntime, PitchPadProjection, RuntimePolicySnapshot};
use crate::audio_engine::source_map_admission::SourceMapRow;
use crate::audio_engine::source_sync_policy::GlobalTimingPolicy;
use crate::audio_engine::timing::InputClock;

fn publish_policy(p: &Productive, sync: bool, revision: u64) {
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
    rows[0].sync_requested = true;
    rows[0].intent_revision = 7;
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
                &serde_json::json!({
                    "schema_version": 1, "sync": sync, "realtime_bpm": false, "revision": revision,
                })
                .to_string(),
            ))
            .unwrap(),
        )
        .unwrap();
}

// These two phase fixtures advance the real timeline. The shared Productive
// harness intentionally retains its legacy frame-zero drain for unrelated tests.
fn drain_actual(p: &mut Productive) -> usize {
    let callback = p.h.callback.as_mut();
    assert_eq!(callback.transport.output_frame(), p.frame);
    drain_control_messages(
        &mut p.h.consumer,
        &mut callback.scheduler,
        p.frame,
        &mut callback.quantization,
        &mut callback.transport,
        &mut callback.mixer,
        &mut callback.feedback,
        &mut callback.retirement,
    )
}

fn capture_then_wait_at_actual(p: &mut Productive, ticket: &ResidentStemTicket) {
    assert_eq!(ticket.effective_mode_status(), "waiting");
    assert_eq!(drain_actual(p), 1);
    wait_until(|| {
        (p.h.consumer.peek().is_ok() || ticket.error().is_some()) && p.h.producer.try_lock().is_ok()
    });
    assert_eq!(ticket.residency_status(), "pending", "{:?}", ticket.error());
    assert_eq!(ticket.effective_mode_status(), "waiting");
}

fn begin_qualified_phase_fixture(p: &mut Productive) {
    assert_eq!(p.frame, 137);
    assert!(matches!(
        p.h.consumer.peek().unwrap(),
        crate::messages::ControlMessage::SetLegacyPadTimingMetadata { id: 0, .. }
    ));
    assert_eq!(drain_actual(p), 1);
    assert!(p.h.consumer.is_empty());
    publish_policy(p, false, 1);
    let (_feedback_producer, feedback_consumer) = rtrb::RingBuffer::new(1);
    p.h.engine.test_control_channels = Some((
        p.h.producer.clone(),
        Arc::new(Mutex::new(feedback_consumer)),
    ));
    p.h.engine.stop_sample(0).unwrap();
    assert_eq!(drain_actual(p), 1);
    assert!(
        !p.h.callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    let birth =
        p.h.engine
            .capture_pad_start(
                0,
                p.loop_region.start as f64 / f64::from(RATE),
                Some(p.loop_region.end as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap();
    drain_actual(p);
    assert_eq!(birth.target_frame(), Some(p.frame));
    assert!(p.h.engine.publish_pad_start(&birth, None).unwrap());
    drain_actual(p);
    assert_eq!(birth.state(), "applied", "{:?}", birth.reason());
    let started = voice(&p.h.callback.mixer);
    assert!(started.phase_launch_qualified && started.phase_birth_unadvanced);
    assert!(
        started
            .source_playback
            .phase_reference_current(&p.h.callback.transport)
    );
    let raw = started.source_playback.position();
    assert_eq!(raw.frame, p.loop_region.start);
    assert_eq!(raw.fraction, 0.0);

    // Genuine silent output creates phase debt without changing the original
    // launch coordinate or injecting an artificial source cursor/reference.
    let pause = p.h.engine.pause_sample(0).unwrap();
    drain_actual(p);
    assert!(
        pause
            .current_observation()
            .unwrap()
            .unwrap()
            .ordinary_paused()
    );
    assert!(voice(&p.h.callback.mixer).paused);
    assert!(render_actual(p, 25).iter().all(|sample| *sample == 0.0));
    assert_eq!(voice(&p.h.callback.mixer).source_playback.position(), raw);
    p.h.engine.resume_sample(0).unwrap();
    drain_actual(p);
    assert!(!voice(&p.h.callback.mixer).paused);
    assert_eq!(voice(&p.h.callback.mixer).source_playback.position(), raw);
    assert_eq!(p.frame, 162);
}

fn policy_effect(p: &mut Productive) -> Arc<RuntimePolicySnapshot> {
    p.h.engine
        .request_input_runtime_policy_adoption(false)
        .unwrap();
    let snapshot =
        p.h.engine
            .input_runtime
            .as_ref()
            .unwrap()
            .runtime_policy_snapshot()
            .unwrap()
            .unwrap();
    drain_actual(p);
    let (_producer, mut parameters) = rtrb::RingBuffer::new(1);
    let callback = p.h.callback.as_mut();
    drain_parameter_messages(
        &mut parameters,
        &mut callback.mixer,
        &mut callback.transport,
    );
    render_scheduled_audio(
        &mut callback.mixer,
        &mut callback.scheduler,
        &mut [],
        &mut [0.0; NUM_SAMPLES],
        p.frame,
        2,
        &mut callback.transport,
        &mut callback.feedback,
        &mut callback.retirement,
    );
    // This collecting fixture has no retirement worker. Release only terminal
    // original policy work OUTSIDE the callback before beginning another attempt.
    callback
        .retirement
        .retired
        .retain(|retired| !matches!(retired, RetiredAudioBuffer::RuntimePolicy(_)));
    snapshot
}

fn render_actual(p: &mut Productive, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames * 2];
    let callback = p.h.callback.as_mut();
    callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        p.frame,
        &mut callback.retirement,
    );
    callback.transport.advance_by_rendered_frames(frames);
    p.frame += frames as u64;
    output
}

#[test]
fn static_real_committed_stem_capture_refuses_unchanged_then_same_selection_progresses_after_off() {
    let mut p = Productive::new(false, 1.0);
    p.h.callback
        .transport
        .advance_by_rendered_frames(p.frame as usize);
    let input_clock = InputClock::new();
    p.h.engine.test_output_format = Some((RATE, 2));
    p.h.engine.input_runtime = Some(InputRuntime::new_with_ownership(
        p.h.producer.clone(),
        input_clock,
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
    publish_policy(&p, true, 2);
    let original = policy_effect(&mut p);
    assert_eq!(
        original.observation().unwrap().state,
        "applied",
        "{:?}",
        original.observation()
    );
    assert!(!voice(&p.h.callback.mixer).source_playback.is_static_phase());
    let phase = policy_effect(&mut p);
    assert!(Arc::ptr_eq(&original, &phase));
    assert_eq!(
        phase.observation().unwrap().state,
        "applied",
        "{:?}",
        phase.observation()
    );
    let clock = voice(&p.h.callback.mixer).source_playback;
    assert!(clock.is_static_phase());
    assert!(!clock.static_phase_settled());
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
    publish_policy(&p, false, 3);
    let off = policy_effect(&mut p);
    assert_eq!(
        off.observation().unwrap().state,
        "applied",
        "{:?}",
        off.observation()
    );
    assert!(!voice(&p.h.callback.mixer).source_playback.is_static_phase());
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
