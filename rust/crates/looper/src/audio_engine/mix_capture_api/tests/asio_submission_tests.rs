use super::*;
use crate::audio_engine::asio_output::test_driver::Factory;
use crate::audio_engine::audio_output::OutputOptions;
use crate::audio_engine::global_rate_status::RateState;
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
            generation_slot: &mut engine.loaded_source_generations.lock().unwrap()[0],
            digest_slot: &mut engine.loaded_source_digests.lock().unwrap()[0],
            digest: "a".repeat(64),
        },
    )
    .unwrap();
    engine.pad_request_ids.lock().unwrap()[0] = 1;
    engine
        .prepared_source_epoch(0)
        .unwrap()
        .store(1, std::sync::atomic::Ordering::Release);
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

struct ColdPcm {
    id: usize,
    publication: crate::audio_engine::input_runtime_binding::SourcePublication,
    binding: crate::audio_engine::input_runtime_binding::InputPadBinding,
    adoption: Arc<std::sync::atomic::AtomicU8>,
    reader: std::sync::Weak<[f32]>,
}

impl ColdPcm {
    fn publish(engine: &AudioEngine, generation: u64, digest: &str) -> Self {
        Self::publish_with_pitch(
            engine,
            generation,
            digest,
            SourcePitchInitialization::FreshNeutral,
        )
    }

    fn publish_with_pitch(
        engine: &AudioEngine,
        generation: u64,
        digest: &str,
        initial_pitch: SourcePitchInitialization,
    ) -> Self {
        Self::publish_at(engine, 0, generation, digest, initial_pitch)
    }

    fn publish_at(
        engine: &AudioEngine,
        id: usize,
        generation: u64,
        digest: &str,
        initial_pitch: SourcePitchInitialization,
    ) -> Self {
        Self::publish_with_geometry(engine, id, generation, digest, initial_pitch, 1024, None)
    }

    fn publish_with_geometry(
        engine: &AudioEngine,
        id: usize,
        generation: u64,
        digest: &str,
        initial_pitch: SourcePitchInitialization,
        frames: usize,
        loop_region: Option<(usize, usize)>,
    ) -> Self {
        use crate::audio_engine::{next_pad_request_id, publish_loaded_sample_with_arm};

        let ownership = &engine.input_runtime_ownership;
        let sample = SampleBuffer {
            residency: None,
            channels: 2,
            samples: (0..2 * frames)
                .map(|sample| ((sample % 512) as f32 - 197.0) / (1024.0 * generation as f32))
                .collect::<Vec<_>>()
                .into(),
        }
        .with_complete_source(96_000);
        let reader = Arc::downgrade(&sample.samples);
        let adoption = Arc::new(std::sync::atomic::AtomicU8::new(0));
        assert_eq!(
            next_pad_request_id(
                &engine.pad_request_ids,
                id,
                engine.prepared_source_epoch(id).unwrap()
            )
            .unwrap(),
            generation,
        );
        let publication = publish_loaded_sample_with_arm(
            engine.control_producer().unwrap(),
            &engine.sample_cache,
            id,
            sample.clone(),
            LoadedSourcePublication {
                output_restore: None,
                initial_mix: None,
                placement: ownership.slot_snapshot(id).unwrap(),
                cold: true,
                cold_epoch: None,
                cold_adoption: Some(adoption.clone()),
                // A different digest installs a new assignment without stopping A.
                replace_assignment: false,
                loop_region,
                resident_cancelled: None,
                incoming_defaults: None,
                initial_pitch,
                intent: None,
                ownership,
                generation,
                rate: 96_000,
                generation_slot: &mut engine.loaded_source_generations.lock().unwrap()[id],
                digest_slot: &mut engine.loaded_source_digests.lock().unwrap()[id],
                digest: digest.repeat(64),
            },
            || Ok(()),
        )
        .unwrap();
        Self {
            id,
            binding: publication.binding(&sample, 96_000),
            publication,
            adoption,
            reader,
        }
    }

    fn assert_ack(&self, engine: &AudioEngine, accepted: bool) {
        use std::sync::atomic::Ordering;

        let ownership = &engine.input_runtime_ownership;
        assert_eq!(
            self.adoption.load(Ordering::Acquire),
            if accepted { 2 } else { 0 }
        );
        assert_eq!(
            ownership.binding_source_adoption_accepted(self.id, self.binding),
            accepted
        );
        assert_eq!(
            ownership.callback_feedback_key(self.id) == self.publication.key(),
            accepted
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WetAdmission {
    Inline,
    Prepared,
    Split,
    Stop,
    Late,
    Tempo,
    TempoAfterClaim,
    Retained,
    PendingBank,
}

fn wet_admission_path(mode: WetAdmission) -> Vec<u8> {
    use crate::audio_engine::constants::NUM_SAMPLES;
    use crate::audio_engine::input_mapping::PitchPadProjection;
    use crate::audio_engine::input_runtime_binding;
    use crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::WorkerGate;
    use std::sync::atomic::Ordering;

    const FRAMES: u64 = 96;
    const F: u64 = 18_240;
    const E: u64 = 24_000;
    let directory = tempfile::tempdir().unwrap().keep();
    let path = directory.join("take.capture");
    eprintln!("ASIO Wet {mode:?}: {}", path.display());
    let factory = Factory::new().unwrap();
    factory
        .inline_wet
        .store(mode == WetAdmission::Inline, Ordering::Release);
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
    let submission = stream.output_submission.as_ref().unwrap().clone();
    let ownership = &engine.input_runtime_ownership;
    let tempo_case = matches!(mode, WetAdmission::Tempo | WetAdmission::TempoAfterClaim);
    let retained_case = matches!(mode, WetAdmission::Retained | WetAdmission::PendingBank);
    let accepted = (tempo_case || retained_case).then(|| ColdPcm::publish(&engine, 1, "a"));
    if let Some(source) = &accepted {
        source.assert_ack(&engine, false);
        // FIFO order makes Play follow the callback's actual OwnACK at frame zero.
        assert!(
            stream
                .producer
                .lock()
                .unwrap()
                .push(ControlMessage::PlaySample {
                    id: 0,
                    volume: 1.0,
                    received_at_ns: None,
                    expected_source_generation: Some(1),
                })
                .is_ok()
        );
    } else {
        queue_pcm(&engine);
    }
    let stopped_source = tempo_case.then(|| {
        let source =
            ColdPcm::publish_at(&engine, 1, 1, "c", SourcePitchInitialization::FreshNeutral);
        source.assert_ack(&engine, false);
        stream
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::PlaySample {
                id: 1,
                volume: 1.0,
                received_at_ns: None,
                expected_source_generation: Some(1),
            })
            .unwrap();
        source
    });
    let weak = Arc::downgrade(
        &engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples,
    );
    let first = stream.test_tick().unwrap();
    assert_eq!(first.output_frame, FRAMES);
    assert!(first.pads[0].active);
    let original_birth = first.pads[0].generation;
    if let Some(source) = &accepted {
        source.assert_ack(&engine, true);
    }
    if let Some(source) = &stopped_source {
        source.assert_ack(&engine, true);
        assert!(first.pads[1].active);
    }
    let runtime = engine.input_runtime.as_ref().unwrap();
    let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
    let mut loaded = vec![false; NUM_SAMPLES];
    loaded[0] = true;
    let mut bindings = vec![None; NUM_SAMPLES];
    bindings[0] = Some(&binding);
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    rows[0].quantize_step_64ths = Some(8);
    runtime
        .set_runtime_state_with_pitch(
            true,
            loaded,
            vec![0.0; NUM_SAMPLES],
            vec![None; NUM_SAMPLES],
            bindings,
            rows,
        )
        .unwrap();

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
    assert!(
        engine
            .start_mix_capture_internal(&ticket, E - FRAMES)
            .unwrap()
    );
    let capture_end = E + if mode == WetAdmission::Split || tempo_case {
        17
    } else {
        FRAMES
    };
    assert!(
        engine
            .stop_mix_capture_internal(&ticket, capture_end)
            .unwrap()
    );
    let rate = |speed| {
        push_global_rate_message(
            &ownership.global_timing_revision,
            &ownership.global_timing_publishing,
            &ownership.global_rate_status,
            &mut stream.parameter_producer.lock().unwrap(),
            Some(speed),
            Some(0.5),
            "ASIO Wet admission",
        )
        .unwrap();
    };
    for end in (2 * FRAMES..=F).step_by(FRAMES as usize) {
        if end == F {
            rate(1.4);
        }
        let tick = stream.test_tick().unwrap();
        assert_eq!(tick.output_frame, end);
        assert!(tick.pads[0].active);
    }
    let clock = stream.output_clock.read().unwrap();
    assert!(clock.valid);
    assert_eq!(clock.output_frame, F - FRAMES);
    let action = runtime
        .capture_gui_pitch(&binding, "pitch.extra:3:set", clock.observed_at_ns)
        .unwrap();
    assert_eq!(action.original_due_frame(), E);
    let gate = (mode == WetAdmission::Late).then(|| WorkerGate::for_pitch(&action));
    let prepared_worker = accepted
        .as_ref()
        .map(|_| WorkerGate::for_pitch_return(&action));
    assert!(engine.publish_pitch_action(&action).unwrap());
    let start = Instant::now();
    let mut ready = None;
    let mut incumbent = first.pads[0].native_state;
    for end in (F + FRAMES..=E).step_by(FRAMES as usize) {
        let tick = stream.test_tick().unwrap();
        assert_eq!(tick.output_frame, end);
        if end == F + FRAMES
            && let Some(gate) = &gate
        {
            gate.wait_for(false);
        }
        if end == F + FRAMES
            && let Some(worker) = &prepared_worker
        {
            assert!(worker.wait_for(true).is_none());
            let (wet_frames, shifts) = worker.prepared_wet();
            assert_eq!(shifts, 1);
            assert!((1..FRAMES as usize).contains(&wet_frames));
            worker.release();
        }
        if action.state() == "prepared" && ready.is_none() {
            ready = Some((end, start.elapsed()));
        }
        if tempo_case && end == E {
            incumbent = tick.pads[0].native_state;
            assert!(tick.pads[1].active);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(ready.is_some(), mode != WetAdmission::Late);
    assert_eq!(action.execution_target(), E);
    assert_eq!(submission.prefix(), (E, false));
    if matches!(mode, WetAdmission::Stop | WetAdmission::Late) {
        assert!(enqueue_stop_with_producer(
            ownership,
            &mut stream.producer.lock().unwrap(),
            Some(0)
        ));
    }
    if matches!(mode, WetAdmission::Tempo | WetAdmission::Late) {
        rate(1.2);
    }
    if tempo_case {
        // A genuine independent STOP drains the aligned SET before the parameter pass.
        assert!(enqueue_stop_with_producer(
            ownership,
            &mut stream.producer.lock().unwrap(),
            Some(1)
        ));
    }
    let mut replacement = None;
    let mut submitted_pcm = None;
    let tick = if let Some(source) = accepted.as_ref().filter(|_| retained_case) {
        use std::sync::mpsc::sync_channel;

        // The vacant STOP makes the original SET run inside the control drain.
        // Without it, SET runs after the drain and B can only be accepted next tick.
        if mode == WetAdmission::Retained {
            assert!(
                stream
                    .producer
                    .lock()
                    .unwrap()
                    .push(ControlMessage::StopSample { id: 1 })
                    .is_ok()
            );
        }
        std::thread::scope(|scope| {
            let (claimed, wait_claim) = sync_channel(1);
            let (published, wait_publication) = sync_channel(1);
            let callback = scope.spawn(move || {
                stream
                    .test_tick_with_effect(Some(Box::new(move || {
                        if claimed.send(()).is_ok() {
                            let _ = wait_publication.recv_timeout(WAIT);
                        }
                    })))
                    .unwrap()
            });
            wait_claim.recv_timeout(WAIT).unwrap();
            assert!(action.effective());
            let next = ColdPcm::publish(&engine, 2, "b");
            next.assert_ack(&engine, false);
            assert_ne!(source.publication.key(), next.publication.key());
            assert_ne!(source.binding.source_address, next.binding.source_address);
            factory.arm_output_ready();
            published.send(()).unwrap();
            let gated = until(|| {
                factory
                    .gated_output_pcm()
                    .map(Some)
                    .or_else(|| callback.is_finished().then_some(None))
            });
            let prefix_before_ready = submission.prefix();
            if let Some(pcm) = gated {
                next.assert_ack(&engine, mode == WetAdmission::Retained);
                factory.release_output_ready(false);
                submitted_pcm = Some(pcm);
            }
            let tick = callback.join().unwrap();
            next.assert_ack(&engine, mode == WetAdmission::Retained);
            assert_eq!(
                tick.pads[0].bank_source_pcm,
                if mode == WetAdmission::Retained {
                    next.binding.source_address
                } else {
                    source.binding.source_address
                }
            );
            assert_eq!(prefix_before_ready, (E, false));
            assert!(
                submitted_pcm.is_some(),
                "accepted retained Wet never reached outputReady"
            );
            replacement = Some(next);
            tick
        })
    } else if tempo_case {
        use std::sync::mpsc::sync_channel;

        factory.arm_output_ready();
        std::thread::scope(|scope| {
            let (claimed, wait_claim) = sync_channel(1);
            let (published, wait_publication) = sync_channel(1);
            let after_claim = (mode == WetAdmission::TempoAfterClaim).then(|| {
                Box::new(move || {
                    if claimed.send(()).is_ok() {
                        let _ = wait_publication.recv_timeout(WAIT);
                    }
                }) as Box<dyn FnOnce() + Send>
            });
            let callback = scope.spawn(move || stream.test_tick_with_effect(after_claim).unwrap());
            if mode == WetAdmission::TempoAfterClaim {
                wait_claim.recv_timeout(WAIT).unwrap();
                assert!(action.effective());
                rate(1.2);
                published.send(()).unwrap();
            }
            submitted_pcm = until(|| {
                factory
                    .gated_output_pcm()
                    .map(Some)
                    .or_else(|| callback.is_finished().then_some(None))
            });
            let prefix_before_ready = submission.prefix();
            if submitted_pcm.is_some() {
                factory.release_output_ready(false);
            }
            let tick = callback.join().unwrap();
            assert_eq!(prefix_before_ready, (E, false));
            assert!(
                submitted_pcm.is_some(),
                "tempo case never reached outputReady"
            );
            tick
        })
    } else {
        stream.test_tick().unwrap()
    };
    eprintln!(
        "Wet {mode:?} T=E={E} ready={ready:?} result={} shifts={} callback={:?}",
        tick.output_frame, tick.native.shifts, tick.callback,
    );
    assert_eq!(tick.output_frame, E + FRAMES);
    assert_eq!(submission.prefix(), (E + FRAMES, false));
    if tempo_case {
        let status = ownership.global_rate_status.read().unwrap();
        let source = accepted.as_ref().unwrap();
        source.assert_ack(&engine, true);
        assert!(tick.pads[0].active && !tick.pads[1].active);
        assert_eq!(tick.pads[0].generation, original_birth);
        assert_eq!(tick.pads[0].source_pcm, source.binding.source_address);
        assert!(!stream.stream_failed.load(Ordering::Acquire));
        if mode == WetAdmission::Tempo {
            assert_eq!(status.effect.execution_frame, Some(E));
            assert_eq!(status.request_revision, status.effect.revision);
            assert_eq!(status.effect.state, RateState::Applied);
            assert_eq!(status.effect.effective.speed.to_bits(), 1.2_f64.to_bits());
            assert_eq!(action.state(), "rejected");
            assert_eq!(action.reason(), Some("timing_changed"));
            assert!(!action.effective());
            assert_eq!(tick.pads[0].native_state, incumbent);
        } else {
            assert!(status.request_revision > status.effect.revision);
            assert_eq!(status.requested.speed.to_bits(), 1.2_f64.to_bits());
            assert_eq!(status.effect.effective.speed.to_bits(), 1.4_f64.to_bits());
            assert_ne!(status.effect.execution_frame, Some(E));
            assert!(action.effective());
            assert_eq!(tick.native.shifts, 0);
            assert!(tick.pads[0].wet_current_source_output && !tick.pads[0].wet_failed);
            assert_eq!(
                tick.pads[0].native_state,
                prepared_worker.as_ref().unwrap().prepared_native()
            );
        }
    } else {
        assert_eq!(tick.native.shifts, u64::from(mode == WetAdmission::Inline));
        assert_eq!(
            tick.pads[0].active,
            !matches!(mode, WetAdmission::Stop | WetAdmission::Late)
        );
        if mode == WetAdmission::Late {
            let status = ownership.global_rate_status.read().unwrap();
            assert_eq!(status.effect.execution_frame, Some(E));
            assert_eq!(action.state(), "rejected");
            assert!(!action.effective());
        } else {
            assert!(action.effective());
            assert_eq!(
                tick.pads[0].wet_current_source_output,
                mode != WetAdmission::Stop
            );
        }
    }
    if let Some(gate) = gate {
        gate.release();
        assert!(gate.wait_for(true).is_some());
    }
    {
        let suffix = stream.test_tick().unwrap();
        assert_eq!(suffix.output_frame, E + 2 * FRAMES);
        assert_eq!(submission.prefix(), (E + 2 * FRAMES, false));
        if mode == WetAdmission::TempoAfterClaim {
            let status = ownership.global_rate_status.read().unwrap();
            assert_eq!(status.request_revision, status.effect.revision);
            assert_eq!(status.effect.execution_frame, Some(E + FRAMES));
            assert_eq!(status.effect.state, RateState::Applied);
            assert_eq!(status.effect.effective.speed.to_bits(), 1.2_f64.to_bits());
        }
        if let Some(source) = accepted.as_ref().filter(|_| retained_case) {
            let next = replacement.as_ref().unwrap();
            next.assert_ack(&engine, true);
            assert_eq!(suffix.pads[0].bank_source_pcm, next.binding.source_address);
            let worker = prepared_worker.as_ref().unwrap();
            let native = worker.prepared_native();
            let (wet_frames, shifts) = worker.prepared_wet();
            eprintln!(
                "Retained Wet F={F} T=E={E} prepared=[{E},{}) submitted=[{E},{}) worker shifts={shifts} native={native:#x}",
                E + wet_frames as u64,
                E + FRAMES
            );
            assert!(!ownership.binding_source_adoption_accepted(0, source.binding));
            assert_eq!(
                ownership.binding_source_generation(0, next.binding),
                Some(2)
            );
            for pad in [tick.pads[0], suffix.pads[0]] {
                assert!(pad.active && pad.wet_current_source_output && !pad.wet_failed);
                assert_eq!(pad.generation, original_birth);
                assert_eq!(pad.source_pcm, source.binding.source_address);
                assert_eq!(pad.native_state, native);
            }
            assert_eq!(
                suffix.pads[0].productive_fed_frames - tick.pads[0].productive_fed_frames,
                FRAMES
            );
            let next_shift = (3..=7)
                .find_map(|block| {
                    let row = stream.test_tick().unwrap();
                    assert_eq!(row.output_frame, E + block * FRAMES);
                    assert_eq!(row.pads[0].native_state, native);
                    assert!(row.pads[0].wet_current_source_output);
                    (row.native.shifts == 1).then_some(row.output_frame)
                })
                .expect("the retained Native must make its next shift");
            assert!(source.reader.upgrade().is_some() && next.reader.upgrade().is_some());
            assert!(enqueue_stop_with_producer(
                ownership,
                &mut stream.producer.lock().unwrap(),
                Some(0)
            ));
            let stopped = stream.test_tick().unwrap();
            assert_eq!(stopped.output_frame, next_shift + FRAMES);
            assert!(!stopped.pads[0].active);
            assert_eq!(stopped.pads[0].source_pcm, next.binding.source_address);
            until(|| source.reader.upgrade().is_none().then_some(()));
            assert!(next.reader.upgrade().is_some());
        }
    }
    engine.shut_down().unwrap();
    let status = until(|| {
        let mut slot = engine.mix_capture_lock().unwrap();
        let capture = slot.as_mut().unwrap();
        capture.poll(false);
        let status = capture.control.status();
        (status.worker_joined && status.rt_released && engine.pitch_work_capacity.used() == 0)
            .then_some(status)
    });
    let ended = owner.lock().unwrap();
    assert!(ended.owner_ended && ended.owner_join_verified && ended.retirement_verified);
    assert_eq!(factory.info().unwrap().active, 0);
    assert_eq!(status.first_frame, Some(E - FRAMES));
    assert_eq!(status.end_frame, Some(capture_end));
    assert!(status.successful_spool());
    assert_eq!(status.failure, None);
    assert_eq!(status.written_frames, capture_end - (E - FRAMES));
    let pcm = fs::read(&path)
        .unwrap()
        .split_off(SPOOL_HEADER_BYTES as usize);
    assert_eq!(pcm.len(), status.written_frames as usize * 8);
    if let Some(submitted) = submitted_pcm {
        let submitted = &submitted[..((capture_end - E) * 2) as usize];
        assert!(
            submitted
                .iter()
                .any(|sample| sample.is_finite() && *sample != 0.0)
        );
        assert_eq!(
            pcm[96 * 8..],
            submitted
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect::<Vec<_>>()
        );
    }
    fs::remove_file(&path).unwrap();
    fs::remove_dir(&directory).unwrap();
    drop(binding);
    drop(action);
    drop(engine);
    assert!(weak.upgrade().is_none());
    if let Some(source) = stopped_source {
        assert!(source.reader.upgrade().is_none());
    }
    if let Some(next) = replacement {
        assert!(next.reader.upgrade().is_none());
    }
    pcm
}

#[test]
fn asio_admits_retained_pitch_owner_wet_shift_after_new_bank_ack() {
    Python::initialize();
    wet_admission_path(WetAdmission::Retained);
}

#[test]
fn asio_preserves_claimed_wet_while_new_bank_awaits_own_ack() {
    Python::initialize();
    wet_admission_path(WetAdmission::PendingBank);
}

#[test]
fn asio_observes_first_due_shift_on_the_original_ordinary_pad_native() {
    ordinary_first_due_shift(None);
}

#[test]
fn asio_ordinary_first_shift_uses_tempo_at_the_actual_request() {
    let (steady, steady_input) = ordinary_first_due_shift(Some(false));
    let (changed, changed_input) = ordinary_first_due_shift(Some(true));
    let prefix_bytes = steady.len() - 2 * size_of::<f32>();
    assert_eq!(&steady[..prefix_bytes], &changed[..prefix_bytes]);
    assert_ne!(steady_input, changed_input);
}

fn ordinary_first_due_shift(paced_tempo_edit: Option<bool>) -> (Vec<u8>, [u32; 2]) {
    use crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::WorkerGate;
    use crate::audio_engine::pitch_intent::PitchIntent;

    const FRAMES: u64 = 96;
    const E: u64 = 4096;
    const EARLY: u64 = 4128;
    const C: u64 = 4320;
    const DUE: u64 = 4512;
    const END: u64 = DUE + FRAMES;
    const PERIOD: Duration = Duration::from_millis(1);
    Python::initialize();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ordinary.capture");
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
    engine.set_trigger_quantization("1/64").unwrap();
    engine
        .set_master_period(E as f64 * 16.0 / 96_000.0)
        .unwrap();
    let stream = engine.stream_handle.as_ref().unwrap();
    let owner = stream.test_owner_observer();
    let submission = stream.output_submission.as_ref().unwrap().clone();
    let source = ColdPcm::publish_with_pitch(
        &engine,
        1,
        "a",
        SourcePitchInitialization::RestoredTuple {
            pitch: PitchIntent::new(0, 3).unwrap(),
            key_lock: Some(false),
        },
    );
    source.assert_ack(&engine, false);
    assert_eq!(stream.test_tick().unwrap().output_frame, FRAMES);
    source.assert_ack(&engine, true);
    let start = engine
        .capture_pad_start(0, 0.0, None, 1.0, false, None, Some(1))
        .unwrap();
    assert_eq!(stream.test_tick().unwrap().output_frame, 2 * FRAMES);
    assert_eq!(start.original_frame(), Some(E));
    let worker = WorkerGate::for_ordinary(&start);
    assert!(engine.publish_pad_start(&start, None).unwrap());
    assert_eq!(stream.test_tick().unwrap().output_frame, 3 * FRAMES);
    worker.wait_for(false);
    worker.release();
    assert_eq!(worker.wait_for(true), None);
    let native = worker.prepared_native();
    let ticket = engine
        .prepare_mix_capture_internal(path.to_str().unwrap(), FRAMES as usize, 4, 4096)
        .unwrap();
    until(|| {
        engine
            .mix_capture_lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .control
            .status()
            .writer_ready
            .then_some(())
    });
    assert!(engine.start_mix_capture_internal(&ticket, C).unwrap());
    assert!(engine.stop_mix_capture_internal(&ticket, END).unwrap());

    let mut deadline = None;
    let mut early_entry = None;
    let mut skipped_slots = 0;
    let mut pace = |first| {
        if paced_tempo_edit.is_none() || first < EARLY {
            return;
        }
        let target = deadline.unwrap_or_else(Instant::now);
        if let Some(delay) = target.checked_duration_since(Instant::now()) {
            std::thread::sleep(delay);
        }
        let now = Instant::now();
        early_entry.get_or_insert(now);
        let elapsed_slots = now.saturating_duration_since(target).as_millis() as u32;
        skipped_slots += elapsed_slots;
        deadline = Some(target + PERIOD * (elapsed_slots + 1));
    };
    let mut fed_before_shift = 0;
    for end in (4 * FRAMES..=DUE).step_by(FRAMES as usize) {
        pace(end - FRAMES);
        let tick = stream.test_tick().unwrap();
        assert_eq!(tick.output_frame, end);
        assert_eq!(submission.prefix(), (end, false));
        assert_eq!(tick.native.shifts, 0);
        let pad = tick.pads[0];
        assert_eq!(pad.active, end > E);
        if end > E {
            assert_eq!(pad.native_state, native);
            assert_eq!(pad.source_pcm, source.binding.source_address);
            assert!(pad.wet_current_source_output && !pad.wet_failed);
            let fed = end - E;
            assert_eq!(pad.frame as u64, fed);
            assert_eq!(pad.fraction, 0.0);
            assert_eq!(pad.pending_fifo_frames, (fed as usize, 511 - fed as usize));
            assert_eq!(pad.native_block_size, 512);
            fed_before_shift = pad.productive_fed_frames;
        }
    }
    assert_eq!(start.execution_frame(), Some(E));
    assert_eq!(start.effective_frame(), Some(E));
    let ownership = &engine.input_runtime_ownership;
    if paced_tempo_edit == Some(true) {
        push_global_rate_message(
            &ownership.global_timing_revision,
            &ownership.global_timing_publishing,
            &ownership.global_rate_status,
            &mut stream.parameter_producer.lock().unwrap(),
            Some(1.2),
            None,
            "ordinary ASIO first shift tempo",
        )
        .unwrap();
    }
    pace(DUE);
    let (tick, submitted_pcm) = if paced_tempo_edit.is_some() {
        let measured_lead = early_entry.unwrap().elapsed();
        let tick = stream.test_tick().unwrap();
        eprintln!(
            "Ordinary paced ASIO tempo={paced_tempo_edit:?} lead={measured_lead:?} skipped_slots={skipped_slots} shift={:?} callback={:?}",
            tick.native, tick.callback
        );
        (tick, None)
    } else {
        std::thread::scope(|scope| {
            factory.arm_output_ready();
            let callback = scope.spawn(|| stream.test_tick().unwrap());
            let submitted = until(|| factory.gated_output_pcm());
            let prefix = submission.prefix();
            factory.release_output_ready(false);
            let tick = callback.join().unwrap();
            assert_eq!(prefix, (DUE, false));
            (tick, Some(submitted))
        })
    };
    if paced_tempo_edit == Some(true) {
        let status = ownership.global_rate_status.read().unwrap();
        assert_eq!(status.effect.execution_frame, Some(DUE));
        assert_eq!(status.request_revision, status.effect.revision);
        assert!(tick.pads[0].frame as u64 > END - E || tick.pads[0].fraction > 0.0);
    }
    assert_eq!(tick.output_frame, END);
    assert_eq!(submission.prefix(), (END, false));
    assert_eq!(tick.native.shifts, 1);
    let dependency = tick.native.first_shift_dependency.unwrap();
    assert_eq!(dependency.ready_prefix, 95);
    assert_eq!(tick.pads[0].native_state, native);
    assert_eq!(tick.pads[0].pending_fifo_frames, (0, 511));
    assert_eq!(
        tick.pads[0].productive_fed_frames - fed_before_shift,
        FRAMES
    );
    eprintln!(
        "Ordinary PAD T=E={E} S=0 C={C} FIFO=(224,287) first shift=[{DUE},{END}) Native={native:#x}",
    );

    assert!(enqueue_stop_with_producer(
        ownership,
        &mut stream.producer.lock().unwrap(),
        Some(0)
    ));
    let stopped = stream.test_tick().unwrap();
    assert_eq!(stopped.output_frame, END + FRAMES);
    assert!(!stopped.pads[0].active);
    assert_eq!(stopped.native.shifts, 0);
    engine.shut_down().unwrap();
    let status = until(|| {
        let mut slot = engine.mix_capture_lock().unwrap();
        let capture = slot.as_mut().unwrap();
        capture.poll(false);
        let status = capture.control.status();
        (status.worker_joined && status.rt_released).then_some(status)
    });
    assert!(status.successful_spool());
    assert_eq!((status.first_frame, status.end_frame), (Some(C), Some(END)));
    assert_eq!(status.written_frames, END - C);
    let captured = fs::read(&path)
        .unwrap()
        .split_off(SPOOL_HEADER_BYTES as usize);
    if let Some(submitted_pcm) = submitted_pcm {
        let submitted_bytes = submitted_pcm
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        assert_eq!(&captured[(DUE - C) as usize * 8..], submitted_bytes);
    }
    let ended = owner.lock().unwrap();
    assert!(ended.owner_ended && ended.owner_join_verified && ended.retirement_verified);
    assert_eq!(factory.info().unwrap().active, 0);
    drop(start);
    drop(engine);
    assert!(source.reader.upgrade().is_none());
    (captured, dependency.final_input_bits)
}

#[test]
fn asio_admits_original_pitch_owner_wet_shift_after_controls() {
    Python::initialize();
    let inline = wet_admission_path(WetAdmission::Inline);
    assert!(
        inline[96 * 8..]
            .chunks_exact(4)
            .map(|sample| f32::from_le_bytes(sample.try_into().unwrap()))
            .any(|sample| sample.is_finite() && sample != 0.0)
    );
    assert_eq!(wet_admission_path(WetAdmission::Prepared), inline);
    assert_eq!(
        wet_admission_path(WetAdmission::Split),
        inline[..(96 + 17) * 8]
    );
    let stopped = wet_admission_path(WetAdmission::Stop);
    assert_eq!(stopped[..96 * 8], inline[..96 * 8]);
    assert!(stopped[96 * 8..].iter().all(|byte| *byte == 0));
    assert_eq!(wet_admission_path(WetAdmission::Late), stopped);
}

#[test]
fn asio_wet_set_uses_exact_scalar_tempo_ingress() {
    Python::initialize();
    wet_admission_path(WetAdmission::Tempo);
    wet_admission_path(WetAdmission::TempoAfterClaim);
}

#[test]
fn asio_admits_late_loop_refresh_at_the_end_of_claimed_wet() {
    aligned_wet_set_with_ordered_control(OrderedWetControl::Loop);
}

#[test]
fn asio_refuses_wet_set_for_a_staged_bpm_lock_that_changes_its_rate() {
    aligned_wet_set_with_ordered_control(OrderedWetControl::StagedBpm(0.75));
}

#[test]
fn asio_admits_wet_set_when_staged_bpm_lock_preserves_its_rate() {
    aligned_wet_set_with_ordered_control(OrderedWetControl::StagedBpm(0.5));
}

#[test]
fn asio_applies_bpm_behind_stop_after_affected_wet() {
    aligned_wet_set_with_ordered_control(OrderedWetControl::BpmBehindStop(0.75));
}

#[test]
fn asio_applies_same_rate_bpm_behind_stop_at_callback_start() {
    aligned_wet_set_with_ordered_control(OrderedWetControl::BpmBehindStop(0.5));
}

#[test]
fn asio_applies_bpm_off_behind_stop_after_affected_wet() {
    aligned_wet_set_with_ordered_control(OrderedWetControl::BpmOff {
        speed: 1.0,
        behind_stop: true,
    });
}

#[test]
fn asio_applies_bpm_off_at_callback_start_when_rate_is_unchanged() {
    aligned_wet_set_with_ordered_control(OrderedWetControl::BpmOff {
        speed: 1.4,
        behind_stop: true,
    });
}

#[test]
fn asio_applies_bpm_off_staged_before_stop_after_affected_wet() {
    aligned_wet_set_with_ordered_control(OrderedWetControl::BpmOff {
        speed: 1.0,
        behind_stop: false,
    });
}

#[test]
fn asio_admits_coupled_master_refresh_after_affected_wet() {
    aligned_wet_set_with_ordered_control(OrderedWetControl::MasterRefresh(0.75));
}

#[test]
fn asio_admits_same_rate_master_refresh_at_the_first_eligible_frame() {
    aligned_wet_set_with_ordered_control(OrderedWetControl::MasterRefresh(0.7));
}

#[test]
fn asio_later_master_refresh_supersedes_foreign_wet_bound_ingress() {
    let reference =
        aligned_wet_set_with_ordered_control(OrderedWetControl::OrderedMasterRefresh(false));
    let ordered =
        aligned_wet_set_with_ordered_control(OrderedWetControl::OrderedMasterRefresh(true));
    assert_eq!(ordered, reference);
}

#[test]
fn asio_bound_master_refresh_preserves_independent_capture_stop() {
    for stop_offset in [93, 96] {
        let reference = aligned_wet_set_with_ordered_control(
            OrderedWetControl::MasterRefreshHistory(false, stop_offset),
        );
        let pending = aligned_wet_set_with_ordered_control(
            OrderedWetControl::MasterRefreshHistory(true, stop_offset),
        );
        assert_eq!(pending, reference);
    }
}

#[test]
fn asio_source_revoked_bound_refresh_preserves_original_interior_wet_set() {
    let reference =
        aligned_wet_set_with_ordered_control(OrderedWetControl::RevokedInteriorRefresh(false));
    let revoked =
        aligned_wet_set_with_ordered_control(OrderedWetControl::RevokedInteriorRefresh(true));
    assert_eq!(revoked, reference);
    let current = aligned_wet_set_with_ordered_control(OrderedWetControl::CurrentInteriorRefresh);
    assert_ne!(current, reference);
}

#[derive(Clone, Copy, Debug)]
enum OrderedWetControl {
    Loop,
    StagedBpm(f64),
    BpmBehindStop(f64),
    BpmOff { speed: f64, behind_stop: bool },
    MasterRefresh(f64),
    OrderedMasterRefresh(bool),
    MasterRefreshHistory(bool, u64),
    RevokedInteriorRefresh(bool),
    CurrentInteriorRefresh,
}

fn aligned_wet_set_with_ordered_control(control: OrderedWetControl) -> Vec<u32> {
    use crate::audio_engine::constant_timing::l3_constant_timing_fixture::publish_synthetic_loaded_timing;
    use crate::audio_engine::constants::NUM_SAMPLES;
    use crate::audio_engine::global_rate_status::BpmReason;
    use crate::audio_engine::input_mapping::PitchPadProjection;
    use crate::audio_engine::input_runtime_binding;
    use crate::audio_engine::master_provenance::{self, MasterProvenance};
    use crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::WorkerGate;
    use std::sync::mpsc::sync_channel;

    const FRAMES: u64 = 96;
    const RATE: u32 = 96_000;
    const F: u64 = 18_240;
    const E: u64 = 24_000;
    let capture_end = match control {
        OrderedWetControl::MasterRefreshHistory(_, offset) => E + offset,
        _ => E + 93,
    };
    const OLD_END: f64 = 90.0;
    const NEW_START: f64 = 8.0;
    let seconds = |frame| frame / f64::from(RATE);
    let anchor_period = match control {
        OrderedWetControl::Loop => None,
        OrderedWetControl::StagedBpm(period) | OrderedWetControl::BpmBehindStop(period) => {
            Some(period)
        }
        OrderedWetControl::BpmOff { speed, .. } => Some(0.5 * speed),
        OrderedWetControl::MasterRefresh(_)
        | OrderedWetControl::OrderedMasterRefresh(_)
        | OrderedWetControl::MasterRefreshHistory(..)
        | OrderedWetControl::RevokedInteriorRefresh(_)
        | OrderedWetControl::CurrentInteriorRefresh => Some(0.7),
    };
    let off_speed = match control {
        OrderedWetControl::BpmOff { speed, .. } => Some(speed),
        _ => None,
    };
    let history = matches!(control, OrderedWetControl::MasterRefreshHistory(..));
    let interior = matches!(
        control,
        OrderedWetControl::RevokedInteriorRefresh(_) | OrderedWetControl::CurrentInteriorRefresh
    );
    let current_refresh = matches!(control, OrderedWetControl::CurrentInteriorRefresh);
    let ordered_refresh = matches!(control, OrderedWetControl::OrderedMasterRefresh(_));
    let master_refresh = history
        || interior
        || ordered_refresh
        || matches!(control, OrderedWetControl::MasterRefresh(_));
    let prelocked = master_refresh || off_speed.is_some();
    let capture_frame = if prelocked { 23_040 } else { F };

    Python::initialize();
    let directory = tempfile::tempdir().unwrap().keep();
    let path = directory.join("take.capture");
    eprintln!("ASIO ordered Wet {control:?} spool: {}", path.display());
    let factory = Factory::new().unwrap();
    let mut engine = AudioEngine::new().unwrap();
    engine.test_asio_driver = Some(factory.clone());
    engine
        .start_audio_output(OutputOptions {
            backend: "asio".into(),
            sample_rate_hz: Some(RATE),
            buffer_frames: Some(FRAMES as u32),
            ..Default::default()
        })
        .unwrap();
    if interior {
        engine.set_master_period(0.5).unwrap();
    }
    let a = ColdPcm::publish_with_geometry(
        &engine,
        0,
        1,
        "a",
        SourcePitchInitialization::FreshNeutral,
        25_600,
        Some((0, OLD_END as usize)),
    );
    let b = ColdPcm::publish_at(&engine, 1, 1, "b", SourcePitchInitialization::FreshNeutral);
    let wet_peer = interior.then(|| {
        ColdPcm::publish_with_geometry(
            &engine,
            4,
            1,
            "d",
            SourcePitchInitialization::FreshNeutral,
            25_600,
            Some((0, OLD_END as usize)),
        )
    });
    let dry_peer = history.then(|| {
        ColdPcm::publish_with_geometry(
            &engine,
            4,
            1,
            "d",
            SourcePitchInitialization::FreshNeutral,
            48_000,
            None,
        )
    });
    let later_source = ordered_refresh.then(|| {
        ColdPcm::publish_with_geometry(
            &engine,
            4,
            1,
            "e",
            SourcePitchInitialization::FreshNeutral,
            48_000,
            Some((0, OLD_END as usize)),
        )
    });
    let anchor_source = anchor_period.map(|_| {
        ColdPcm::publish_with_geometry(
            &engine,
            2,
            1,
            "c",
            SourcePitchInitialization::FreshNeutral,
            48_000,
            prelocked.then_some((0, OLD_END as usize)),
        )
    });
    let stream = engine.stream_handle.as_ref().unwrap();
    let owner = stream.test_owner_observer();
    let submission = stream.output_submission.as_ref().unwrap().clone();
    if anchor_source.is_some() {
        engine.set_bpm_lock_with_binding(false, None).unwrap();
    }
    for id in [0, 1] {
        stream
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::PlaySample {
                id,
                volume: 1.0,
                received_at_ns: None,
                expected_source_generation: Some(1),
            })
            .unwrap();
    }
    let playing = stream.test_tick().unwrap();
    assert_eq!(playing.output_frame, FRAMES);
    assert!(playing.pads[0].active && playing.pads[1].active);
    a.assert_ack(&engine, true);
    b.assert_ack(&engine, true);
    assert!(!playing.pads[2].active);
    if let Some(source) = &anchor_source {
        source.assert_ack(&engine, true);
        engine.set_pad_timing_intent(2, "automatic").unwrap();
    }
    engine.set_pad_timing_intent(0, "automatic").unwrap();
    let timing = publish_synthetic_loaded_timing(
        &engine,
        &stream.producer,
        0,
        if prelocked { 0.7 } else { 0.5 },
    );
    assert_eq!(timing.publication_status().unwrap(), "pending");
    let anchor_timing = anchor_period.map(|period| {
        let timing = publish_synthetic_loaded_timing(&engine, &stream.producer, 2, period);
        assert_eq!(timing.publication_status().unwrap(), "pending");
        timing
    });
    let dry_timing = dry_peer.as_ref().map(|_| {
        engine.set_pad_timing_intent(4, "automatic").unwrap();
        publish_synthetic_loaded_timing(&engine, &stream.producer, 4, 0.5)
    });
    let wet_timing = wet_peer.as_ref().map(|_| {
        engine.set_pad_timing_intent(4, "automatic").unwrap();
        publish_synthetic_loaded_timing(&engine, &stream.producer, 4, 0.7)
    });
    let later_timing = later_source.as_ref().map(|_| {
        engine.set_pad_timing_intent(4, "automatic").unwrap();
        publish_synthetic_loaded_timing(&engine, &stream.producer, 4, 0.7)
    });
    assert_eq!(stream.test_tick().unwrap().output_frame, 2 * FRAMES);
    assert_eq!(timing.publication_status().unwrap(), "accepted");
    let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
    assert!(binding.current() && binding.available() && binding.binding.accepted.is_some());
    let mut anchor = anchor_timing.as_ref().map(|timing| {
        assert_eq!(timing.publication_status().unwrap(), "accepted");
        let binding = input_runtime_binding::capture(&engine, 2).unwrap().unwrap();
        assert!(binding.current() && binding.available() && binding.binding.accepted.is_some());
        binding
    });
    let dry_binding = dry_timing.as_ref().map(|timing| {
        assert_eq!(timing.publication_status().unwrap(), "accepted");
        let binding = input_runtime_binding::capture(&engine, 4).unwrap().unwrap();
        assert!(binding.current() && binding.available());
        assert_eq!(binding.binding.accepted.unwrap().period_seconds, 0.5);
        engine
            .set_pad_key_lock_with_source(4, true, Some(1))
            .unwrap();
        binding
    });
    let later_binding = later_timing.as_ref().map(|timing| {
        assert_eq!(timing.publication_status().unwrap(), "accepted");
        later_source.as_ref().unwrap().assert_ack(&engine, true);
        let binding = input_runtime_binding::capture(&engine, 4).unwrap().unwrap();
        assert!(binding.current() && binding.available());
        assert_eq!(binding.binding.accepted.unwrap().period_seconds, 0.7);
        binding
    });
    let stream = engine.stream_handle.as_ref().unwrap();
    let runtime = engine.input_runtime.as_ref().unwrap();
    let ownership = &engine.input_runtime_ownership;
    let wet_binding = wet_peer.as_ref().map(|source| {
        source.assert_ack(&engine, true);
        assert_eq!(
            wet_timing.as_ref().unwrap().publication_status().unwrap(),
            "accepted"
        );
        let binding = input_runtime_binding::capture(&engine, 4).unwrap().unwrap();
        assert_eq!(binding.binding.accepted.unwrap().period_seconds, 0.7);
        binding
    });
    let mut early_a = None;
    let mut interior_action = None;
    if let Some(d) = &wet_binding {
        let mut loaded = vec![false; NUM_SAMPLES];
        let mut bindings = vec![None; NUM_SAMPLES];
        let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
        for (id, original) in [(0, &binding), (4, d)] {
            loaded[id] = true;
            bindings[id] = Some(original);
            rows[id].quantize_step_64ths = Some(8);
        }
        runtime
            .set_runtime_state_with_pitch(
                true,
                loaded,
                vec![0.0; NUM_SAMPLES],
                vec![None; NUM_SAMPLES],
                bindings,
                rows,
            )
            .unwrap();
    }
    let mut next_tick = 3 * FRAMES;
    let prior_lock = if prelocked {
        // Keep the real varispeed glide in the worker's replay history and
        // settle A before capturing the aligned SET.
        let glide_tick = capture_frame - 41 * FRAMES;
        let lock_tick = if off_speed == Some(1.0) {
            glide_tick
        } else {
            capture_frame - 2 * FRAMES
        };
        let previous = anchor.as_ref().unwrap();
        let original_source = master_provenance::source_identity(previous).unwrap();
        for end in (next_tick..=lock_tick).step_by(FRAMES as usize) {
            if interior && end == 1056 + FRAMES {
                stream
                    .producer
                    .lock()
                    .unwrap()
                    .push(ControlMessage::PlaySample {
                        id: 4,
                        volume: 1.0,
                        received_at_ns: None,
                        expected_source_generation: Some(1),
                    })
                    .unwrap();
            }
            if end == glide_tick && off_speed != Some(1.0) {
                push_global_rate_message(
                    &ownership.global_timing_revision,
                    &ownership.global_timing_publishing,
                    &ownership.global_rate_status,
                    &mut stream.parameter_producer.lock().unwrap(),
                    Some(1.4),
                    None,
                    "ASIO ordered Wet prelock speed",
                )
                .unwrap();
            }
            if end == lock_tick {
                engine
                    .set_bpm_lock_with_binding(true, Some(previous))
                    .unwrap();
            }
            let observed = stream.test_tick().unwrap();
            assert_eq!(observed.output_frame, end);
            if interior && end == 1056 + FRAMES {
                let born = observed.pads[4];
                assert!(born.active && !born.key_lock && !born.active_native);
                assert_eq!(born.generation, 1);
                assert_eq!(
                    born.source_pcm,
                    wet_peer.as_ref().unwrap().binding.source_address
                );
                eprintln!("ASIO source revoked original D born=1056 owner={born:?}");
            }
            if interior && end == F + FRAMES {
                let applied = ownership.global_rate_status.read().unwrap();
                assert_eq!(applied.effect.execution_frame, Some(F));
                assert_eq!(applied.effect.state, RateState::Applied);
                assert_eq!(
                    applied.effect.effective.period,
                    Some(0.5 * (E + 16 - F) as f64 / (E - F) as f64)
                );
            }
            if interior && end == F {
                let clock = stream.output_clock.read().unwrap();
                let original = runtime
                    .capture_gui_pitch(&binding, "pitch.extra:3:set", clock.observed_at_ns)
                    .unwrap();
                assert_eq!(original.original_due_frame(), E);
                early_a = Some(original);
                // A real period edit preserves beat position and places D's
                // original half-beat at E+16. Both admissions precede all Work.
                let period = 0.5 * (E + 16 - F) as f64 / (E - F) as f64;
                push_global_rate_message(
                    &ownership.global_timing_revision,
                    &ownership.global_timing_publishing,
                    &ownership.global_rate_status,
                    &mut stream.parameter_producer.lock().unwrap(),
                    None,
                    Some(period),
                    "ASIO two-original Wet grid",
                )
                .unwrap();
            }
            if interior && end == 21_120 {
                let clock = stream.output_clock.read().unwrap();
                let original = runtime
                    .capture_gui_pitch(
                        wet_binding.as_ref().unwrap(),
                        "pitch.extra:3:set",
                        clock.observed_at_ns,
                    )
                    .unwrap();
                assert_eq!(original.original_due_frame(), E + 16);
                interior_action = Some(original);
            }
        }
        next_tick = lock_tick + FRAMES;
        let (revision, bpm) = ownership.global_rate_status.read_bpm().unwrap();
        assert_eq!(bpm.revision, revision);
        assert!(bpm.requested && bpm.effective && bpm.applied);
        let before = ownership.global_rate_status.read().unwrap();
        assert_eq!(bpm.execution_frame, Some(lock_tick - FRAMES));
        assert_eq!(before.effect.effective.speed, off_speed.unwrap_or(1.4));
        assert!((before.effect.effective.period.unwrap() - 0.5).abs() < 1e-12);
        assert_eq!(
            before.master_provenance,
            MasterProvenance::qualified(original_source, previous.binding)
        );
        Some((revision, bpm, before, original_source))
    } else {
        None
    };
    let refresh_period = match control {
        OrderedWetControl::MasterRefresh(period) => Some(period),
        OrderedWetControl::OrderedMasterRefresh(_) => Some(0.75),
        OrderedWetControl::MasterRefreshHistory(..) => Some(0.75),
        OrderedWetControl::RevokedInteriorRefresh(_)
        | OrderedWetControl::CurrentInteriorRefresh => Some(0.75),
        _ => None,
    };
    let master = if let Some(period) = refresh_period {
        let (revision, bpm, before, original_source) = prior_lock.as_ref().unwrap();
        if history {
            let lock = ownership.key_lock_status(4).unwrap();
            assert!(lock.ready && lock.effective);
            stream
                .producer
                .lock()
                .unwrap()
                .push(ControlMessage::PlaySample {
                    id: 4,
                    volume: 1.0,
                    received_at_ns: None,
                    expected_source_generation: Some(1),
                })
                .unwrap();
        }
        let previous_timing = anchor.as_ref().unwrap().binding.accepted.unwrap();
        let newer = publish_synthetic_loaded_timing(&engine, &stream.producer, 2, period);
        assert_eq!(newer.publication_status().unwrap(), "pending");
        let born = stream.test_tick().unwrap();
        assert_eq!(born.output_frame, next_tick);
        if history {
            let dry = born.pads[4];
            assert!(dry.active && dry.key_lock && !dry.active_native);
            assert_eq!(dry.rate_target, 1.0);
            assert_eq!(dry.frame, FRAMES as usize);
            assert_eq!(
                dry.source_pcm,
                dry_peer.as_ref().unwrap().binding.source_address
            );
        }
        next_tick += FRAMES;
        assert_eq!(newer.publication_status().unwrap(), "accepted");
        let current = input_runtime_binding::capture(&engine, 2).unwrap().unwrap();
        assert!(current.current() && current.available());
        assert_eq!(
            master_provenance::source_identity(&current),
            Some(*original_source)
        );
        let accepted = current.binding.accepted.unwrap();
        assert_ne!(
            accepted.publication_epoch,
            previous_timing.publication_epoch
        );
        assert!((accepted.period_seconds - period).abs() < 1e-12);
        let after = ownership.global_rate_status.read().unwrap();
        assert_eq!(
            after.effect.effective.period,
            before.effect.effective.period
        );
        assert_eq!(after.master_provenance, before.master_provenance);
        let (still_requested, still_applied) = ownership.global_rate_status.read_bpm().unwrap();
        assert_eq!(
            (still_requested, still_applied.revision),
            (*revision, bpm.revision)
        );
        assert!(still_applied.applied && still_applied.effective);
        anchor = Some(current);
        Some((
            accepted.period_seconds / after.effect.effective.speed,
            *revision,
        ))
    } else {
        None
    };
    let mut loaded = vec![false; NUM_SAMPLES];
    loaded[0] = true;
    let mut bindings = vec![None; NUM_SAMPLES];
    bindings[0] = Some(&binding);
    loaded[2] = anchor.is_some();
    bindings[2] = anchor.as_ref();
    loaded[4] = history || ordered_refresh;
    bindings[4] = dry_binding.as_ref().or(later_binding.as_ref());
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    rows[0].quantize_step_64ths = Some(8);
    if !interior {
        runtime
            .set_runtime_state_with_pitch(
                true,
                loaded,
                vec![0.0; NUM_SAMPLES],
                vec![None; NUM_SAMPLES],
                bindings,
                rows,
            )
            .unwrap();
    }

    let capture = engine
        .prepare_mix_capture_internal(path.to_str().unwrap(), FRAMES as usize, 4, 4096)
        .unwrap();
    let capture_status = |engine: &AudioEngine| {
        let mut slot = engine.mix_capture_lock().unwrap();
        let state = slot.as_mut().unwrap();
        state.poll(engine.mix_capture_stream_failed());
        state.control.status()
    };
    until(|| capture_status(&engine).writer_ready.then_some(()));
    assert!(
        engine
            .start_mix_capture_internal(&capture, E - FRAMES)
            .unwrap()
    );
    assert!(
        engine
            .stop_mix_capture_internal(&capture, capture_end)
            .unwrap()
    );
    for end in (next_tick..=capture_frame).step_by(FRAMES as usize) {
        if end == capture_frame && !prelocked {
            push_global_rate_message(
                &ownership.global_timing_revision,
                &ownership.global_timing_publishing,
                &ownership.global_rate_status,
                &mut stream.parameter_producer.lock().unwrap(),
                Some(1.4),
                Some(0.5),
                "ASIO ordered Wet refresh",
            )
            .unwrap();
        }
        let tick = stream.test_tick().unwrap();
        assert_eq!(tick.output_frame, end);
        if history && end == capture_frame {
            assert!(tick.pads[4].active && tick.pads[4].key_lock && !tick.pads[4].active_native);
            assert_eq!(tick.pads[4].rate_target, 1.0);
            assert_eq!(
                tick.pads[4].source_pcm,
                dry_peer.as_ref().unwrap().binding.source_address
            );
        }
    }
    let clock = stream.output_clock.read().unwrap();
    assert!(clock.valid && clock.output_frame == capture_frame - FRAMES);
    let action = early_a.unwrap_or_else(|| {
        runtime
            .capture_gui_pitch(&binding, "pitch.extra:3:set", clock.observed_at_ns)
            .unwrap()
    });
    assert_eq!(action.original_due_frame(), E);
    let worker = WorkerGate::for_pitch_return(&action);
    let interior_worker = interior_action.as_ref().map(WorkerGate::for_pitch_return);
    assert!(engine.publish_pitch_action(&action).unwrap());
    if let Some(original) = &interior_action {
        assert!(engine.publish_pitch_action(original).unwrap());
    }
    assert_eq!(
        stream.test_tick().unwrap().output_frame,
        capture_frame + FRAMES
    );
    assert!(worker.wait_for(true).is_none());
    worker.release();
    assert_eq!(worker.prepared_wet(), (64, 1));
    if history || interior || ordered_refresh {
        assert_eq!(worker.prepared_fifo(), ((512, 448, 63), Some((64, 1))));
    }
    let original_native = worker.prepared_native();
    let interior_native = interior_worker.as_ref().map(|worker| {
        assert!(worker.wait_for(true).is_none());
        let fifo = worker.prepared_fifo();
        let native = worker.prepared_native();
        worker.release();
        eprintln!(
            "ASIO source revoked original D E={} fifo={fifo:?} native={native:#x}",
            E + 16
        );
        assert_eq!(fifo, ((512, 432, 79), Some((80, 1))));
        assert_ne!(native, original_native);
        native
    });
    let mut before = playing;
    let mut previous = before.pads[0];
    for end in (capture_frame + 2 * FRAMES..=E).step_by(FRAMES as usize) {
        previous = before.pads[0];
        before = stream.test_tick().unwrap();
        assert_eq!(before.output_frame, end);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(action.state(), "prepared");
    assert_eq!(action.execution_target(), E);
    if let Some(d) = &interior_action {
        assert_eq!(d.original_due_frame(), E + 16);
        assert_eq!(d.execution_target(), E + 16);
        assert_eq!(d.state(), "prepared");
        assert!(before.pads[4].active);
        assert_eq!(
            before.pads[4].source_pcm,
            wet_peer.as_ref().unwrap().binding.source_address
        );
    }
    assert_eq!(submission.prefix(), (E, false));
    assert!(before.pads[0].active && before.pads[1].active);
    if history {
        assert_eq!(before.capture_stop, Some(capture_end));
        assert!(before.pads[4].active && before.pads[4].key_lock && !before.pads[4].active_native);
        assert_eq!(before.pads[4].rate_target, 1.0);
        assert_eq!(
            before.pads[4].source_pcm,
            dry_peer.as_ref().unwrap().binding.source_address
        );
    }
    let position = |pad: crate::audio_engine::asio_output::test_driver::PadObservation| {
        pad.frame as f64 + pad.fraction
    };
    let at_e = position(before.pads[0]);
    if !prelocked {
        assert!((at_e - 65.6).abs() < 1e-8, "actual cursor at E: {at_e}");
    } else if off_speed.is_some() {
        let settled = (position(previous) + FRAMES as f64 * 1.4).rem_euclid(OLD_END);
        assert!(
            (at_e - settled).abs() < 1e-8,
            "settled locked cursor at E: {at_e}"
        );
    }
    let bpm_behind_stop = matches!(
        control,
        OrderedWetControl::BpmBehindStop(_)
            | OrderedWetControl::BpmOff {
                behind_stop: true,
                ..
            }
    );
    let stop = || {
        assert!(enqueue_stop_with_producer(
            ownership,
            &mut stream.producer.lock().unwrap(),
            Some(1)
        ));
    };
    if bpm_behind_stop {
        stop();
    }
    let bpm_request = anchor.as_ref().filter(|_| !master_refresh).map(|anchor| {
        // Both commands are in the FIFO before the callback observes the revision.
        let enabled = off_speed.is_none();
        engine
            .set_bpm_lock_with_binding(enabled, enabled.then_some(anchor))
            .unwrap();
        let (revision, effect) = ownership.global_rate_status.read_bpm().unwrap();
        assert!(effect.revision < revision);
        (
            revision,
            effect,
            ownership
                .global_timing_revision
                .load(std::sync::atomic::Ordering::Acquire),
        )
    });
    if !bpm_behind_stop {
        stop();
    }

    if let (Some(anchor_binding), Some(period), Some((revision, previous, drain_revision))) =
        (&anchor, anchor_period, bpm_request)
    {
        let next = ColdPcm::publish_at(&engine, 3, 1, "d", SourcePitchInitialization::FreshNeutral);
        next.assert_ack(&engine, false);
        factory.arm_output_ready();
        let (tick, pcm, gated_prefix, gated_ack, gated_capture, staged, (requested, bpm)) =
            std::thread::scope(|scope| {
                let (record_stage, staged) = sync_channel(1);
                let callback_ownership = ownership.clone();
                let observed_action = action.clone();
                let callback = scope.spawn(|| {
                    stream.test_tick_with_effect(Some(Box::new(move || {
                        let _ = record_stage.try_send((
                            callback_ownership.global_rate_status.read_bpm(),
                            callback_ownership
                                .global_timing_revision
                                .load(std::sync::atomic::Ordering::Acquire),
                            observed_action.effective(),
                        ));
                    })))
                });
                let pcm = until(|| {
                    factory
                        .gated_output_pcm()
                        .map(Some)
                        .or_else(|| callback.is_finished().then_some(None))
                });
                let gated_prefix = submission.prefix();
                let gated_ack = ownership.binding_source_adoption_accepted(3, next.binding);
                let gated_capture = engine
                    .mix_capture_lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .control
                    .status();
                let bpm = ownership.global_rate_status.read_bpm();
                if pcm.is_some() {
                    factory.release_output_ready(false);
                }
                (
                    callback.join().unwrap().unwrap(),
                    pcm,
                    gated_prefix,
                    gated_ack,
                    gated_capture,
                    staged.try_recv().unwrap(),
                    bpm.unwrap(),
                )
            });
        let rate = ownership.global_rate_status.read().unwrap();
        let confirmed = submission.prefix();
        let actual_end = position(tick.pads[0]);
        let timing_revision = ownership
            .global_timing_revision
            .load(std::sync::atomic::Ordering::Acquire);
        let failed = engine.mix_capture_stream_failed();
        a.assert_ack(&engine, true);
        b.assert_ack(&engine, true);
        let anchor_pcm = anchor_source.as_ref().unwrap();
        anchor_pcm.assert_ack(&engine, true);
        next.assert_ack(&engine, true);
        engine.shut_down().unwrap();
        let status = until(|| {
            let status = capture_status(&engine);
            (status.worker_joined && status.rt_released && engine.pitch_work_capacity.used() == 0)
                .then_some(status)
        });
        {
            let ended = owner.lock().unwrap();
            assert!(ended.owner_ended && ended.owner_join_verified && ended.retirement_verified);
        }
        assert_eq!(factory.info().unwrap().active, 0);
        let enabled = off_speed.is_none();
        let provenance = if enabled {
            MasterProvenance::qualified(
                master_provenance::source_identity(anchor_binding).unwrap(),
                anchor_binding.binding,
            )
        } else {
            MasterProvenance::Absent
        };
        let action_state = action.state();
        let action_reason = action.reason();
        let action_effective = action.effective();
        drop(anchor);
        drop(anchor_timing);
        drop(timing);
        drop(binding);
        drop(action);
        drop(engine);
        assert!(
            a.reader.upgrade().is_none()
                && b.reader.upgrade().is_none()
                && anchor_pcm.reader.upgrade().is_none()
                && next.reader.upgrade().is_none()
        );
        let (observed, observed_drain, observed_set) = staged;
        let (observed_revision, observed_effect) = observed.unwrap();
        eprintln!(
            "ASIO staged BPM {control:?} period={period} revision={revision} executed={:?} provenance={:?} hook={observed_revision}/{} SET={observed_set}->{action_effective}/{action_state} worker={original_native:#x} actual_native={:#x} output={} cursor={at_e}->{actual_end} gated={gated_prefix:?} confirmed={confirmed:?} capture={:?}..{:?} frames={} failure={:?} wet_failed={}",
            bpm.execution_frame,
            rate.master_provenance,
            observed_effect.revision,
            tick.pads[0].native_state,
            tick.output_frame,
            status.first_frame,
            status.end_frame,
            status.written_frames,
            status.failure,
            tick.pads[0].wet_failed,
        );
        assert_eq!((requested, bpm.revision), (revision, revision));
        assert_eq!(
            (bpm.requested, bpm.effective, bpm.applied),
            (enabled, enabled, true)
        );
        assert_eq!(
            rate.effect.effective.period,
            enabled.then_some(period / 1.4)
        );
        assert_eq!(rate.effect.effective.speed, off_speed.unwrap_or(1.4));
        assert_eq!(rate.master_provenance, provenance);
        assert!(gated_ack);
        assert!(tick.pads[0].active && !tick.pads[1].active && !tick.pads[2].active);
        assert_eq!(tick.pads[0].generation, playing.pads[0].generation);
        assert_eq!(tick.pads[0].source_pcm, a.binding.source_address);
        assert_eq!(observed_revision, revision);
        if let Some((prior_revision, prior_bpm, _, _)) = prior_lock {
            assert!(observed_set && observed_effect.effective && !observed_effect.requested);
            assert_eq!(
                (observed_drain, timing_revision),
                (drain_revision, drain_revision)
            );
            if bpm_behind_stop {
                assert_eq!(observed_effect.revision, prior_revision);
                assert_eq!(observed_effect.execution_frame, prior_bpm.execution_frame);
            } else {
                assert_eq!(observed_effect.revision, revision);
                assert_eq!(observed_effect.reason, BpmReason::DrainPending);
                assert!(!observed_effect.applied);
            }
        } else if bpm_behind_stop {
            assert!(observed_set);
            assert_eq!(observed_effect.revision, previous.revision);
            assert_eq!(observed_effect.execution_frame, previous.execution_frame);
            assert_eq!(
                (observed_drain, timing_revision),
                (drain_revision, drain_revision)
            );
        } else {
            assert_eq!(observed_effect.revision, revision);
            assert_eq!(observed_effect.reason, BpmReason::DrainPending);
            assert!(!observed_effect.applied);
        }
        let conflict = off_speed.map_or(period != 0.5, |speed| speed != 1.4);
        if conflict && !bpm_behind_stop && enabled {
            assert_eq!(action_state, "rejected");
            assert_eq!(action_reason, Some("timing_changed"));
            assert!(!action_effective);
            assert_eq!(tick.pads[0].native_state, before.pads[0].native_state);
            assert!(
                (actual_end - 15.2).abs() < 1e-8,
                "glide cursor {actual_end}"
            );
        } else {
            assert!(action_effective);
            assert_eq!(tick.pads[0].native_state, original_native);
        }
        let execution = if (bpm_behind_stop || !enabled) && conflict {
            E + 64
        } else {
            E
        };
        assert_eq!(
            (bpm.execution_frame, bpm.reason),
            (Some(execution), BpmReason::None)
        );
        if action_effective {
            assert!(tick.pads[0].wet_current_source_output);
            let advance = if conflict {
                64.0 * 1.4 + (FRAMES - 64) as f64 * 1.35
            } else {
                FRAMES as f64 * 1.4
            };
            let expected_end = (at_e + advance).rem_euclid(OLD_END);
            assert!(
                (actual_end - expected_end).abs() < 1e-8,
                "cursor {actual_end} after BPM execution at {execution}"
            );
        }
        assert!(!failed && !tick.pads[0].wet_failed);
        assert_eq!(gated_prefix, (E, false));
        assert!(!gated_capture.terminal_decided && gated_capture.payload_sha256.is_none());
        assert_eq!(confirmed, (E + FRAMES, false));
        assert_eq!(tick.output_frame, E + FRAMES);
        assert_eq!(tick.callback.frames, FRAMES as u32);
        assert_eq!(
            tick.callback.output_ready_calls,
            before.callback.output_ready_calls + 1
        );
        assert_eq!(tick.native.shifts, 0);
        assert_eq!(
            (status.first_frame, status.end_frame),
            (Some(E - FRAMES), Some(capture_end))
        );
        assert!(status.successful_spool());
        assert_eq!(status.written_frames, FRAMES + capture_end - E);
        let pcm = pcm.expect("actual outputReady gate");
        let expected = &pcm[..(2 * (capture_end - E)) as usize];
        assert!(expected[..128].iter().any(|sample| *sample != 0.0));
        assert!(expected[128..].iter().any(|sample| *sample != 0.0));
        let captured = fs::read(&path).unwrap();
        assert_eq!(
            &captured[SPOOL_HEADER_BYTES as usize + FRAMES as usize * 8..],
            expected
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect::<Vec<_>>()
        );
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&directory).unwrap();
        return Vec::new();
    }

    // The first taps at E+1 coincide. At E+18 the old loop reads near zero
    // and the refreshed loop would read eight source frames later.
    if !master_refresh {
        assert!(at_e + 1.4 + 1.0 < OLD_END && at_e + 1.4 > NEW_START);
        assert!((at_e + 18.0 * 1.4 - OLD_END - 0.8).abs() < 1e-8);
    }
    let (refresh_binding, start, master_period, next_id) = match (master, &anchor) {
        (Some((period, _)), Some(anchor)) => (anchor, 0.0, Some(period), 3),
        _ => (&binding, NEW_START, None, 2),
    };
    factory.arm_output_ready();
    let (
        tick,
        pcm,
        refresh,
        earlier_refresh,
        next,
        gated_prefix,
        gated_ack,
        gated_capture,
        bound_event,
    ) = std::thread::scope(|scope| {
        let (claimed, wait_claim) = sync_channel(1);
        let (published, wait_publication) = sync_channel(1);
        let (bound, wait_bound) = sync_channel(1);
        let (revoked, wait_revoked) = sync_channel(1);
        let bind_hook = matches!(
            control,
            OrderedWetControl::RevokedInteriorRefresh(true)
                | OrderedWetControl::CurrentInteriorRefresh
        );
        let callback = scope.spawn(move || {
            stream.test_tick_with_effect(Some(Box::new(move || {
                if bind_hook {
                    crate::audio_engine::audio_stream::on_refresh_bound_for_test(
                        move |frame, identity| {
                            bound.send((frame, identity)).unwrap();
                            wait_revoked.recv_timeout(WAIT).unwrap();
                        },
                    );
                }
                if claimed.send(()).is_ok() {
                    let _ = wait_publication.recv_timeout(WAIT);
                }
            })))
        });
        wait_claim.recv_timeout(WAIT).unwrap();
        assert!(action.effective());
        let first_refresh = (!matches!(
            control,
            OrderedWetControl::MasterRefreshHistory(false, _)
                | OrderedWetControl::OrderedMasterRefresh(false)
                | OrderedWetControl::RevokedInteriorRefresh(false)
        ))
        .then(|| {
            engine
                .refresh_current_constant_timing(
                    refresh_binding,
                    seconds(start),
                    Some(seconds(OLD_END)),
                    master_period,
                )
                .unwrap()
        });
        let (refresh, earlier_refresh) = if let Some(later) = &later_binding {
            (
                Some(
                    engine
                        .refresh_current_constant_timing(
                            later,
                            0.0,
                            Some(seconds(OLD_END)),
                            Some(0.7 / 1.4),
                        )
                        .unwrap(),
                ),
                first_refresh,
            )
        } else {
            (first_refresh, None)
        };
        if let Some(refresh) = &refresh {
            assert_eq!(refresh.publication_status(), "pending");
        }
        let next = ColdPcm::publish_at(
            &engine,
            next_id,
            1,
            "next",
            SourcePitchInitialization::FreshNeutral,
        );
        next.assert_ack(&engine, false);
        let global_before_revoke = ownership
            .global_timing_revision
            .load(std::sync::atomic::Ordering::Acquire);
        let order_before_revoke = ownership
            .accepted_master_refresh_order
            .load(std::sync::atomic::Ordering::Acquire);
        if interior && !bind_hook {
            engine.set_pad_timing_intent(2, "manual").unwrap();
        }
        published.send(()).unwrap();
        let bound_event = bind_hook.then(|| {
            let observed = wait_bound.recv_timeout(WAIT).unwrap();
            assert_eq!(
                observed,
                (E + 64, refresh.as_ref().unwrap().execution_identity())
            );
            assert_eq!(refresh.as_ref().unwrap().publication_status(), "pending");
            assert_eq!(refresh.as_ref().unwrap().execution(), None);
            assert_eq!(interior_action.as_ref().unwrap().state(), "prepared");
            if !current_refresh {
                engine.set_pad_timing_intent(2, "manual").unwrap();
            }
            revoked.send(()).unwrap();
            observed
        });
        if interior {
            assert!(!refresh_binding.current());
            assert_eq!(
                ownership
                    .global_timing_revision
                    .load(std::sync::atomic::Ordering::Acquire),
                global_before_revoke
            );
            assert_eq!(
                ownership
                    .accepted_master_refresh_order
                    .load(std::sync::atomic::Ordering::Acquire),
                order_before_revoke
            );
        }
        let pcm = until(|| {
            factory
                .gated_output_pcm()
                .map(Some)
                .or_else(|| callback.is_finished().then_some(None))
        });
        let gated_prefix = submission.prefix();
        let gated_ack = ownership.binding_source_adoption_accepted(next_id, next.binding);
        let gated_capture = capture_status(&engine);
        if pcm.is_some() {
            factory.release_output_ready(false);
        }
        (
            callback.join().unwrap().unwrap(),
            pcm,
            refresh,
            earlier_refresh,
            next,
            gated_prefix,
            gated_ack,
            gated_capture,
            bound_event,
        )
    });
    if interior {
        let pcm = pcm.expect("two-original Wet request reached outputReady");
        let old = &prior_lock.as_ref().unwrap().2;
        let rate = ownership.global_rate_status.read().unwrap();
        let d = interior_action.as_ref().unwrap();
        let d_effect = (d.state(), d.reason(), d.effective_output_frame());
        let d_sequence = d.sequence();
        let d_owner = tick.pads[4];
        eprintln!("ASIO source revoked bound={bound_event:?} D={d_effect:?} owner={d_owner:?}");
        let expected_period = if current_refresh {
            Some(0.75 / 1.4)
        } else {
            old.effect.effective.period
        };
        let expected_provenance = if current_refresh {
            MasterProvenance::qualified(
                master_provenance::source_identity(refresh_binding).unwrap(),
                refresh_binding.binding,
            )
        } else {
            old.master_provenance
        };
        assert_eq!(rate.effect.effective.period, expected_period);
        assert_eq!(rate.master_provenance, expected_provenance);
        assert!(action.effective());
        assert_eq!(action.effective_output_frame(), Some(E));
        assert_eq!(tick.pads[0].native_state, original_native);
        assert!(tick.pads[0].wet_current_source_output && !tick.pads[0].wet_failed);
        assert_eq!(tick.pads[0].adopted_native_request, Some(action.sequence()));
        assert_eq!(tick.native.shifts, 0);
        for id in [0, 2, 4] {
            assert_eq!(tick.loops[id], before.loops[id]);
            assert_eq!(tick.loops[id], (0, Some(OLD_END as usize)));
        }
        assert!(gated_ack);
        assert_eq!(gated_prefix, (E, false));
        assert!(!gated_capture.terminal_decided && gated_capture.payload_sha256.is_none());
        assert_eq!(submission.prefix(), (E + FRAMES, false));
        assert!(tick.pads[0].active && !tick.pads[1].active && !tick.pads[2].active);
        for (id, source) in [(0, &a), (4, wet_peer.as_ref().unwrap())] {
            assert_eq!(tick.pads[id].generation, before.pads[id].generation);
            assert_eq!(tick.pads[id].source_pcm, source.binding.source_address);
        }
        for end in [E + 2 * FRAMES, E + 3 * FRAMES] {
            let later = stream.test_tick().unwrap();
            assert_eq!(later.output_frame, end);
            assert_eq!(later.deferred_refresh, None);
            let rate = ownership.global_rate_status.read().unwrap();
            assert_eq!(rate.effect.effective.period, expected_period);
            assert_eq!(rate.master_provenance, expected_provenance);
        }
        if let Some(refresh) = &refresh {
            assert_eq!(refresh.settled_frame(), Some(E + 64));
            if current_refresh {
                assert_eq!(refresh.publication_status(), "accepted");
                assert_eq!(refresh.execution(), Some((E + 64, 0)));
                assert!(refresh.is_current());
            } else {
                assert_eq!(refresh.publication_status(), "rejected");
                assert_eq!(refresh.execution(), None);
                assert!(!refresh.is_current());
            }
        }
        for source in [
            &a,
            &b,
            anchor_source.as_ref().unwrap(),
            wet_peer.as_ref().unwrap(),
            &next,
        ] {
            source.assert_ack(&engine, true);
        }
        engine.shut_down().unwrap();
        let status = until(|| {
            let status = capture_status(&engine);
            (status.worker_joined && status.rt_released && engine.pitch_work_capacity.used() == 0)
                .then_some(status)
        });
        let ended = owner.lock().unwrap();
        assert!(ended.owner_ended && ended.owner_join_verified && ended.retirement_verified);
        assert_eq!(factory.info().unwrap().active, 0);
        drop(ended);
        drop(refresh);
        drop(anchor);
        drop(anchor_timing);
        drop(timing);
        drop(binding);
        drop(wet_binding);
        drop(wet_timing);
        drop(action);
        drop(interior_action);
        drop(interior_worker);
        drop(engine);
        for source in [
            &a,
            &b,
            anchor_source.as_ref().unwrap(),
            wet_peer.as_ref().unwrap(),
            &next,
        ] {
            assert!(source.reader.upgrade().is_none());
        }
        let captured = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&directory).unwrap();
        assert_eq!(status.end_frame, Some(capture_end));
        assert_eq!(status.first_frame, Some(E - FRAMES));
        assert_eq!(status.written_frames, FRAMES + capture_end - E);
        assert!(status.successful_spool());
        let expected = &pcm[..(2 * (capture_end - E)) as usize];
        assert_eq!(
            &captured[SPOOL_HEADER_BYTES as usize + FRAMES as usize * 8..],
            expected
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect::<Vec<_>>()
        );
        // Check the effect after true close so a failing causal arm also proves custody END.
        if current_refresh {
            assert_eq!(d_effect, ("rejected", Some("timing_changed"), None));
            assert_eq!(d_owner.native_state, before.pads[4].native_state);
            assert_eq!(d_owner.adopted_native_request, None);
        } else {
            assert_eq!(d_effect, ("applied", None, Some(E + 16)));
            assert_eq!(d_owner.native_state, interior_native.unwrap());
            assert_eq!(d_owner.adopted_native_request, Some(d_sequence));
            assert!(d_owner.wet_current_source_output && !d_owner.wet_failed);
        }
        use sha2::{Digest, Sha256};
        let pcm_bytes: Vec<_> = pcm.iter().flat_map(|sample| sample.to_le_bytes()).collect();
        eprintln!(
            "ASIO source revoked closed {control:?} start={:?} end={:?} frames={} D_work={d_sequence} pcm_sha={:x} recording_sha={:x}",
            status.first_frame,
            status.end_frame,
            status.written_frames,
            Sha256::digest(&pcm_bytes),
            Sha256::digest(&captured[SPOOL_HEADER_BYTES as usize..])
        );
        let mut oracle: Vec<_> = pcm.iter().map(|sample| sample.to_bits()).collect();
        oracle.extend(
            captured[SPOOL_HEADER_BYTES as usize..]
                .chunks_exact(4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap())),
        );
        return oracle;
    }
    if history {
        let old = &prior_lock.as_ref().unwrap().2;
        let rate = ownership.global_rate_status.read().unwrap();
        let before_close = capture_status(&engine);
        assert_eq!(before_close.captured_end, Some(capture_end));
        let pcm = pcm.expect("actual outputReady gate");
        let dry = tick.pads[4];
        assert_eq!(rate.effect.effective.period, old.effect.effective.period);
        assert_eq!(rate.master_provenance, old.master_provenance);
        assert!(dry.active && dry.key_lock && !dry.active_native);
        assert_eq!(dry.rate_target, 1.0);
        assert_eq!(dry.generation, before.pads[4].generation);
        assert_eq!(dry.source_pcm, before.pads[4].source_pcm);
        assert_eq!(dry.frame, before.pads[4].frame + FRAMES as usize);
        assert!(action.effective() && tick.pads[0].wet_current_source_output);
        assert_eq!(tick.pads[0].native_state, original_native);
        assert!(tick.pads[0].active && !tick.pads[1].active && !tick.pads[2].active);
        assert_eq!(tick.pads[0].generation, before.pads[0].generation);
        assert_eq!(tick.pads[0].source_pcm, before.pads[0].source_pcm);
        assert_eq!(tick.native.shifts, 0);
        assert!(gated_ack);
        assert_eq!(gated_prefix, (E, false));
        assert!(!gated_capture.terminal_decided && gated_capture.payload_sha256.is_none());
        assert_eq!(submission.prefix(), (E + FRAMES, false));
        if let Some(refresh) = &refresh {
            assert_eq!(refresh.publication_status(), "pending");
            assert_eq!(refresh.execution(), None);
            assert_eq!(
                tick.deferred_refresh,
                Some((E + 64, refresh.execution_identity()))
            );
        } else {
            assert_eq!(tick.deferred_refresh, None);
        }
        eprintln!(
            "ASIO bound refresh history {control:?} deferred={:?} D={dry:?} capture_before_close={before_close:?}",
            tick.deferred_refresh
        );
        push_global_rate_message(
            &ownership.global_timing_revision,
            &ownership.global_timing_publishing,
            &ownership.global_rate_status,
            &mut stream.parameter_producer.lock().unwrap(),
            Some(1.4),
            None,
            "ASIO bound refresh history revocation",
        )
        .unwrap();
        let later_revision = ownership.global_rate_status.requested_revision();
        let revoked = stream.test_tick().unwrap();
        let settled = stream.test_tick().unwrap();
        let later = ownership.global_rate_status.read().unwrap();
        assert_eq!(later.effect.revision, later_revision);
        eprintln!(
            "ASIO bound refresh history later scalar revision={later_revision} state={:?} frame={:?}",
            later.effect.state, later.effect.execution_frame
        );
        if let Some(refresh) = &refresh {
            assert_eq!(refresh.publication_status(), "rejected");
            assert_eq!(refresh.execution(), None);
        }
        assert_eq!(revoked.deferred_refresh, None);
        assert_eq!(settled.deferred_refresh, None);
        assert_eq!(
            ownership
                .global_rate_status
                .read()
                .unwrap()
                .master_provenance,
            old.master_provenance
        );
        assert_eq!(
            ownership
                .global_rate_status
                .read()
                .unwrap()
                .effect
                .effective
                .period,
            old.effect.effective.period
        );
        a.assert_ack(&engine, true);
        b.assert_ack(&engine, true);
        anchor_source.as_ref().unwrap().assert_ack(&engine, true);
        dry_peer.as_ref().unwrap().assert_ack(&engine, true);
        next.assert_ack(&engine, true);
        engine.shut_down().unwrap();
        let status = until(|| {
            let status = capture_status(&engine);
            (status.worker_joined && status.rt_released && engine.pitch_work_capacity.used() == 0)
                .then_some(status)
        });
        let ended = owner.lock().unwrap();
        assert!(ended.owner_ended && ended.owner_join_verified && ended.retirement_verified);
        assert_eq!(factory.info().unwrap().active, 0);
        drop(ended);
        drop(refresh);
        drop(anchor);
        drop(anchor_timing);
        drop(timing);
        drop(binding);
        drop(dry_timing);
        drop(dry_binding);
        drop(action);
        drop(engine);
        assert!(
            a.reader.upgrade().is_none()
                && b.reader.upgrade().is_none()
                && anchor_source.as_ref().unwrap().reader.upgrade().is_none()
                && dry_peer.as_ref().unwrap().reader.upgrade().is_none()
                && next.reader.upgrade().is_none()
        );
        let captured = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&directory).unwrap();
        eprintln!(
            "ASIO bound refresh history {control:?} closed capture={:?}..{:?} frames={} failure={:?}",
            status.first_frame, status.end_frame, status.written_frames, status.failure
        );
        assert_eq!(status.end_frame, Some(capture_end));
        assert_eq!(status.written_frames, FRAMES + capture_end - E);
        assert!(status.successful_spool());
        let expected = &pcm[..(2 * (capture_end - E)) as usize];
        assert_eq!(
            &captured[SPOOL_HEADER_BYTES as usize + FRAMES as usize * 8..],
            expected
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect::<Vec<_>>()
        );
        let mut oracle: Vec<_> = pcm.iter().map(|sample| sample.to_bits()).collect();
        oracle.extend(
            captured[SPOOL_HEADER_BYTES as usize..]
                .chunks_exact(4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap())),
        );
        return oracle;
    }
    let refresh = refresh.unwrap();
    let execution = refresh.execution();
    let confirmed = submission.prefix();
    let current = refresh.is_current();
    let actual_end_position = position(tick.pads[0]);
    let publication = refresh.publication_status();
    let rate = ownership.global_rate_status.read().unwrap();
    let bpm = ownership.global_rate_status.read_bpm();
    let failed = engine.mix_capture_stream_failed();
    let provenance = later_binding.as_ref().or(anchor.as_ref()).map(|anchor| {
        MasterProvenance::qualified(
            master_provenance::source_identity(anchor).unwrap(),
            anchor.binding,
        )
    });
    a.assert_ack(&engine, true);
    b.assert_ack(&engine, true);
    if let Some(source) = &anchor_source {
        source.assert_ack(&engine, true);
    }
    if let Some(source) = &later_source {
        source.assert_ack(&engine, true);
        assert!(!tick.pads[4].active);
        let settled = stream.test_tick().unwrap();
        let later = stream.test_tick().unwrap();
        assert_eq!(settled.output_frame, E + 2 * FRAMES);
        assert_eq!(later.output_frame, E + 3 * FRAMES);
        let state = ownership.global_rate_status.read().unwrap();
        assert_eq!(state.effect.effective.period, rate.effect.effective.period);
        assert_eq!(state.master_provenance, rate.master_provenance);
        assert!(refresh.is_current());
    }
    let earlier_effect = earlier_refresh.as_ref().map(|earlier| {
        (
            earlier.execution_identity(),
            earlier.publication_status(),
            earlier.execution(),
            earlier.is_current(),
        )
    });
    next.assert_ack(&engine, true);
    engine.shut_down().unwrap();
    let status = until(|| {
        let status = capture_status(&engine);
        (status.worker_joined && status.rt_released && engine.pitch_work_capacity.used() == 0)
            .then_some(status)
    });
    let ended = owner.lock().unwrap();
    assert!(ended.owner_ended && ended.owner_join_verified && ended.retirement_verified);
    assert_eq!(factory.info().unwrap().active, 0);
    drop(refresh);
    drop(earlier_refresh);
    drop(later_binding);
    drop(later_timing);
    drop(anchor);
    drop(anchor_timing);
    drop(timing);
    drop(binding);
    let action_effective = action.effective();
    drop(action);
    drop(engine);
    assert!(
        a.reader.upgrade().is_none()
            && b.reader.upgrade().is_none()
            && anchor_source
                .as_ref()
                .is_none_or(|c| c.reader.upgrade().is_none())
            && next.reader.upgrade().is_none()
            && later_source
                .as_ref()
                .is_none_or(|source| source.reader.upgrade().is_none())
    );

    eprintln!(
        "ASIO ordered Wet {control:?} E={E} refresh={publication} effect_frame_and_native={execution:?} original_native={original_native:#x} A_native={:#x} current={current} output={} cursor={at_e}->{actual_end_position} master={:?} provenance={:?} gated={gated_prefix:?} confirmed={confirmed:?} capture={:?}..{:?} frames={} failure={:?} wet_failed={}",
        tick.pads[0].native_state,
        tick.output_frame,
        rate.effect.effective.period,
        rate.master_provenance,
        status.first_frame,
        status.end_frame,
        status.written_frames,
        status.failure,
        tick.pads[0].wet_failed,
    );

    if ordered_refresh {
        eprintln!("ASIO ordered master ingress {control:?} earlier={earlier_effect:?}");
        assert!(!gated_capture.terminal_decided && gated_capture.payload_sha256.is_none());
        if let Some((_, status, execution, current)) = earlier_effect {
            assert_eq!(status, "rejected");
            assert_eq!(execution, None);
            assert!(!current);
        }
    }
    assert_eq!(publication, "accepted");
    assert!(current);
    if let Some((period, revision)) = master {
        let period = if ordered_refresh { 0.7 / 1.4 } else { period };
        assert_eq!(rate.effect.effective.period, Some(period));
        assert_eq!(Some(rate.master_provenance), provenance);
        let (requested, applied) = bpm.unwrap();
        assert_eq!((requested, applied.revision), (revision, revision));
        assert!(applied.applied && applied.effective);
        assert!(action_effective);
        assert_eq!(tick.pads[0].native_state, original_native);
        assert!(!tick.pads[2].active);
        let same_rate = ordered_refresh || matches!(control, OrderedWetControl::MasterRefresh(0.7));
        assert_eq!(execution, Some((if same_rate { E + 1 } else { E + 64 }, 0)));
        let advance = if same_rate {
            FRAMES as f64 * 1.4
        } else {
            64.0 * 1.4 + (FRAMES - 64) as f64 * 1.35
        };
        assert!((actual_end_position - (at_e + advance).rem_euclid(OLD_END)).abs() < 1e-8);
    } else {
        assert_eq!(execution, Some((E + 64, original_native)));
        assert!((actual_end_position - 28.0).abs() < 1e-8);
    }
    assert!(!failed && !tick.pads[0].wet_failed);
    assert_eq!(gated_prefix, (E, false));
    assert!(gated_ack);
    assert_eq!(tick.output_frame, E + FRAMES);
    assert_eq!(confirmed, (E + FRAMES, false));
    assert_eq!(tick.callback.frames, FRAMES as u32);
    assert_eq!(
        tick.callback.output_ready_calls,
        before.callback.output_ready_calls + 1
    );
    assert_eq!(tick.native.shifts, 0);
    assert!(tick.pads[0].active && !tick.pads[1].active);
    assert_eq!(tick.pads[0].source_pcm, a.binding.source_address);
    assert_eq!(tick.pads[0].generation, playing.pads[0].generation);
    assert_eq!(
        (status.first_frame, status.end_frame),
        (Some(E - FRAMES), Some(capture_end))
    );
    assert!(status.successful_spool());
    assert_eq!(status.written_frames, FRAMES + capture_end - E);
    let pcm = pcm.expect("actual outputReady gate");
    let expected = &pcm[..(2 * (capture_end - E)) as usize];
    assert!(expected[..128].iter().any(|sample| *sample != 0.0));
    assert!(expected[128..].iter().any(|sample| *sample != 0.0));
    let captured = fs::read(&path).unwrap();
    assert_eq!(
        &captured[SPOOL_HEADER_BYTES as usize + FRAMES as usize * 8..],
        expected
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>()
    );
    fs::remove_file(&path).unwrap();
    fs::remove_dir(&directory).unwrap();
    if ordered_refresh {
        let mut oracle: Vec<_> = pcm.iter().map(|sample| sample.to_bits()).collect();
        oracle.extend(
            captured[SPOOL_HEADER_BYTES as usize..]
                .chunks_exact(4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap())),
        );
        oracle
    } else {
        Vec::new()
    }
}

#[test]
fn asio_missing_interior_set_releases_same_frame_stop_and_capture_end() {
    interior_set_at_request(InteriorSet::Missing, false);
}

#[test]
fn asio_prepared_interior_set_uses_fitting_original_fifo() {
    let inline = interior_set_at_request(InteriorSet::Fit, true);
    assert_eq!(interior_set_at_request(InteriorSet::Fit, false), inline);
}

#[test]
fn asio_prepared_interior_set_retains_unshifted_no_fit() {
    let inline = interior_set_at_request(InteriorSet::NoFit, true);
    assert_eq!(interior_set_at_request(InteriorSet::NoFit, false), inline);
}

#[test]
fn asio_prepared_interior_set_yields_to_bound_same_pad_restart() {
    interior_set_at_request(InteriorSet::BoundRestart, true);
    interior_set_at_request(InteriorSet::BoundRestart, false);
}

#[test]
fn asio_prepared_interior_set_preserves_bound_peer_exclusive() {
    interior_set_at_request(InteriorSet::PeerExclusive, false);
}

#[test]
fn asio_prepared_interior_set_yields_to_bound_bpm_history() {
    let with_set = interior_set_at_request(InteriorSet::BoundBpm, false);
    let without_set = interior_set_at_request(InteriorSet::BoundBpmBaseline, false);
    assert_eq!(with_set, without_set);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InteriorSet {
    Missing,
    Fit,
    NoFit,
    BoundRestart,
    PeerExclusive,
    BoundBpmBaseline,
    BoundBpm,
}

fn interior_set_at_request(case: InteriorSet, inline: bool) -> Vec<u32> {
    use crate::audio_engine::constant_timing::l3_constant_timing_fixture::publish_synthetic_loaded_timing;
    use crate::audio_engine::constants::NUM_SAMPLES;
    use crate::audio_engine::global_rate_status::BpmReason;
    use crate::audio_engine::input_mapping::PitchPadProjection;
    use crate::audio_engine::input_runtime_binding;
    use crate::audio_engine::master_provenance::{self, MasterProvenance};
    use crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::WorkerGate;

    const FRAMES: u64 = 96;
    const REQUEST: u64 = 24_000;
    const FIRST: u64 = REQUEST + 7;
    const BPM_CAPTURE: u64 = 19_968;
    const BPM_APPLY: u64 = 24_064;
    const SPEED: f64 = 1.2;
    let missing = case == InteriorSet::Missing;
    let no_fit = case == InteriorSet::NoFit;
    let exclusive = matches!(case, InteriorSet::Missing | InteriorSet::PeerExclusive);
    let restart = case == InteriorSet::BoundRestart;
    let bound_bpm = matches!(case, InteriorSet::BoundBpmBaseline | InteriorSet::BoundBpm);
    let bpm_baseline = case == InteriorSet::BoundBpmBaseline;
    let effect = if no_fit { 24_048 } else { 24_016 };
    let capture_frame = if missing { 18_240 } else { 21_120 };
    // The second ready case uses another real birth phase, with a shift that
    // fits 96 frames but cannot fit the remainder of its actual request.
    let birth = if no_fit {
        576
    } else if bound_bpm {
        1056
    } else {
        0
    };
    let capture_end = REQUEST
        + match case {
            InteriorSet::Fit => 64,
            InteriorSet::Missing => 73,
            InteriorSet::NoFit | InteriorSet::BoundRestart | InteriorSet::PeerExclusive => 93,
            InteriorSet::BoundBpmBaseline | InteriorSet::BoundBpm => 96,
        };
    Python::initialize();
    let directory = tempfile::tempdir().unwrap().keep();
    let path = directory.join("take.capture");
    eprintln!("ASIO interior SET spool: {}", path.display());
    let factory = Factory::new().unwrap();
    factory
        .inline_wet
        .store(inline, std::sync::atomic::Ordering::Release);
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
    let period = effect as f64 / 48_000.0;
    if !bound_bpm {
        engine.set_master_period(period).unwrap();
    }
    engine.set_trigger_quantization("immediate").unwrap();
    let stream = engine.stream_handle.as_ref().unwrap();
    let owner = stream.test_owner_observer();
    let submission = stream.output_submission.as_ref().unwrap().clone();
    let a = ColdPcm::publish(&engine, 1, "a");
    let b = ColdPcm::publish_with_geometry(
        &engine,
        1,
        1,
        "b",
        SourcePitchInitialization::FreshNeutral,
        if bound_bpm { 48_000 } else { 1024 },
        None,
    );
    a.assert_ack(&engine, false);
    b.assert_ack(&engine, false);
    let play_a = |stream: &crate::audio_engine::audio_stream::AudioStreamHandle| {
        stream
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::PlaySample {
                id: 0,
                volume: 1.0,
                received_at_ns: None,
                expected_source_generation: Some(1),
            })
            .unwrap();
    };
    if birth == 0 {
        play_a(stream);
    }
    assert_eq!(stream.test_tick().unwrap().pads[0].active, birth == 0);
    a.assert_ack(&engine, true);
    b.assert_ack(&engine, true);
    let (anchor, lead_start) = if bound_bpm {
        engine
            .set_pad_timing_intent_with_source(1, "automatic", Some(1))
            .unwrap();
        let stream = engine.stream_handle.as_ref().unwrap();
        let timing = publish_synthetic_loaded_timing(&engine, &stream.producer, 1, period);
        assert_eq!(stream.test_tick().unwrap().output_frame, 2 * FRAMES);
        assert_eq!(timing.publication_status().unwrap(), "accepted");
        let anchor = input_runtime_binding::capture(&engine, 1).unwrap().unwrap();
        let period = anchor.binding.accepted.unwrap().period_seconds;
        engine.set_master_period(period).unwrap();
        engine.set_bpm_lock_with_binding(true, None).unwrap();
        let key_lock = engine
            .set_pad_key_lock_with_source(1, true, Some(1))
            .unwrap();
        let tick = engine.stream_handle.as_ref().unwrap().test_tick().unwrap();
        assert_eq!(tick.output_frame, 3 * FRAMES);
        let lock = engine.input_runtime_ownership.key_lock_status(1).unwrap();
        assert_eq!(lock.request_id, key_lock);
        assert!(lock.effective && lock.ready);
        (Some(anchor), 4 * FRAMES)
    } else {
        (None, 2 * FRAMES)
    };
    let stream = engine.stream_handle.as_ref().unwrap();
    if !exclusive && !restart {
        stream
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::PlaySample {
                id: 1,
                volume: 0.25,
                received_at_ns: None,
                expected_source_generation: Some(1),
            })
            .unwrap();
    }
    let lead_end = birth.max(if exclusive || restart { 0 } else { FRAMES }) + FRAMES;
    for end in (lead_start..=lead_end).step_by(FRAMES as usize) {
        if end - FRAMES == birth {
            play_a(stream);
        }
        let tick = stream.test_tick().unwrap();
        assert_eq!(tick.output_frame, end);
        assert_eq!(tick.pads[0].active, end > birth);
    }
    engine.set_trigger_quantization("1/16").unwrap();
    let stream = engine.stream_handle.as_ref().unwrap();
    let ownership = &engine.input_runtime_ownership;

    let runtime = engine.input_runtime.as_ref().unwrap();
    let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
    let mut loaded = vec![false; NUM_SAMPLES];
    loaded[0] = true;
    let mut bindings = vec![None; NUM_SAMPLES];
    bindings[0] = Some(&binding);
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    rows[0].quantize_step_64ths = Some(8);
    runtime
        .set_runtime_state_with_pitch(
            true,
            loaded,
            vec![0.0; NUM_SAMPLES],
            vec![None; NUM_SAMPLES],
            bindings,
            rows,
        )
        .unwrap();
    let mut bpm_revision = None;
    let mut previous_position = None;
    let mut first_delta = None;
    let mut varied = None;
    for end in (lead_end + FRAMES..=capture_frame).step_by(FRAMES as usize) {
        let frame = end - FRAMES;
        if if bound_bpm {
            end == BPM_CAPTURE
        } else {
            end == capture_frame
        } {
            // Set the real rate before either action freezes a source trajectory.
            push_global_rate_message(
                &ownership.global_timing_revision,
                &ownership.global_timing_publishing,
                &ownership.global_rate_status,
                &mut stream.parameter_producer.lock().unwrap(),
                Some(SPEED),
                None,
                "ASIO interior SET",
            )
            .unwrap();
        }
        if let Some(anchor) = &anchor {
            if frame == BPM_CAPTURE {
                assert!(anchor.current() && anchor.available());
                engine
                    .set_bpm_lock_with_binding(true, Some(anchor))
                    .unwrap();
                let (revision, previous) = ownership.global_rate_status.read_bpm().unwrap();
                assert!(revision > previous.revision && previous.applied && previous.effective);
                bpm_revision = Some(revision);
            }
        }
        let tick = stream.test_tick().unwrap();
        assert_eq!(tick.output_frame, end);
        assert_eq!(tick.pads[0].active, end > birth);
        if bound_bpm && frame >= BPM_CAPTURE {
            assert!(tick.pads[1].active && tick.pads[1].key_lock);
            assert_eq!(tick.pads[1].source_pcm, b.binding.source_address);
            assert!(!tick.pads[1].active_native);
            assert_eq!(tick.pads[1].adopted_native_request, None);
            assert_eq!(tick.bound_rate_frame, Some(BPM_APPLY));
            let position = tick.pads[0].frame as f64 + tick.pads[0].fraction;
            if let Some(previous) = previous_position {
                let delta = f64::rem_euclid(position - previous, 1024.0);
                let first = *first_delta.get_or_insert((frame, delta));
                if (delta - first.1).abs() > 1e-6 {
                    varied = Some((first, (frame, delta)));
                }
            }
            previous_position = Some(position);
            // Pace only the fake driver. The product worker runs independently.
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let rate = ownership.global_rate_status.read().unwrap();
    assert_eq!(
        rate.effect.execution_frame,
        Some(if bound_bpm {
            BPM_CAPTURE - FRAMES
        } else {
            capture_frame - FRAMES
        })
    );
    assert_eq!(rate.request_revision, rate.effect.revision);
    if let Some(revision) = bpm_revision {
        let (early, late) = varied.expect("actual A cursor must vary within retained SET history");
        assert!((effect - 4096..effect).contains(&early.0));
        assert!((effect - 4096..effect).contains(&late.0));
        let (requested, pending) = ownership.global_rate_status.read_bpm().unwrap();
        assert_eq!((requested, pending.revision), (revision, revision));
        assert!(pending.requested && pending.effective && !pending.applied);
        assert_eq!(pending.reason, BpmReason::HistoryPending);
        assert_eq!(pending.execution_frame, None);
        eprintln!(
            "ASIO interior BPM capture={BPM_CAPTURE} bound={BPM_APPLY} scalar={:?} BPM={revision} A_cursor_deltas={early:?},{late:?}",
            rate.effect.execution_frame
        );
    }
    let clock = stream.output_clock.read().unwrap();
    assert!(clock.valid);
    let pending = (!bpm_baseline).then(|| {
        let action = runtime
            .capture_gui_pitch(&binding, "pitch.extra:3:set", clock.observed_at_ns)
            .unwrap();
        assert_eq!(action.original_due_frame(), effect);
        let worker = if missing {
            WorkerGate::for_pitch(&action)
        } else {
            WorkerGate::for_pitch_return(&action)
        };
        assert!(engine.publish_pitch_action(&action).unwrap());
        (action, worker)
    });
    assert_eq!(
        stream.test_tick().unwrap().output_frame,
        capture_frame + FRAMES
    );
    let original = pending.as_ref().and_then(|(action, worker)| {
        assert!(worker.wait_for(!missing).is_none());
        assert_eq!(action.execution_target(), effect);
        if missing { return None; }
        let ((block, input, output), wet) = worker.prepared_fifo();
        let need = block.checked_sub(input).unwrap();
        let native = worker.prepared_native();
        worker.release();
        eprintln!(
            "ASIO interior SET {case:?} inline={inline} E={effect} block={block} input={input} output={output} need={need} cap={} Wet={wet:?} Native={native:#x}",
            REQUEST + FRAMES - effect,
        );
        assert_eq!((block, input), if no_fit || bound_bpm { (512, 432) } else { (512, 464) });
        assert_eq!(output + 1, need);
        assert_eq!(wet, (!inline && !no_fit).then_some((need, 1)));
        if bound_bpm {
            assert_eq!(effect + need as u64, REQUEST + FRAMES);
            assert!((effect..effect + need as u64).contains(&BPM_APPLY));
        }
        Some(native)
    });

    // A peer exclusive ends A. A same-pad restart validates the captured A pitch.
    let start = (exclusive || restart).then(|| {
        let start = engine
            .capture_pad_start(
                usize::from(exclusive),
                0.0,
                None,
                0.25,
                exclusive,
                None,
                Some(1),
            )
            .unwrap();
        assert!(pending.as_ref().unwrap().0.sequence() < start.sequence());
        start
    });
    assert_eq!(
        stream.test_tick().unwrap().output_frame,
        capture_frame + 2 * FRAMES
    );
    if let Some(start) = &start {
        assert_eq!(start.original_frame(), Some(effect));
        assert!(engine.publish_pad_start(start, None).unwrap());
    }
    let mut before = None;
    for end in (capture_frame + 3 * FRAMES..=REQUEST).step_by(FRAMES as usize) {
        let tick = stream.test_tick().unwrap();
        assert_eq!(tick.output_frame, end);
        before = Some(tick);
    }
    let before = before.unwrap();
    if let Some((action, _)) = pending.as_ref().filter(|_| !missing) {
        assert_eq!(action.state(), "prepared");
    }
    let next = if let Some(start) = &start {
        assert_eq!(start.execution_frame(), Some(effect));
        assert_eq!(start.state(), "scheduled", "{:?}", start.reason());
        None
    } else {
        assert!(before.pads[1].active);
        if !bound_bpm {
            assert!(enqueue_stop_with_producer(
                ownership,
                &mut stream.producer.lock().unwrap(),
                Some(1),
            ));
        } else {
            assert_eq!(before.bound_rate_frame, Some(BPM_APPLY));
        }
        let next = ColdPcm::publish_at(
            &engine,
            2,
            1,
            "next",
            SourcePitchInitialization::FreshNeutral,
        );
        next.assert_ack(&engine, false);
        Some(next)
    };
    assert_eq!(submission.prefix(), (REQUEST, false));
    let ticket = engine
        .prepare_mix_capture_internal(path.to_str().unwrap(), FRAMES as usize, 4, 4096)
        .unwrap();
    let capture_status = || {
        engine
            .mix_capture_lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .control
            .status()
    };
    until(|| capture_status().writer_ready.then_some(()));
    assert!(engine.start_mix_capture_internal(&ticket, FIRST).unwrap());
    assert!(
        engine
            .stop_mix_capture_internal(&ticket, capture_end)
            .unwrap()
    );
    factory.arm_output_ready();
    let (tick, pcm, gated, prefix) = std::thread::scope(|scope| {
        let tick = scope.spawn(|| stream.test_tick().unwrap());
        let pcm = until(|| {
            factory
                .gated_output_pcm()
                .map(Some)
                .or_else(|| tick.is_finished().then_some(None))
        });
        let gated = capture_status();
        let prefix = submission.prefix();
        if pcm.is_some() {
            factory.release_output_ready(false);
        }
        (tick.join().unwrap(), pcm, gated, prefix)
    });
    let applied = start.as_ref().and_then(|start| start.effective_frame());
    let pitch_state = pending
        .as_ref()
        .map_or("absent", |(action, _)| action.state());
    eprintln!(
        "ASIO interior SET E={effect} request={REQUEST}..{} pitch={pitch_state} exclusive={applied:?} A={} B={} captured_end={:?} sealed_end={:?} raw={} rate={:?} Native={:#x} shifts={}",
        tick.output_frame,
        tick.pads[0].active,
        tick.pads[1].active,
        gated.captured_end,
        gated.end_frame,
        gated.written_frames,
        rate.effect.execution_frame,
        tick.pads[0].native_state,
        tick.native.shifts,
    );
    let pcm = pcm.expect("interior SET request reached actual outputReady");
    assert_eq!(tick.output_frame, REQUEST + FRAMES);
    assert_eq!(prefix, (REQUEST, false));
    assert!(!gated.terminal_decided && gated.payload_sha256.is_none());
    assert_eq!(submission.prefix(), (REQUEST + FRAMES, false));
    if missing {
        assert_eq!(engine.pitch_work_capacity.used(), 1);
        let worker = &pending.as_ref().unwrap().1;
        worker.release();
        worker.wait_for(true);
    }
    if let Some(next) = &next {
        next.assert_ack(&engine, true);
    }
    if let (Some(revision), Some(anchor)) = (bpm_revision, &anchor) {
        let (requested, applied) = ownership.global_rate_status.read_bpm().unwrap();
        let after = ownership.global_rate_status.read().unwrap();
        assert_eq!((requested, applied.revision), (revision, revision));
        assert!(applied.applied && applied.effective && applied.requested);
        assert_eq!(applied.reason, BpmReason::None);
        assert_eq!(applied.execution_frame, Some(BPM_APPLY));
        assert_eq!(tick.bound_rate_frame, None);
        assert_eq!(after.effect.effective.speed, SPEED);
        assert_eq!(
            after.effect.effective.period,
            Some(anchor.binding.accepted.unwrap().period_seconds / SPEED)
        );
        assert_eq!(
            after.master_provenance,
            MasterProvenance::qualified(
                master_provenance::source_identity(anchor).unwrap(),
                anchor.binding
            )
        );
        assert!(tick.pads[0].active && tick.pads[1].active && tick.pads[1].key_lock);
        for (id, source) in [(0, &a), (1, &b)] {
            assert_eq!(tick.pads[id].generation, before.pads[id].generation);
            assert_eq!(tick.pads[id].source_pcm, source.binding.source_address);
        }
        let b_native_request = tick.pads[1].adopted_native_request.unwrap();
        assert!(tick.pads[1].active_native);
        assert_ne!(tick.pads[1].native_state, before.pads[1].native_state);
        assert_eq!(tick.pads[0].native_state, before.pads[0].native_state);
        assert!(!tick.pads[0].active_native && !tick.pads[0].wet_failed);
        assert_eq!(tick.native.shifts, 0);
        let apply_sample = (2 * (BPM_APPLY - REQUEST)) as usize;
        assert!(pcm[..apply_sample].iter().any(|sample| *sample != 0.0));
        assert!(pcm[apply_sample..].iter().any(|sample| *sample != 0.0));
        eprintln!(
            "ASIO interior BPM applied={BPM_APPLY} BPM={revision} rate_revision={} B_work={b_native_request} B_native={:#x}",
            after.effect.revision, tick.pads[1].native_state
        );
        assert!(enqueue_stop_with_producer(
            ownership,
            &mut stream.producer.lock().unwrap(),
            Some(1)
        ));
        let stopped = stream.test_tick().unwrap();
        assert_eq!(stopped.output_frame, REQUEST + 2 * FRAMES);
        assert!(stopped.pads[0].active && !stopped.pads[1].active);
        assert!(capture_status().rt_sealed);
    }
    // Continue with the original stream to observe late custody and the spool.
    let final_status = until(|| {
        stream.test_tick().unwrap();
        let mut slot = engine.mix_capture_lock().unwrap();
        let capture = slot.as_mut().unwrap();
        capture.poll(engine.mix_capture_stream_failed());
        let status = capture.control.status();
        (status.worker_joined && status.rt_released && engine.pitch_work_capacity.used() == 0)
            .then_some(status)
    });
    let effective_frame = pending
        .as_ref()
        .and_then(|(action, _)| action.effective_output_frame());
    let reason = pending.as_ref().and_then(|(action, _)| action.reason());
    engine.shut_down().unwrap();
    let ended = owner.lock().unwrap();
    assert!(ended.owner_ended && ended.owner_join_verified && ended.retirement_verified);
    assert_eq!(factory.info().unwrap().active, 0);
    drop(start);
    drop(pending);
    drop(anchor);
    drop(engine);
    assert!(a.reader.upgrade().is_none() && b.reader.upgrade().is_none());
    assert!(next.is_none_or(|next| next.reader.upgrade().is_none()));

    if bound_bpm {
        assert_eq!(effective_frame, None);
        assert_eq!(reason, (!bpm_baseline).then_some("timing_changed"));
    } else if exclusive {
        assert_eq!(applied, Some(effect), "the bound exclusive STOP retains E");
        assert_eq!(effective_frame, (!missing).then_some(effect));
        assert!(!tick.pads[0].active && tick.pads[1].active);
    } else if restart && !inline {
        assert_eq!(
            applied,
            Some(effect),
            "the captured same-pad restart retains E"
        );
        assert_eq!(effective_frame, None);
        assert_eq!(reason, Some("timing_changed"));
        assert!(tick.pads[0].active && tick.pads[0].generation > before.pads[0].generation);
        assert_ne!(tick.pads[0].native_state, original.unwrap());
    } else {
        assert_eq!(effective_frame, Some(effect));
        assert_eq!(applied, None);
        assert!(tick.pads[0].active && !tick.pads[1].active);
        assert!(tick.pads[0].wet_current_source_output && !tick.pads[0].wet_failed);
        assert_eq!(tick.pads[0].native_state, original.unwrap());
        assert_eq!(tick.pads[0].generation, before.pads[0].generation);
        assert_eq!(tick.native.shifts, u64::from(inline && !no_fit));
    }
    assert_eq!(
        tick.callback.output_ready_calls,
        before.callback.output_ready_calls + 1
    );
    if case == InteriorSet::Fit {
        assert!(
            pcm[2 * (effect - REQUEST) as usize..2 * (capture_end - REQUEST) as usize]
                .iter()
                .any(|sample| *sample != 0.0)
        );
    }
    // A stop at the exclusive callback end executes in the following callback.
    assert_eq!(gated.rt_sealed, capture_end < REQUEST + FRAMES);
    assert_eq!(gated.captured_end, Some(capture_end));
    assert!(final_status.successful_spool());
    assert_eq!(final_status.written_frames, capture_end - FIRST);
    assert_eq!(
        (final_status.first_frame, final_status.end_frame),
        (Some(FIRST), Some(capture_end))
    );
    let expected: Vec<_> = pcm
        [2 * (FIRST - REQUEST) as usize..2 * (capture_end - REQUEST) as usize]
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    assert_eq!(
        &fs::read(&path).unwrap()[SPOOL_HEADER_BYTES as usize..],
        expected
    );
    if bound_bpm {
        eprintln!(
            "ASIO interior BPM {case:?} prefix={prefix:?}->({},{}) capture={FIRST}..{capture_end} frames={} STOP={} OwnACK and checked Close verified",
            REQUEST + FRAMES,
            false,
            final_status.written_frames,
            REQUEST + FRAMES,
        );
    }
    fs::remove_file(&path).unwrap();
    fs::remove_dir(&directory).unwrap();
    pcm.into_iter().map(f32::to_bits).collect()
}

#[test]
fn asio_unbound_bpm_on_after_terminal_scalar_preserves_aligned_wet_contract() {
    let without_set = terminal_scalar_bpm_at_aligned_set(0.7, false);
    assert_eq!(terminal_scalar_bpm_at_aligned_set(0.7, true), without_set);
}

#[test]
fn asio_unbound_bpm_on_after_terminal_scalar_admits_the_same_rate() {
    terminal_scalar_bpm_at_aligned_set(0.5, true);
}

fn terminal_scalar_bpm_at_aligned_set(period: f64, with_set: bool) -> Vec<u32> {
    use crate::audio_engine::constant_timing::l3_constant_timing_fixture::publish_synthetic_loaded_timing;
    use crate::audio_engine::constants::NUM_SAMPLES;
    use crate::audio_engine::global_rate_status::{BpmReason, RateError, RateState};
    use crate::audio_engine::input_mapping::PitchPadProjection;
    use crate::audio_engine::input_runtime_binding;
    use crate::audio_engine::master_provenance::MasterProvenance;
    use crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::WorkerGate;

    const FRAMES: u64 = 96;
    const RATE: u32 = 96_000;
    const UNITY: u64 = 19_872;
    const BIRTH_B: u64 = 19_968;
    const SCALAR_CAPTURE: u64 = 20_160;
    const SCALAR_BOUND: u64 = 24_256;
    const E: u64 = 24_000;
    const H: u64 = E + 64;
    const FIRST: u64 = E + 7;
    const END: u64 = E + 93;
    const MASTER: f64 = 0.5;
    const LOOP_END: usize = 90;

    Python::initialize();
    let directory = tempfile::tempdir().unwrap().keep();
    let path = directory.join("take.capture");
    eprintln!(
        "ASIO terminal scalar period={period} SET={with_set} spool: {}",
        path.display()
    );
    let factory = Factory::new().unwrap();
    let mut engine = AudioEngine::new().unwrap();
    engine.test_asio_driver = Some(factory.clone());
    engine
        .start_audio_output(OutputOptions {
            backend: "asio".into(),
            sample_rate_hz: Some(RATE),
            buffer_frames: Some(FRAMES as u32),
            ..Default::default()
        })
        .unwrap();
    engine.set_bpm_lock_with_binding(false, None).unwrap();
    let a = ColdPcm::publish_with_geometry(
        &engine,
        0,
        1,
        "a",
        SourcePitchInitialization::FreshNeutral,
        25_600,
        Some((0, LOOP_END)),
    );
    let b = ColdPcm::publish_with_geometry(
        &engine,
        1,
        1,
        "b",
        SourcePitchInitialization::FreshNeutral,
        48_000,
        None,
    );
    let stream = engine.stream_handle.as_ref().unwrap();
    let owner = stream.test_owner_observer();
    let submission = stream.output_submission.as_ref().unwrap().clone();
    let play = |stream: &crate::audio_engine::audio_stream::AudioStreamHandle, id| {
        stream
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::PlaySample {
                id,
                volume: 1.0,
                received_at_ns: None,
                expected_source_generation: Some(1),
            })
            .unwrap();
    };
    play(stream, 0);
    let a_born = stream.test_tick().unwrap();
    assert_eq!(a_born.output_frame, FRAMES);
    assert!(a_born.pads[0].active && !a_born.pads[0].key_lock && !a_born.pads[1].active);
    a.assert_ack(&engine, true);
    b.assert_ack(&engine, true);
    engine
        .set_pad_timing_intent_with_source(0, "automatic", Some(1))
        .unwrap();
    engine
        .set_pad_timing_intent_with_source(1, "automatic", Some(1))
        .unwrap();
    let timing_a = publish_synthetic_loaded_timing(&engine, &stream.producer, 0, period);
    let timing_b = publish_synthetic_loaded_timing(&engine, &stream.producer, 1, MASTER);
    assert_eq!(stream.test_tick().unwrap().output_frame, 2 * FRAMES);
    assert_eq!(timing_a.publication_status().unwrap(), "accepted");
    assert_eq!(timing_b.publication_status().unwrap(), "accepted");
    let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
    let accepted_period = binding.binding.accepted.unwrap().period_seconds;
    assert!((accepted_period - period).abs() < 1e-12);
    let mut loaded = vec![false; NUM_SAMPLES];
    loaded[0] = true;
    let mut bindings = vec![None; NUM_SAMPLES];
    bindings[0] = Some(&binding);
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    rows[0].quantize_step_64ths = Some(8);
    // Publish hierarchy before either rate receipt freezes its authority.
    engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_pitch(
            true,
            loaded,
            vec![0.0; NUM_SAMPLES],
            vec![None; NUM_SAMPLES],
            bindings,
            rows,
        )
        .unwrap();
    let key_lock = engine
        .set_pad_key_lock_with_source(1, true, Some(1))
        .unwrap();
    let stream = engine.stream_handle.as_ref().unwrap();
    let ownership = &engine.input_runtime_ownership;
    let push_rate = |speed, period| {
        push_global_rate_message(
            &ownership.global_timing_revision,
            &ownership.global_timing_publishing,
            &ownership.global_rate_status,
            &mut stream.parameter_producer.lock().unwrap(),
            Some(speed),
            period,
            "ASIO terminal scalar",
        )
        .unwrap();
        ownership.global_rate_status.requested_revision()
    };
    push_rate(1.4, Some(MASTER));
    for frame in (2 * FRAMES..UNITY).step_by(FRAMES as usize) {
        assert_eq!(stream.test_tick().unwrap().output_frame, frame + FRAMES);
    }
    let lock = ownership.key_lock_status(1).unwrap();
    assert_eq!(lock.request_id, key_lock);
    assert!(lock.effective && lock.ready);
    let (off_revision, off) = ownership.global_rate_status.read_bpm().unwrap();
    assert_eq!(off_revision, off.revision);
    assert!(!off.effective && !off.requested && off.applied);
    let unity_revision = push_rate(1.0, None);
    let before_birth = stream.test_tick().unwrap();
    assert_eq!(before_birth.output_frame, BIRTH_B);
    assert!(!before_birth.pads[1].active);
    let unity = ownership.global_rate_status.read().unwrap().effect;
    assert_eq!(unity.revision, unity_revision);
    assert_eq!(unity.state, RateState::Applied);
    assert_eq!(unity.execution_frame, Some(UNITY));
    assert_eq!(unity.effective.speed, 1.0);
    assert_eq!(unity.effective.period, Some(MASTER));
    play(stream, 1);
    let b_born = stream.test_tick().unwrap();
    assert_eq!(b_born.output_frame, BIRTH_B + FRAMES);
    assert!(b_born.pads[1].active && b_born.pads[1].key_lock && !b_born.pads[1].active_native);
    assert_eq!(b_born.pads[1].rate_target, 1.0);
    assert_eq!(b_born.pads[1].frame, FRAMES as usize);
    assert_eq!(stream.test_tick().unwrap().output_frame, SCALAR_CAPTURE);

    let scalar_revision = push_rate(1.2, None);
    engine.set_bpm_lock_with_binding(true, None).unwrap();
    let (bpm_revision, previous_bpm) = ownership.global_rate_status.read_bpm().unwrap();
    assert_eq!(previous_bpm.revision, off_revision);
    assert!(bpm_revision > off_revision);
    let captured = stream.test_tick().unwrap();
    assert_eq!(captured.output_frame, SCALAR_CAPTURE + FRAMES);
    let b_work = captured.pads[1]
        .bound_scalar_participant
        .unwrap()
        .1
        .expect("captured B Native work");
    assert_ne!(b_work, 0);
    let assert_pending = |tick: crate::audio_engine::asio_output::test_driver::TickObservation| {
        assert_eq!(tick.bound_rate_frame, Some(SCALAR_BOUND));
        assert!(tick.pads[0].active && !tick.pads[0].key_lock);
        assert!(tick.pads[1].active && tick.pads[1].key_lock && !tick.pads[1].active_native);
        assert_eq!(tick.pads[0].generation, a_born.pads[0].generation);
        assert_eq!(tick.pads[1].generation, b_born.pads[1].generation);
        assert_eq!(tick.pads[0].source_pcm, a.binding.source_address);
        assert_eq!(tick.pads[1].source_pcm, b.binding.source_address);
        assert_eq!(
            tick.pads[0].bound_scalar_participant,
            Some((a_born.pads[0].generation, None))
        );
        assert_eq!(
            tick.pads[1].bound_scalar_participant,
            Some((b_born.pads[1].generation, Some(b_work)))
        );
        assert_eq!(tick.pads[0].rate_target, 1.0);
        assert_eq!(tick.pads[1].rate_target, 1.0);
        let rate = ownership.global_rate_status.read().unwrap();
        assert_eq!(
            (rate.request_revision, rate.effect.revision),
            (scalar_revision, scalar_revision)
        );
        assert_eq!(rate.effect.state, RateState::Preparing);
        assert_eq!(rate.effect.execution_frame, Some(SCALAR_BOUND));
        assert_eq!(rate.effect.requested.speed, 1.2);
        assert_eq!(rate.effect.effective.speed, 1.0);
        let (requested, bpm) = ownership.global_rate_status.read_bpm().unwrap();
        assert_eq!((requested, bpm.revision), (bpm_revision, bpm_revision));
        assert!(bpm.requested && !bpm.effective && !bpm.applied);
        assert_eq!(bpm.reason, BpmReason::RatePending);
        assert_eq!(bpm.execution_frame, None);
    };
    assert_pending(captured);

    let runtime = engine.input_runtime.as_ref().unwrap();
    let action = with_set.then(|| {
        let clock = stream.output_clock.read().unwrap();
        let action = runtime
            .capture_gui_pitch(&binding, "pitch.extra:3:set", clock.observed_at_ns)
            .unwrap();
        assert_eq!(action.original_due_frame(), E);
        let worker = WorkerGate::for_pitch_return(&action);
        assert!(engine.publish_pitch_action(&action).unwrap());
        (action, worker)
    });
    let mut before = stream.test_tick().unwrap();
    assert_eq!(before.output_frame, SCALAR_CAPTURE + 2 * FRAMES);
    let original_native = action.as_ref().map(|(action, worker)| {
        assert!(worker.wait_for(true).is_none());
        assert_eq!(action.execution_target(), E);
        let ((block, input, output), wet) = worker.prepared_fifo();
        eprintln!("ASIO terminal scalar Worker E={E} H={H} block={block} input={input} output={output} Wet={wet:?}");
        assert_eq!((block, input, output), (512, 448, 63));
        assert_eq!(wet, Some((64, 1)));
        let native = worker.prepared_native();
        worker.release();
        native
    });
    let cursor = |tick: crate::audio_engine::asio_output::test_driver::TickObservation| {
        tick.pads[0].frame as f64 + tick.pads[0].fraction
    };
    let mut first_delta = None;
    let mut varied = None;
    for end in (before.output_frame + FRAMES..=E).step_by(FRAMES as usize) {
        let previous = cursor(before);
        before = stream.test_tick().unwrap();
        assert_eq!(before.output_frame, end);
        if before.bound_rate_frame.is_none() {
            let rate = ownership.global_rate_status.read().unwrap().effect;
            panic!(
                "Scalar released before STOP at {end}: {:?}/{:?}/{:?}",
                rate.state, rate.error, rate.execution_frame
            );
        }
        // The observed 1.0–1.4 varispeed crosses one whole loop plus a remainder.
        let delta = LOOP_END as f64 + (cursor(before) - previous).rem_euclid(LOOP_END as f64);
        let first = *first_delta.get_or_insert((end, delta));
        if (delta - first.1).abs() > 1e-6 {
            varied = Some((first, (end, delta)));
        }
    }
    let (early, late) = varied.expect("real varying A cursor inside retained history");
    assert!((E - 4096..E).contains(&early.0) && (E - 4096..=E).contains(&late.0));
    assert_pending(before);
    if let Some((action, _)) = &action {
        assert_eq!(action.state(), "prepared");
        assert_eq!(action.execution_target(), E);
    }
    assert_eq!(submission.prefix(), (E, false));
    assert!(enqueue_stop_with_producer(
        ownership,
        &mut stream.producer.lock().unwrap(),
        Some(1)
    ));
    let c = ColdPcm::publish_at(&engine, 2, 1, "c", SourcePitchInitialization::FreshNeutral);
    c.assert_ack(&engine, false);
    let ticket = engine
        .prepare_mix_capture_internal(path.to_str().unwrap(), FRAMES as usize, 4, 4096)
        .unwrap();
    let capture_status = || {
        engine
            .mix_capture_lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .control
            .status()
    };
    until(|| capture_status().writer_ready.then_some(()));
    assert!(engine.start_mix_capture_internal(&ticket, FIRST).unwrap());
    assert!(engine.stop_mix_capture_internal(&ticket, END).unwrap());
    factory.arm_output_ready();
    let (tick, pcm, gated, prefix) = std::thread::scope(|scope| {
        let tick = scope.spawn(|| stream.test_tick().unwrap());
        let pcm = until(|| {
            factory
                .gated_output_pcm()
                .map(Some)
                .or_else(|| tick.is_finished().then_some(None))
        });
        let gated = capture_status();
        let prefix = submission.prefix();
        if pcm.is_some() {
            factory.release_output_ready(false);
        }
        (tick.join().unwrap(), pcm, gated, prefix)
    });
    let rate = ownership.global_rate_status.read().unwrap();
    let (requested, bpm) = ownership.global_rate_status.read_bpm().unwrap();
    let set_effect = action
        .as_ref()
        .and_then(|(action, _)| action.effective_output_frame());
    let set_reason = action.as_ref().and_then(|(action, _)| action.reason());
    eprintln!(
        "ASIO terminal scalar period={period} SET={with_set} A_deltas={early:?},{late:?} B_work={b_work} Scalar={scalar_revision}/{:?}/{:?}/{:?} BPM={bpm_revision}/{:?}/{:?} STOP_B={} SET_effect={set_effect:?}/{set_reason:?} A_target={} A_Native={:#x} wet_failed={} shifts={} outputReady={prefix:?}->{:?}",
        rate.effect.state,
        rate.effect.error,
        rate.effect.execution_frame,
        bpm.execution_frame,
        bpm.reason,
        !tick.pads[1].active,
        tick.pads[0].rate_target,
        tick.pads[0].native_state,
        tick.pads[0].wet_failed,
        tick.native.shifts,
        submission.prefix(),
    );
    let pcm = pcm.expect("actual outputReady with the whole request");
    assert_eq!(factory.info().unwrap().rate, f64::from(RATE));
    assert_eq!(tick.callback.frames, FRAMES as u32);
    assert_eq!(tick.output_frame, E + FRAMES);
    assert_eq!(
        tick.callback.output_ready_calls,
        before.callback.output_ready_calls + 1
    );
    assert_eq!(prefix, (E, false));
    assert_eq!(submission.prefix(), (E + FRAMES, false));
    assert!(!gated.terminal_decided && gated.payload_sha256.is_none());
    assert_eq!(
        (rate.request_revision, rate.effect.revision),
        (scalar_revision, scalar_revision)
    );
    assert_eq!(rate.effect.state, RateState::Rejected);
    assert_eq!(rate.effect.execution_frame, Some(SCALAR_BOUND));
    assert_eq!(tick.bound_rate_frame, None);
    if set_effect.is_none() {
        // In the conflict Red, SET itself can also invalidate the captured A tuple.
        assert_eq!(rate.effect.error, RateError::ParticipantChanged);
    }
    assert_eq!((requested, bpm.revision), (bpm_revision, bpm_revision));
    assert_eq!(bpm.execution_frame, Some(E));
    assert_eq!(bpm.reason, BpmReason::None);
    assert!(bpm.requested && bpm.effective && bpm.applied);
    assert_eq!(rate.effect.effective.speed, 1.0);
    assert_eq!(rate.effect.effective.period, Some(MASTER));
    assert_eq!(rate.master_provenance, MasterProvenance::Unknown);
    assert!(tick.pads[0].active && !tick.pads[1].active && !tick.pads[2].active);
    assert_eq!(tick.pads[0].source_pcm, a.binding.source_address);
    assert_eq!(tick.pads[0].generation, before.pads[0].generation);
    assert_eq!(
        tick.pads[0].rate_target.to_bits(),
        (accepted_period / MASTER).to_bits()
    );
    c.assert_ack(&engine, true);
    let status = until(|| {
        let mut slot = engine.mix_capture_lock().unwrap();
        let state = slot.as_mut().unwrap();
        state.poll(engine.mix_capture_stream_failed());
        let status = state.control.status();
        (status.worker_joined && status.rt_released && engine.pitch_work_capacity.used() == 0)
            .then_some(status)
    });
    engine.shut_down().unwrap();
    let ended = owner.lock().unwrap();
    assert!(ended.owner_ended && ended.owner_join_verified && ended.retirement_verified);
    assert_eq!(factory.info().unwrap().active, 0);
    drop(action);
    drop(binding);
    drop(timing_a);
    drop(timing_b);
    drop(engine);
    assert!(
        a.reader.upgrade().is_none()
            && b.reader.upgrade().is_none()
            && c.reader.upgrade().is_none()
    );
    assert!(status.successful_spool());
    assert_eq!(
        (status.first_frame, status.end_frame, status.written_frames),
        (Some(FIRST), Some(END), END - FIRST)
    );
    assert_eq!(pcm.len(), 2 * FRAMES as usize);
    assert!(pcm.iter().any(|sample| *sample != 0.0));
    let expected: Vec<_> = pcm[2 * (FIRST - E) as usize..2 * (END - E) as usize]
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    assert_eq!(
        &fs::read(&path).unwrap()[SPOOL_HEADER_BYTES as usize..],
        expected
    );
    eprintln!(
        "ASIO terminal scalar capture {FIRST}..{END} bitwise to driver PCM, OwnACK and checked Close"
    );
    if let Some(native) = original_native.filter(|_| period == MASTER) {
        assert_eq!(set_effect, Some(E));
        assert_eq!(set_reason, None);
        assert!(tick.pads[0].active_native);
        assert_eq!(tick.pads[0].native_state, native);
    } else {
        assert_eq!(set_effect, None);
        assert_eq!(set_reason, with_set.then_some("timing_changed"));
        assert!(!tick.pads[0].active_native);
        assert_eq!(tick.pads[0].native_state, before.pads[0].native_state);
    }
    assert!(!tick.pads[0].wet_failed);
    assert_eq!(tick.native.shifts, 0);
    fs::remove_file(&path).unwrap();
    fs::remove_dir(&directory).unwrap();
    pcm.into_iter().map(f32::to_bits).collect()
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
    let submission = stream.output_submission.as_ref().unwrap().clone();
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
