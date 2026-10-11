//! Genuine WindowWork/own ACK, paused storage and future bank custody through native retirement.
use super::*;
use crate::audio_engine::input_runtime_binding::capture;
use crate::audio_engine::mixer::prepared_native_mixer_tests::finite_lifecycle_tests::{
    AlgebraicClock, RawChronology, lifecycle_filter_after_cut,
};
use crate::audio_engine::resident_relocation::launch_with_producer;

fn voice(mixer: &RtMixer) -> &crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
}

fn voice_mut(mixer: &mut RtMixer) -> &mut crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter_mut()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
}

fn render(
    loaded: &mut Loaded,
    reference: &mut RtMixer,
    frame: &mut u64,
    frames: usize,
) -> Vec<f32> {
    let mut actual = vec![0.0; frames * 2];
    loaded.callback.mixer.render_rt_at_output_frame(
        &mut actual,
        &mut [0.0; NUM_SAMPLES],
        *frame,
        &mut loaded.callback.retirement,
    );
    let mut expected = vec![0.0; frames * 2];
    reference.render_at_output_frame(*frame, &mut expected, &mut [0.0; NUM_SAMPLES]);
    assert_eq!(actual, expected, "real lifecycle worker at {frame}");
    assert!(
        voice(&loaded.callback.mixer)
            .source_playback
            .matches_exact(&voice(reference).source_playback)
    );
    assert_eq!(
        voice(&loaded.callback.mixer).stretch.pending_fifo_frames(),
        voice(reference).stretch.pending_fifo_frames()
    );
    *frame += frames as u64;
    actual
}

fn drain_at_output_frame(loaded: &mut Loaded, frame: u64) -> usize {
    let previous = loaded.callback.transport.output_frame();
    assert!(frame >= previous);
    loaded
        .callback
        .transport
        .advance_by_rendered_frames((frame - previous) as usize);
    crate::audio_engine::audio_stream::drain_control_messages(
        &mut loaded.consumer,
        &mut loaded.callback.scheduler,
        frame,
        &mut loaded.callback.quantization,
        &mut loaded.callback.transport,
        &mut loaded.callback.mixer,
        &mut loaded.callback.feedback,
        &mut loaded.callback.retirement,
    )
}

fn complete_mixer(mono: &[f32], start: usize, end: usize) -> RtMixer {
    // Complete PCM is built from original WAV integers, never from a resident view/reader.
    let source = SampleBuffer {
        residency: None,
        channels: 2,
        samples: mono
            .iter()
            .flat_map(|value| [*value; 2])
            .collect::<Vec<_>>()
            .into(),
    }
    .with_complete_source(48_000);
    let mut mixer = RtMixer::new(2, 48_000.0);
    let ownership =
        Arc::new(crate::audio_engine::input_runtime_binding::InputRuntimeOwnership::tracked());
    ownership.publish_source(0, &source, 48_000, 1);
    mixer.set_input_runtime_ownership(ownership);
    mixer.load_sample(0, source);
    mixer.set_pad_loop_region(0, start as f64 / 48_000.0, Some(end as f64 / 48_000.0));
    mixer.set_speed(0.73);
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    // The complete source stages its live ON after the same explicit clock cut.
    // Every new owned voice must first warm Native while keeping the dry baseline.
    mixer.set_pad_key_lock(0, true);
    mixer
}

fn wait_prepared(actual: &mut RtMixer, reference: &mut RtMixer) -> (u64, usize) {
    wait_until(Duration::from_secs(5), || {
        voice_mut(actual).stretch.source_preparation_ready()
            && voice_mut(reference).stretch.source_preparation_ready()
    });
    let target = voice_mut(actual)
        .stretch
        .prepared_target_output_frame()
        .unwrap();
    assert_eq!(
        Some(target),
        voice_mut(reference).stretch.prepared_target_output_frame()
    );
    let native = voice_mut(actual).stretch.prepared_native_address();
    let taps = voice_mut(actual)
        .stretch
        .prepared_tap_observation()
        .unwrap();
    assert_eq!(taps.missing_reads, 0);
    assert_eq!(taps.left_reads, 4096 * 2);
    (target, native)
}

fn through_adoption(
    loaded: &mut Loaded,
    reference: &mut RtMixer,
    frame: &mut u64,
    target: u64,
    native: usize,
) {
    let mut audible = false;
    let mut step = 0;
    while *frame < target + 1777 {
        let frames =
            [1, 127, 384, 96, 257, 512, 31][step % 7].min((target + 1777 - *frame) as usize);
        let output = render(loaded, reference, frame, frames);
        if *frame > target {
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.native_state_address(),
                native
            );
            audible |= output.iter().any(|sample| sample.abs() > 0.001);
        }
        step += 1;
    }
    assert!(
        audible,
        "later genuine finite Native continuation is silent"
    );
}

/// Independent original B knots, Raw Native and continuous EQ for the acknowledged retrigger.
/// Both actual stereo channels are the same original mono WAV value, 8192 / 32768.
struct ConstantBankOracle {
    source: SampleBuffer,
    clock: AlgebraicClock,
    raw: RawChronology,
    dry_warm_remaining: usize,
    dry_mode: bool,
    filter: crate::audio_engine::dsp::PerPadDspChain,
    checkpoint: Option<(u64, RawChronology)>,
    checkpoint_target: u64,
    prepared_address: Option<usize>,
    live_address: Option<usize>,
    adopted: bool,
    warm_audible: bool,
    adopted_audible: bool,
}

impl ConstantBankOracle {
    fn new(capture_frame: u64) -> Self {
        // These independently supplied constant knots cover the helper's physical origin too;
        // no resident sample, SourcePlayback or SourceReadPlan contributes oracle input.
        let source = SampleBuffer {
            residency: None,
            channels: 1,
            samples: vec![8192.0 / 32768.0; 1000].into(),
        };
        let clock = AlgebraicClock::new(200.0, 1.97);
        let mut prepared_clock = clock.clone();
        let mut prepared = RawChronology::new(1.97, true);
        prepared.render(&source, &mut prepared_clock, 4096);
        let raw = RawChronology::new(1.97, false);
        let dry_warm_remaining = raw.block_size();
        Self {
            source,
            clock,
            raw,
            dry_warm_remaining,
            dry_mode: false,
            filter: lifecycle_filter_after_cut(),
            checkpoint: Some((capture_frame + 4096, prepared)),
            checkpoint_target: capture_frame + 4096,
            prepared_address: None,
            live_address: None,
            adopted: false,
            warm_audible: false,
            adopted_audible: false,
        }
    }

    fn accept_checkpoint(&mut self, target: u64, address: usize, live_address: usize) {
        assert_eq!(self.checkpoint.as_ref().unwrap().0, target);
        assert_ne!(address, live_address);
        self.prepared_address = Some(address);
        self.live_address = Some(live_address);
    }

    fn keep_previous_dry_after_failure(&mut self) {
        self.dry_mode = true;
        self.dry_warm_remaining = 0;
        self.checkpoint = None;
        self.prepared_address = None;
        self.live_address = None;
    }

    fn render(
        &mut self,
        loaded: &mut Loaded,
        reference: &mut RtMixer,
        frame: &mut u64,
        frames: usize,
    ) {
        let start = *frame;
        // Retain the exact complete PCM, source trajectory and FIFO checks on every callback.
        let actual = render(loaded, reference, frame, frames);
        let mut dry_clock = self.clock.clone();
        let dry = dry_clock.dry(&self.source, frames);
        let prefix = self.checkpoint.as_ref().map_or(frames, |(target, _)| {
            target.saturating_sub(start).min(frames as u64) as usize
        });
        let mut upstream = if self.dry_mode {
            self.clock.dry(&self.source, prefix)
        } else {
            self.raw.render(&self.source, &mut self.clock, prefix)
        };
        if prefix < frames {
            let (_, prepared) = self.checkpoint.take().unwrap();
            self.raw = prepared;
            upstream.extend(
                self.raw
                    .render(&self.source, &mut self.clock, frames - prefix),
            );
            self.adopted = true;
        }
        // Independent Native's own block size defines the first dry handover prefix.
        // FIFO chronology and the 4096-frame adoption remain checked without alteration.
        let warm = frames.min(self.dry_warm_remaining);
        upstream[..warm].copy_from_slice(&dry[..warm]);
        self.dry_warm_remaining -= warm;
        if self.adopted {
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.native_state_address(),
                self.prepared_address.unwrap()
            );
        } else if let Some(live_address) = self.live_address {
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.native_state_address(),
                live_address
            );
        }
        assert_eq!(upstream.len(), frames);
        for (offset, (stereo, raw)) in actual.chunks_exact(2).zip(upstream).enumerate() {
            self.filter.begin_frame();
            let expected = self.filter.process_sample(0, raw);
            for (channel, value) in stereo.iter().enumerate() {
                assert!(
                    (*value - expected).abs() <= 5.0e-5,
                    "B independent Raw/EQ at {} channel {channel}: {value} != {expected}",
                    start + offset as u64
                );
            }
            if start + (offset as u64) < self.checkpoint_target {
                self.warm_audible |= expected.abs() > 0.001;
            } else {
                self.adopted_audible |= expected.abs() > 0.001;
            }
        }
        let position = voice(&loaded.callback.mixer).source_playback.position();
        assert!(
            (position.frame as f64 - 100.0 + position.fraction - self.clock.phase(0)).abs()
                < 1.0e-8
        );
        if self.dry_mode {
            assert!(
                voice(&loaded.callback.mixer)
                    .stretch
                    .productive_history()
                    .is_none()
            );
        } else {
            assert!(
                voice(&loaded.callback.mixer)
                    .stretch
                    .productive_history()
                    .is_some()
            );
        }
    }
}

fn start_finite(loaded: &mut Loaded) -> ResidentWindowTicket {
    loaded.callback.mixer.set_speed(0.73);
    let ready = loaded.prepare(WindowRequest {
        loop_region: Some((24.0 / 48_000.0, Some(96.0 / 48_000.0))),
        key_lock: Some(true),
        ..WindowRequest::default()
    });
    loaded.adopt(&ready);
    assert!(launch_with_producer(&loaded.engine, &ready, false, 1, &loaded.producer).unwrap());
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    while loaded.callback.feedback_rx.pop().is_ok() {}
    ready
}

#[test]
fn actual_paused_storage_ack_fences_pending_work_and_preserves_adopted_native_reader_peak() {
    for adopted_before_pause in [false, true] {
        let mut loaded = Loaded::new();
        let a = start_finite(&mut loaded);
        let mut reference = complete_mixer(&loaded.mono, 24, 96);
        let mut frame = 0;
        render(&mut loaded, &mut reference, &mut frame, 13);
        let (target, prepared) = wait_prepared(&mut loaded.callback.mixer, &mut reference);
        if adopted_before_pause {
            through_adoption(&mut loaded, &mut reference, &mut frame, target, prepared);
        }
        let native = voice(&loaded.callback.mixer).stretch.native_state_address();
        let position = voice(&loaded.callback.mixer).source_playback.position();
        let fifo = voice(&loaded.callback.mixer).stretch.pending_fifo_frames();
        let history = voice(&loaded.callback.mixer)
            .stretch
            .productive_history()
            .unwrap();
        let original = loaded.sample();
        let original_binding = original.resident_binding();
        let reader = Arc::downgrade(&original.samples);
        loaded.callback.mixer.pause_sample_at_output_frame(0, frame);
        reference.pause_sample_at_output_frame(0, frame);
        let storage = loaded.prepare(WindowRequest {
            storage_range: Some((20.0 / 48_000.0, 100.0 / 48_000.0)),
            ..WindowRequest::default()
        });
        loaded.adopt(&storage);
        assert!(!a.is_current());
        assert!(storage.is_current());
        assert_ne!(loaded.sample().resident_binding(), original_binding);
        assert!(reader.upgrade().is_some());
        assert_eq!(
            voice(&loaded.callback.mixer).stretch.native_state_address(),
            native
        );
        assert_eq!(
            voice(&loaded.callback.mixer).stretch.pending_fifo_frames(),
            fifo
        );
        assert_eq!(
            voice(&loaded.callback.mixer).source_playback.position(),
            position
        );
        assert_eq!(
            voice(&loaded.callback.mixer)
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            history.fed_output_frames
        );
        assert!(
            render(&mut loaded, &mut reference, &mut frame, 257)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        loaded
            .callback
            .mixer
            .resume_sample_at_output_frame(0, frame);
        reference.resume_sample_at_output_frame(0, frame);
        render(&mut loaded, &mut reference, &mut frame, 1);
        let (new_target, new_native) = wait_prepared(&mut loaded.callback.mixer, &mut reference);
        assert_eq!(
            voice(&loaded.callback.mixer).stretch.native_state_address(),
            native
        );
        through_adoption(
            &mut loaded,
            &mut reference,
            &mut frame,
            new_target,
            new_native,
        );
        assert!(
            voice(&loaded.callback.mixer)
                .stretch
                .adopted_request_id()
                .unwrap()
                >= 2
        );
        drop(original);
        let lease = loaded
            .engine
            .project_assets
            .cold_lease_for_reader(&loaded.sample())
            .unwrap();
        let held = loaded
            .engine
            .project_assets
            .held_reader_backings(&lease, None)
            .unwrap();
        let registered: usize = held
            .iter()
            .map(|samples| samples.len() * std::mem::size_of::<f32>())
            .sum();
        let next = loaded.prepare(WindowRequest {
            storage_range: Some((16.0 / 48_000.0, 112.0 / 48_000.0)),
            ..WindowRequest::default()
        });
        loaded.pending(&next);
        let observation = next.read_observation_for_test().unwrap();
        assert_eq!(
            observation.admitted_peak_bytes,
            registered + (112 - 16) * 2 * std::mem::size_of::<f32>() + 64 * 1024
        );
        drop(held);
        loaded.adopt(&next);
        for frames in [1, 31, 257, 512, 127, 384, 96].into_iter().cycle().take(24) {
            render(&mut loaded, &mut reference, &mut frame, frames);
        }
        loaded
            .callback
            .mixer
            .stop_sample_rt(0, &mut loaded.callback.retirement);
        wait_until(Duration::from_secs(5), || reader.upgrade().is_none());
    }
}

fn replace_with_actual_full_bank(loaded: &mut Loaded) -> SampleBuffer {
    let path = loaded.directory.path().join("future-keylock.wav");
    write_pcm16(&path, 48_000, 1, &vec![8192_i16; 1000]);
    let (producer, mut consumer) = rtrb::RingBuffer::new(4);
    let request = admit_for_format_selected(
        &loaded.engine,
        0,
        path.to_string_lossy().into_owned(),
        (
            false,
            false,
            false,
            Some(ResidentLoadHint {
                start_s: 100.0 / 48_000.0,
                end_s: 300.0 / 48_000.0,
                key_lock: true,
            }),
        ),
        Arc::new(Mutex::new(producer)),
        (2, 48_000, loaded.directory.path().to_owned()),
    )
    .unwrap();
    wait_until(Duration::from_secs(10), || !consumer.is_empty());
    while loaded.callback.feedback_rx.pop().is_ok() {}
    assert_eq!(loaded.callback.drain(&mut consumer), 1);
    assert!(matches!(
        terminal(&loaded.engine, request),
        LoaderEvent::Success { .. }
    ));
    wait_until(Duration::from_secs(10), || {
        loaded.engine.cold_loading[0].load(Ordering::Acquire) == 0
    });
    loaded.sample()
}

#[test]
fn actual_future_bank_window_ack_keeps_old_voice_then_reserved_retrigger_cuts_and_releases_history()
{
    for paused in [false, true] {
        for fault in [
            None,
            Some((false, false)),
            Some((false, true)),
            Some((true, false)),
            Some((true, true)),
        ] {
            let mut loaded = Loaded::new();
            start_finite(&mut loaded);
            let mut reference = complete_mixer(&loaded.mono, 24, 96);
            let mut frame = 0;
            render(&mut loaded, &mut reference, &mut frame, 13);
            let (target, prepared) = wait_prepared(&mut loaded.callback.mixer, &mut reference);
            through_adoption(&mut loaded, &mut reference, &mut frame, target, prepared);
            let old_generation = voice(&loaded.callback.mixer).generation;
            let old = loaded.sample();
            let old_reader = Arc::downgrade(&old.samples);
            let old_position = voice(&loaded.callback.mixer).source_playback.position();
            let old_region = loaded.callback.mixer.loop_region_frames(0);
            if paused {
                loaded.callback.mixer.pause_sample_at_output_frame(0, frame);
                reference.pause_sample_at_output_frame(0, frame);
            }
            let before = capture(&loaded.engine, 0).unwrap().unwrap();
            let new_full = replace_with_actual_full_bank(&mut loaded);
            assert!(!before.current());
            assert!(!new_full.same_source(&old));
            assert_eq!(
                (new_full.resident_start(), new_full.resident_end()),
                (0, 1000)
            );
            assert!(
                voice(&loaded.callback.mixer)
                    .sample
                    .as_ref()
                    .unwrap()
                    .same_window(&old)
            );
            assert_eq!(
                voice(&loaded.callback.mixer).source_playback.position(),
                old_position
            );
            assert_eq!(
                voice(&loaded.callback.mixer)
                    .source_loop_region
                    .unwrap()
                    .start,
                old_region.0
            );
            let future = loaded.prepare(WindowRequest {
                loop_region: Some((100.0 / 48_000.0, Some(300.0 / 48_000.0))),
                key_lock: Some(true),
                ..WindowRequest::default()
            });
            loaded.pending(&future);
            let reads = future.read_observation_for_test().unwrap();
            assert_eq!(reads.source.read_bytes, (300 - 100) * 2 * 4);
            assert_eq!(reads.source.allocated_bytes, (300 - 100) * 2 * 4);
            assert_eq!(reads.stems.read_bytes, [0; 5]);
            assert!(
                reads.admitted_peak_bytes
                    >= (96 - 24) * 2 * 4 + 1000 * 2 * 4 + (300 - 100) * 2 * 4 + 64 * 1024
            );
            loaded.adopt(&future);
            let after = capture(&loaded.engine, 0).unwrap().unwrap();
            assert!(after.current() && after.available());
            assert_eq!(after.binding.resident, loaded.sample().resident_binding());
            assert!(
                voice(&loaded.callback.mixer)
                    .sample
                    .as_ref()
                    .unwrap()
                    .same_window(&old)
            );
            assert_eq!(
                voice(&loaded.callback.mixer).source_playback.position(),
                old_position
            );
            if paused {
                assert!(
                    render(&mut loaded, &mut reference, &mut frame, 257)
                        .iter()
                        .all(|sample| *sample == 0.0)
                );
                loaded
                    .callback
                    .mixer
                    .resume_sample_at_output_frame(0, frame);
                reference.resume_sample_at_output_frame(0, frame);
            }
            // An actual rate intent forces fresh old-A work; B's own ACK is not A history authority.
            loaded.callback.mixer.set_speed(1.97);
            reference.set_speed(1.97);
            render(&mut loaded, &mut reference, &mut frame, 1);
            let (old_target, old_prepared) =
                wait_prepared(&mut loaded.callback.mixer, &mut reference);
            through_adoption(
                &mut loaded,
                &mut reference,
                &mut frame,
                old_target,
                old_prepared,
            );
            assert_eq!(
                voice(&loaded.callback.mixer)
                    .stretch
                    .productive_history()
                    .unwrap()
                    .binding
                    .source_address,
                old.source_address()
            );
            let history = voice(&loaded.callback.mixer)
                .stretch
                .productive_history()
                .unwrap();
            let native = voice(&loaded.callback.mixer).stretch.native_state_address();
            let fifo = voice(&loaded.callback.mixer).stretch.pending_fifo_frames();
            let position = voice(&loaded.callback.mixer).source_playback.position();
            let parameters = loaded.callback.mixer.loop_region_frames(0);
            while loaded.callback.feedback_rx.pop().is_ok() {}
            // Use the actual monotonic callback clock and leave a genuine future scheduled event.
            loaded.callback.transport.advance_by_rendered_frames(
                (frame - loaded.callback.transport.output_frame()) as usize,
            );
            loaded.callback.quantization = TriggerQuantization::Grid { step_64ths: 16 };
            assert!(
                launch_with_producer(&loaded.engine, &future, false, 2, &loaded.producer).unwrap()
            );
            loaded.callback.retirement.capacity = 0;
            assert_eq!(drain_at_output_frame(&mut loaded, frame), 0);
            assert_eq!(
                voice(&loaded.callback.mixer).source_playback.position(),
                position
            );
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.native_state_address(),
                native
            );
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.pending_fifo_frames(),
                fifo
            );
            assert_eq!(
                voice(&loaded.callback.mixer)
                    .stretch
                    .productive_history()
                    .unwrap()
                    .binding,
                history.binding
            );
            assert_eq!(loaded.callback.mixer.loop_region_frames(0), parameters);
            assert!(old_reader.upgrade().is_some());
            loaded.callback.retirement.capacity = usize::MAX;
            assert_eq!(drain_at_output_frame(&mut loaded, frame), 1);
            assert!(
                voice(&loaded.callback.mixer)
                    .sample
                    .as_ref()
                    .unwrap()
                    .same_window(&old)
            );
            let target = loaded.callback.scheduler.peek_next_target_frame().unwrap();
            assert!(target >= frame);
            while frame < target {
                let frames = 127.min((target - frame) as usize);
                render(&mut loaded, &mut reference, &mut frame, frames);
            }
            loaded.callback.transport.advance_by_rendered_frames(
                (frame - loaded.callback.transport.output_frame()) as usize,
            );
            let event = loaded
                .callback
                .scheduler
                .pop_due_at_callback_start(frame)
                .unwrap();
            assert_eq!(event.execution_frame, frame);
            assert!(!event.was_late);
            let execution_position = voice(&loaded.callback.mixer).source_playback.position();
            let execution_native = voice(&loaded.callback.mixer).stretch.native_state_address();
            let execution_fifo = voice(&loaded.callback.mixer).stretch.pending_fifo_frames();
            let execution_history = voice(&loaded.callback.mixer)
                .stretch
                .productive_history()
                .unwrap();
            loaded.callback.retirement.capacity = 0;
            crate::audio_engine::audio_stream::execute_scheduled_command(
                &mut loaded.callback.mixer,
                &mut loaded.callback.transport,
                event.execution_frame,
                event.command.clone(),
                &mut loaded.callback.feedback,
                &mut loaded.callback.retirement,
            );
            assert!(
                voice(&loaded.callback.mixer)
                    .sample
                    .as_ref()
                    .unwrap()
                    .same_window(&old)
            );
            assert_eq!(
                voice(&loaded.callback.mixer).source_playback.position(),
                execution_position
            );
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.native_state_address(),
                execution_native
            );
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.pending_fifo_frames(),
                execution_fifo
            );
            assert_eq!(
                voice(&loaded.callback.mixer)
                    .stretch
                    .productive_history()
                    .unwrap()
                    .binding,
                execution_history.binding
            );
            assert_eq!(
                voice(&loaded.callback.mixer)
                    .stretch
                    .productive_history()
                    .unwrap()
                    .fed_output_frames,
                execution_history.fed_output_frames
            );
            assert_eq!(loaded.callback.mixer.loop_region_frames(0), parameters);
            assert!(old_reader.upgrade().is_some());
            loaded.callback.retirement.capacity = usize::MAX;
            crate::audio_engine::audio_stream::execute_scheduled_command(
                &mut loaded.callback.mixer,
                &mut loaded.callback.transport,
                event.execution_frame,
                event.command,
                &mut loaded.callback.feedback,
                &mut loaded.callback.retirement,
            );
            assert!(
                voice(&loaded.callback.mixer)
                    .sample
                    .as_ref()
                    .unwrap()
                    .same_window(&loaded.sample())
            );
            assert!(
                voice(&loaded.callback.mixer)
                    .stretch
                    .productive_history()
                    .is_none()
            );
            assert!(
                loaded
                    .callback
                    .mixer
                    .dsp_source_history_for_test(0)
                    .is_none()
            );
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.pending_fifo_frames(),
                (0, 0)
            );
            let own = future.key_lock_request_id_for_test().unwrap();
            let new_source = loaded.sample();
            let waiting = loaded
                .engine
                .input_runtime_ownership
                .key_lock_status(0)
                .unwrap();
            assert_eq!(waiting.request_id, own);
            assert_eq!(waiting.source_address, new_source.source_address());
            assert_eq!(waiting.window_revision, new_source.window_revision());
            assert_eq!(
                Some(waiting.source_generation),
                loaded
                    .engine
                    .input_runtime_ownership
                    .binding_source_generation(0, after.binding)
            );
            assert!(!waiting.effective && !waiting.ready && waiting.state == "waiting");
            assert_eq!(voice(&loaded.callback.mixer).generation, old_generation + 1);
            assert_eq!(
                voice(&loaded.callback.mixer)
                    .source_playback
                    .position()
                    .frame,
                100
            );
            assert_eq!(
                voice(&loaded.callback.mixer)
                    .source_playback
                    .position()
                    .fraction,
                0.0
            );
            let mut new_reference = complete_mixer(&vec![8192.0 / 32768.0; 1000], 100, 300);
            new_reference.set_speed(1.97);
            assert!(new_reference.play_sample_at_output_frame(0, 1.0, frame));
            loaded.callback.mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
            new_reference.set_pad_eq(0, -12.0, -3.0, 4.0);
            // Retrigger cuts source/DSP history while the absolute output clock remains monotonic.
            let capture_frame = frame;
            let mut b_oracle = ConstantBankOracle::new(capture_frame);
            if let Some((exhausted, after_warming)) = fault {
                if after_warming {
                    let block = b_oracle.raw.block_size();
                    b_oracle.render(&mut loaded, &mut new_reference, &mut frame, block);
                    assert!(b_oracle.raw.used);
                    let status = loaded
                        .engine
                        .input_runtime_ownership
                        .key_lock_status(0)
                        .unwrap();
                    assert_eq!(status.request_id, own);
                    assert!(!status.effective && !status.ready && status.state == "waiting");
                }
                let before_failure = voice(&loaded.callback.mixer).source_playback;
                if exhausted {
                    voice_mut(&mut loaded.callback.mixer)
                        .stretch
                        .exhaust_warmed_reserve();
                } else {
                    voice(&loaded.callback.mixer)
                        .stretch
                        .fail_preparation_worker();
                }
                assert!(
                    voice(&loaded.callback.mixer)
                        .source_playback
                        .matches_exact(&before_failure)
                );
                new_reference.set_pad_key_lock(0, false);
                b_oracle.keep_previous_dry_after_failure();
                for frames in [1, 127, 384, 512, 777] {
                    b_oracle.render(&mut loaded, &mut new_reference, &mut frame, frames);
                }
                let failed = loaded
                    .engine
                    .input_runtime_ownership
                    .key_lock_status(0)
                    .unwrap();
                assert_eq!(failed.request_id, own);
                assert_eq!(failed.source_generation, waiting.source_generation);
                assert_eq!(failed.source_address, waiting.source_address);
                assert_eq!(failed.window_revision, waiting.window_revision);
                assert!(!failed.effective && !failed.ready && failed.state == "error");
                assert!(failed.error.is_some());
                assert_eq!(voice(&loaded.callback.mixer).generation, old_generation + 1);
                assert!(
                    voice(&loaded.callback.mixer)
                        .sample
                        .as_ref()
                        .unwrap()
                        .same_window(&new_source)
                );
                assert!(future.is_current());
                drop(old);
                wait_until(Duration::from_secs(5), || old_reader.upgrade().is_none());
                continue;
            }
            b_oracle.render(&mut loaded, &mut new_reference, &mut frame, 1);
            let (new_target, new_prepared) =
                wait_prepared(&mut loaded.callback.mixer, &mut new_reference);
            assert_eq!(new_target, capture_frame + 4096);
            b_oracle.accept_checkpoint(
                new_target,
                new_prepared,
                voice(&loaded.callback.mixer).stretch.native_state_address(),
            );
            // The callback crossing 4096 is split at the actual target in the Raw oracle only;
            // one filter instance consumes both sides and every later irregular callback.
            let mut step = 0;
            while frame < new_target + 1777 {
                let frames = [1, 127, 384, 96, 257, 512, 31][step % 7]
                    .min((new_target + 1777 - frame) as usize);
                b_oracle.render(&mut loaded, &mut new_reference, &mut frame, frames);
                step += 1;
            }
            for frames in [31, 257, 1, 384, 96, 512, 127] {
                b_oracle.render(&mut loaded, &mut new_reference, &mut frame, frames);
            }
            assert!(b_oracle.adopted);
            let wet = loaded
                .engine
                .input_runtime_ownership
                .key_lock_status(0)
                .unwrap();
            assert_eq!(wet.request_id, own);
            assert_eq!(wet.source_generation, waiting.source_generation);
            assert_eq!(wet.source_address, waiting.source_address);
            assert_eq!(wet.window_revision, waiting.window_revision);
            assert!(wet.effective && wet.ready && wet.state == "wet");
            assert_eq!(voice(&loaded.callback.mixer).generation, old_generation + 1);
            assert!(
                b_oracle.warm_audible,
                "genuine B warm Raw/EQ prefix is silent"
            );
            assert!(
                b_oracle.adopted_audible,
                "genuine B adopted Raw/EQ continuation is silent"
            );
            assert_ne!(
                voice(&loaded.callback.mixer)
                    .stretch
                    .productive_history()
                    .unwrap()
                    .binding
                    .source_address,
                old.source_address()
            );
            drop(old);
            drop(new_full);
            wait_until(Duration::from_secs(5), || old_reader.upgrade().is_none());
        }
    }
}

#[test]
fn actual_window_history_saturation_rejects_before_superseding_ticket_or_effective_audio() {
    pyo3::Python::initialize();
    for already_pending in [false, true] {
        let mut loaded = Loaded::new();
        let acknowledged = start_finite(&mut loaded);
        let mut reference = complete_mixer(&loaded.mono, 24, 96);
        let mut frame = 0;
        render(&mut loaded, &mut reference, &mut frame, 13);
        let (target, prepared) = wait_prepared(&mut loaded.callback.mixer, &mut reference);
        through_adoption(&mut loaded, &mut reference, &mut frame, target, prepared);
        let previous = if already_pending {
            let pending = loaded.prepare(WindowRequest {
                storage_range: Some((20.0 / 48_000.0, 100.0 / 48_000.0)),
                ..WindowRequest::default()
            });
            loaded.pending(&pending);
            pending
        } else {
            acknowledged
        };
        let previous_status = previous.publication_status();
        assert_eq!(
            previous_status,
            if already_pending {
                "pending"
            } else {
                "accepted"
            }
        );
        assert!(previous.is_current());
        assert!(previous.error().unwrap().is_none());

        // Keep genuine existing reader backings alive, then apply capacity pressure with
        // distinct small PCM Arcs from original WAV values in this same existing history.
        // This creates no source permit, new registry, loader or callback allocation.
        let (mut retained, before_history) = {
            let mut history = loaded.engine.cold_pcm_history.lock().unwrap();
            let pad = &mut history[0];
            pad.retain(|reader| reader.strong_count() > 0);
            let mut retained: Vec<Arc<[f32]>> =
                pad.iter().filter_map(std::sync::Weak::upgrade).collect();
            assert!(retained.len() < 128);
            while pad.len() < 128 {
                let index = pad.len() % loaded.mono.len();
                let samples: Arc<[f32]> = Arc::from([loaded.mono[index]; 2]);
                pad.push(Arc::downgrade(&samples));
                retained.push(samples);
            }
            assert_eq!(pad.len(), 128);
            assert_eq!(retained.len(), 128);
            (retained, pad.clone())
        };
        let bank = loaded.sample();
        let binding = capture(&loaded.engine, 0).unwrap().unwrap();
        let source_epoch = loaded.engine.prepared_source_epochs[0].load(Ordering::Acquire);
        let request_id = loaded.engine.pad_request_ids.lock().unwrap()[0];
        let control_pending = loaded
            .engine
            .input_runtime_ownership
            .resident_control_pending(0);
        let queue_empty = loaded.consumer.is_empty();
        let native = voice(&loaded.callback.mixer).stretch.native_state_address();
        let fifo = voice(&loaded.callback.mixer).stretch.pending_fifo_frames();
        let position = voice(&loaded.callback.mixer).source_playback.position();
        let productive = voice(&loaded.callback.mixer)
            .stretch
            .productive_history()
            .unwrap();
        let parameters = loaded.callback.mixer.loop_region_frames(0);
        let ratio = voice(&loaded.callback.mixer).source_playback.tempo_ratio();
        let rate_target = voice(&loaded.callback.mixer).source_playback.rate_target();
        assert!(loaded.callback.mixer.key_lock_for_measurement(0));

        for request in [
            WindowRequest {
                storage_range: Some((16.0 / 48_000.0, 112.0 / 48_000.0)),
                ..WindowRequest::default()
            },
            WindowRequest {
                loop_region: Some((80.0 / 48_000.0, Some(111.0 / 48_000.0))),
                key_lock: Some(false),
                ..WindowRequest::default()
            },
        ] {
            let error =
                prepare_window_with_producer(&loaded.engine, 0, request, loaded.producer.clone())
                    .err()
                    .expect("128 live backing histories must reject a changed WindowWork");
            assert!(
                error
                    .to_string()
                    .contains("same-pad source reader history full (128 live assignments)")
            );
            assert_eq!(previous.publication_status(), previous_status);
            assert!(
                previous.is_current(),
                "failed reservation advanced the old intent epoch"
            );
            assert!(
                previous.error().unwrap().is_none(),
                "failed reservation cancelled the previous work"
            );
            assert_eq!(
                loaded.engine.prepared_source_epochs[0].load(Ordering::Acquire),
                source_epoch
            );
            assert_eq!(loaded.engine.pad_request_ids.lock().unwrap()[0], request_id);
            assert_eq!(
                loaded
                    .engine
                    .input_runtime_ownership
                    .resident_control_pending(0),
                control_pending
            );
            assert_eq!(loaded.consumer.is_empty(), queue_empty);
            assert!(binding.current() && binding.available());
            assert_eq!(
                capture(&loaded.engine, 0).unwrap().unwrap().binding,
                binding.binding
            );
            assert!(loaded.sample().same_window(&bank));
            assert!(
                voice(&loaded.callback.mixer)
                    .sample
                    .as_ref()
                    .unwrap()
                    .same_window(&bank)
            );
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.native_state_address(),
                native
            );
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.pending_fifo_frames(),
                fifo
            );
            assert_eq!(
                voice(&loaded.callback.mixer).source_playback.position(),
                position
            );
            let unchanged = voice(&loaded.callback.mixer)
                .stretch
                .productive_history()
                .unwrap();
            assert_eq!(unchanged.binding, productive.binding);
            assert_eq!(unchanged.fed_output_frames, productive.fed_output_frames);
            assert_eq!(loaded.callback.mixer.loop_region_frames(0), parameters);
            assert_eq!(
                voice(&loaded.callback.mixer).source_playback.tempo_ratio(),
                ratio
            );
            assert_eq!(
                voice(&loaded.callback.mixer).source_playback.rate_target(),
                rate_target
            );
            assert!(loaded.callback.mixer.key_lock_for_measurement(0));
            let history = loaded.engine.cold_pcm_history.lock().unwrap();
            assert_eq!(history[0].len(), 128);
            assert!(
                history[0]
                    .iter()
                    .zip(&before_history)
                    .all(|(current, before)| current.ptr_eq(before))
            );
            assert!(
                retained
                    .iter()
                    .all(|samples| Arc::strong_count(samples) > 0)
            );
        }
        // Rejection preserves nontrivial actual wet output through later irregular calls.
        let mut audible = false;
        for frames in [1, 31, 257, 512, 127, 384, 96].into_iter().cycle().take(16) {
            audible |= render(&mut loaded, &mut reference, &mut frame, frames)
                .iter()
                .any(|sample| sample.abs() > 0.001);
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.native_state_address(),
                native
            );
            assert!(previous.is_current());
        }
        assert!(audible);
        // Removing only test pressure lets the same prior pending work obtain its own ACK.
        retained.clear();
        if already_pending {
            loaded.adopt(&previous);
            assert_eq!(previous.publication_status(), "accepted");
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.native_state_address(),
                native
            );
            for frames in [31, 127, 384, 96, 257] {
                render(&mut loaded, &mut reference, &mut frame, frames);
            }
        }
    }
}
