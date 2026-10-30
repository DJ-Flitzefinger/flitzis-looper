//! A restored scalar value cannot restore an already captured source-control history.
#![cfg(windows)]

use super::*;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_mapping::PitchPadProjection;
use crate::audio_engine::input_runtime_binding;
use crate::audio_engine::pitch_actions::CapturedPitchAction;
use crate::audio_engine::resident_relocation::{
    WindowRequest, prepare_window_with_producer, reconcile,
};

const ORIGINAL_FRAME: u64 = 1;
const EXECUTION_FRAME: u64 = ORIGINAL_FRAME + 8192;
const INTERVENING_FRAMES: usize = 257;
const AFTER_REJECTION_FRAMES: usize = 24_000;
const POLICIES: [&str; 2] = ["first_sound_at_grid", "attack_at_grid"];

#[derive(Clone, Copy, Debug)]
enum LaunchKind {
    Ordinary,
    FreshPitchRetrigger,
}

enum Launch {
    Ordinary(CapturedPadStart),
    FreshPitchRetrigger(CapturedPitchAction),
}

impl Launch {
    fn sequence(&self) -> u64 {
        match self {
            Self::Ordinary(start) => start.sequence(),
            Self::FreshPitchRetrigger(action) => action.sequence(),
        }
    }

    fn assert_original_frames(&self) {
        match self {
            Self::Ordinary(start) => {
                assert_eq!(start.target_frame(), Some(ORIGINAL_FRAME));
                assert_eq!(start.original_frame(), Some(ORIGINAL_FRAME));
                assert_eq!(start.execution_frame(), Some(EXECUTION_FRAME));
            }
            Self::FreshPitchRetrigger(action) => {
                assert_eq!(action.original_due_frame(), ORIGINAL_FRAME);
                assert_eq!(action.execution_target(), EXECUTION_FRAME);
            }
        }
    }

    fn assert_rejected(&self) {
        self.assert_original_frames();
        match self {
            Self::Ordinary(start) => {
                assert_eq!(start.state(), "rejected", "{:?}", start.reason());
                assert_eq!(start.reason(), Some("timing_changed"));
                assert_eq!(start.effective_frame(), None);
            }
            Self::FreshPitchRetrigger(action) => {
                assert_eq!(action.state(), "rejected", "{:?}", action.reason());
                assert_eq!(action.reason(), Some("timing_changed"));
                assert!(!action.effective());
            }
        }
    }

    fn assert_applied_at(&self, execution: u64) {
        match self {
            Self::Ordinary(start) => {
                assert_eq!(start.state(), "applied", "{:?}", start.reason());
                assert_eq!(start.original_frame(), Some(ORIGINAL_FRAME));
                assert_eq!(start.execution_frame(), Some(execution));
                assert_eq!(start.effective_frame(), Some(execution));
            }
            Self::FreshPitchRetrigger(action) => {
                assert_eq!(action.original_due_frame(), ORIGINAL_FRAME);
                assert_eq!(action.execution_target(), execution);
                assert!(action.effective(), "{:?}", action.reason());
                assert_eq!(action.state(), "applied");
            }
        }
    }
}

fn idle_policy(fixture: &mut Fixture, policy: &str) {
    fixture.drain();
    // The real idle render republishes stopped ownership; the policy is selected
    // before this input is captured, so both policies use the same public path.
    assert!(fixture.render(1).iter().all(|sample| *sample == 0.0));
    assert!(fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior(policy)
            .unwrap()
    );
    assert_eq!(fixture.transport.output_frame(), ORIGINAL_FRAME);
}

fn prepared_launch(kind: LaunchKind, policy: &str) -> (Fixture, Launch) {
    let mut fixture = Fixture::new(true);
    if matches!(kind, LaunchKind::Ordinary) {
        fixture
            .mixer
            .set_pad_pitch_intent(0, PitchIntent::new(0, 7).unwrap())
            .unwrap();
    }
    idle_policy(&mut fixture, policy);
    let launch = match kind {
        LaunchKind::Ordinary => {
            let start = capture(&fixture, 1.0);
            fixture.drain();
            assert_eq!(start.target_frame(), Some(ORIGINAL_FRAME));
            assert!(
                fixture
                    .control
                    .engine
                    .publish_pad_start(&start, None)
                    .unwrap()
            );
            fixture.drain();
            assert!(start.accepted(), "{:?}", start.reason());
            Launch::Ordinary(start)
        }
        LaunchKind::FreshPitchRetrigger => {
            let action = fixture.capture(0, "pitch.extra:7:set_retrigger");
            fixture.publish(&action);
            Launch::FreshPitchRetrigger(action)
        }
    };
    launch.assert_original_frames();
    assert_eq!(fixture.scheduler.len(), 1);
    wait_ready(&mut fixture, &[launch.sequence()]);
    assert_eq!(fixture.transport.output_frame(), ORIGINAL_FRAME);
    assert_eq!(fixture.starts(0), 0);
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
    (fixture, launch)
}

fn assert_rejected_at_original_execution(fixture: &mut Fixture, launch: &Launch) {
    launch.assert_original_frames();
    assert_eq!(fixture.starts(0), 0);
    assert_eq!(fixture.scheduler.len(), 1);
    assert!(
        fixture
            .render_to(EXECUTION_FRAME)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!(fixture.starts(0), 0);
    // This is the actual first output sample at E; no startup/FIFO prefix is skipped.
    assert_eq!(fixture.render(1), vec![0.0]);
    launch.assert_rejected();
    assert_eq!(fixture.starts(0), 0);
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
    assert!(fixture.scheduler.is_empty());
    assert!(
        fixture
            .pcm(AFTER_REJECTION_FRAMES)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    launch.assert_rejected();
    assert_eq!(fixture.starts(0), 0);
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
    assert!(
        fixture.scheduler.is_empty(),
        "the original must not be requantized"
    );
}

#[test]
fn prepared_original_and_fresh_retrigger_reject_a_rate_aba_at_their_original_execution() {
    for policy in POLICIES {
        for kind in [LaunchKind::Ordinary, LaunchKind::FreshPitchRetrigger] {
            let (mut fixture, launch) = prepared_launch(kind, policy);
            fixture.mixer.set_speed(1.37);
            assert!(
                fixture
                    .render(INTERVENING_FRAMES)
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
            assert!(fixture.transport.output_frame() < EXECUTION_FRAME);
            fixture.mixer.set_speed(1.0);
            assert_rejected_at_original_execution(&mut fixture, &launch);
            let expected_pitch = match kind {
                LaunchKind::Ordinary => PitchIntent::new(0, 7).unwrap(),
                LaunchKind::FreshPitchRetrigger => PitchIntent::default(),
            };
            assert_eq!(fixture.mixer.pad_pitch_intent(0), Some(expected_pitch));
        }
    }
}

#[test]
fn original_grid_target_rejects_a_rate_aba_between_target_capture_and_late_admission() {
    for policy in POLICIES {
        let mut fixture = Fixture::new(true);
        let _synthetic_files = install_synthetic_complete_lease(&mut fixture);
        fixture
            .mixer
            .set_pad_pitch_intent(0, PitchIntent::new(0, 7).unwrap())
            .unwrap();
        idle_policy(&mut fixture, policy);
        assert!(fixture.render_to(1_000).iter().all(|sample| *sample == 0.0));
        let start = capture(&fixture, 1.0);
        let window = fixture
            .control
            .engine
            .prepare_captured_pad_start(&start)
            .unwrap();
        let message = fixture.control.consumer.pop().unwrap();
        assert!(matches!(&message, ControlMessage::CapturePadStartTarget(_)));
        process_control_message(
            message,
            &mut fixture.scheduler,
            fixture.transport.output_frame(),
            &mut TriggerQuantization::Grid { step_64ths: 16 },
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut fixture.feedback,
            &mut fixture.retirement,
        );
        assert_ne!(window.publication_status(), "accepted");
        assert_eq!(start.target_frame(), Some(24_000));
        assert_eq!(start.original_frame(), Some(24_000));
        assert!(!start.accepted());
        assert!(fixture.scheduler.is_empty());
        assert!(
            fixture
                .render_to(30_000)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        fixture.mixer.set_speed(1.37);
        assert!(
            fixture
                .render(INTERVENING_FRAMES)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        fixture.mixer.set_speed(1.0);
        // The original storage ACK must preserve the target's earlier control fence.
        drain_with_mode(&mut fixture, TriggerQuantization::Grid { step_64ths: 16 });
        assert_eq!(
            window.publication_status(),
            "accepted",
            "{:?}",
            window.error()
        );
        assert!(window.is_current());
        assert!(
            fixture
                .control
                .engine
                .publish_pad_start(&start, Some(&window))
                .unwrap()
        );
        drain_with_mode(&mut fixture, TriggerQuantization::Grid { step_64ths: 16 });
        assert_eq!(start.state(), "rejected", "{:?}", start.reason());
        assert_eq!(start.reason(), Some("timing_changed"));
        assert_eq!(start.target_frame(), Some(24_000));
        assert_eq!(start.original_frame(), Some(24_000));
        assert_eq!(start.effective_frame(), None);
        assert_eq!(fixture.starts(0), 0);
        assert!(fixture.scheduler.is_empty());
        assert!(
            fixture
                .pcm(AFTER_REJECTION_FRAMES)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert_eq!(start.state(), "rejected");
        assert_eq!(start.target_frame(), Some(24_000));
        assert_eq!(start.original_frame(), Some(24_000));
        assert_eq!(start.effective_frame(), None);
        assert_eq!(fixture.starts(0), 0);
        assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
        assert!(fixture.scheduler.is_empty());
    }
}

fn change_loop(fixture: &mut Fixture, id: usize, start: usize, end: usize) {
    fixture.mixer.set_pad_loop_region(
        id,
        start as f64 / f64::from(RATE),
        Some(end as f64 / f64::from(RATE)),
    );
    assert_eq!(fixture.mixer.loop_region_frames(id), (start, Some(end)));
}

#[test]
fn prepared_original_and_fresh_retrigger_reject_a_same_pad_loop_aba() {
    for policy in POLICIES {
        for kind in [LaunchKind::Ordinary, LaunchKind::FreshPitchRetrigger] {
            let (mut fixture, launch) = prepared_launch(kind, policy);
            change_loop(&mut fixture, 0, LOOP_START + 2_400, LOOP_END - 2_400);
            assert!(
                fixture
                    .render(INTERVENING_FRAMES)
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
            assert!(fixture.transport.output_frame() < EXECUTION_FRAME);
            change_loop(&mut fixture, 0, LOOP_START, LOOP_END);
            assert_rejected_at_original_execution(&mut fixture, &launch);
            let expected_pitch = match kind {
                LaunchKind::Ordinary => PitchIntent::new(0, 7).unwrap(),
                LaunchKind::FreshPitchRetrigger => PitchIntent::default(),
            };
            assert_eq!(fixture.mixer.pad_pitch_intent(0), Some(expected_pitch));
        }
    }
}

#[test]
fn unrelated_pad_loop_change_preserves_the_original_and_fresh_retrigger_launch() {
    for policy in POLICIES {
        for kind in [LaunchKind::Ordinary, LaunchKind::FreshPitchRetrigger] {
            let (mut fixture, launch) = prepared_launch(kind, policy);
            change_loop(&mut fixture, 1, LOOP_START + 2_400, LOOP_END - 2_400);
            fixture.render(0);
            let execution = match &launch {
                Launch::Ordinary(start) => start.execution_frame().unwrap(),
                Launch::FreshPitchRetrigger(action) => action.execution_target(),
            };
            assert_eq!(
                execution,
                if matches!(kind, LaunchKind::Ordinary) && policy == "first_sound_at_grid" {
                    ORIGINAL_FRAME
                } else {
                    EXECUTION_FRAME
                }
            );
            assert!(
                fixture
                    .mixer
                    .voices
                    .iter()
                    .filter(|voice| voice.active)
                    .all(|voice| voice.launch_unadvanced)
            );
            assert!(
                fixture
                    .render_to(execution)
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
            assert_eq!(fixture.starts(0), usize::from(execution == ORIGINAL_FRAME));
            let mut pcm = fixture.render(1);
            launch.assert_applied_at(execution);
            assert_eq!(fixture.starts(0), 1);
            assert_eq!(fixture.starts(1), 0);
            assert_eq!(
                fixture.voice(0).stretch.adopted_request_id(),
                Some(launch.sequence())
            );
            pcm.extend(fixture.pcm(4_095));
            assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
            assert_eq!(fixture.starts(0), 1);
            assert_eq!(
                fixture.mixer.loop_region_frames(0),
                (LOOP_START, Some(LOOP_END))
            );
            assert!(fixture.scheduler.is_empty());
        }
    }
}

fn publish_runtime_subloop(fixture: &Fixture, start: usize, end: usize) {
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    for id in [0, 1] {
        loaded[id] = true;
        starts[id] = if id == 0 { start } else { LOOP_START } as f64 / f64::from(RATE);
        ends[id] = Some(if id == 0 { end } else { LOOP_END } as f64 / f64::from(RATE));
        bindings[id] = input_runtime_binding::capture(&fixture.control.engine, id).unwrap();
        assert!(bindings[id].is_some());
        rows[id].key_lock = true;
    }
    fixture
        .control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_pitch(
            true,
            loaded,
            starts,
            ends,
            bindings.iter().map(Option::as_ref).collect(),
            rows,
        )
        .unwrap();
}

#[test]
fn separate_reused_resident_loop_edit_rejects_the_original_prepared_subloop_launch() {
    const SUBLOOP_START: usize = LOOP_START + 2_400;
    const SUBLOOP_END: usize = LOOP_END - 2_400;
    for policy in POLICIES {
        for kind in [LaunchKind::Ordinary, LaunchKind::FreshPitchRetrigger] {
            let mut fixture = Fixture::new(true);
            let _synthetic_files = install_synthetic_complete_lease(&mut fixture);
            change_loop(&mut fixture, 0, SUBLOOP_START, SUBLOOP_END);
            publish_runtime_subloop(&fixture, SUBLOOP_START, SUBLOOP_END);
            if matches!(kind, LaunchKind::Ordinary) {
                fixture
                    .mixer
                    .set_pad_pitch_intent(0, PitchIntent::new(0, 7).unwrap())
                    .unwrap();
            }
            idle_policy(&mut fixture, policy);
            let launch = match kind {
                LaunchKind::Ordinary => {
                    let start = fixture
                        .control
                        .engine
                        .capture_pad_start(
                            0,
                            SUBLOOP_START as f64 / f64::from(RATE),
                            Some(SUBLOOP_END as f64 / f64::from(RATE)),
                            1.0,
                            false,
                            None,
                            None,
                        )
                        .unwrap();
                    fixture.drain();
                    assert!(
                        fixture
                            .control
                            .engine
                            .publish_pad_start(&start, None)
                            .unwrap()
                    );
                    fixture.drain();
                    assert!(start.accepted(), "{:?}", start.reason());
                    Launch::Ordinary(start)
                }
                LaunchKind::FreshPitchRetrigger => {
                    let action = fixture.capture(0, "pitch.extra:7:set_retrigger");
                    assert_eq!(
                        action.loop_region(),
                        (
                            SUBLOOP_START as f64 / f64::from(RATE),
                            Some(SUBLOOP_END as f64 / f64::from(RATE)),
                        )
                    );
                    fixture.publish(&action);
                    Launch::FreshPitchRetrigger(action)
                }
            };
            launch.assert_original_frames();
            wait_ready(&mut fixture, &[launch.sequence()]);
            assert_eq!(fixture.transport.output_frame(), ORIGINAL_FRAME);
            assert_eq!(fixture.starts(0), 0);
            let before = fixture.control.engine.sample_cache.lock().unwrap()[0]
                .clone()
                .unwrap();
            assert_eq!(before.resident_start(), LOOP_START);
            assert_eq!(before.resident_end(), LOOP_END);
            let ticket = prepare_window_with_producer(
                &fixture.control.engine,
                0,
                WindowRequest {
                    loop_region: Some((
                        LOOP_START as f64 / f64::from(RATE),
                        Some(LOOP_END as f64 / f64::from(RATE)),
                    )),
                    ..WindowRequest::default()
                },
                fixture.control.producer.clone(),
            )
            .unwrap();
            assert_ne!(ticket.publication_status(), "accepted");
            assert_eq!(
                fixture.mixer.loop_region_frames(0),
                (SUBLOOP_START, Some(SUBLOOP_END))
            );
            fixture.drain();
            reconcile(&fixture.control.engine).unwrap();
            assert_eq!(
                ticket.publication_status(),
                "accepted",
                "{:?}",
                ticket.error()
            );
            assert!(ticket.is_current());
            let after = fixture.control.engine.sample_cache.lock().unwrap()[0]
                .clone()
                .unwrap();
            assert!(after.same_source(&before));
            assert!(after.same_window(&before));
            assert_eq!(after.resident_binding(), before.resident_binding());
            assert_eq!(after.resident_start(), LOOP_START);
            assert_eq!(after.resident_end(), LOOP_END);
            assert_eq!(
                fixture.mixer.loop_region_frames(0),
                (LOOP_START, Some(LOOP_END))
            );
            assert_rejected_at_original_execution(&mut fixture, &launch);
            assert_eq!(
                fixture.mixer.loop_region_frames(0),
                (LOOP_START, Some(LOOP_END)),
                "the rejected old input must preserve the separately acknowledged loop"
            );
            let expected_pitch = match kind {
                LaunchKind::Ordinary => PitchIntent::new(0, 7).unwrap(),
                LaunchKind::FreshPitchRetrigger => PitchIntent::default(),
            };
            assert_eq!(fixture.mixer.pad_pitch_intent(0), Some(expected_pitch));
        }
    }
}
