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

#[derive(Clone, Copy, Debug)]
enum OrderedWetControl {
    Loop,
    StagedBpm(f64),
    BpmBehindStop(f64),
    BpmOff { speed: f64, behind_stop: bool },
    MasterRefresh(f64),
}

fn aligned_wet_set_with_ordered_control(control: OrderedWetControl) {
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
    const CAPTURE_END: u64 = E + 93;
    const OLD_END: f64 = 90.0;
    const NEW_START: f64 = 8.0;
    let seconds = |frame| frame / f64::from(RATE);
    let anchor_period = match control {
        OrderedWetControl::Loop => None,
        OrderedWetControl::StagedBpm(period) | OrderedWetControl::BpmBehindStop(period) => {
            Some(period)
        }
        OrderedWetControl::BpmOff { speed, .. } => Some(0.5 * speed),
        OrderedWetControl::MasterRefresh(_) => Some(0.7),
    };
    let off_speed = match control {
        OrderedWetControl::BpmOff { speed, .. } => Some(speed),
        _ => None,
    };
    let master_refresh = matches!(control, OrderedWetControl::MasterRefresh(_));
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
    let ownership = &engine.input_runtime_ownership;
    assert_eq!(timing.publication_status().unwrap(), "pending");
    let anchor_timing = anchor_period.map(|period| {
        let timing = publish_synthetic_loaded_timing(&engine, &stream.producer, 2, period);
        assert_eq!(timing.publication_status().unwrap(), "pending");
        timing
    });
    assert_eq!(stream.test_tick().unwrap().output_frame, 2 * FRAMES);
    assert_eq!(timing.publication_status().unwrap(), "accepted");
    let runtime = engine.input_runtime.as_ref().unwrap();
    let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
    assert!(binding.current() && binding.available() && binding.binding.accepted.is_some());
    let mut anchor = anchor_timing.as_ref().map(|timing| {
        assert_eq!(timing.publication_status().unwrap(), "accepted");
        let binding = input_runtime_binding::capture(&engine, 2).unwrap().unwrap();
        assert!(binding.current() && binding.available() && binding.binding.accepted.is_some());
        binding
    });
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
            assert_eq!(stream.test_tick().unwrap().output_frame, end);
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
    let master = if let OrderedWetControl::MasterRefresh(period) = control {
        let (revision, bpm, before, original_source) = prior_lock.as_ref().unwrap();
        let previous_timing = anchor.as_ref().unwrap().binding.accepted.unwrap();
        let newer = publish_synthetic_loaded_timing(&engine, &stream.producer, 2, period);
        assert_eq!(newer.publication_status().unwrap(), "pending");
        assert_eq!(stream.test_tick().unwrap().output_frame, next_tick);
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
            .stop_mix_capture_internal(&capture, CAPTURE_END)
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
        assert_eq!(stream.test_tick().unwrap().output_frame, end);
    }
    let clock = stream.output_clock.read().unwrap();
    assert!(clock.valid && clock.output_frame == capture_frame - FRAMES);
    let action = runtime
        .capture_gui_pitch(&binding, "pitch.extra:3:set", clock.observed_at_ns)
        .unwrap();
    assert_eq!(action.original_due_frame(), E);
    let worker = WorkerGate::for_pitch_return(&action);
    assert!(engine.publish_pitch_action(&action).unwrap());
    assert_eq!(
        stream.test_tick().unwrap().output_frame,
        capture_frame + FRAMES
    );
    assert!(worker.wait_for(true).is_none());
    worker.release();
    assert_eq!(worker.prepared_wet(), (64, 1));
    let original_native = worker.prepared_native();
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
    assert_eq!(submission.prefix(), (E, false));
    assert!(before.pads[0].active && before.pads[1].active);
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
            (Some(E - FRAMES), Some(CAPTURE_END))
        );
        assert!(status.successful_spool());
        assert_eq!(status.written_frames, FRAMES + CAPTURE_END - E);
        let pcm = pcm.expect("actual outputReady gate");
        let expected = &pcm[..(2 * (CAPTURE_END - E)) as usize];
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
        return;
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
    let (tick, pcm, refresh, next, gated_prefix, gated_ack) = std::thread::scope(|scope| {
        let (claimed, wait_claim) = sync_channel(1);
        let (published, wait_publication) = sync_channel(1);
        let callback = scope.spawn(|| {
            stream.test_tick_with_effect(Some(Box::new(move || {
                if claimed.send(()).is_ok() {
                    let _ = wait_publication.recv_timeout(WAIT);
                }
            })))
        });
        wait_claim.recv_timeout(WAIT).unwrap();
        assert!(action.effective());
        let refresh = engine
            .refresh_current_constant_timing(
                refresh_binding,
                seconds(start),
                Some(seconds(OLD_END)),
                master_period,
            )
            .unwrap();
        assert_eq!(refresh.publication_status(), "pending");
        let next = ColdPcm::publish_at(
            &engine,
            next_id,
            1,
            "next",
            SourcePitchInitialization::FreshNeutral,
        );
        next.assert_ack(&engine, false);
        published.send(()).unwrap();
        let pcm = until(|| {
            factory
                .gated_output_pcm()
                .map(Some)
                .or_else(|| callback.is_finished().then_some(None))
        });
        let gated_prefix = submission.prefix();
        let gated_ack = ownership.binding_source_adoption_accepted(next_id, next.binding);
        if pcm.is_some() {
            factory.release_output_ready(false);
        }
        (
            callback.join().unwrap().unwrap(),
            pcm,
            refresh,
            next,
            gated_prefix,
            gated_ack,
        )
    });
    let execution = refresh.execution();
    let confirmed = submission.prefix();
    let current = refresh.is_current();
    let actual_end_position = position(tick.pads[0]);
    let publication = refresh.publication_status();
    let rate = ownership.global_rate_status.read().unwrap();
    let bpm = ownership.global_rate_status.read_bpm();
    let failed = engine.mix_capture_stream_failed();
    let provenance = anchor.as_ref().map(|anchor| {
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

    assert_eq!(publication, "accepted");
    assert!(current);
    if let Some((period, revision)) = master {
        assert_eq!(rate.effect.effective.period, Some(period));
        assert_eq!(Some(rate.master_provenance), provenance);
        let (requested, applied) = bpm.unwrap();
        assert_eq!((requested, applied.revision), (revision, revision));
        assert!(applied.applied && applied.effective);
        assert!(action_effective);
        assert_eq!(tick.pads[0].native_state, original_native);
        assert!(!tick.pads[2].active);
        let same_rate = matches!(control, OrderedWetControl::MasterRefresh(0.7));
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
        (Some(E - FRAMES), Some(CAPTURE_END))
    );
    assert!(status.successful_spool());
    assert_eq!(status.written_frames, FRAMES + CAPTURE_END - E);
    let pcm = pcm.expect("actual outputReady gate");
    let expected = &pcm[..(2 * (CAPTURE_END - E)) as usize];
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
}

#[test]
fn asio_missing_interior_set_releases_same_frame_stop_and_capture_end() {
    use crate::audio_engine::constants::NUM_SAMPLES;
    use crate::audio_engine::input_mapping::PitchPadProjection;
    use crate::audio_engine::input_runtime_binding;
    use crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::WorkerGate;

    const FRAMES: u64 = 96;
    const F: u64 = 18_240;
    const REQUEST: u64 = 24_000;
    const E: u64 = 24_016;
    const FIRST: u64 = REQUEST + 7;
    const STOP: u64 = REQUEST + 73;
    Python::initialize();
    let directory = tempfile::tempdir().unwrap().keep();
    let path = directory.join("take.capture");
    eprintln!("ASIO interior SET spool: {}", path.display());
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
    engine.set_master_period(E as f64 / 48_000.0).unwrap();
    engine.set_trigger_quantization("1/16").unwrap();
    let stream = engine.stream_handle.as_ref().unwrap();
    let owner = stream.test_owner_observer();
    let submission = stream.output_submission.as_ref().unwrap().clone();
    let ownership = &engine.input_runtime_ownership;
    let a = ColdPcm::publish(&engine, 1, "a");
    let b = ColdPcm::publish_at(&engine, 1, 1, "b", SourcePitchInitialization::FreshNeutral);
    a.assert_ack(&engine, false);
    b.assert_ack(&engine, false);
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
    assert!(stream.test_tick().unwrap().pads[0].active);
    a.assert_ack(&engine, true);
    b.assert_ack(&engine, true);

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
    for end in (2 * FRAMES..=F).step_by(FRAMES as usize) {
        if end == F {
            // Set the real rate before either action freezes a source trajectory.
            push_global_rate_message(
                &ownership.global_timing_revision,
                &ownership.global_timing_publishing,
                &ownership.global_rate_status,
                &mut stream.parameter_producer.lock().unwrap(),
                Some(1.2),
                None,
                "ASIO interior SET",
            )
            .unwrap();
        }
        assert_eq!(stream.test_tick().unwrap().output_frame, end);
    }
    let rate = ownership.global_rate_status.read().unwrap();
    assert_eq!(rate.effect.execution_frame, Some(F - FRAMES));
    assert_eq!(rate.request_revision, rate.effect.revision);
    let clock = stream.output_clock.read().unwrap();
    assert!(clock.valid);
    let action = runtime
        .capture_gui_pitch(&binding, "pitch.extra:3:set", clock.observed_at_ns)
        .unwrap();
    assert_eq!(action.original_due_frame(), E);
    let worker = WorkerGate::for_pitch(&action);
    assert!(engine.publish_pitch_action(&action).unwrap());
    assert_eq!(stream.test_tick().unwrap().output_frame, F + FRAMES);
    worker.wait_for(false);
    assert_eq!(action.execution_target(), E);

    // A genuine independently captured exclusive launch stops A at its own E.
    let start = engine
        .capture_pad_start(1, 0.0, None, 0.25, true, None, Some(1))
        .unwrap();
    assert!(action.sequence() < start.sequence());
    assert_eq!(stream.test_tick().unwrap().output_frame, F + 2 * FRAMES);
    assert_eq!(start.original_frame(), Some(E));
    assert!(engine.publish_pad_start(&start, None).unwrap());
    for end in (F + 3 * FRAMES..=REQUEST).step_by(FRAMES as usize) {
        assert_eq!(stream.test_tick().unwrap().output_frame, end);
    }
    assert_eq!(start.execution_frame(), Some(E));
    assert_eq!(start.state(), "scheduled", "{:?}", start.reason());
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
    assert!(engine.stop_mix_capture_internal(&ticket, STOP).unwrap());
    factory.arm_output_ready();
    let (tick, pcm, gated, prefix) = std::thread::scope(|scope| {
        let tick = scope.spawn(|| stream.test_tick().unwrap());
        let pcm = until(|| factory.gated_output_pcm());
        let gated = capture_status();
        let prefix = submission.prefix();
        factory.release_output_ready(false);
        (tick.join().unwrap(), pcm, gated, prefix)
    });
    let applied = start.effective_frame();
    let launch_state = start.state();
    let launch_reason = start.reason();
    let pitch_state = action.state();
    eprintln!(
        "ASIO interior SET E={E} request={REQUEST}..{} pitch={pitch_state} exclusive={applied:?}/{launch_state}/{launch_reason:?} A={} B={} captured_end={:?} sealed_end={:?} raw={} rate={:?}",
        tick.output_frame,
        tick.pads[0].active,
        tick.pads[1].active,
        gated.captured_end,
        gated.end_frame,
        gated.written_frames,
        rate.effect.execution_frame,
    );
    assert_eq!(tick.output_frame, REQUEST + FRAMES);
    assert_eq!(prefix, (REQUEST, false));
    assert!(!gated.terminal_decided && gated.payload_sha256.is_none());
    assert_eq!(submission.prefix(), (REQUEST + FRAMES, false));
    assert_eq!(engine.pitch_work_capacity.used(), 1);
    worker.release();
    worker.wait_for(true);
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
    assert!(!action.effective() && action.state() == "rejected");
    engine.shut_down().unwrap();
    let ended = owner.lock().unwrap();
    assert!(ended.owner_ended && ended.owner_join_verified && ended.retirement_verified);
    assert_eq!(factory.info().unwrap().active, 0);
    drop(start);
    drop(action);
    drop(engine);
    assert!(a.reader.upgrade().is_none() && b.reader.upgrade().is_none());

    assert_eq!(
        applied,
        Some(E),
        "missing SET must release the exclusive STOP at E"
    );
    assert!(!tick.pads[0].active && tick.pads[1].active);
    assert_eq!(pitch_state, "rejected");
    assert!(gated.rt_sealed);
    assert_eq!(gated.captured_end, Some(STOP));
    assert!(final_status.successful_spool());
    assert_eq!(final_status.written_frames, STOP - FIRST);
    assert_eq!(
        (final_status.first_frame, final_status.end_frame),
        (Some(FIRST), Some(STOP))
    );
    let expected: Vec<_> = pcm[2 * (FIRST - REQUEST) as usize..2 * (STOP - REQUEST) as usize]
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    assert_eq!(
        &fs::read(&path).unwrap()[SPOOL_HEADER_BYTES as usize..],
        expected
    );
    fs::remove_file(&path).unwrap();
    fs::remove_dir(&directory).unwrap();
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
