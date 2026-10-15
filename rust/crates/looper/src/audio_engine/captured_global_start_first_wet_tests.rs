//! A fresh prepared GLOBAL birth cannot inherit its stopped predecessor's wet receipt.

use super::*;
use crate::audio_engine::audio_stream::drain_parameter_messages;
use crate::audio_engine::dsp::PerPadDspChain;
use crate::audio_engine::input_runtime_binding::KeyLockStatus;
use crate::audio_engine::mixer::prepared_native_mixer_tests::finite_lifecycle_tests::{
    AlgebraicClock, RawChronology,
};
use crate::messages::{ControlParameterMessage, SampleBuffer};
use rtrb::RingBuffer;
use std::sync::{Arc, Mutex};

const RATIO: f64 = 0.75;
const HISTORY: usize = 4096;

fn mode_status(fixture: &Fixture, id: usize, request: u64) -> KeyLockStatus {
    let status = fixture
        .control
        .engine
        .input_runtime_ownership
        .key_lock_status(id)
        .unwrap();
    let sample = fixture.control.engine.sample_cache.lock().unwrap()[id]
        .clone()
        .unwrap();
    assert_eq!(status.request_id, request);
    assert_eq!(status.source_address, sample.source_address());
    assert_eq!(status.window_revision, sample.window_revision());
    status
}

/// Complete analytic fixture PCM, addressed by the independent algebraic clock.
/// Neither the mixer reader nor its adopted Native/FIFO output supplies this oracle.
fn complete_tone(hz: f64) -> SampleBuffer {
    SampleBuffer {
        channels: 1,
        residency: None,
        samples: Arc::from(
            (0..144_000)
                .map(|frame| {
                    (0.25 * (frame as f64 * hz * std::f64::consts::TAU / f64::from(RATE)).sin())
                        as f32
                })
                .collect::<Vec<_>>(),
        ),
    }
}

#[test]
fn stopped_confirmed_wet_global_birth_rearms_each_receipt_before_first_output_and_failure_is_local()
{
    for fail_before_first_output in [false, true] {
        let mut fixture = Fixture::new(true);
        let (parameters, mut parameter_consumer) = RingBuffer::<ControlParameterMessage>::new(8);
        fixture.control.engine.test_parameter_producer = Some(Arc::new(Mutex::new(parameters)));
        fixture.control.engine.set_speed(RATIO).unwrap();
        drain_parameter_messages(
            &mut parameter_consumer,
            &mut fixture.mixer,
            &mut fixture.transport,
        );
        let requests = [0, 1].map(|id| {
            let request = fixture.control.engine.set_pad_key_lock(id, true).unwrap();
            assert_ne!(request, 0);
            request
        });
        fixture.drain();

        // Establish the predecessor's completed first-wet guard through real output.
        // Assigning readiness or effective arrays would leave the original bug unexercised.
        let predecessor = public_capture(&fixture);
        fixture.drain();
        let predecessor_ticket = fixture
            .control
            .engine
            .publish_captured_global_start(&predecessor)
            .unwrap()
            .unwrap();
        fixture.drain();
        wait_group(&mut fixture);
        let previous_e = predecessor.owner.members[0]
            .start
            .execution_frame()
            .unwrap();
        assert!(
            fixture
                .render_to(previous_e)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        fixture.render(1);
        assert_eq!(predecessor_ticket.publication_status(), "accepted");
        let previous_native = [0, 1].map(|id| {
            assert!(fixture.voice(id).stretch.wet_output_ready());
            let status = mode_status(&fixture, id, requests[id]);
            assert!(status.effective && status.ready && status.state == "wet");
            fixture.voice(id).stretch.native_state_address()
        });
        for id in [0, 1] {
            fixture.control.engine.stop_sample(id).unwrap();
        }
        fixture.drain();
        assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));

        // Capture T once, let it pass while stopped, then admit one prepared group at E.
        let captured = public_capture(&fixture);
        drain_grid(&mut fixture);
        let original = captured.original_frame().unwrap();
        assert_eq!(original, 24_000);
        assert!(
            fixture
                .render_to(30_000)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        let ticket = fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .unwrap()
            .unwrap();
        drain_grid(&mut fixture);
        let execution = fixture.scheduler.peek_next_target_frame().unwrap();
        assert_eq!(execution, 38_192);
        assert_eq!(captured.original_frame(), Some(original));
        assert_eq!(ticket.publication_status(), "pending");
        for (id, member) in captured.owner.members.iter().enumerate() {
            assert!(member.start.accepted());
            assert_eq!(member.start.original_frame(), Some(original));
            assert_eq!(member.start.execution_frame(), Some(execution));
            assert!(member.start.sequence() > predecessor.owner.members[id].start.sequence());
        }
        wait_group(&mut fixture);
        assert!(
            fixture
                .render_to(execution)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));

        // The real empty callback applies all due births, without consuming a PCM sample.
        assert!(fixture.render(0).is_empty());
        assert_eq!(fixture.transport.output_frame(), execution);
        assert_eq!(ticket.publication_status(), "accepted");
        assert_eq!(captured.state(), "applied");
        assert_eq!((fixture.starts(0), fixture.starts(1)), (2, 2));
        assert!(fixture.scheduler.is_empty());
        let native = [0, 1].map(|id| {
            let voice = fixture.voice(id);
            assert_eq!(voice.volume, 1.0);
            assert_eq!(voice.source_playback.position().frame, LOOP_START);
            assert_eq!(voice.source_playback.position().fraction, 0.0);
            assert_eq!(voice.source_playback.tempo_ratio(), RATIO);
            assert_eq!(
                voice.stretch.adopted_request_id(),
                Some(captured.owner.members[id].start.sequence())
            );
            assert!(
                voice
                    .stretch
                    .owns_pitch_contract(PitchIntent::default(), true)
            );
            assert!(!voice.stretch.wet_output_ready());
            let history = voice.stretch.productive_history().unwrap();
            assert_eq!(history.fed_output_frames, HISTORY as u64);
            assert_eq!(history.next_position.frame, LOOP_START);
            assert_eq!(
                history.binding.source_address,
                voice.sample.as_ref().unwrap().source_address()
            );
            let address = voice.stretch.native_state_address();
            assert_ne!(address, 0);
            assert_ne!(address, previous_native[id]);
            let status = mode_status(&fixture, id, requests[id]);
            assert!(!status.effective && !status.ready && status.state == "waiting");
            address
        });

        let sources = [complete_tone(160.0), complete_tone(320.0)];
        let mut clocks = std::array::from_fn::<_, 2, _>(|_| {
            AlgebraicClock::physical(LOOP_START, LOOP_END, RATIO)
        });
        let mut raw = [0, 1].map(|id| {
            let mut raw = RawChronology::new(RATIO, true);
            // H=4096 at 3/4 speed reaches E's loop start from 3072 source frames back.
            clocks[id].seek_physical(LOOP_END - HISTORY * 3 / 4, 144_000);
            raw.render(&sources[id], &mut clocks[id], HISTORY);
            assert_eq!(clocks[id].physical_position(0), (LOOP_START, 0.0, false));
            assert_eq!(
                fixture.voice(id).stretch.pending_fifo_frames(),
                (
                    HISTORY % raw.block_size(),
                    raw.block_size() - 1 - HISTORY % raw.block_size()
                )
            );
            raw
        });
        let mut dry_clock = AlgebraicClock::physical(LOOP_START, LOOP_END, RATIO);
        let mut filters = [0, 1].map(|id| PerPadDspChain::new(id, RATE as f32, 1));
        if fail_before_first_output {
            // Exhaust this lane's own clean reserve. Worker failure is shared by
            // all lanes and would correctly invalidate both pending ON voices.
            fixture
                .mixer
                .voices
                .iter_mut()
                .find(|voice| voice.active && voice.sample_id == 0)
                .unwrap()
                .stretch
                .exhaust_warmed_reserve();
        }
        let mut difference_from_dry = 0.0_f32;
        let mut consumed = 0;
        for frames in [1, 127, 384, 513] {
            let actual = fixture.render(frames);
            let dry = dry_clock.dry(&sources[0], frames);
            let first = raw[0].render(&sources[0], &mut clocks[0], frames);
            let peer = raw[1].render(&sources[1], &mut clocks[1], frames);
            for frame in 0..frames {
                difference_from_dry += (first[frame] - dry[frame]).abs();
                let mut expected = 0.0;
                for (id, sample) in [
                    if fail_before_first_output {
                        dry[frame]
                    } else {
                        first[frame]
                    },
                    peer[frame],
                ]
                .into_iter()
                .enumerate()
                {
                    filters[id].begin_frame();
                    expected += filters[id].process_sample(0, sample);
                }
                assert!(
                    (actual[frame] - expected).abs() < 2.0e-5,
                    "first-output PCM changed: fault={fail_before_first_output} frame={} actual={} expected={expected}",
                    consumed + frame,
                    actual[frame]
                );
            }
            consumed += frames;
            for id in [0, 1] {
                let voice = fixture.voice(id);
                let status = mode_status(&fixture, id, requests[id]);
                let (frame, fraction, explicit) = clocks[id].physical_position(0);
                assert!(!explicit);
                assert_eq!(voice.source_playback.position().frame, frame);
                assert_eq!(voice.source_playback.position().fraction, fraction);
                assert_eq!(
                    captured.owner.members[id].start.effective_frame(),
                    Some(execution)
                );
                if id == 0 && fail_before_first_output {
                    assert!(!status.effective && !status.ready && status.state == "error");
                    assert!(status.error.is_some());
                    assert!(!fixture.mixer.key_lock_for_measurement(id));
                    assert!(!voice.stretch.wet_output_ready());
                } else {
                    assert!(status.effective && status.ready && status.state == "wet");
                    assert!(voice.stretch.wet_output_ready());
                    assert_eq!(voice.stretch.native_state_address(), native[id]);
                    let history = voice.stretch.productive_history().unwrap();
                    assert_eq!(history.fed_output_frames, (HISTORY + consumed) as u64);
                    let input = (HISTORY + consumed) % raw[id].block_size();
                    assert_eq!(
                        voice.stretch.pending_fifo_frames(),
                        (input, raw[id].block_size() - 1 - input)
                    );
                }
            }
        }
        assert!(
            difference_from_dry > 0.01,
            "raw Native oracle must distinguish dry fallback"
        );
        assert_eq!((fixture.starts(0), fixture.starts(1)), (2, 2));
        assert_eq!(captured.original_frame(), Some(original));
    }
}
