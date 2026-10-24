//! Existing prepared-stem custody refuses static history and recovers after OFF.

use super::*;
use crate::audio_engine::audio_stream::{drain_parameter_messages, render_scheduled_audio};
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
    p.h.callback.drain(&mut p.h.consumer);
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
    let generation = voice(&p.h.callback.mixer).generation;
    let sample = voice(&p.h.callback.mixer).sample.clone().unwrap();
    let native = voice(&p.h.callback.mixer).stretch.native_state_address();
    let transition = p.h.callback.mixer.stem_transition_for_measurement(0);
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    let refused = p.request("all_stems", 0b1010);
    p.h.callback.drain(&mut p.h.consumer);
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
    p.capture_then_wait(&fresh);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
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
