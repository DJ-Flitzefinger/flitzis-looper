//! Real bounded worker -> command drain -> ACK control proofs with raw WAV oracles.
use super::*;
use crate::audio_engine::resident_relocation::{
    ResidentWindowTicket, WindowRequest, prepare_window_with_producer, reconcile,
};

struct Loaded {
    directory: tempfile::TempDir,
    engine: AudioEngine,
    callback: Callback,
    mono: Vec<f32>,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    consumer: rtrb::Consumer<ControlMessage>,
}

impl Loaded {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("control.wav");
        let mono = wav(&source, 48_000);
        let engine = AudioEngine::new().unwrap();
        let mut callback = Callback::new(&engine, 48_000);
        let (request, mut load) = selected_load(
            &engine,
            &source,
            directory.path(),
            48_000,
            ResidentLoadHint {
                start_s: 32.0 / 48_000.0,
                end_s: 64.0 / 48_000.0,
                key_lock: false,
            },
        );
        wait_until(Duration::from_secs(10), || load.peek().is_ok());
        assert_eq!(callback.drain(&mut load), 1);
        assert!(matches!(
            terminal(&engine, request),
            LoaderEvent::Success { .. }
        ));
        wait_until(Duration::from_secs(10), || {
            engine.cold_loading[0].load(Ordering::Acquire) == 0
        });
        let (producer, consumer) = rtrb::RingBuffer::new(8);
        Self {
            directory,
            engine,
            callback,
            mono,
            producer: Arc::new(Mutex::new(producer)),
            consumer,
        }
    }

    fn prepare(&self, request: WindowRequest) -> ResidentWindowTicket {
        prepare_window_with_producer(&self.engine, 0, request, self.producer.clone()).unwrap()
    }

    fn pending(&mut self, ticket: &ResidentWindowTicket) {
        wait_until(Duration::from_secs(10), || {
            if matches!(
                self.consumer.peek(),
                Ok(ControlMessage::CaptureResidentSeek(_))
            ) {
                self.callback.drain(&mut self.consumer);
            }
            (ticket.publication_status() == "pending" && !self.consumer.is_empty())
                || matches!(
                    ticket.publication_status(),
                    "accepted" | "failed" | "cancelled"
                )
        });
        assert!(
            matches!(ticket.publication_status(), "pending" | "accepted"),
            "{:?}",
            ticket.error().unwrap()
        );
        assert!(ticket.is_current());
    }

    fn adopt(&mut self, ticket: &ResidentWindowTicket) {
        self.pending(ticket);
        // The publication becomes pending immediately before the single producer
        // commits its already-reserved ring slot. Wait for the actual drain ACK.
        wait_until(Duration::from_secs(10), || {
            self.callback.drain(&mut self.consumer);
            ticket.publication_status() == "accepted"
        });
        assert_eq!(
            ticket.publication_status(),
            "accepted",
            "{:?}",
            ticket.error().unwrap()
        );
        assert!(ticket.is_current());
        reconcile(&self.engine).unwrap();
        assert!(ticket.is_current());
    }

    fn sample(&self) -> SampleBuffer {
        self.engine.sample_cache.lock().unwrap()[0].clone().unwrap()
    }

    fn midi_runtime(&self) -> crate::audio_engine::input_mapping::InputRuntime {
        let binding = crate::audio_engine::input_runtime_binding::capture(&self.engine, 0)
            .unwrap()
            .unwrap();
        let runtime = crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
            self.producer.clone(),
            crate::audio_engine::timing::InputClock::new(),
            self.engine.input_runtime_ownership.clone(),
        );
        let mut loaded = vec![false; NUM_SAMPLES];
        loaded[0] = true;
        let mut bindings = vec![None; NUM_SAMPLES];
        bindings[0] = Some(&binding);
        runtime
            .set_runtime_state(
                true,
                loaded,
                vec![32.0 / 48_000.0; NUM_SAMPLES],
                vec![Some(64.0 / 48_000.0); NUM_SAMPLES],
                bindings,
            )
            .unwrap();
        runtime.set_enabled(true);
        runtime.replace_mappings(vec![
            ("midi:note:1:60".into(), "pad.trigger:0".into()),
            ("midi:note:1:61".into(), "pad.stop:0".into()),
        ]);
        runtime
    }

    fn replace_bank(&mut self) -> SampleBuffer {
        let replacement = self.directory.path().join("replacement.wav");
        write_pcm16(&replacement, 48_000, 1, &vec![8192_i16; 1000]);
        let (producer, mut load) = rtrb::RingBuffer::new(4);
        let request = admit_for_format_selected(
            &self.engine,
            0,
            replacement.to_string_lossy().into_owned(),
            (
                false,
                false,
                false,
                Some(ResidentLoadHint {
                    start_s: 100.0 / 48_000.0,
                    end_s: 300.0 / 48_000.0,
                    key_lock: false,
                }),
            ),
            Arc::new(Mutex::new(producer)),
            (2, 48_000, self.directory.path().to_owned()),
        )
        .unwrap();
        wait_until(Duration::from_secs(10), || !load.is_empty());
        assert_eq!(self.callback.drain(&mut load), 1);
        assert!(matches!(
            terminal(&self.engine, request),
            LoaderEvent::Success { .. }
        ));
        wait_until(Duration::from_secs(10), || {
            self.engine.cold_loading[0].load(Ordering::Acquire) == 0
        });
        self.sample()
    }
}

#[test]
fn actual_worker_stop_revisions_cover_ui_midi_and_global_queued_scheduled_and_fresh_starts() {
    use crate::audio_engine::global_playback_batch;
    use crate::audio_engine::input_runtime_binding::{capture, enqueue_stop_with_producer};
    use crate::audio_engine::resident_relocation::launch_with_producer;
    for route in 0..3 {
        for scheduled in [false, true] {
            for all in [false, true] {
                let mut loaded = Loaded::new();
                let ready = loaded.prepare(WindowRequest {
                    loop_region: Some((32.0 / 48_000.0, Some(64.0 / 48_000.0))),
                    key_lock: Some(false),
                    ..WindowRequest::default()
                });
                loaded.adopt(&ready);
                let original = loaded.sample();
                while loaded.callback.feedback_rx.pop().is_ok() {}
                let midi = loaded.midi_runtime();
                loaded.callback.transport.advance_by_rendered_frames(1);
                loaded.callback.quantization = TriggerQuantization::Grid { step_64ths: 16 };
                let mut global = None;
                match route {
                    0 => assert!(
                        launch_with_producer(&loaded.engine, &ready, false, 123, &loaded.producer)
                            .unwrap()
                    ),
                    1 => assert!(midi.trigger_pad(0, 123)),
                    _ => {
                        let binding = capture(&loaded.engine, 0).unwrap().unwrap();
                        global = Some(
                            global_playback_batch::enqueue(
                                &loaded.engine,
                                &loaded.producer,
                                vec![(&binding, 32.0 / 48_000.0, Some(64.0 / 48_000.0))],
                                true,
                                Some(123),
                            )
                            .unwrap(),
                        );
                    }
                }
                if scheduled {
                    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
                    assert_eq!(loaded.callback.scheduler.len(), 1);
                }
                // A rejected native STOP admission must not revoke earlier starts.
                while !loaded.producer.lock().unwrap().is_full() {
                    loaded
                        .producer
                        .lock()
                        .unwrap()
                        .push(ControlMessage::Ping())
                        .unwrap();
                }
                let revision = loaded.engine.input_runtime_ownership.launch_revision(0);
                assert_eq!(loaded.engine.admitted_launch_ids(), vec![0]);
                assert!(!enqueue_stop_with_producer(
                    &loaded.engine.input_runtime_ownership,
                    &mut loaded.producer.lock().unwrap(),
                    if all { None } else { Some(0) },
                ));
                assert_eq!(
                    loaded.engine.input_runtime_ownership.launch_revision(0),
                    revision
                );
                assert_eq!(loaded.engine.admitted_launch_ids(), vec![0]);
                // Discard only pressure Pings off the realtime path; retain the admitted start.
                let first = loaded.consumer.pop().unwrap();
                while loaded.consumer.pop().is_ok() {}
                if !scheduled {
                    loaded.producer.lock().unwrap().push(first).unwrap();
                } else {
                    assert!(matches!(first, ControlMessage::Ping()));
                }
                assert!(enqueue_stop_with_producer(
                    &loaded.engine.input_runtime_ownership,
                    &mut loaded.producer.lock().unwrap(),
                    if all { None } else { Some(0) },
                ));
                assert!(loaded.engine.admitted_launch_ids().is_empty());
                assert_eq!(
                    loaded.callback.drain(&mut loaded.consumer),
                    if scheduled { 1 } else { 2 }
                );
                while loaded.callback.feedback_rx.pop().is_ok() {}
                if scheduled {
                    let target = loaded.callback.scheduler.peek_next_target_frame().unwrap();
                    let event = loaded
                        .callback
                        .scheduler
                        .pop_due_at_callback_start(target)
                        .unwrap();
                    crate::audio_engine::audio_stream::execute_scheduled_command(
                        &mut loaded.callback.mixer,
                        &mut loaded.callback.transport,
                        event.execution_frame,
                        event.command,
                        &mut loaded.callback.feedback,
                        &mut loaded.callback.retirement,
                    );
                }
                if let Some(ticket) = global {
                    assert_eq!(ticket.publication_status(), "rejected");
                }
                assert!(
                    !loaded
                        .callback
                        .mixer
                        .voices
                        .iter()
                        .any(|voice| voice.active)
                );
                assert_eq!(ready.publication_status(), "accepted");
                assert!(ready.is_current());
                assert!(loaded.sample().same_window(&original));
                // A new gesture after STOP captures the new revision and remains admitted.
                loaded.callback.quantization = TriggerQuantization::Immediate;
                match route {
                    0 => assert!(
                        launch_with_producer(&loaded.engine, &ready, false, 124, &loaded.producer)
                            .unwrap()
                    ),
                    1 => assert!(midi.trigger_pad(0, 124)),
                    _ => {
                        let binding = capture(&loaded.engine, 0).unwrap().unwrap();
                        let ticket = global_playback_batch::enqueue(
                            &loaded.engine,
                            &loaded.producer,
                            vec![(&binding, 32.0 / 48_000.0, Some(64.0 / 48_000.0))],
                            true,
                            Some(124),
                        )
                        .unwrap();
                        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
                        assert_eq!(ticket.publication_status(), "accepted");
                    }
                }
                if route != 2 {
                    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
                }
                assert!(
                    loaded
                        .callback
                        .mixer
                        .voices
                        .iter()
                        .any(|voice| voice.active)
                );
            }
        }
    }
}

#[test]
fn actual_worker_claimed_ui_and_midi_start_tail_receives_ordered_stop_before_ui_feedback() {
    use crate::audio_engine::audio_stream::{AudioMessageSink, drain_control_messages};
    use crate::audio_engine::input_runtime_binding::enqueue_stop_with_producer;
    use crate::audio_engine::resident_relocation::launch_with_producer;
    // This sink is a test-only control interleave. Production callback sinks only
    // write the bounded feedback ring and never call a control API or take locks.
    struct StopAtStart<'a> {
        engine: &'a AudioEngine,
        producer: &'a Arc<Mutex<rtrb::Producer<ControlMessage>>>,
        queued: bool,
        messages: Vec<AudioMessage>,
    }
    impl AudioMessageSink for StopAtStart<'_> {
        fn push_audio_message(&mut self, message: AudioMessage) {
            if matches!(message, AudioMessage::SampleStarted { id: 0 }) && !self.queued {
                // The start passed its final native guard. The controller still
                // sees an inactive pad, but retains its admitted source-bound id.
                let targets = self.engine.admitted_launch_ids();
                assert_eq!(targets, vec![0]);
                self.engine.cancel_pad_launches(0).unwrap();
                assert_eq!(self.engine.admitted_launch_ids(), targets);
                assert!(enqueue_stop_with_producer(
                    &self.engine.input_runtime_ownership,
                    &mut self.producer.lock().unwrap(),
                    Some(targets[0]),
                ));
                assert!(self.engine.admitted_launch_ids().is_empty());
                self.queued = true;
            }
            self.messages.push(message);
        }
        fn available_audio_message_slots(&mut self) -> usize {
            usize::MAX
        }
    }
    for native_midi in [false, true] {
        let mut loaded = Loaded::new();
        let ready = loaded.prepare(WindowRequest {
            loop_region: Some((32.0 / 48_000.0, Some(64.0 / 48_000.0))),
            ..WindowRequest::default()
        });
        loaded.adopt(&ready);
        while loaded.callback.feedback_rx.pop().is_ok() {}
        let midi = loaded.midi_runtime();
        if native_midi {
            assert!(midi.trigger_pad(0, 123));
        } else {
            assert!(
                launch_with_producer(&loaded.engine, &ready, false, 123, &loaded.producer).unwrap()
            );
        }
        let mut feedback = StopAtStart {
            engine: &loaded.engine,
            producer: &loaded.producer,
            queued: false,
            messages: Vec::new(),
        };
        assert_eq!(
            drain_control_messages(
                &mut loaded.consumer,
                &mut loaded.callback.scheduler,
                0,
                &mut loaded.callback.quantization,
                &mut loaded.callback.transport,
                &mut loaded.callback.mixer,
                &mut feedback,
                &mut loaded.callback.retirement,
            ),
            2
        );
        assert!(feedback.queued);
        assert!(matches!(
            feedback.messages.as_slice(),
            [
                AudioMessage::SampleStarted { id: 0 },
                AudioMessage::SampleStopped { id: 0 },
            ]
        ));
        assert!(
            !loaded
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active)
        );
        assert_eq!(ready.publication_status(), "accepted");
        assert!(ready.is_current());
        drop(feedback);
        if native_midi {
            assert!(midi.trigger_pad(0, 124));
        } else {
            assert!(
                launch_with_producer(&loaded.engine, &ready, false, 124, &loaded.producer).unwrap()
            );
        }
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
        assert!(
            loaded
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active)
        );
    }
}

#[test]
fn actual_worker_native_midi_stop_dispatch_revokes_scheduled_launch_without_changing_ack() {
    let mut loaded = Loaded::new();
    let ready = loaded.prepare(WindowRequest {
        loop_region: Some((32.0 / 48_000.0, Some(64.0 / 48_000.0))),
        ..WindowRequest::default()
    });
    loaded.adopt(&ready);
    while loaded.callback.feedback_rx.pop().is_ok() {}
    let midi = loaded.midi_runtime();
    loaded.callback.transport.advance_by_rendered_frames(1);
    loaded.callback.quantization = TriggerQuantization::Grid { step_64ths: 16 };
    assert!(midi.inject_midi_message(&[0x90, 60, 100]));
    let mut event = None;
    wait_until(Duration::from_secs(3), || {
        event = midi.poll_event();
        event.is_some()
    });
    let event = event.unwrap();
    assert!(event.dispatched && event.direct);
    assert_eq!(loaded.engine.admitted_launch_ids(), vec![0]);
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    assert_eq!(loaded.callback.scheduler.len(), 1);
    assert!(midi.inject_midi_message(&[0x90, 61, 100]));
    let mut stopped = None;
    wait_until(Duration::from_secs(3), || {
        stopped = midi.poll_event();
        stopped.is_some()
    });
    let stopped = stopped.unwrap();
    assert!(stopped.dispatched && stopped.direct);
    assert!(loaded.engine.admitted_launch_ids().is_empty());
    assert!(matches!(
        loaded.consumer.peek(),
        Ok(ControlMessage::StopSample { id: 0 })
    ));
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    while loaded.callback.feedback_rx.pop().is_ok() {}
    let target = loaded.callback.scheduler.peek_next_target_frame().unwrap();
    let pending = loaded
        .callback
        .scheduler
        .pop_due_at_callback_start(target)
        .unwrap();
    crate::audio_engine::audio_stream::execute_scheduled_command(
        &mut loaded.callback.mixer,
        &mut loaded.callback.transport,
        pending.execution_frame,
        pending.command,
        &mut loaded.callback.feedback,
        &mut loaded.callback.retirement,
    );
    assert!(
        !loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active)
    );
    assert_eq!(ready.publication_status(), "accepted");
    assert!(ready.is_current());
    loaded.callback.quantization = TriggerQuantization::Immediate;
    assert!(midi.trigger_pad(0, event.received_at_ns));
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    assert!(
        loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active)
    );
}

#[test]
fn actual_worker_exact_ticket_launch_cancellation_preserves_ack_and_new_owner() {
    use crate::audio_engine::resident_relocation::launch_with_producer;
    let mut loaded = Loaded::new();
    let ready = loaded.prepare(WindowRequest {
        loop_region: Some((32.0 / 48_000.0, Some(64.0 / 48_000.0))),
        ..WindowRequest::default()
    });
    loaded.adopt(&ready);
    while loaded.callback.feedback_rx.pop().is_ok() {}
    assert!(launch_with_producer(&loaded.engine, &ready, false, 123, &loaded.producer).unwrap());
    assert!(ready.cancel_launch());
    assert!(!ready.cancel_launch());
    assert_eq!(loaded.engine.admitted_launch_ids(), vec![0]);
    assert_eq!(ready.publication_status(), "accepted");
    assert!(ready.is_current());
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    assert!(
        !loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active)
    );
    let newer = loaded.prepare(WindowRequest {
        loop_region: Some((32.0 / 48_000.0, Some(64.0 / 48_000.0))),
        ..WindowRequest::default()
    });
    loaded.adopt(&newer);
    assert!(launch_with_producer(&loaded.engine, &newer, false, 124, &loaded.producer).unwrap());
    assert!(!ready.cancel_launch());
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    assert!(newer.is_current());
    assert!(
        loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active)
    );
}

#[test]
fn actual_worker_capture_pressure_cancel_shutdown_and_stopped_seek_preserve_effective_pcm() {
    for shutdown in [false, true] {
        let mut loaded = Loaded::new();
        assert!(
            loaded
                .callback
                .mixer
                .play_sample_rt(0, 1.0, &mut loaded.callback.retirement)
        );
        let bank = loaded.replace_bank();
        let seek = loaded.prepare(WindowRequest {
            seek_position_s: Some(12.0 / 48_000.0),
            ..WindowRequest::default()
        });
        assert!(matches!(
            loaded.consumer.peek(),
            Ok(ControlMessage::CaptureResidentSeek(_))
        ));
        loaded.callback.retirement.capacity = 2;
        assert_eq!(
            loaded.callback.drain(&mut loaded.consumer),
            0,
            "capture must reserve optional old stem retirement too"
        );
        assert_eq!(seek.publication_status(), "preparing");
        loaded
            .callback
            .render_continuation(&loaded.mono[32..64], 0, 77);
        if shutdown {
            crate::audio_engine::resident_relocation::cancel_all(&loaded.engine).unwrap();
            loaded.engine.cold_cancelled.store(true, Ordering::Release);
            let start = Instant::now();
            loaded.engine.cold_jobs.shutdown();
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "shutdown waited for the undrained capture deadline"
            );
        } else {
            assert!(seek.cancel());
        }
        assert_eq!(seek.publication_status(), "cancelled");
        assert!(!seek.is_current());
        loaded.callback.retirement.capacity = usize::MAX;
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
        reconcile(&loaded.engine).unwrap();
        assert!(loaded.sample().same_window(&bank));
        loaded
            .callback
            .render_continuation(&loaded.mono[32..64], 77, 99);
        assert!(
            !loaded
                .engine
                .input_runtime_ownership
                .resident_control_pending(0)
        );
    }
    let mut stopped = Loaded::new();
    let old = stopped.sample();
    let seek = stopped.prepare(WindowRequest {
        seek_position_s: Some(999.0),
        ..WindowRequest::default()
    });
    assert_eq!(stopped.callback.drain(&mut stopped.consumer), 1);
    assert_eq!(seek.publication_status(), "accepted");
    assert_eq!(seek.effective_seek_seconds(), None);
    reconcile(&stopped.engine).unwrap();
    assert!(seek.is_current());
    assert!(
        stopped.sample().same_window(&old),
        "stopped seek prepared or adopted an unnecessary full window"
    );
}

#[test]
fn actual_worker_resident_seek_acks_without_copying_current_or_previous_finite_pin() {
    for replace in [false, true] {
        let mut loaded = Loaded::new();
        assert!(
            loaded
                .callback
                .mixer
                .play_sample_rt(0, 1.0, &mut loaded.callback.retirement)
        );
        let old = loaded.sample();
        let bank = if replace {
            loaded.replace_bank()
        } else {
            old.clone()
        };
        let seek = loaded.prepare(WindowRequest {
            seek_position_s: Some(48.0 / 48_000.0),
            ..WindowRequest::default()
        });
        loaded.callback.retirement.capacity = 0;
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 0);
        assert_eq!(seek.publication_status(), "preparing");
        loaded.callback.retirement.capacity = usize::MAX;
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
        assert_eq!(seek.publication_status(), "accepted");
        assert_eq!(seek.effective_seek_seconds(), Some(48.0 / 48_000.0));
        reconcile(&loaded.engine).unwrap();
        assert!(seek.is_current());
        assert!(loaded.sample().same_window(&bank));
        let voice = loaded
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active)
            .unwrap();
        assert!(Arc::ptr_eq(
            &voice.sample.as_ref().unwrap().samples,
            &old.samples
        ));
        assert_eq!(voice.sample.as_ref().unwrap().resident_end(), 64);
        loaded
            .callback
            .render_continuation(&loaded.mono[32..64], 16, 113);
    }
}

#[test]
fn actual_worker_retrigger_prepares_new_bank_while_old_source_keeps_playing_until_explicit_start() {
    let mut loaded = Loaded::new();
    let old = loaded.sample();
    let old_backing = Arc::downgrade(&old.samples);
    assert!(
        loaded
            .callback
            .mixer
            .play_sample_rt(0, 1.0, &mut loaded.callback.retirement)
    );
    let bank = loaded.replace_bank();
    let ready = loaded.prepare(WindowRequest {
        loop_region: Some((400.0 / 48_000.0, Some(500.0 / 48_000.0))),
        key_lock: Some(false),
        ..WindowRequest::default()
    });
    loaded.pending(&ready);
    loaded.callback.retirement.capacity = 0;
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 0);
    loaded
        .callback
        .render_continuation(&loaded.mono[32..64], 0, 77);
    loaded.callback.retirement.capacity = usize::MAX;
    loaded.adopt(&ready);
    let next = loaded.sample();
    assert!(next.same_source(&bank));
    assert_eq!((next.resident_start(), next.resident_end()), (400, 500));
    let voice = loaded
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap();
    assert!(voice.sample.as_ref().unwrap().same_window(&old));
    assert_eq!(
        voice.source_loop_region.unwrap(),
        crate::audio_engine::source_reader::FrameRange { start: 32, end: 64 }
    );
    loaded
        .callback
        .render_continuation(&loaded.mono[32..64], 77, 101);
    assert!(
        crate::audio_engine::resident_relocation::launch_with_producer(
            &loaded.engine,
            &ready,
            false,
            123,
            &loaded.producer,
        )
        .unwrap()
    );
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    let voice = loaded
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap();
    assert!(voice.sample.as_ref().unwrap().same_window(&next));
    assert!(voice.frozen_stems.is_none());
    loaded.callback.render_continuation(&[0.25; 100], 0, 117);
    drop(old);
    wait_until(Duration::from_secs(10), || old_backing.upgrade().is_none());
}

#[test]
fn actual_worker_guarded_ui_launch_keeps_timestamp_survives_runtime_refresh_and_rejects_scheduled_stale_owners()
 {
    use crate::audio_engine::resident_relocation::launch_with_producer;
    pyo3::Python::initialize();
    for change in 0..5 {
        let mut loaded = Loaded::new();
        let ready = loaded.prepare(WindowRequest {
            loop_region: Some((32.0 / 48_000.0, Some(64.0 / 48_000.0))),
            key_lock: Some(false),
            ..WindowRequest::default()
        });
        loaded.adopt(&ready);
        while loaded.callback.feedback_rx.pop().is_ok() {}
        if change == 0 {
            while !loaded.producer.lock().unwrap().is_full() {
                loaded
                    .producer
                    .lock()
                    .unwrap()
                    .push(ControlMessage::Ping())
                    .unwrap();
            }
            assert!(
                launch_with_producer(&loaded.engine, &ready, false, 123, &loaded.producer)
                    .unwrap_err()
                    .to_string()
                    .contains("resident command queue is full")
            );
            assert!(ready.is_current());
            assert_eq!(loaded.callback.drain(&mut loaded.consumer), 8);
            while loaded.callback.feedback_rx.pop().is_ok() {}
        }
        assert!(
            loaded
                .callback
                .mixer
                .play_sample_rt(0, 1.0, &mut loaded.callback.retirement)
        );
        let generation = loaded
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active)
            .unwrap()
            .generation;
        loaded.callback.transport.advance_by_rendered_frames(1);
        loaded.callback.quantization = TriggerQuantization::Grid { step_64ths: 16 };
        assert!(
            launch_with_producer(&loaded.engine, &ready, false, 123, &loaded.producer).unwrap()
        );
        assert!(matches!(
            loaded.consumer.peek(),
            Ok(ControlMessage::TriggerInputPad {
                received_at_ns: 123,
                resident_control: Some(_),
                ..
            })
        ));
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
        assert_eq!(loaded.callback.scheduler.len(), 1);
        match change {
            0 => {
                loaded.engine.input_runtime_ownership.runtime[0].fetch_add(1, Ordering::AcqRel);
            }
            1 => {
                let (request, mut load) = selected_load(
                    &loaded.engine,
                    &loaded.directory.path().join("control.wav"),
                    loaded.directory.path(),
                    48_000,
                    ResidentLoadHint {
                        start_s: 32.0 / 48_000.0,
                        end_s: 64.0 / 48_000.0,
                        key_lock: false,
                    },
                );
                wait_until(Duration::from_secs(10), || !load.is_empty());
                assert_eq!(loaded.callback.drain(&mut load), 1);
                assert!(matches!(
                    terminal(&loaded.engine, request),
                    LoaderEvent::Success { .. }
                ));
                while loaded.callback.feedback_rx.pop().is_ok() {}
            }
            2 => {
                let next = loaded.prepare(WindowRequest {
                    loop_region: Some((80.0 / 48_000.0, Some(111.0 / 48_000.0))),
                    ..WindowRequest::default()
                });
                loaded.adopt(&next);
            }
            3 => {
                let cancelled = loaded.prepare(WindowRequest {
                    loop_region: Some((80.0 / 48_000.0, Some(111.0 / 48_000.0))),
                    ..WindowRequest::default()
                });
                assert!(cancelled.cancel());
            }
            _ => {
                loaded.engine.input_runtime_ownership.authority[0].fetch_add(1, Ordering::AcqRel);
            }
        }
        let target = loaded.callback.scheduler.peek_next_target_frame().unwrap();
        let event = loaded
            .callback
            .scheduler
            .pop_due_at_callback_start(target)
            .unwrap();
        assert!(matches!(
            &event.command,
            crate::audio_engine::scheduler::ScheduledCommand::TriggerInputPad {
                received_at_ns: 123,
                resident_control: Some(_),
                ..
            }
        ));
        crate::audio_engine::audio_stream::execute_scheduled_command(
            &mut loaded.callback.mixer,
            &mut loaded.callback.transport,
            event.execution_frame,
            event.command,
            &mut loaded.callback.feedback,
            &mut loaded.callback.retirement,
        );
        let voice = loaded
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active);
        if change == 0 {
            assert!(ready.is_current());
            assert!(
                voice.unwrap().generation > generation,
                "UI launch was tied to unrelated MIDI runtime intent"
            );
        } else {
            assert!(!ready.is_current());
            assert!(
                voice.is_none_or(|voice| voice.generation == generation),
                "stale scheduled UI launch restarted another effective owner"
            );
            assert!(
                !launch_with_producer(&loaded.engine, &ready, false, 123, &loaded.producer)
                    .unwrap()
            );
        }
    }
}

#[test]
fn actual_worker_captured_old_pin_rejects_stop_seek_aba_and_new_source_before_ack() {
    for change in 0..3 {
        let mut loaded = Loaded::new();
        assert!(
            loaded
                .callback
                .mixer
                .play_sample_rt(0, 1.0, &mut loaded.callback.retirement)
        );
        let bank = loaded.replace_bank();
        let seek = loaded.prepare(WindowRequest {
            seek_position_s: Some(12.0 / 48_000.0),
            ..WindowRequest::default()
        });
        loaded.pending(&seek);
        assert_eq!(seek.publication_status(), "pending");
        let mut replacement_load = None;
        match change {
            0 => loaded
                .callback
                .mixer
                .stop_sample_rt(0, &mut loaded.callback.retirement),
            1 => {
                assert!(loaded.callback.mixer.seek_sample(0, 48.0 / 48_000.0));
            }
            _ => {
                let (request, load) = selected_load(
                    &loaded.engine,
                    &loaded.directory.path().join("control.wav"),
                    loaded.directory.path(),
                    48_000,
                    ResidentLoadHint {
                        start_s: 32.0 / 48_000.0,
                        end_s: 64.0 / 48_000.0,
                        key_lock: false,
                    },
                );
                replacement_load = Some((request, load));
            }
        }
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
        assert_ne!(seek.publication_status(), "accepted");
        assert!(!seek.is_current());
        reconcile(&loaded.engine).unwrap();
        assert!(loaded.sample().same_window(&bank));
        if change == 1 {
            loaded
                .callback
                .render_continuation(&loaded.mono[32..64], 16, 97);
        }
        if let Some((request, mut load)) = replacement_load {
            wait_until(Duration::from_secs(10), || !load.is_empty());
            while loaded.callback.feedback_rx.pop().is_ok() {}
            assert_eq!(loaded.callback.drain(&mut load), 1);
            assert!(matches!(
                terminal(&loaded.engine, request),
                LoaderEvent::Success { .. }
            ));
            assert!(!seek.is_current());
        }
    }
}

#[test]
fn actual_worker_old_finite_pin_seek_reads_original_lease_and_keeps_replacement_bank() {
    let mut loaded = Loaded::new();
    let old = loaded.sample();
    let old_backing = Arc::downgrade(&old.samples);
    assert!(
        loaded
            .callback
            .mixer
            .play_sample_rt(0, 1.0, &mut loaded.callback.retirement)
    );
    loaded
        .callback
        .render_continuation(&loaded.mono[32..64], 0, 7);
    let replacement = loaded.directory.path().join("replacement.wav");
    write_pcm16(&replacement, 48_000, 1, &vec![8192_i16; 1000]);
    let (producer, mut load) = rtrb::RingBuffer::new(4);
    let request = admit_for_format_selected(
        &loaded.engine,
        0,
        replacement.to_string_lossy().into_owned(),
        (
            false,
            false,
            false,
            Some(ResidentLoadHint {
                start_s: 100.0 / 48_000.0,
                end_s: 300.0 / 48_000.0,
                key_lock: false,
            }),
        ),
        Arc::new(Mutex::new(producer)),
        (2, 48_000, loaded.directory.path().to_owned()),
    )
    .unwrap();
    wait_until(Duration::from_secs(10), || !load.is_empty());
    assert_eq!(loaded.callback.drain(&mut load), 1);
    assert!(matches!(
        terminal(&loaded.engine, request),
        LoaderEvent::Success { .. }
    ));
    wait_until(Duration::from_secs(10), || {
        loaded.engine.cold_loading[0].load(Ordering::Acquire) == 0
    });
    let bank = loaded.sample();
    assert_eq!(bank.frame_count(), 1000);
    assert!(
        loaded
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active)
            .unwrap()
            .sample
            .as_ref()
            .unwrap()
            .same_window(&old)
    );
    drop(old);
    let seek = loaded.prepare(WindowRequest {
        seek_position_s: Some(999.0),
        ..WindowRequest::default()
    });
    loaded.adopt(&seek);
    assert_eq!(seek.effective_seek_seconds(), Some(256.0 / 48_000.0));
    assert!(loaded.sample().same_window(&bank));
    let voice = loaded
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap();
    let original = voice.sample.as_ref().unwrap();
    assert_eq!(
        (
            original.frame_count(),
            original.resident_start(),
            original.resident_end()
        ),
        (256, 0, 256)
    );
    assert!(!original.same_source(&bank));
    loaded
        .callback
        .render_continuation(&loaded.mono[32..64], 0, 101);
    wait_until(Duration::from_secs(10), || old_backing.upgrade().is_none());
}

#[test]
fn actual_worker_loop_and_all_keep_old_effective_until_ack_and_use_full_wav_oracle() {
    let mut loaded = Loaded::new();
    assert!(
        loaded
            .callback
            .mixer
            .play_sample_rt(0, 1.0, &mut loaded.callback.retirement)
    );
    loaded
        .callback
        .render_continuation(&loaded.mono[32..64], 0, 7);
    let old = loaded.sample();
    let next = loaded.prepare(WindowRequest {
        loop_region: Some((80.0 / 48_000.0, Some(111.0 / 48_000.0))),
        key_lock: Some(false),
        ..WindowRequest::default()
    });
    loaded.pending(&next);
    assert!(
        loaded
            .engine
            .input_runtime_ownership
            .resident_control_pending(0)
    );
    loaded.callback.retirement.capacity = 0;
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 0);
    assert!(loaded.sample().same_window(&old));
    loaded
        .callback
        .render_continuation(&loaded.mono[32..64], 7, 11);
    loaded.callback.retirement.capacity = usize::MAX;
    loaded.adopt(&next);
    assert_eq!(
        (
            loaded.sample().resident_start(),
            loaded.sample().resident_end()
        ),
        (80, 111)
    );
    assert_eq!(loaded.callback.mixer.loop_region_frames(0), (80, Some(111)));
    loaded
        .callback
        .render_continuation(&loaded.mono[80..111], 0, 99);
    let all = loaded.prepare(WindowRequest {
        loop_region: Some((0.0, None)),
        key_lock: Some(false),
        ..WindowRequest::default()
    });
    loaded.adopt(&all);
    assert!(
        !next.is_current(),
        "newer exact intent left old ACK promotable"
    );
    assert_eq!(
        (
            loaded.sample().resident_start(),
            loaded.sample().resident_end()
        ),
        (0, 256)
    );
    // Existing in-range phase is retained by ALL; a fresh launch proves source zero.
    loaded.callback.render_oracle(&loaded.mono, 0, 48_000);
    assert!(
        !loaded
            .engine
            .input_runtime_ownership
            .resident_control_pending(0)
    );
}

#[test]
fn actual_worker_nonresident_intro_tail_and_paused_seek_preserve_source_end_clamp() {
    let mut loaded = Loaded::new();
    assert!(
        loaded
            .callback
            .mixer
            .play_sample_rt(0, 1.0, &mut loaded.callback.retirement)
    );
    let intro = loaded.prepare(WindowRequest {
        seek_position_s: Some(12.0 / 48_000.0),
        ..WindowRequest::default()
    });
    loaded.adopt(&intro);
    assert_eq!(intro.effective_seek_seconds(), Some(12.0 / 48_000.0));
    let mut output = vec![0.0; 79 * 2];
    loaded.callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        0,
        &mut loaded.callback.retirement,
    );
    for (index, stereo) in output.chunks_exact(2).enumerate() {
        let frame = if index < 20 {
            12 + index
        } else {
            32 + (index - 20) % 32
        };
        assert_eq!(stereo, [loaded.mono[frame]; 2]);
    }
    loaded.callback.mixer.pause_sample_at_output_frame(0, 79);
    let tail = loaded.prepare(WindowRequest {
        seek_position_s: Some(251.0 / 48_000.0),
        ..WindowRequest::default()
    });
    loaded.adopt(&tail);
    assert_eq!(tail.effective_seek_seconds(), Some(251.0 / 48_000.0));
    assert!(
        loaded
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active)
            .unwrap()
            .paused
    );
    output.fill(0.0);
    loaded.callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        79,
        &mut loaded.callback.retirement,
    );
    assert!(output.iter().all(|sample| *sample == 0.0));
    loaded.callback.mixer.resume_sample_at_output_frame(0, 158);
    loaded.callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        158,
        &mut loaded.callback.retirement,
    );
    for (index, stereo) in output.chunks_exact(2).enumerate() {
        let frame = if index < 5 {
            251 + index
        } else {
            32 + (index - 5) % 32
        };
        assert_eq!(stereo, [loaded.mono[frame]; 2]);
    }
    let end = loaded.prepare(WindowRequest {
        seek_position_s: Some(999.0),
        ..WindowRequest::default()
    });
    loaded.adopt(&end);
    // The existing native source-end clamp is the exclusive full frame count;
    // the subsequent reader wraps to the loop without reading that endpoint.
    assert_eq!(end.effective_seek_seconds(), Some(256.0 / 48_000.0));
}

#[test]
fn actual_worker_supersession_cancel_admission_pressure_and_aba_never_promote_old_ack() {
    let mut loaded = Loaded::new();
    let old = loaded.sample();
    let first = loaded.prepare(WindowRequest {
        loop_region: Some((80.0 / 48_000.0, Some(111.0 / 48_000.0))),
        ..WindowRequest::default()
    });
    loaded.pending(&first);
    let second = loaded.prepare(WindowRequest {
        loop_region: Some((120.0 / 48_000.0, Some(151.0 / 48_000.0))),
        ..WindowRequest::default()
    });
    assert_eq!(first.publication_status(), "cancelled");
    assert!(!first.is_current());
    loaded.pending(&second);
    wait_until(Duration::from_secs(10), || loaded.consumer.slots() >= 2);
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 2);
    assert_eq!(second.publication_status(), "accepted");
    reconcile(&loaded.engine).unwrap();
    assert_eq!(
        (
            loaded.sample().resident_start(),
            loaded.sample().resident_end()
        ),
        (120, 151)
    );
    assert!(!loaded.sample().same_window(&old));
    let cancelled = loaded.prepare(WindowRequest {
        loop_region: Some((0.0, None)),
        ..WindowRequest::default()
    });
    loaded.pending(&cancelled);
    assert!(cancelled.cancel());
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    reconcile(&loaded.engine).unwrap();
    assert_eq!(loaded.sample().resident_start(), 120);
    assert!(
        !second.is_current(),
        "new cancelled intent must not revive old ACK"
    );
    // Full native queue is rejected before cancelling the latest prepared intent.
    let pending = loaded.prepare(WindowRequest {
        loop_region: Some((80.0 / 48_000.0, Some(111.0 / 48_000.0))),
        ..WindowRequest::default()
    });
    loaded.pending(&pending);
    while !loaded.producer.lock().unwrap().is_full() {
        loaded
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::Ping())
            .unwrap();
    }
    assert!(
        prepare_window_with_producer(
            &loaded.engine,
            0,
            WindowRequest {
                loop_region: Some((0.0, None)),
                ..WindowRequest::default()
            },
            loaded.producer.clone()
        )
        .is_err()
    );
    assert!(pending.is_current());
    loaded.callback.drain(&mut loaded.consumer);
    reconcile(&loaded.engine).unwrap();
    assert_eq!(pending.publication_status(), "accepted");
    assert!(pending.is_current());
    // Ping pressure used the real one-slot feedback ring. Drain its Pong before
    // asking the new cold assignment to reserve that same genuine ACK slot.
    while loaded.callback.feedback_rx.pop().is_ok() {}
    // Source unload/reassignment ABA is fenced by native generation, even for same original bytes.
    let (request, mut load) = selected_load(
        &loaded.engine,
        &loaded.directory.path().join("control.wav"),
        loaded.directory.path(),
        48_000,
        ResidentLoadHint {
            start_s: 32.0 / 48_000.0,
            end_s: 64.0 / 48_000.0,
            key_lock: false,
        },
    );
    assert!(!pending.is_current());
    wait_until(Duration::from_secs(10), || load.peek().is_ok());
    assert_eq!(loaded.callback.drain(&mut load), 1);
    assert!(matches!(
        terminal(&loaded.engine, request),
        LoaderEvent::Success { .. }
    ));
    assert!(!pending.is_current());
    assert!(loaded.directory.path().exists());
}
#[test]
fn actual_worker_cancel_capture_covers_admission_after_empty_scan_and_claimed_tail() {
    use crate::audio_engine::audio_stream::{AudioMessageSink, drain_control_messages};
    use crate::audio_engine::input_runtime_binding::{
        cancel_launches_with_producer, enqueue_stop_with_producer,
    };
    use crate::audio_engine::resident_relocation::launch_with_producer;

    // Only the test sink interleaves control. Production feedback sinks remain
    // bounded ring writes and never take a producer/control mutex on RT.
    struct StopAfterClaim<'a> {
        engine: &'a AudioEngine,
        producer: &'a Arc<Mutex<rtrb::Producer<ControlMessage>>>,
        all: bool,
        captured: Option<Vec<usize>>,
        messages: Vec<AudioMessage>,
    }
    impl AudioMessageSink for StopAfterClaim<'_> {
        fn push_audio_message(&mut self, message: AudioMessage) {
            if matches!(message, AudioMessage::SampleStarted { id: 0 }) && self.captured.is_none() {
                // Native has passed the final guard; Python has received no event.
                // The former separate empty scan cannot supply this STOP target.
                let targets = cancel_launches_with_producer(
                    &self.engine.input_runtime_ownership,
                    Some(self.producer),
                    if self.all { None } else { Some(0) },
                )
                .unwrap();
                assert_eq!(targets, vec![0]);
                assert_eq!(self.engine.admitted_launch_ids(), targets);
                assert!(enqueue_stop_with_producer(
                    &self.engine.input_runtime_ownership,
                    &mut self.producer.lock().unwrap(),
                    if self.all { None } else { Some(targets[0]) },
                ));
                self.captured = Some(targets);
            }
            self.messages.push(message);
        }

        fn available_audio_message_slots(&mut self) -> usize {
            usize::MAX
        }
    }

    for all in [false, true] {
        for native_midi in [false, true] {
            let mut loaded = Loaded::new();
            let ready = loaded.prepare(WindowRequest {
                loop_region: Some((32.0 / 48_000.0, Some(64.0 / 48_000.0))),
                ..WindowRequest::default()
            });
            loaded.adopt(&ready);
            while loaded.callback.feedback_rx.pop().is_ok() {}
            let midi = loaded.midi_runtime();
            let old_python_scan = loaded.engine.admitted_launch_ids();
            assert!(old_python_scan.is_empty());
            // Admission occurs in the precise gap after the old Python scan.
            if native_midi {
                assert!(midi.inject_midi_message(&[0x90, 60, 100]));
                let mut event = None;
                wait_until(Duration::from_secs(3), || {
                    event = midi.poll_event();
                    event.is_some()
                });
                let event = event.unwrap();
                assert!(event.dispatched && event.direct);
            } else {
                assert!(
                    launch_with_producer(&loaded.engine, &ready, false, 123, &loaded.producer)
                        .unwrap()
                );
            }
            let mut feedback = StopAfterClaim {
                engine: &loaded.engine,
                producer: &loaded.producer,
                all,
                captured: None,
                messages: Vec::new(),
            };
            assert_eq!(
                drain_control_messages(
                    &mut loaded.consumer,
                    &mut loaded.callback.scheduler,
                    0,
                    &mut loaded.callback.quantization,
                    &mut loaded.callback.transport,
                    &mut loaded.callback.mixer,
                    &mut feedback,
                    &mut loaded.callback.retirement,
                ),
                2
            );
            assert_eq!(feedback.captured, Some(vec![0]));
            assert!(matches!(
                feedback.messages.as_slice(),
                [
                    AudioMessage::SampleStarted { id: 0 },
                    AudioMessage::SampleStopped { id: 0 }
                ]
            ));
            drop(feedback);
            assert!(loaded.engine.admitted_launch_ids().is_empty());
            assert!(
                !loaded
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.active)
            );
            assert_eq!(ready.publication_status(), "accepted");
            assert!(ready.is_current());
            assert!(midi.trigger_pad(0, 124));
            assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
            assert!(
                loaded
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.active)
            );
        }
    }
}

#[test]
fn actual_worker_cancel_capture_obeys_shared_producer_mutex() {
    use crate::audio_engine::input_runtime_binding::cancel_launches_with_producer;
    let mut loaded = Loaded::new();
    let ready = loaded.prepare(WindowRequest {
        loop_region: Some((32.0 / 48_000.0, Some(64.0 / 48_000.0))),
        ..WindowRequest::default()
    });
    loaded.adopt(&ready);
    let midi = loaded.midi_runtime();
    assert!(midi.trigger_pad(0, 123));
    let revision = loaded.engine.input_runtime_ownership.launch_revision(0);
    let producer = loaded.producer.lock().unwrap();
    let ownership = loaded.engine.input_runtime_ownership.clone();
    let same_producer = loaded.producer.clone();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let cancellation = std::thread::spawn(move || {
        entered_tx.send(()).unwrap();
        let targets =
            cancel_launches_with_producer(&ownership, Some(&same_producer), None).unwrap();
        finished_tx.send(targets).unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        finished_rx.recv_timeout(Duration::from_millis(25)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(
        loaded.engine.input_runtime_ownership.launch_revision(0),
        revision
    );
    drop(producer);
    assert_eq!(
        finished_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        vec![0]
    );
    cancellation.join().unwrap();
    assert_ne!(
        loaded.engine.input_runtime_ownership.launch_revision(0),
        revision
    );
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    assert!(
        !loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active)
    );
    assert_eq!(loaded.engine.admitted_launch_ids(), vec![0]);
    assert!(ready.is_current());
}

#[test]
fn actual_worker_full_queue_cancel_capture_retains_ordered_stop_target() {
    use crate::audio_engine::input_runtime_binding::{
        cancel_launches_with_producer, enqueue_stop_with_producer,
    };
    for all in [false, true] {
        let mut loaded = Loaded::new();
        let ready = loaded.prepare(WindowRequest {
            loop_region: Some((32.0 / 48_000.0, Some(64.0 / 48_000.0))),
            ..WindowRequest::default()
        });
        loaded.adopt(&ready);
        let midi = loaded.midi_runtime();
        assert!(midi.trigger_pad(0, 123));
        while !loaded.producer.lock().unwrap().is_full() {
            loaded
                .producer
                .lock()
                .unwrap()
                .push(ControlMessage::Ping())
                .unwrap();
        }
        let targets = cancel_launches_with_producer(
            &loaded.engine.input_runtime_ownership,
            Some(&loaded.producer),
            if all { None } else { Some(0) },
        )
        .unwrap();
        assert_eq!(targets, vec![0]);
        let cancelled_revision = loaded.engine.input_runtime_ownership.launch_revision(0);
        assert!(!enqueue_stop_with_producer(
            &loaded.engine.input_runtime_ownership,
            &mut loaded.producer.lock().unwrap(),
            if all { None } else { Some(targets[0]) },
        ));
        assert_eq!(loaded.engine.admitted_launch_ids(), targets);
        assert_eq!(
            loaded.engine.input_runtime_ownership.launch_revision(0),
            cancelled_revision
        );
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 8);
        assert!(
            !loaded
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active)
        );
        assert_eq!(loaded.engine.admitted_launch_ids(), targets);
        assert!(enqueue_stop_with_producer(
            &loaded.engine.input_runtime_ownership,
            &mut loaded.producer.lock().unwrap(),
            if all { None } else { Some(targets[0]) },
        ));
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
        assert!(loaded.engine.admitted_launch_ids().is_empty());
        assert!(ready.is_current());
        assert_eq!(ready.publication_status(), "accepted");
        assert!(midi.trigger_pad(0, 124));
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
        assert!(
            loaded
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active)
        );
    }
}

#[test]
fn actual_worker_disabled_dispatcher_rejects_captured_mapping_after_shutdown_fence() {
    use crate::audio_engine::input_runtime_binding::cancel_launches_before_shutdown;
    let mut loaded = Loaded::new();
    let ready = loaded.prepare(WindowRequest {
        loop_region: Some((32.0 / 48_000.0, Some(64.0 / 48_000.0))),
        ..WindowRequest::default()
    });
    loaded.adopt(&ready);
    let midi = loaded.midi_runtime();
    let late_dispatch = midi
        .capture_mapping_dispatch_for_test("midi:note:1:60", 123)
        .unwrap();
    midi.set_enabled(false);
    cancel_launches_before_shutdown(
        &loaded.engine.input_runtime_ownership,
        Some(&loaded.producer),
    );
    assert!(!late_dispatch());
    assert!(loaded.consumer.is_empty());
    assert!(loaded.engine.admitted_launch_ids().is_empty());
    assert!(ready.is_current());
    assert_eq!(ready.publication_status(), "accepted");
    // Explicit public triggers intentionally retain their existing semantics.
    assert!(midi.trigger_pad(0, 124));
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    assert!(
        loaded
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active)
    );
    midi.set_enabled(true);
    assert!(midi
        .capture_mapping_dispatch_for_test("midi:note:1:60", 125)
        .unwrap()());
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    assert!(ready.is_current());
}
