//! Real mode WindowWork/ACK and continuous complete-PCM/Raw-Native/FIFO/EQ oracles.
use super::*;
use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
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

struct ModeOracle {
    loaded: Box<Loaded>,
    complete: Box<RtMixer>,
    complete_ownership: Arc<InputRuntimeOwnership>,
    source: SampleBuffer,
    clock: AlgebraicClock,
    raw: RawChronology,
    filter: crate::audio_engine::dsp::PerPadDspChain,
    frame: u64,
    wet: bool,
    dry_warm_remaining: usize,
    candidate: Option<(u64, usize, RawChronology)>,
    adopted: usize,
    difference: f32,
}

impl ModeOracle {
    fn new(initial: bool) -> Self {
        Self::new_at_ratio(initial, 0.73)
    }

    fn new_at_ratio(initial: bool, ratio: f64) -> Self {
        let mut loaded = Box::new(Loaded::new());
        let ready = loaded.prepare(WindowRequest {
            loop_region: Some((24.0 / 48_000.0, Some(96.0 / 48_000.0))),
            key_lock: Some(initial),
            ..Default::default()
        });
        loaded.adopt(&ready);
        loaded.callback.mixer.set_speed(ratio);
        assert!(launch_with_producer(&loaded.engine, &ready, false, 1, &loaded.producer).unwrap());
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
        let source = SampleBuffer {
            channels: 1,
            residency: None,
            samples: loaded.mono.clone().into(),
        };
        let full = SampleBuffer {
            channels: 2,
            residency: None,
            samples: loaded
                .mono
                .iter()
                .flat_map(|value| [*value; 2])
                .collect::<Vec<_>>()
                .into(),
        }
        .with_complete_source(48_000);
        let mut complete = Box::new(RtMixer::new(2, 48_000.0));
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        ownership.publish_source(0, &full, 48_000, 1);
        complete.set_input_runtime_ownership(ownership.clone());
        complete.load_sample(0, full);
        complete.set_pad_loop_region(0, 24.0 / 48_000.0, Some(96.0 / 48_000.0));
        complete.set_speed(ratio);
        assert!(complete.play_sample_at_output_frame(0, 1.0, 0));
        // The complete-PCM control stages the same first wet handover on its live
        // dry voice. The productive finite side instead arms the owned mode stopped.
        complete.set_pad_key_lock(0, initial);
        for mixer in [complete.as_mut(), &mut loaded.callback.mixer] {
            mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
        }
        let raw = RawChronology::new(ratio, false);
        let dry_warm_remaining = if initial { raw.block_size() } else { 0 };
        Self {
            loaded,
            complete,
            complete_ownership: ownership,
            source,
            clock: AlgebraicClock::physical(24, 96, ratio),
            raw,
            filter: lifecycle_filter_after_cut(),
            frame: 0,
            wet: initial,
            dry_warm_remaining,
            candidate: None,
            adopted: 0,
            difference: 0.0,
        }
    }

    fn mode(&mut self, enabled: bool) -> ResidentWindowTicket {
        let bank_region = (
            self.loaded.sample().resident_start(),
            self.loaded.sample().resident_end(),
        );
        let before = voice(&self.loaded.callback.mixer).source_playback;
        let generation = voice(&self.loaded.callback.mixer).generation;
        let region = voice(&self.loaded.callback.mixer).source_loop_region;
        let ticket = self.loaded.prepare(WindowRequest {
            key_lock: Some(enabled),
            ..Default::default()
        });
        self.loaded.adopt(&ticket);
        assert!(
            voice(&self.loaded.callback.mixer)
                .source_playback
                .matches_exact(&before)
        );
        assert_eq!(voice(&self.loaded.callback.mixer).generation, generation);
        assert_eq!(
            voice(&self.loaded.callback.mixer).source_loop_region,
            region
        );
        assert_eq!(
            (
                self.loaded.sample().resident_start(),
                self.loaded.sample().resident_end()
            ),
            bank_region
        );
        assert_eq!(
            self.loaded.sample().residency.as_ref().unwrap().context,
            if enabled {
                ResidentContext::KeyLockFiniteLoop
            } else {
                ResidentContext::FiniteLoop
            }
        );
        assert_eq!(
            self.loaded.callback.mixer.key_lock_for_measurement(0),
            enabled
        );
        self.complete.set_pad_key_lock(0, enabled);
        if enabled && !self.wet {
            self.raw =
                RawChronology::new(voice(&self.complete).source_playback.tempo_ratio(), false);
            self.dry_warm_remaining = self.raw.block_size();
        } else if !enabled {
            self.dry_warm_remaining = 0;
        }
        self.wet = enabled;
        self.candidate = None;
        ticket
    }

    fn arm_stopped_on_and_start(&mut self, stop_while_armed: bool) -> ResidentWindowTicket {
        assert!(!self.wet);
        let ratio = self.clock.ratio_for_test();
        let previous_output_frame = self.loaded.callback.transport.output_frame();
        self.loaded
            .callback
            .transport
            .advance_by_rendered_frames((self.frame - previous_output_frame) as usize);
        self.loaded
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::StopSample { id: 0 })
            .unwrap();
        assert_eq!(self.loaded.callback.drain(&mut self.loaded.consumer), 1);
        self.complete.stop_sample(0);
        assert!(
            !self
                .loaded
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active)
        );
        let stopped = self.loaded.callback.mixer.voices[0].source_playback;
        let stopped_generation = self.loaded.callback.mixer.voices[0].generation;
        let previous = self.loaded.sample();
        assert!(
            self.loaded
                .callback
                .mixer
                .dsp_source_history_for_test(0)
                .is_none()
        );

        let armed_ticket = self.loaded.prepare(WindowRequest {
            key_lock: Some(true),
            ..Default::default()
        });
        self.loaded.adopt(&armed_ticket);
        assert!(
            self.loaded.callback.mixer.voices[0]
                .source_playback
                .matches_exact(&stopped)
        );
        assert_eq!(
            self.loaded.callback.mixer.voices[0].generation,
            stopped_generation
        );
        let armed_source = self.loaded.sample();
        assert!(Arc::ptr_eq(&armed_source.samples, &previous.samples));
        assert_eq!(
            (armed_source.resident_start(), armed_source.resident_end()),
            (24, 96)
        );
        assert!(
            self.loaded
                .callback
                .mixer
                .dsp_source_history_for_test(0)
                .is_none()
        );
        let status = self
            .loaded
            .engine
            .input_runtime_ownership
            .key_lock_status(0)
            .unwrap();
        assert_eq!(
            Some(status.request_id),
            armed_ticket.key_lock_request_id_for_test()
        );
        assert!(status.effective && status.ready && status.state == "armed");

        if stop_while_armed {
            // A second accepted stop may validate the private reserve, but cannot
            // certify a voice which has not yet emitted its first live wet frame.
            self.loaded
                .producer
                .lock()
                .unwrap()
                .push(ControlMessage::StopSample { id: 0 })
                .unwrap();
            assert_eq!(self.loaded.callback.drain(&mut self.loaded.consumer), 1);
            let status = self
                .loaded
                .engine
                .input_runtime_ownership
                .key_lock_status(0)
                .unwrap();
            assert_eq!(
                Some(status.request_id),
                armed_ticket.key_lock_request_id_for_test()
            );
            assert!(status.effective && status.ready && status.state == "armed");
            assert_eq!(
                self.loaded.callback.mixer.voices[0].generation,
                stopped_generation
            );
        }

        // The mode-only receipt deliberately has no launch geometry. The real start
        // producer captures an explicit same-region WindowWork operation of its own.
        let ticket = self.loaded.prepare(WindowRequest {
            loop_region: Some((24.0 / 48_000.0, Some(96.0 / 48_000.0))),
            key_lock: Some(true),
            ..Default::default()
        });
        self.loaded.adopt(&ticket);
        assert!(
            self.loaded.callback.mixer.voices[0]
                .source_playback
                .matches_exact(&stopped)
        );
        assert_eq!(
            self.loaded.callback.mixer.voices[0].generation,
            stopped_generation
        );
        let start_source = self.loaded.sample();
        assert!(Arc::ptr_eq(&start_source.samples, &armed_source.samples));
        assert!(
            launch_with_producer(
                &self.loaded.engine,
                &ticket,
                false,
                1,
                &self.loaded.producer
            )
            .unwrap()
        );
        assert_eq!(self.loaded.callback.drain(&mut self.loaded.consumer), 1);
        assert!(
            self.complete
                .play_sample_at_output_frame(0, 1.0, self.frame)
        );
        self.complete.set_pad_key_lock(0, true);
        assert_eq!(
            voice(&self.loaded.callback.mixer).generation,
            stopped_generation + 1
        );
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .source_playback
                .position()
                .frame,
            24
        );
        let status = self
            .loaded
            .engine
            .input_runtime_ownership
            .key_lock_status(0)
            .unwrap();
        assert_eq!(
            Some(status.request_id),
            ticket.key_lock_request_id_for_test()
        );
        assert_eq!(status.window_revision, start_source.window_revision());
        assert!(!status.effective && !status.ready && status.state == "waiting");

        // Stop/start explicitly cuts the source and EQ histories. The mode ACK above
        // did neither; all subsequent failure/success samples retain this new history.
        self.clock = AlgebraicClock::physical(24, 96, ratio);
        self.raw = RawChronology::new(ratio, false);
        self.filter.reset();
        self.wet = true;
        self.dry_warm_remaining = self.raw.block_size();
        self.candidate = None;
        self.difference = 0.0;
        ticket
    }

    fn retrigger_owned_on(&mut self) -> u64 {
        assert!(self.wet);
        let previous = self
            .loaded
            .engine
            .input_runtime_ownership
            .key_lock_status(0)
            .unwrap();
        assert!(previous.request_id != 0 && previous.effective && previous.ready);
        let generation = voice(&self.loaded.callback.mixer).generation;
        let source = voice(&self.loaded.callback.mixer)
            .sample
            .as_ref()
            .unwrap()
            .clone();
        let ratio = self.clock.ratio_for_test();
        let previous_output_frame = self.loaded.callback.transport.output_frame();
        self.loaded
            .callback
            .transport
            .advance_by_rendered_frames((self.frame - previous_output_frame) as usize);
        self.loaded
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::PlaySample {
                id: 0,
                volume: 1.0,
                received_at_ns: None,
            })
            .unwrap();
        assert_eq!(self.loaded.callback.drain(&mut self.loaded.consumer), 1);
        // The independent complete source deliberately stages its new live ON after
        // the explicit cut, so a prior Native receipt cannot certify this generation.
        self.complete.set_pad_key_lock(0, false);
        assert!(
            self.complete
                .play_sample_at_output_frame(0, 1.0, self.frame)
        );
        self.complete.set_pad_key_lock(0, true);
        let restarted = voice(&self.loaded.callback.mixer);
        assert_eq!(restarted.generation, generation + 1);
        assert!(restarted.sample.as_ref().unwrap().same_window(&source));
        assert_eq!(restarted.source_playback.position().frame, 24);
        assert_eq!(restarted.source_playback.position().fraction, 0.0);
        assert_eq!(restarted.stretch.pending_fifo_frames(), (0, 0));
        assert!(restarted.stretch.productive_history().is_none());
        assert!(
            self.loaded
                .callback
                .mixer
                .dsp_source_history_for_test(0)
                .is_none()
        );
        let waiting = self
            .loaded
            .engine
            .input_runtime_ownership
            .key_lock_status(0)
            .unwrap();
        assert_eq!(waiting.request_id, previous.request_id);
        assert_eq!(waiting.source_generation, previous.source_generation);
        assert_eq!(waiting.source_address, previous.source_address);
        assert_eq!(waiting.window_revision, previous.window_revision);
        assert!(!waiting.effective && !waiting.ready && waiting.state == "waiting");
        self.clock = AlgebraicClock::physical(24, 96, ratio);
        self.raw = RawChronology::new(ratio, false);
        self.filter.reset();
        self.dry_warm_remaining = self.raw.block_size();
        self.candidate = None;
        self.difference = 0.0;
        previous.request_id
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut actual = vec![0.0; frames * 2];
        self.loaded.callback.mixer.render_rt_at_output_frame(
            &mut actual,
            &mut [0.0; NUM_SAMPLES],
            self.frame,
            &mut self.loaded.callback.retirement,
        );
        let mut full = vec![0.0; frames * 2];
        self.complete
            .render_at_output_frame(self.frame, &mut full, &mut [0.0; NUM_SAMPLES]);
        assert_eq!(actual, full, "mode complete PCM at {}", self.frame);
        let mut dry_clock = self.clock.clone();
        let dry = dry_clock.dry(&self.source, frames);
        let prefix = self.candidate.as_ref().map_or(frames, |(target, _, _)| {
            target.saturating_sub(self.frame).min(frames as u64) as usize
        });
        let mut expected = if self.wet {
            self.raw.render(&self.source, &mut self.clock, prefix)
        } else {
            self.clock.dry(&self.source, prefix)
        };
        if prefix < frames {
            let (_, native, raw) = self.candidate.take().unwrap();
            self.raw = raw;
            expected.extend(
                self.raw
                    .render(&self.source, &mut self.clock, frames - prefix),
            );
            assert_eq!(
                voice(&self.loaded.callback.mixer)
                    .stretch
                    .native_state_address(),
                native
            );
            self.adopted += 1;
        }
        let warm = frames.min(self.dry_warm_remaining);
        expected[..warm].copy_from_slice(&dry[..warm]);
        self.dry_warm_remaining -= warm;
        for (offset, (wet, dry)) in expected.iter().zip(dry).enumerate() {
            if self.wet {
                self.difference = self.difference.max((*wet - dry).abs());
            }
            self.filter.begin_frame();
            let filtered = self.filter.process_sample(0, *wet);
            for channel in 0..2 {
                assert!(
                    (actual[offset * 2 + channel] - filtered).abs() <= 5.0e-5,
                    "mode raw/EQ frame {}: {} != {filtered}",
                    self.frame + offset as u64,
                    actual[offset * 2 + channel]
                );
            }
        }
        assert!(
            voice(&self.loaded.callback.mixer)
                .source_playback
                .matches_exact(&voice(&self.complete).source_playback)
        );
        let position = voice(&self.loaded.callback.mixer)
            .source_playback
            .position();
        assert!(
            (position.frame as f64 - 24.0 + position.fraction - self.clock.phase(0)).abs() < 1.0e-8
        );
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .stretch
                .pending_fifo_frames(),
            voice(&self.complete).stretch.pending_fifo_frames()
        );
        self.frame += frames as u64;
        actual
    }

    fn prepare(&mut self) -> (u64, usize) {
        self.clock.begin_chunk(1);
        let mut preparation_clock = self.clock.clone();
        let mut raw = RawChronology::new(self.clock.ratio_for_test(), true);
        raw.render(&self.source, &mut preparation_clock, 4096);
        let start = self.frame;
        self.render(1);
        wait_until(Duration::from_secs(5), || {
            voice_mut(&mut self.loaded.callback.mixer)
                .stretch
                .source_preparation_ready()
                && voice_mut(&mut self.complete)
                    .stretch
                    .source_preparation_ready()
        });
        let target = voice_mut(&mut self.loaded.callback.mixer)
            .stretch
            .prepared_target_output_frame()
            .unwrap();
        let native = voice_mut(&mut self.loaded.callback.mixer)
            .stretch
            .prepared_native_address();
        assert_eq!(target, start + 4096);
        let taps = voice_mut(&mut self.loaded.callback.mixer)
            .stretch
            .prepared_tap_observation()
            .unwrap();
        assert_eq!(taps.left_reads, 4096 * 2);
        assert_eq!(taps.missing_reads, 0);
        assert!(taps.min_frame.is_some_and(|frame| frame >= 24));
        assert!(taps.max_frame.is_some_and(|frame| frame < 96));
        self.candidate = Some((target, native, raw));
        (target, native)
    }

    fn through_adoption(&mut self) {
        let target = self.candidate.as_ref().unwrap().0;
        let mut step = 0;
        while self.frame < target + 1777 {
            self.render(
                [1, 127, 384, 96, 257, 512, 31][step % 7]
                    .min((target + 1777 - self.frame) as usize),
            );
            step += 1;
        }
    }

    fn paused_mode(&mut self, enabled: bool, gap: usize) {
        for mixer in [&mut self.loaded.callback.mixer, self.complete.as_mut()] {
            mixer.pause_sample_at_output_frame(0, self.frame);
        }
        let source = voice(&self.loaded.callback.mixer).source_playback;
        self.mode(enabled);
        let mut silence = vec![0.0; gap * 2];
        self.loaded.callback.mixer.render_rt_at_output_frame(
            &mut silence,
            &mut [0.0; NUM_SAMPLES],
            self.frame,
            &mut self.loaded.callback.retirement,
        );
        assert!(silence.iter().all(|sample| *sample == 0.0));
        self.complete
            .render_at_output_frame(self.frame, &mut silence, &mut [0.0; NUM_SAMPLES]);
        assert!(
            voice(&self.loaded.callback.mixer)
                .source_playback
                .matches_exact(&source)
        );
        self.frame += gap as u64;
        for mixer in [&mut self.loaded.callback.mixer, self.complete.as_mut()] {
            mixer.resume_sample_at_output_frame(0, self.frame);
        }
    }

    fn replace_bank(&mut self) {
        let old = voice(&self.loaded.callback.mixer)
            .sample
            .as_ref()
            .unwrap()
            .source_address();
        self.loaded.replace_bank();
        let replacement = SampleBuffer {
            residency: None,
            channels: 2,
            samples: vec![8192.0 / 32768.0; 2000].into(),
        }
        .with_complete_source(48_000);
        self.complete_ownership
            .publish_source(0, &replacement, 48_000, 2);
        self.complete.load_sample(0, replacement);
        self.complete
            .set_pad_loop_region(0, 100.0 / 48_000.0, Some(300.0 / 48_000.0));
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .sample
                .as_ref()
                .unwrap()
                .source_address(),
            old
        );
        assert!(
            voice(&self.loaded.callback.mixer)
                .source_admission
                .is_some()
        );
        assert!(voice(&self.loaded.callback.mixer).frozen_stems.is_some());
        assert_ne!(self.loaded.sample().source_address(), old);
    }
}

#[test]
fn actual_mode_only_worker_both_live_directions_pause_and_later_native_adoptions_match_raw_eq() {
    for initial in [false, true] {
        let mut oracle = ModeOracle::new(initial);
        if initial {
            oracle.prepare();
            oracle.through_adoption();
        } else {
            for frames in [13, 777, 31] {
                oracle.render(frames);
            }
        }
        let generation = voice(&oracle.loaded.callback.mixer).generation;
        oracle.mode(!initial);
        if !initial {
            oracle.prepare();
            oracle.through_adoption();
        } else {
            for frames in [1, 127, 384, 512] {
                oracle.render(frames);
            }
        }
        oracle.paused_mode(false, 257);
        for frames in [31, 1, 777] {
            oracle.render(frames);
        }
        oracle.paused_mode(true, 389);
        oracle.prepare();
        oracle.through_adoption();
        oracle.mode(false);
        for frames in [1, 127, 384, 512, 777] {
            oracle.render(frames);
        }
        oracle.mode(true);
        oracle.prepare();
        oracle.through_adoption();
        assert_eq!(voice(&oracle.loaded.callback.mixer).generation, generation);
        assert_eq!(oracle.adopted, 3);
        assert!(
            oracle.difference > 0.02,
            "ON never produced a nontrivial Native/dry difference"
        );
        let status = oracle
            .loaded
            .engine
            .input_runtime_ownership
            .key_lock_status(0)
            .unwrap();
        assert_eq!(status.state, "wet");
        assert!(status.ready && status.effective);
    }
}

#[test]
fn actual_mode_only_window_ack_for_new_bank_keeps_retained_old_voice_own_native_source_phase() {
    for paused in [false, true] {
        let mut oracle = ModeOracle::new(false);
        oracle.render(777);
        let old = voice(&oracle.loaded.callback.mixer)
            .sample
            .as_ref()
            .unwrap()
            .clone();
        let old_reader = Arc::downgrade(&old.samples);
        oracle.replace_bank();
        if paused {
            oracle.paused_mode(true, 257);
        } else {
            oracle.mode(true);
        }
        let ticket_source = oracle.loaded.sample().source_address();
        oracle.prepare();
        oracle.through_adoption();
        let history = voice(&oracle.loaded.callback.mixer)
            .stretch
            .productive_history()
            .unwrap();
        assert_eq!(history.binding.source_address, old.source_address());
        assert_ne!(history.binding.source_address, ticket_source);
        oracle.mode(false);
        for frames in [1, 127, 384, 512] {
            oracle.render(frames);
        }
        oracle.mode(true);
        oracle.prepare();
        oracle.through_adoption();
        assert_eq!(oracle.adopted, 2);
        assert!(old_reader.upgrade().is_some());
        assert!(oracle.difference > 0.02);
        drop(old);
        oracle
            .loaded
            .callback
            .mixer
            .stop_sample_rt(0, &mut oracle.loaded.callback.retirement);
        wait_until(Duration::from_secs(5), || old_reader.upgrade().is_none());
    }
}

#[test]
fn actual_mode_window_rejects_coupled_live_finite_geometry_and_unsupported_intro_tail() {
    for explicit in [None, Some(5.0 / 48_000.0), Some(112.0 / 48_000.0)] {
        let mut oracle = ModeOracle::new(false);
        oracle.render(77);
        if let Some(position) = explicit {
            let seek = oracle.loaded.prepare(WindowRequest {
                seek_position_s: Some(position),
                ..Default::default()
            });
            oracle.loaded.adopt(&seek);
            assert!(
                oracle.loaded.sample().resident_start() == 0
                    && oracle.loaded.sample().resident_end()
                        == oracle.loaded.sample().frame_count()
            );
        }
        let before = voice(&oracle.loaded.callback.mixer).source_playback;
        let generation = voice(&oracle.loaded.callback.mixer).generation;
        let bank = oracle.loaded.sample();
        let request = if explicit.is_some() {
            WindowRequest {
                storage_range: Some((24.0 / 48_000.0, 96.0 / 48_000.0)),
                key_lock: Some(true),
                ..Default::default()
            }
        } else {
            WindowRequest {
                loop_region: Some((28.0 / 48_000.0, Some(88.0 / 48_000.0))),
                key_lock: Some(true),
                ..Default::default()
            }
        };
        let rejected = oracle.loaded.prepare(request);
        oracle.loaded.pending(&rejected);
        wait_until(Duration::from_secs(5), || {
            oracle.loaded.callback.drain(&mut oracle.loaded.consumer);
            rejected.publication_status() == "rejected"
        });
        assert!(!oracle.loaded.callback.mixer.key_lock_for_measurement(0));
        assert!(
            voice(&oracle.loaded.callback.mixer)
                .source_playback
                .matches_exact(&before)
        );
        assert_eq!(voice(&oracle.loaded.callback.mixer).generation, generation);
        assert!(oracle.loaded.sample().same_window(&bank));
        reconcile(&oracle.loaded.engine).unwrap();
    }
}

#[test]
fn actual_mode_ack_never_certifies_on_after_native_worker_failure_or_exhausted_warm_reserve() {
    for exhausted in [false, true] {
        let mut oracle = ModeOracle::new(false);
        oracle.render(77);
        let before = voice(&oracle.loaded.callback.mixer).source_playback;
        let generation = voice(&oracle.loaded.callback.mixer).generation;
        let bank = oracle.loaded.sample();
        if exhausted {
            voice_mut(&mut oracle.loaded.callback.mixer)
                .stretch
                .exhaust_warmed_reserve();
        } else {
            voice(&oracle.loaded.callback.mixer)
                .stretch
                .fail_preparation_worker();
        }
        let ticket = oracle.loaded.prepare(WindowRequest {
            key_lock: Some(true),
            ..Default::default()
        });
        oracle.loaded.pending(&ticket);
        wait_until(Duration::from_secs(5), || {
            oracle.loaded.callback.drain(&mut oracle.loaded.consumer);
            ticket.publication_status() == "rejected"
        });
        reconcile(&oracle.loaded.engine).unwrap();
        let status = oracle
            .loaded
            .engine
            .input_runtime_ownership
            .key_lock_status(0)
            .unwrap();
        assert_eq!(
            status.request_id,
            ticket.key_lock_request_id_for_test().unwrap()
        );
        assert_eq!(status.state, "error");
        assert!(!status.effective && !status.ready && status.error.is_some());
        assert!(!oracle.loaded.callback.mixer.key_lock_for_measurement(0));
        assert!(oracle.loaded.sample().same_window(&bank));
        assert!(
            voice(&oracle.loaded.callback.mixer)
                .source_playback
                .matches_exact(&before)
        );
        for frames in [1, 127, 384, 512, 777] {
            oracle.render(frames);
        }
        assert_eq!(voice(&oracle.loaded.callback.mixer).generation, generation);
    }
    for paused in [false, true] {
        for after_warming in [false, true] {
            let mut oracle = ModeOracle::new(false);
            oracle.render(77);
            if paused {
                oracle.paused_mode(true, 257);
            } else {
                oracle.mode(true);
            }
            if after_warming {
                // Exactly one Raw Native block warms, while the callback still emits dry.
                let block = oracle.raw.block_size();
                oracle.render(block);
                assert!(oracle.raw.used);
                assert!(
                    !oracle
                        .loaded
                        .engine
                        .input_runtime_ownership
                        .key_lock_status(0)
                        .unwrap()
                        .ready
                );
            }
            let before = voice(&oracle.loaded.callback.mixer).source_playback;
            let generation = voice(&oracle.loaded.callback.mixer).generation;
            voice(&oracle.loaded.callback.mixer)
                .stretch
                .fail_preparation_worker();
            // The reference keeps the previous effective dry mode, including its live EQ history.
            oracle.complete.set_pad_key_lock(0, false);
            oracle.wet = false;
            oracle.dry_warm_remaining = 0;
            for frames in [1, 127, 384, 512, 777] {
                oracle.render(frames);
            }
            let status = oracle
                .loaded
                .engine
                .input_runtime_ownership
                .key_lock_status(0)
                .unwrap();
            assert_eq!(status.state, "error");
            assert!(!status.effective && !status.ready);
            assert!(!oracle.loaded.callback.mixer.key_lock_for_measurement(0));
            assert_eq!(voice(&oracle.loaded.callback.mixer).generation, generation);
            assert_eq!(
                voice(&oracle.loaded.callback.mixer)
                    .source_playback
                    .position(),
                before.position_at(1801)
            );
        }
    }
}

#[test]
fn actual_stopped_armed_on_guards_first_play_wet_and_preserves_dry_raw_eq_after_failure() {
    for stop_while_armed in [false, true] {
        for fault in [
            None,
            Some((false, false)),
            Some((false, true)),
            Some((true, false)),
            Some((true, true)),
        ] {
            let mut oracle = ModeOracle::new(false);
            oracle.render(77);
            let ticket = oracle.arm_stopped_on_and_start(stop_while_armed);
            let own = ticket.key_lock_request_id_for_test().unwrap();
            let source = oracle.loaded.sample();
            let generation = voice(&oracle.loaded.callback.mixer).generation;
            if let Some((exhausted, after_warming)) = fault {
                if after_warming {
                    let block = oracle.raw.block_size();
                    oracle.render(block);
                    assert!(oracle.raw.used);
                    let status = oracle
                        .loaded
                        .engine
                        .input_runtime_ownership
                        .key_lock_status(0)
                        .unwrap();
                    assert_eq!(status.request_id, own);
                    assert!(!status.effective && !status.ready && status.state == "waiting");
                }
                let before = voice(&oracle.loaded.callback.mixer).source_playback;
                if exhausted {
                    voice_mut(&mut oracle.loaded.callback.mixer)
                        .stretch
                        .exhaust_warmed_reserve();
                } else {
                    voice(&oracle.loaded.callback.mixer)
                        .stretch
                        .fail_preparation_worker();
                }
                assert!(
                    voice(&oracle.loaded.callback.mixer)
                        .source_playback
                        .matches_exact(&before)
                );
                // Keep the complete-PCM/algebraic/EQ reference on the dry baseline.
                // Failure must not reset the history already emitted during warmup.
                oracle.complete.set_pad_key_lock(0, false);
                oracle.wet = false;
                oracle.dry_warm_remaining = 0;
                for frames in [1, 127, 384, 512, 777] {
                    oracle.render(frames);
                }
                let status = oracle
                    .loaded
                    .engine
                    .input_runtime_ownership
                    .key_lock_status(0)
                    .unwrap();
                assert_eq!(status.request_id, own);
                assert_eq!(status.window_revision, source.window_revision());
                assert_eq!(status.source_address, source.source_address());
                assert!(!status.effective && !status.ready && status.state == "error");
                assert!(status.error.is_some());
                assert!(!oracle.loaded.callback.mixer.key_lock_for_measurement(0));
            } else {
                // Preparation ACK alone cannot certify this armed voice. Feed the first
                // independent Raw Native block dry, then verify the actual wet boundary.
                oracle.prepare();
                let remaining = oracle.raw.block_size() - 1;
                oracle.render(remaining);
                let status = oracle
                    .loaded
                    .engine
                    .input_runtime_ownership
                    .key_lock_status(0)
                    .unwrap();
                assert_eq!(status.request_id, own);
                assert!(!status.effective && !status.ready && status.state == "waiting");
                oracle.render(1);
                let status = oracle
                    .loaded
                    .engine
                    .input_runtime_ownership
                    .key_lock_status(0)
                    .unwrap();
                assert_eq!(status.request_id, own);
                assert!(status.effective && status.ready && status.state == "wet");
                oracle.through_adoption();
                assert_eq!(oracle.adopted, 1);
                assert!(oracle.difference > 0.02);
                let status = oracle
                    .loaded
                    .engine
                    .input_runtime_ownership
                    .key_lock_status(0)
                    .unwrap();
                assert_eq!(status.request_id, own);
                assert!(status.effective && status.ready && status.state == "wet");
            }
            assert_eq!(voice(&oracle.loaded.callback.mixer).generation, generation);
            assert!(oracle.loaded.sample().same_window(&source));
            assert!(ticket.is_current());
        }
    }
}

#[test]
fn actual_owned_on_retrigger_rearms_first_wet_and_failed_preparation_keeps_new_dry_raw_eq() {
    for fault in [
        None,
        Some((false, false)),
        Some((false, true)),
        Some((true, false)),
        Some((true, true)),
    ] {
        let mut oracle = ModeOracle::new(true);
        oracle.prepare();
        oracle.through_adoption();
        assert_eq!(oracle.adopted, 1);
        let own = oracle.retrigger_owned_on();
        let source = oracle.loaded.sample();
        let generation = voice(&oracle.loaded.callback.mixer).generation;
        if let Some((exhausted, after_warming)) = fault {
            if after_warming {
                oracle.render(oracle.raw.block_size());
                assert!(oracle.raw.used);
                let status = oracle
                    .loaded
                    .engine
                    .input_runtime_ownership
                    .key_lock_status(0)
                    .unwrap();
                assert_eq!(status.request_id, own);
                assert!(!status.effective && !status.ready && status.state == "waiting");
            }
            let before = voice(&oracle.loaded.callback.mixer).source_playback;
            if exhausted {
                voice_mut(&mut oracle.loaded.callback.mixer)
                    .stretch
                    .exhaust_warmed_reserve();
            } else {
                voice(&oracle.loaded.callback.mixer)
                    .stretch
                    .fail_preparation_worker();
            }
            assert!(
                voice(&oracle.loaded.callback.mixer)
                    .source_playback
                    .matches_exact(&before)
            );
            oracle.complete.set_pad_key_lock(0, false);
            oracle.wet = false;
            oracle.dry_warm_remaining = 0;
            for frames in [1, 127, 384, 512, 777] {
                oracle.render(frames);
            }
            let status = oracle
                .loaded
                .engine
                .input_runtime_ownership
                .key_lock_status(0)
                .unwrap();
            assert_eq!(status.request_id, own);
            assert_eq!(status.window_revision, source.window_revision());
            assert_eq!(status.source_address, source.source_address());
            assert!(!status.effective && !status.ready && status.state == "error");
            assert!(status.error.is_some());
            assert!(!oracle.loaded.callback.mixer.key_lock_for_measurement(0));
        } else {
            oracle.prepare();
            oracle.render(oracle.raw.block_size() - 1);
            let waiting = oracle
                .loaded
                .engine
                .input_runtime_ownership
                .key_lock_status(0)
                .unwrap();
            assert_eq!(waiting.request_id, own);
            assert!(!waiting.effective && !waiting.ready && waiting.state == "waiting");
            oracle.render(1);
            let wet = oracle
                .loaded
                .engine
                .input_runtime_ownership
                .key_lock_status(0)
                .unwrap();
            assert_eq!(wet.request_id, own);
            assert!(wet.effective && wet.ready && wet.state == "wet");
            oracle.through_adoption();
            assert_eq!(oracle.adopted, 2);
            assert!(oracle.difference > 0.02);
        }
        assert_eq!(voice(&oracle.loaded.callback.mixer).generation, generation);
        assert!(oracle.loaded.sample().same_window(&source));
    }

    // A dirty predecessor must not change this generation's first wet boundary when
    // its first callback is a whole Native block rather than irregular subdivisions.
    let mut left = ModeOracle::new(true);
    left.prepare();
    left.through_adoption();
    let left_own = left.retrigger_owned_on();
    let block = left.raw.block_size();
    let mut expected = left.render(block);
    let waiting = left
        .loaded
        .engine
        .input_runtime_ownership
        .key_lock_status(0)
        .unwrap();
    assert_eq!(waiting.request_id, left_own);
    assert!(!waiting.effective && !waiting.ready && waiting.state == "waiting");
    expected.extend(left.render(1));
    expected.extend(left.render(1023));
    let mut right = ModeOracle::new(true);
    right.prepare();
    right.through_adoption();
    let right_own = right.retrigger_owned_on();
    assert_eq!(right.raw.block_size(), block);
    let mut actual = Vec::new();
    for frames in [1, 127, block - 128] {
        actual.extend(right.render(frames));
        let waiting = right
            .loaded
            .engine
            .input_runtime_ownership
            .key_lock_status(0)
            .unwrap();
        assert_eq!(waiting.request_id, right_own);
        assert!(!waiting.effective && !waiting.ready && waiting.state == "waiting");
    }
    for frames in [1, 31, 257, 735] {
        actual.extend(right.render(frames));
        let wet = right
            .loaded
            .engine
            .input_runtime_ownership
            .key_lock_status(0)
            .unwrap();
        assert_eq!(wet.request_id, right_own);
        assert!(wet.effective && wet.ready && wet.state == "wet");
    }
    assert_eq!(
        actual, expected,
        "owned retrigger first wet output moved with callback partitions"
    );
    assert!(left.difference > 0.02 && right.difference > 0.02);
}

#[test]
fn actual_mode_first_wet_boundary_is_independent_of_callback_partition_and_late_failure_keeps_wet()
{
    let mut left = ModeOracle::new(false);
    left.render(77);
    left.mode(true);
    let mut expected = Vec::new();
    for frames in [1024, 1024, 256] {
        expected.extend(left.render(frames));
    }
    let status = left
        .loaded
        .engine
        .input_runtime_ownership
        .key_lock_status(0)
        .unwrap();
    assert!(status.effective && status.ready && status.state == "wet");
    let mut right = ModeOracle::new(false);
    right.render(77);
    right.mode(true);
    let mut actual = Vec::new();
    for frames in [1, 127, 384, 96, 257, 512, 31, 512, 384] {
        actual.extend(right.render(frames));
    }
    assert_eq!(actual.len(), expected.len());
    assert_eq!(
        actual, expected,
        "first wet output moved with callback partitions"
    );
    voice(&right.loaded.callback.mixer)
        .stretch
        .fail_preparation_worker();
    for frames in [31, 127, 512] {
        right.render(frames);
    }
    let status = right
        .loaded
        .engine
        .input_runtime_ownership
        .key_lock_status(0)
        .unwrap();
    assert!(status.effective && !status.ready && status.state == "error");
    assert!(right.loaded.callback.mixer.key_lock_for_measurement(0));
}

#[test]
fn actual_legacy_mode_command_does_not_relabel_prior_own_request_or_retain_its_error() {
    let mut oracle = ModeOracle::new(false);
    oracle.render(77);
    let tracked = oracle.mode(true);
    let own = tracked.key_lock_request_id_for_test().unwrap();
    assert_eq!(
        oracle
            .loaded
            .engine
            .input_runtime_ownership
            .key_lock_status(0)
            .unwrap()
            .request_id,
        own
    );
    for after_failure in [false, true] {
        if after_failure {
            voice(&oracle.loaded.callback.mixer)
                .stretch
                .fail_preparation_worker();
            let ticket = oracle.loaded.prepare(WindowRequest {
                key_lock: Some(true),
                ..Default::default()
            });
            oracle.loaded.pending(&ticket);
            wait_until(Duration::from_secs(5), || {
                oracle.loaded.callback.drain(&mut oracle.loaded.consumer);
                ticket.publication_status() == "rejected"
            });
            reconcile(&oracle.loaded.engine).unwrap();
            let status = oracle
                .loaded
                .engine
                .input_runtime_ownership
                .key_lock_status(0)
                .unwrap();
            assert_eq!(
                status.request_id,
                ticket.key_lock_request_id_for_test().unwrap()
            );
            assert_eq!(status.state, "error");
        }
        let before = voice(&oracle.loaded.callback.mixer).source_playback;
        oracle
            .loaded
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::SetPadKeyLock {
                id: 0,
                enabled: false,
            })
            .unwrap();
        assert_eq!(oracle.loaded.callback.drain(&mut oracle.loaded.consumer), 1);
        let status = oracle
            .loaded
            .engine
            .input_runtime_ownership
            .key_lock_status(0)
            .unwrap();
        assert_eq!(status.request_id, 0);
        assert!(
            status.ready && !status.effective && status.state == "dry" && status.error.is_none()
        );
        assert!(
            voice(&oracle.loaded.callback.mixer)
                .source_playback
                .matches_exact(&before)
        );
        oracle.complete.set_pad_key_lock(0, false);
        oracle.wet = false;
        oracle.dry_warm_remaining = 0;
        for frames in [1, 127, 384, 512] {
            oracle.render(frames);
        }
    }
}

#[test]
fn actual_neutral_origin_queued_speed_and_bpm_targets_guard_first_wet_and_failure_with_raw_eq() {
    use crate::audio_engine::audio_stream::drain_parameter_messages;
    use crate::messages::ControlParameterMessage;
    for bpm in [false, true] {
        for paused in [false, true] {
            for fail_before_wet in [false, true] {
                let mut oracle = ModeOracle::new_at_ratio(false, 1.0);
                oracle.render(777);
                if paused {
                    for mixer in [&mut oracle.loaded.callback.mixer, oracle.complete.as_mut()] {
                        mixer.pause_sample_at_output_frame(0, oracle.frame);
                    }
                }
                let before = voice(&oracle.loaded.callback.mixer).source_playback;
                let generation = voice(&oracle.loaded.callback.mixer).generation;
                let (mut parameters, mut receiver) = rtrb::RingBuffer::new(4);
                if bpm {
                    parameters
                        .push(ControlParameterMessage::SetLegacyPadBpm {
                            id: 0,
                            bpm: Some(120.0),
                            through_epoch: 0,
                        })
                        .unwrap();
                    parameters
                        .push(ControlParameterMessage::SetMasterPeriod(0.5 / 0.73))
                        .unwrap();
                    // This discrete command precedes the actual mode WindowWork transaction in
                    // the same production callback control queue; parameters drain first.
                    oracle
                        .loaded
                        .producer
                        .lock()
                        .unwrap()
                        .push(ControlMessage::SetBpmLock(true))
                        .unwrap();
                    oracle.complete.set_pad_bpm(0, Some(120.0));
                    oracle.complete.set_master_period(0.5 / 0.73);
                    oracle.complete.set_bpm_lock(true);
                } else {
                    parameters
                        .push(ControlParameterMessage::SetSpeed(0.73))
                        .unwrap();
                    oracle.complete.set_speed(0.73);
                }
                drain_parameter_messages(
                    &mut receiver,
                    &mut oracle.loaded.callback.mixer,
                    &mut oracle.loaded.callback.transport,
                );
                assert!(
                    receiver.is_empty(),
                    "queued parameters were not actually drained"
                );
                oracle.clock.set_target(0.73);
                assert!(
                    voice(&oracle.loaded.callback.mixer)
                        .source_playback
                        .matches_exact(&before)
                );
                let ticket = oracle.mode(true);
                assert_eq!(
                    voice(&oracle.loaded.callback.mixer)
                        .source_playback
                        .tempo_ratio(),
                    1.0
                );
                assert!(
                    voice(&oracle.loaded.callback.mixer)
                        .source_playback
                        .matches_exact(&before)
                );
                let status = oracle
                    .loaded
                    .engine
                    .input_runtime_ownership
                    .key_lock_status(0)
                    .unwrap();
                assert_eq!(
                    status.request_id,
                    ticket.key_lock_request_id_for_test().unwrap()
                );
                if paused {
                    assert!(status.effective && status.ready && status.state == "armed");
                    let mut silence = vec![0.0; 257 * 2];
                    oracle.loaded.callback.mixer.render_rt_at_output_frame(
                        &mut silence,
                        &mut [0.0; NUM_SAMPLES],
                        oracle.frame,
                        &mut oracle.loaded.callback.retirement,
                    );
                    assert!(silence.iter().all(|sample| *sample == 0.0));
                    oracle.complete.render_at_output_frame(
                        oracle.frame,
                        &mut silence,
                        &mut [0.0; NUM_SAMPLES],
                    );
                    assert!(
                        voice(&oracle.loaded.callback.mixer)
                            .source_playback
                            .matches_exact(&before)
                    );
                    oracle.frame += 257;
                    for mixer in [&mut oracle.loaded.callback.mixer, oracle.complete.as_mut()] {
                        mixer.resume_sample_at_output_frame(0, oracle.frame);
                    }
                    let waiting = oracle
                        .loaded
                        .engine
                        .input_runtime_ownership
                        .key_lock_status(0)
                        .unwrap();
                    assert_eq!(waiting.request_id, status.request_id);
                    assert!(!waiting.effective && !waiting.ready && waiting.state == "waiting");
                } else {
                    assert!(
                        !status.effective && !status.ready && status.state == "waiting",
                        "queued target was incorrectly certified as a neutral armed mode"
                    );
                }
                if fail_before_wet {
                    voice(&oracle.loaded.callback.mixer)
                        .stretch
                        .fail_preparation_worker();
                    oracle.complete.set_pad_key_lock(0, false);
                    oracle.wet = false;
                    oracle.dry_warm_remaining = 0;
                    for frames in [1, 127, 384, 512, 777, 1024, 257] {
                        oracle.render(frames);
                    }
                    let status = oracle
                        .loaded
                        .engine
                        .input_runtime_ownership
                        .key_lock_status(0)
                        .unwrap();
                    assert!(!status.effective && !status.ready && status.state == "error");
                    assert!(!oracle.loaded.callback.mixer.key_lock_for_measurement(0));
                } else {
                    oracle.prepare();
                    oracle.through_adoption();
                    assert_eq!(oracle.adopted, 1);
                    assert!(oracle.difference > 0.02);
                    let status = oracle
                        .loaded
                        .engine
                        .input_runtime_ownership
                        .key_lock_status(0)
                        .unwrap();
                    assert!(status.effective && status.ready && status.state == "wet");
                }
                assert_eq!(voice(&oracle.loaded.callback.mixer).generation, generation);
            }
        }
    }
}
