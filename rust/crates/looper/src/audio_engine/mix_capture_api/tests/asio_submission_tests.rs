use super::*;
use crate::audio_engine::asio_output::test_driver::Factory;
use crate::audio_engine::audio_output::OutputOptions;
use crate::audio_engine::input_runtime_binding::enqueue_stop_with_producer;
use crate::audio_engine::mix_capture::{SPOOL_HEADER_BYTES, SpoolState, inspect_spool};
use crate::audio_engine::transport::QuantizeGrid;
use crate::audio_engine::{
    LoadedSourcePublication, SourcePitchInitialization, publish_loaded_sample,
    push_global_rate_message,
};
use crate::messages::SampleBuffer;

fn queue_pcm(engine: &AudioEngine) {
    let ownership = &engine.input_runtime_ownership;
    let producer = engine.control_producer().unwrap();
    let sample = SampleBuffer {
        residency: None,
        channels: 2,
        samples: (0..2048)
            .map(|sample| ((sample % 512) as f32 - 197.0) / 1024.0)
            .collect::<Vec<_>>()
            .into(),
    };
    publish_loaded_sample(
        producer,
        &engine.sample_cache,
        0,
        sample,
        LoadedSourcePublication {
            output_restore: None,
            initial_mix: None,
            placement: ownership.slot_snapshot(0).unwrap(),
            cold: false,
            cold_epoch: None,
            cold_adoption: None,
            replace_assignment: false,
            loop_region: None,
            resident_cancelled: None,
            incoming_defaults: None,
            initial_pitch: SourcePitchInitialization::FreshNeutral,
            intent: None,
            ownership,
            generation: 1,
            rate: 96_000,
            generation_slot: &mut (0, 0),
            digest_slot: &mut None,
            digest: "a".repeat(64),
        },
    )
    .unwrap();
    producer
        .lock()
        .unwrap()
        .push(ControlMessage::PlaySample {
            id: 0,
            volume: 1.0,
            received_at_ns: None,
            expected_source_generation: Some(1),
        })
        .unwrap();
}

fn until<T>(mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(result) = check() {
            return result;
        }
        assert!(Instant::now() < deadline, "capture observation timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn asio_submission_gates_actual_capture_through_output_ready() {
    const FIRST: usize = 17;
    const END: usize = 83;
    Python::initialize();
    for fail in [false, true] {
        // Keep the original spool on an assertion or unverified reader end.
        let directory = tempfile::tempdir().unwrap().keep();
        let path = directory.join("take.capture");
        eprintln!("ASIO capture fixture: {}", path.display());
        let factory = Factory::new().unwrap();
        let mut engine = AudioEngine::new().unwrap();
        engine.test_asio_driver = Some(factory.clone());
        engine
            .start_audio_output(OutputOptions {
                backend: "asio".into(),
                sample_rate_hz: Some(96_000),
                buffer_frames: Some(96),
                ..Default::default()
            })
            .unwrap();
        let stream = engine.stream_handle.as_ref().unwrap();
        let owner = stream.test_owner_observer();
        queue_pcm(&engine);
        let ticket = engine
            .prepare_mix_capture_internal(path.to_str().unwrap(), 96, 4, 4096)
            .unwrap();
        until(|| {
            let slot = engine.mix_capture_lock().unwrap();
            let control = &slot.as_ref().unwrap().control;
            assert!(control.backend_submission_required());
            control.status().writer_ready.then_some(())
        });
        assert!(
            engine
                .start_mix_capture_internal(&ticket, FIRST as u64)
                .unwrap()
        );
        assert!(
            engine
                .stop_mix_capture_internal(&ticket, END as u64)
                .unwrap()
        );

        factory.arm_output_ready();
        let pcm = std::thread::scope(|scope| {
            let tick = scope.spawn(|| stream.test_tick().unwrap());
            let pcm = until(|| factory.gated_output_pcm());
            until(|| {
                let slot = engine.mix_capture_lock().unwrap();
                let control = &slot.as_ref().unwrap().control;
                let status = control.status();
                assert!(!status.terminal_decided && status.payload_sha256.is_none());
                control.waiting_for_submission().then(|| {
                    let status = control.status();
                    assert!(status.rt_sealed);
                    assert_eq!(status.written_frames, (END - FIRST) as u64);
                })
            });
            factory.release_output_ready(fail);
            assert_eq!(tick.join().unwrap().output_frame, 96);
            pcm
        });
        let expected = &pcm[FIRST * 2..END * 2];
        assert!(expected.windows(2).any(|pair| pair[0] != pair[1]));
        assert!(expected.iter().any(|sample| sample.abs() > 0.01));
        let final_status = until(|| {
            let mut slot = engine.mix_capture_lock().unwrap();
            let capture = slot.as_mut().unwrap();
            capture.poll(engine.mix_capture_stream_failed());
            let status = capture.control.status();
            (status.worker_joined && status.rt_released).then_some(status)
        });
        assert!(final_status.terminal_decided);
        assert_eq!(final_status.first_frame, Some(FIRST as u64));
        assert_eq!(final_status.end_frame, Some(END as u64));
        assert_eq!(final_status.successful_spool(), !fail);
        assert_eq!(final_status.payload_sha256.is_some(), !fail);
        assert_eq!(final_status.failure, fail.then_some(CaptureFailure::Stream));
        let spool = inspect_spool(&path).unwrap();
        assert_eq!(
            spool.state,
            if fail {
                SpoolState::FailedIncomplete
            } else {
                SpoolState::SealedCandidate
            }
        );
        let bytes = fs::read(&path).unwrap();
        let expected: Vec<_> = expected
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        assert_eq!(&bytes[SPOOL_HEADER_BYTES as usize..], expected);

        engine.shut_down().unwrap();
        let ended = owner.lock().unwrap();
        assert!(ended.owner_ended && ended.owner_join_verified && ended.retirement_verified);
        assert_eq!(factory.info().unwrap().active, 0);
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&directory).unwrap();
    }
}

#[test]
fn asio_inline_admission_refuses_a_committed_device_interval_before_controls() {
    const FRAMES: u64 = 96;
    Python::initialize();
    let directory = tempfile::tempdir().unwrap().keep();
    let path = directory.join("take.capture");
    eprintln!("ASIO admission fixture: {}", path.display());
    let factory = Factory::new().unwrap();
    let mut engine = AudioEngine::new().unwrap();
    engine.test_asio_driver = Some(factory.clone());
    engine
        .start_audio_output(OutputOptions {
            backend: "asio".into(),
            sample_rate_hz: Some(96_000),
            buffer_frames: Some(FRAMES as u32),
            ..Default::default()
        })
        .unwrap();
    let stream = engine.stream_handle.as_ref().unwrap();
    let owner = stream.test_owner_observer();
    let submission = stream.asio_submission.as_ref().unwrap().clone();
    let ownership = &engine.input_runtime_ownership;
    queue_pcm(&engine);
    for end in [FRAMES, 2 * FRAMES, 3 * FRAMES] {
        let tick = stream.test_tick().unwrap();
        assert_eq!(tick.output_frame, end);
        assert!(tick.pads[0].active && tick.callback.left_peak > 0.01);
    }
    let device_next = 3 * FRAMES;
    let clock = stream.output_clock.read().unwrap();
    assert!(clock.valid);
    assert_eq!(clock.output_frame, device_next - FRAMES);
    let grid = QuantizeGrid::from_step_64ths(1).unwrap();
    let input_time = Some(clock.observed_at_ns);
    let original_t = clock.input_target_frame(input_time, clock.observed_at_ns, grid);
    assert_eq!(original_t, Some(0));
    assert_eq!(submission.prefix(), (device_next, false));
    let geometry = engine.loop_acceptance.callback_geometry().unwrap();
    assert_eq!(geometry["callback_count"], 3);

    let ticket = engine
        .prepare_mix_capture_internal(path.to_str().unwrap(), FRAMES as usize, 4, 4096)
        .unwrap();
    until(|| {
        let slot = engine.mix_capture_lock().unwrap();
        slot.as_ref()
            .unwrap()
            .control
            .status()
            .writer_ready
            .then_some(())
    });
    stream.test_render_ahead();
    assert_eq!(
        engine.loop_acceptance.callback_geometry().unwrap(),
        geometry
    );
    let still_current = stream.output_clock.read().unwrap();
    assert_eq!(still_current, clock);
    assert_eq!(
        still_current.input_target_frame(input_time, clock.observed_at_ns, grid),
        original_t
    );
    assert_eq!(submission.prefix(), (device_next, false));
    assert!(
        engine
            .start_mix_capture_internal(&ticket, device_next + 17)
            .unwrap()
    );
    assert!(
        engine
            .stop_mix_capture_internal(&ticket, device_next + 83)
            .unwrap()
    );
    assert!(enqueue_stop_with_producer(
        ownership,
        &mut stream.producer.lock().unwrap(),
        Some(0)
    ));
    push_global_rate_message(
        &ownership.global_timing_revision,
        &ownership.global_timing_publishing,
        &ownership.global_rate_status,
        &mut stream.parameter_producer.lock().unwrap(),
        Some(1.3),
        Some(0.4),
        "ASIO admission probe",
    )
    .unwrap();
    let tick = stream.test_tick().unwrap();
    assert_eq!(tick.output_frame, device_next + FRAMES);
    assert!(tick.pads[0].active, "queued STOP must not execute");
    assert_eq!(
        (tick.callback.left_peak, tick.callback.right_peak),
        (0.0, 0.0)
    );
    assert!(
        stream
            .stream_failed
            .load(std::sync::atomic::Ordering::Acquire)
    );
    assert_eq!(submission.prefix(), (device_next, true));
    let invalidated = stream.output_clock.read().unwrap();
    assert!(!invalidated.valid);
    assert_eq!(invalidated.output_frame, clock.output_frame);
    let rate = ownership.global_rate_status.read().unwrap();
    assert!(rate.request_revision > rate.effect.revision);
    assert!(rate.effect.execution_frame.is_none());
    {
        let mut feedback = stream.consumer.lock().unwrap();
        assert!(
            !std::iter::from_fn(|| feedback.pop().ok()).any(|event| matches!(
                event,
                AudioMessage::SampleStopped { id: 0 }
                    | AudioMessage::ContentFeedback { id: 0, kind: 2, .. }
            ))
        );
    }
    {
        let mut slot = engine.mix_capture_lock().unwrap();
        let capture = slot.as_mut().unwrap();
        capture.poll(engine.mix_capture_stream_failed());
        let status = capture.control.status();
        assert_eq!(status.failure, Some(CaptureFailure::Stream));
        assert!(!status.rt_released);
        assert!(status.first_frame.is_none() && status.end_frame.is_none());
    }
    engine.shut_down().unwrap();
    until(|| {
        let mut slot = engine.mix_capture_lock().unwrap();
        let capture = slot.as_mut().unwrap();
        capture.poll(false);
        let status = capture.control.status();
        assert!(status.first_frame.is_none() && status.end_frame.is_none());
        assert!(!status.successful_spool());
        (status.worker_joined && status.rt_released).then_some(())
    });
    let ended = owner.lock().unwrap();
    assert!(ended.owner_ended && ended.owner_join_verified && ended.retirement_verified);
    assert_eq!(factory.info().unwrap().active, 0);
    fs::remove_file(&path).unwrap();
    fs::remove_dir(&directory).unwrap();
}
