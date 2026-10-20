//! Actual finite Seek WindowWork/ACK and independent one-shot Intro/Tail Native/EQ chronology.
use super::*;
use crate::audio_engine::input_runtime_binding::{InputRuntimeOwnership, capture};
use crate::audio_engine::mixer::prepared_native_mixer_tests::finite_lifecycle_tests::{
    AlgebraicClock, RawChronology, lifecycle_filter_after_cut,
};
use crate::audio_engine::resident_relocation::launch_with_producer;
use crate::audio_engine::source_reader::{
    ExplicitSeekMode, reset_tap_observation_for_test, tap_observation_for_test,
};

const START: usize = 24;
const END: usize = 96;

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

struct SeekOracle {
    loaded: Box<Loaded>,
    complete: Box<RtMixer>,
    source: SampleBuffer,
    clock: AlgebraicClock,
    raw: RawChronology,
    filter: crate::audio_engine::dsp::PerPadDspChain,
    frame: u64,
    dry_warm: usize,
    candidate: Option<(u64, usize, RawChronology)>,
    adoptions: usize,
    wet_audible: bool,
}

impl SeekOracle {
    fn new(ratio: f64) -> Self {
        let mut loaded = Box::new(Loaded::new());
        loaded.callback.mixer.set_speed(ratio);
        let ticket = loaded.prepare(WindowRequest {
            loop_region: Some((START as f64 / 48_000.0, Some(END as f64 / 48_000.0))),
            key_lock: Some(true),
            ..Default::default()
        });
        loaded.adopt(&ticket);
        // This cut/seek fixture starts with live OFF-to-ON preparation at frame zero.
        loaded.callback.mixer.set_pad_key_lock(0, false);
        assert!(!loaded.callback.mixer.key_lock_for_measurement(0));
        assert!(launch_with_producer(&loaded.engine, &ticket, false, 1, &loaded.producer).unwrap());
        let start = match loaded.consumer.peek().unwrap() {
            ControlMessage::CapturePadStartTarget(start) => start.clone(),
            _ => panic!("resident launch must retain its original target before prepared work"),
        };
        assert_eq!(loaded.callback.drain(&mut loaded.consumer), 2);
        assert_eq!(start.state(), "applied", "{:?}", start.reason());
        assert_eq!(start.original_frame(), Some(0));
        assert_eq!(start.execution_frame(), Some(0));
        assert_eq!(start.effective_frame(), Some(0));
        assert!(start.is_current() && ticket.is_current());
        assert!(loaded.callback.scheduler.is_empty());
        assert!(
            voice(&loaded.callback.mixer)
                .sample
                .as_ref()
                .unwrap()
                .same_window(&loaded.sample())
        );
        assert_eq!(
            voice(&loaded.callback.mixer).stretch.pending_fifo_frames(),
            (0, 0)
        );
        loaded.callback.mixer.set_pad_key_lock(0, true);
        assert!(loaded.callback.mixer.key_lock_for_measurement(0));
        // Both independent references originate in the original WAV integer vector.
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
                .flat_map(|sample| [*sample; 2])
                .collect::<Vec<_>>()
                .into(),
        }
        .with_complete_source(48_000);
        let mut complete = Box::new(RtMixer::new(2, 48_000.0));
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        ownership.publish_test_source(0, &full, 48_000, 1);
        complete.set_input_runtime_ownership(ownership);
        complete.load_sample(0, full);
        complete.set_pad_loop_region(0, START as f64 / 48_000.0, Some(END as f64 / 48_000.0));
        complete.set_speed(ratio);
        assert!(complete.play_sample_at_output_frame(0, 1.0, 0));
        complete.set_pad_key_lock(0, true);
        for mixer in [complete.as_mut(), &mut loaded.callback.mixer] {
            mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
        }
        let raw = RawChronology::new(ratio, false);
        let dry_warm = raw.block_size();
        Self {
            loaded,
            complete,
            source,
            clock: AlgebraicClock::physical(START, END, ratio),
            raw,
            filter: lifecycle_filter_after_cut(),
            frame: 0,
            dry_warm,
            candidate: None,
            adoptions: 0,
            wet_audible: false,
        }
    }

    fn drain(&mut self) -> usize {
        let previous = self.loaded.callback.transport.output_frame();
        assert!(self.frame >= previous);
        self.loaded
            .callback
            .transport
            .advance_by_rendered_frames((self.frame - previous) as usize);
        crate::audio_engine::audio_stream::drain_control_messages(
            &mut self.loaded.consumer,
            &mut self.loaded.callback.scheduler,
            self.frame,
            &mut self.loaded.callback.quantization,
            &mut self.loaded.callback.transport,
            &mut self.loaded.callback.mixer,
            &mut self.loaded.callback.feedback,
            &mut self.loaded.callback.retirement,
        )
    }

    fn render(&mut self, frames: usize) {
        let start = self.frame;
        reset_tap_observation_for_test();
        let mut actual = vec![0.0; frames * 2];
        self.loaded.callback.mixer.render_rt_at_output_frame(
            &mut actual,
            &mut [0.0; NUM_SAMPLES],
            start,
            &mut self.loaded.callback.retirement,
        );
        let taps = tap_observation_for_test();
        let sample = voice(&self.loaded.callback.mixer).sample.as_ref().unwrap();
        assert_eq!(taps.left_reads, frames * 2);
        assert_eq!(taps.missing_reads, 0);
        assert!(
            taps.min_frame
                .is_some_and(|frame| frame >= sample.resident_start())
        );
        assert!(
            taps.max_frame
                .is_some_and(|frame| frame < sample.resident_end())
        );
        assert!(self.loaded.callback.mixer.key_lock_for_measurement(0));
        assert!(
            voice(&self.loaded.callback.mixer)
                .stretch
                .productive_history()
                .is_some()
        );
        let mut expected = vec![0.0; frames * 2];
        self.complete
            .render_at_output_frame(start, &mut expected, &mut [0.0; NUM_SAMPLES]);
        assert_eq!(actual, expected, "complete Seek oracle at {start}");
        let mut dry_clock = self.clock.clone();
        let dry = dry_clock.dry(&self.source, frames);
        let prefix = self.candidate.as_ref().map_or(frames, |(target, _, _)| {
            target.saturating_sub(start).min(frames as u64) as usize
        });
        let mut upstream = self.raw.render(&self.source, &mut self.clock, prefix);
        if prefix < frames {
            let (_, address, raw) = self.candidate.take().unwrap();
            self.raw = raw;
            upstream.extend(
                self.raw
                    .render(&self.source, &mut self.clock, frames - prefix),
            );
            assert_eq!(
                voice(&self.loaded.callback.mixer)
                    .stretch
                    .native_state_address(),
                address
            );
            self.adoptions += 1;
        }
        let warm = frames.min(self.dry_warm);
        upstream[..warm].copy_from_slice(&dry[..warm]);
        self.dry_warm -= warm;
        for (offset, (stereo, sample)) in actual.chunks_exact(2).zip(upstream).enumerate() {
            self.filter.begin_frame();
            let expected = self.filter.process_sample(0, sample);
            for actual in stereo {
                assert!(
                    (*actual - expected).abs() <= 5.0e-5,
                    "independent Intro/Tail Raw Native/EQ {}: {actual} != {expected}",
                    start + offset as u64
                );
            }
        }
        assert!(
            voice(&self.loaded.callback.mixer)
                .source_playback
                .matches_exact(&voice(&self.complete).source_playback)
        );
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .stretch
                .pending_fifo_frames(),
            voice(&self.complete).stretch.pending_fifo_frames()
        );
        let (frame, fraction, explicit) = self.clock.physical_position(0);
        let position = voice(&self.loaded.callback.mixer)
            .source_playback
            .position();
        assert_eq!(position.frame, frame);
        assert!((position.fraction - fraction).abs() < 1.0e-9);
        assert_eq!(position.seek_mode != ExplicitSeekMode::Normal, explicit);
        if self.adoptions > 0 {
            self.wet_audible |= actual.iter().any(|value| value.abs() > 0.001);
        }
        self.frame += frames as u64;
    }

    fn prepare_native(&mut self) {
        assert!(self.candidate.is_none());
        self.clock.begin_chunk(1);
        let mut copied_clock = self.clock.clone();
        let mut raw = RawChronology::new(self.clock.ratio_for_test(), true);
        raw.render(&self.source, &mut copied_clock, 4096);
        let capture_frame = self.frame;
        self.render(1);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let actual_ready = voice_mut(&mut self.loaded.callback.mixer)
                .stretch
                .source_preparation_ready();
            let reference_ready = voice_mut(&mut self.complete)
                .stretch
                .source_preparation_ready();
            if actual_ready && reference_ready {
                break;
            }
            let active = voice(&self.loaded.callback.mixer);
            assert!(
                Instant::now() < deadline,
                "owned seek preparation missing: capture={capture_frame} output={} adoptions={} actual_ready={actual_ready} reference_ready={reference_ready} position={:?} ratio={} target={} paused={} source_range=({}, {}) history={:?}",
                self.frame,
                self.adoptions,
                active.source_playback.position(),
                active.source_playback.tempo_ratio(),
                active.source_playback.rate_target(),
                active.paused,
                active.sample.as_ref().unwrap().resident_start(),
                active.sample.as_ref().unwrap().resident_end(),
                active.stretch.productive_history(),
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let target = voice_mut(&mut self.loaded.callback.mixer)
            .stretch
            .prepared_target_output_frame()
            .unwrap();
        assert_eq!(target, capture_frame + 4096);
        assert_eq!(
            Some(target),
            voice_mut(&mut self.complete)
                .stretch
                .prepared_target_output_frame()
        );
        let address = voice_mut(&mut self.loaded.callback.mixer)
            .stretch
            .prepared_native_address();
        assert_ne!(
            address,
            voice(&self.loaded.callback.mixer)
                .stretch
                .native_state_address()
        );
        let taps = voice_mut(&mut self.loaded.callback.mixer)
            .stretch
            .prepared_tap_observation();
        let sample = voice(&self.loaded.callback.mixer).sample.as_ref().unwrap();
        if sample.resident_start() == 0 && sample.resident_end() == sample.frame_count() {
            // The explicit ALL complete-PCM exception has no finite coverage record.
            // Its real 4096-frame preparation is still checked by target/FIFO/native
            // adoption and independent complete PCM plus Raw Native/EQ suffixes.
            assert!(taps.is_none());
        } else {
            let taps = taps.expect("every genuinely finite preparation records executed taps");
            assert_eq!(taps.left_reads, 4096 * 2);
            assert!(taps.right_reads > 0);
            assert_eq!(taps.missing_reads, 0);
            assert!(
                taps.min_frame
                    .is_some_and(|frame| frame >= sample.resident_start())
            );
            assert!(
                taps.max_frame
                    .is_some_and(|frame| frame < sample.resident_end())
            );
        }
        assert_eq!(
            voice_mut(&mut self.loaded.callback.mixer)
                .stretch
                .prepared_fifo_frames(),
            voice_mut(&mut self.complete).stretch.prepared_fifo_frames()
        );
        self.candidate = Some((target, address, raw));
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
        assert!(self.wet_audible);
    }

    fn continue_owned_native(&mut self, frames: usize) {
        assert!(self.candidate.is_none());
        let initial_frame = self.frame;
        let active = voice(&self.loaded.callback.mixer);
        let native = active.stretch.native_state_address();
        let request = active.stretch.adopted_request_id();
        let generation = active.generation;
        let history = active.stretch.productive_history().unwrap();
        let primed = active.stretch.native_primed_output_frames();
        let prepared_taps = active.stretch.native_tap_observation();
        let reference = voice(&self.complete);
        let reference_native = reference.stretch.native_state_address();
        let reference_request = reference.stretch.adopted_request_id();
        let adoptions = self.adoptions;
        assert_ne!(native, 0);
        assert!(request.is_some() && reference_request.is_some());
        let mut step = 0;
        while self.frame - initial_frame < frames as u64 {
            let remaining = frames - (self.frame - initial_frame) as usize;
            // render() retains the full independent complete-PCM and Raw Native/
            // FIFO/EQ comparisons, including every physical interpolation tap.
            self.render([1, 127, 384, 96, 257, 512, 31][step % 7].min(remaining));
            let active = voice(&self.loaded.callback.mixer);
            let current = active.stretch.productive_history().unwrap();
            assert_eq!(active.stretch.native_state_address(), native);
            assert_eq!(active.stretch.adopted_request_id(), request);
            assert_eq!(active.generation, generation);
            assert_eq!(active.stretch.native_primed_output_frames(), primed);
            assert_eq!(active.stretch.native_tap_observation(), prepared_taps);
            assert_eq!(current.binding, history.binding);
            assert_eq!(
                current.fed_output_frames,
                history.fed_output_frames + self.frame - initial_frame
            );
            assert_eq!(
                (
                    current.next_position.frame,
                    current.next_position.fraction.to_bits(),
                    current.next_position.seek_mode
                ),
                (
                    active.source_playback.position().frame,
                    active.source_playback.position().fraction.to_bits(),
                    active.source_playback.position().seek_mode
                ),
                "the same actual Native history must consume every continued source frame"
            );
            assert!(
                !voice(&self.loaded.callback.mixer)
                    .stretch
                    .source_preparation_pending()
            );
            assert!(!voice(&self.complete).stretch.source_preparation_pending());
            assert!(
                !voice_mut(&mut self.loaded.callback.mixer)
                    .stretch
                    .source_preparation_ready()
            );
            assert!(
                !voice_mut(&mut self.complete)
                    .stretch
                    .source_preparation_ready()
            );
            assert_eq!(
                voice(&self.complete).stretch.native_state_address(),
                reference_native
            );
            assert_eq!(
                voice(&self.complete).stretch.adopted_request_id(),
                reference_request
            );
            assert!(self.candidate.is_none());
            assert_eq!(self.adoptions, adoptions);
            step += 1;
        }
        assert!(self.wet_audible);
    }

    fn seek(&mut self, target: usize, resident: bool) {
        let previous = voice(&self.loaded.callback.mixer)
            .sample
            .as_ref()
            .unwrap()
            .clone();
        let bank = self.loaded.sample();
        let generation = voice(&self.loaded.callback.mixer).generation;
        let before = capture(&self.loaded.engine, 0).unwrap().unwrap();
        let ticket = self.loaded.prepare(WindowRequest {
            seek_position_s: Some(target as f64 / 48_000.0),
            ..Default::default()
        });
        if !resident {
            wait_until(Duration::from_secs(5), || {
                if matches!(
                    self.loaded.consumer.peek(),
                    Ok(ControlMessage::CaptureResidentSeek(_))
                ) {
                    self.drain();
                }
                ticket.publication_status() == "pending" && !self.loaded.consumer.is_empty()
            });
            let read = ticket.read_observation_for_test().unwrap();
            let expected_range = if target < START {
                (0, END)
            } else if target >= END {
                (START, self.source.frame_count())
            } else {
                (START, END)
            };
            assert_eq!(
                (read.source.start_frame, read.source.end_frame),
                expected_range
            );
            assert_eq!(
                read.source.read_bytes,
                ((expected_range.1 - expected_range.0) * 2 * std::mem::size_of::<f32>()) as u64
            );
            assert_eq!(read.source.allocated_bytes, read.source.read_bytes as usize);
            assert_eq!(read.stems.read_bytes, [0; 5]);
            assert!(read.admitted_peak_bytes < crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES);
            self.loaded.callback.retirement.capacity = 0;
            assert_eq!(self.drain(), 0);
            assert_eq!(ticket.publication_status(), "pending");
            assert!(
                voice(&self.loaded.callback.mixer)
                    .sample
                    .as_ref()
                    .unwrap()
                    .same_window(&previous)
            );
            self.loaded.callback.retirement.capacity = usize::MAX;
        }
        wait_until(Duration::from_secs(5), || {
            self.drain();
            ticket.publication_status() == "accepted"
        });
        assert_eq!(
            ticket.publication_status(),
            "accepted",
            "{:?}",
            ticket.error().unwrap()
        );
        reconcile(&self.loaded.engine).unwrap();
        assert!(ticket.is_current());
        assert_eq!(
            ticket.effective_seek_seconds(),
            Some(target.min(self.source.frame_count()) as f64 / 48_000.0)
        );
        assert!(
            self.complete
                .seek_sample_at_output_frame(0, target as f64 / 48_000.0, self.frame)
        );
        assert_ne!(voice(&self.loaded.callback.mixer).generation, generation);
        assert!(
            voice(&self.loaded.callback.mixer)
                .stretch
                .productive_history()
                .is_none()
        );
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .stretch
                .pending_fifo_frames(),
            (0, 0)
        );
        assert!(
            self.loaded
                .callback
                .mixer
                .dsp_source_history_for_test(0)
                .is_none()
        );
        let old_voice = !voice(&self.loaded.callback.mixer)
            .sample
            .as_ref()
            .unwrap()
            .same_source(&bank);
        if old_voice || resident {
            assert!(self.loaded.sample().same_window(&bank));
        } else {
            assert!(!before.current());
            let fresh = capture(&self.loaded.engine, 0).unwrap().unwrap();
            assert!(fresh.current() && fresh.available());
            assert_eq!(
                fresh.binding.resident,
                self.loaded.sample().resident_binding()
            );
        }
        self.clock.seek_physical(target, self.source.frame_count());
        self.raw = RawChronology::new(self.clock.ratio_for_test(), false);
        // This is an explicit cut under already effective ON, not the initial pending ON.
        // Native's ordinary zero adapter prefix reaches the newly reset EQ.
        self.dry_warm = 0;
        self.filter = lifecycle_filter_after_cut();
        self.candidate = None;
        self.wet_audible = false;
    }

    fn paused_gap(&mut self, frames: usize) {
        // Seek retains the original exclusive source-end intent before rendering.
        // A paused render may configure its copied domain and normalize that
        // representation, but must consume no active frames or Native/filter input.
        let expected = |mixer: &RtMixer| {
            let active = voice(mixer);
            let sample = active.sample.as_ref().unwrap();
            let region = active.source_loop_region.unwrap_or_else(|| {
                let (start, end) = mixer.loop_region_frames(0);
                crate::audio_engine::source_reader::effective_loop_region(
                    start,
                    end,
                    sample.frame_count(),
                )
                .unwrap()
            });
            let mut playback = active.source_playback;
            playback.configure_domain(
                active
                    .source_timing
                    .loop_domain(sample.frame_count(), region),
            );
            playback
        };
        let position = expected(&self.loaded.callback.mixer);
        let complete_position = expected(&self.complete);
        let native = voice(&self.loaded.callback.mixer)
            .stretch
            .native_state_address();
        let fifo = voice(&self.loaded.callback.mixer)
            .stretch
            .pending_fifo_frames();
        let request = voice(&self.loaded.callback.mixer)
            .stretch
            .adopted_request_id();
        let history = voice(&self.loaded.callback.mixer)
            .stretch
            .productive_history()
            .map(|history| (history.binding, history.fed_output_frames));
        for mixer in [&mut self.loaded.callback.mixer, self.complete.as_mut()] {
            mixer.pause_sample_at_output_frame(0, self.frame);
            let mut output = vec![0.0; frames * 2];
            mixer.render_at_output_frame(self.frame, &mut output, &mut [0.0; NUM_SAMPLES]);
            assert!(output.iter().all(|sample| *sample == 0.0));
        }
        assert!(
            voice(&self.loaded.callback.mixer)
                .source_playback
                .matches_exact(&position)
        );
        assert!(
            voice(&self.complete)
                .source_playback
                .matches_exact(&complete_position)
        );
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .stretch
                .pending_fifo_frames(),
            fifo
        );
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .stretch
                .adopted_request_id(),
            request
        );
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .stretch
                .productive_history()
                .map(|history| (history.binding, history.fed_output_frames)),
            history,
            "paused output must feed no samples into the existing Native history"
        );
        self.frame += frames as u64;
        for mixer in [&mut self.loaded.callback.mixer, self.complete.as_mut()] {
            mixer.resume_sample_at_output_frame(0, self.frame);
        }
    }

    fn physical_edit(&mut self, start: usize, end: Option<usize>) {
        let prior_native = voice(&self.loaded.callback.mixer)
            .stretch
            .native_state_address();
        let prior_fifo = voice(&self.loaded.callback.mixer)
            .stretch
            .pending_fifo_frames();
        let prior_history = voice(&self.loaded.callback.mixer)
            .stretch
            .productive_history()
            .unwrap();
        let prior_position = voice(&self.loaded.callback.mixer)
            .source_playback
            .position();
        let ticket = self.loaded.prepare(WindowRequest {
            loop_region: Some((
                start as f64 / 48_000.0,
                end.map(|end| end as f64 / 48_000.0),
            )),
            ..Default::default()
        });
        wait_until(Duration::from_secs(5), || {
            ticket.publication_status() == "pending" && !self.loaded.consumer.is_empty()
        });
        let read = ticket.read_observation_for_test().unwrap();
        assert_eq!(
            (read.source.start_frame, read.source.end_frame),
            (start, end.unwrap_or(self.source.frame_count()))
        );
        self.loaded.callback.retirement.capacity = 0;
        assert_eq!(self.drain(), 0);
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .stretch
                .native_state_address(),
            prior_native
        );
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .stretch
                .pending_fifo_frames(),
            prior_fifo
        );
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .source_playback
                .position(),
            prior_position
        );
        assert_eq!(
            voice(&self.loaded.callback.mixer)
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            prior_history.fed_output_frames
        );
        self.loaded.callback.retirement.capacity = usize::MAX;
        wait_until(Duration::from_secs(5), || {
            self.drain();
            ticket.publication_status() == "accepted"
        });
        reconcile(&self.loaded.engine).unwrap();
        assert!(ticket.is_current());
        assert_eq!(
            self.loaded.callback.mixer.loop_region_frames(0),
            (start, end)
        );
        assert_eq!(
            (
                self.loaded.sample().resident_start(),
                self.loaded.sample().resident_end()
            ),
            (start, end.unwrap_or(self.source.frame_count()))
        );
        self.complete.set_pad_loop_region(
            0,
            start as f64 / 48_000.0,
            end.map(|end| end as f64 / 48_000.0),
        );
        let cut = self
            .clock
            .edit_physical(start, end.unwrap_or(self.source.frame_count()));
        if cut {
            self.raw = RawChronology::new(self.clock.ratio_for_test(), false);
            self.dry_warm = 0;
            self.filter = lifecycle_filter_after_cut();
        } else {
            assert_eq!(
                voice(&self.loaded.callback.mixer)
                    .stretch
                    .native_state_address(),
                prior_native
            );
            assert_eq!(
                voice(&self.loaded.callback.mixer)
                    .stretch
                    .pending_fifo_frames(),
                prior_fifo
            );
        }
        self.candidate = None;
        self.wet_audible = false;
    }

    fn replace_complete_bank(&mut self) -> SampleBuffer {
        // A stays genuinely finite/wet. B uses the retained explicit complete cold-load
        // exception; this is no acceptance of finite initial KEYLOCK loading (P4b/P5b).
        let replacement = self
            .loaded
            .directory
            .path()
            .join("wet-complete-replacement.wav");
        write_pcm16(&replacement, 48_000, 1, &vec![8192_i16; 1000]);
        let (producer, mut load) = rtrb::RingBuffer::new(4);
        let request = admit_for_format_selected(
            &self.loaded.engine,
            0,
            replacement.to_string_lossy().into_owned(),
            (false, false, false, None),
            Arc::new(Mutex::new(producer)),
            (2, 48_000, self.loaded.directory.path().to_owned()),
        )
        .unwrap();
        wait_until(Duration::from_secs(10), || !load.is_empty());
        assert_eq!(self.loaded.callback.drain(&mut load), 1);
        let event = terminal(&self.loaded.engine, request);
        assert!(
            matches!(event, LoaderEvent::Success { .. }),
            "wet complete-B cold ACK: {event:?}"
        );
        wait_until(Duration::from_secs(10), || {
            self.loaded.engine.cold_loading[0].load(Ordering::Acquire) == 0
        });
        let bank = self.loaded.sample();
        assert_eq!((bank.resident_start(), bank.resident_end()), (0, 1000));
        assert!(
            voice(&self.loaded.callback.mixer)
                .sample
                .as_ref()
                .unwrap()
                .samples
                .len()
                < self.source.frame_count() * 2
        );
        bank
    }
}

pub(super) fn resident_seek_matrix() {
    for replace in [false, true] {
        let mut oracle = SeekOracle::new(0.73);
        oracle.prepare_native();
        oracle.through_adoption();
        if replace {
            oracle.replace_complete_bank();
        }
        oracle.seek(48, true);
        oracle.seek(48, true); // Same phase still discards Native/FIFO/filter and increments generation.
        oracle.prepare_native();
        oracle.through_adoption();
        oracle.seek(48, true);
        oracle.prepare_native();
        oracle.through_adoption();
        assert_eq!(oracle.adoptions, 3);
    }
}

pub(super) fn intro_tail_seek_matrix() {
    for ratio in [0.73, 1.97] {
        for target in [12, 251, 999] {
            let mut oracle = SeekOracle::new(ratio);
            oracle.prepare_native();
            oracle.through_adoption();
            oracle.seek(target, false);
            oracle.paused_gap(257);
            oracle.prepare_native();
            oracle.through_adoption();
            assert_eq!(oracle.adoptions, 2);
            let next_ratio = if ratio < 1.0 { 1.97 } else { 0.73 };
            oracle.loaded.callback.mixer.set_speed(next_ratio);
            oracle.complete.set_speed(next_ratio);
            oracle.clock.set_target(next_ratio);
            // A rate target and a paused gap preserve the exact productive feed;
            // neither is a new physical source cut requiring another worker.
            // Derive the complete .05-per-512-frame glide from declared inputs.
            let glide_frames = ((next_ratio - ratio).abs() / 0.05).ceil() as usize * 512;
            oracle.continue_owned_native(glide_frames + 1777);
            assert_eq!(oracle.clock.ratio_for_test(), next_ratio);
            assert_eq!(
                voice(&oracle.loaded.callback.mixer)
                    .source_playback
                    .tempo_ratio(),
                next_ratio
            );
            oracle.paused_gap(97);
            oracle.continue_owned_native(1777);
            assert_eq!(oracle.adoptions, 2);
        }
    }
}

pub(super) fn old_voice_seek_matrix() {
    for paused in [false, true] {
        let mut oracle = SeekOracle::new(0.73);
        oracle.prepare_native();
        oracle.through_adoption();
        let original_address = voice(&oracle.loaded.callback.mixer)
            .sample
            .as_ref()
            .unwrap()
            .source_address();
        let replacement = oracle.replace_complete_bank();
        if paused {
            oracle
                .loaded
                .callback
                .mixer
                .pause_sample_at_output_frame(0, oracle.frame);
            oracle
                .complete
                .pause_sample_at_output_frame(0, oracle.frame);
        }
        oracle.seek(251, false);
        assert!(oracle.loaded.sample().same_window(&replacement));
        assert_eq!(
            voice(&oracle.loaded.callback.mixer)
                .sample
                .as_ref()
                .unwrap()
                .source_address(),
            original_address
        );
        if paused {
            assert!(voice(&oracle.loaded.callback.mixer).paused);
            oracle
                .loaded
                .callback
                .mixer
                .resume_sample_at_output_frame(0, oracle.frame);
            oracle
                .complete
                .resume_sample_at_output_frame(0, oracle.frame);
        }
        oracle.prepare_native();
        oracle.through_adoption();
        assert_eq!(
            voice(&oracle.loaded.callback.mixer)
                .stretch
                .productive_history()
                .unwrap()
                .binding
                .source_address,
            original_address
        );
        oracle.seek(48, true);
        oracle.prepare_native();
        oracle.through_adoption();
        assert!(oracle.loaded.sample().same_window(&replacement));
        let old_reader = Arc::downgrade(
            &voice(&oracle.loaded.callback.mixer)
                .sample
                .as_ref()
                .unwrap()
                .samples,
        );
        assert!(old_reader.upgrade().is_some());
        oracle
            .loaded
            .callback
            .mixer
            .stop_sample_rt(0, &mut oracle.loaded.callback.retirement);
        wait_until(Duration::from_secs(5), || {
            let mut output = [0.0; 62];
            oracle.loaded.callback.mixer.render_rt_at_output_frame(
                &mut output,
                &mut [0.0; NUM_SAMPLES],
                oracle.frame,
                &mut oracle.loaded.callback.retirement,
            );
            old_reader.upgrade().is_none()
        });
    }
}

#[test]
fn actual_worker_finite_physical_edits_all_exception_and_return_keep_or_cut_real_wet_chronology() {
    for ratio in [0.73, 1.97] {
        let mut oracle = SeekOracle::new(ratio);
        oracle.prepare_native();
        oracle.through_adoption();
        oracle.physical_edit(16, Some(112)); // Continuity: current read remains inside new markers.
        oracle.prepare_native();
        oracle.through_adoption();
        oracle.physical_edit(128, Some(192)); // Genuine physical clamp: discard old FIFO and filter.
        oracle.prepare_native();
        oracle.through_adoption();
        oracle.physical_edit(0, None); // Existing explicitly admitted complete-track ALL exception.
        assert_eq!(
            oracle.loaded.sample().resident_end(),
            oracle.source.frame_count()
        );
        oracle.prepare_native();
        oracle.through_adoption();
        oracle.physical_edit(128, Some(192)); // Return to finite provisioning after that exception.
        assert!(oracle.loaded.sample().samples.len() < oracle.source.samples.len() * 2);
        oracle.prepare_native();
        oracle.through_adoption();
        let native = voice(&oracle.loaded.callback.mixer)
            .stretch
            .native_state_address();
        let origin = voice(&oracle.loaded.callback.mixer)
            .stretch
            .native_origin_tuple();
        let request = voice(&oracle.loaded.callback.mixer)
            .stretch
            .adopted_request_id();
        let fifo = voice(&oracle.loaded.callback.mixer)
            .stretch
            .pending_fifo_frames();
        let playback = voice(&oracle.loaded.callback.mixer).source_playback;
        let fed = voice(&oracle.loaded.callback.mixer)
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames;
        oracle.paused_gap(97);
        assert!(
            voice(&oracle.loaded.callback.mixer)
                .source_playback
                .matches_exact(&playback)
        );
        assert_eq!(
            voice(&oracle.loaded.callback.mixer)
                .stretch
                .pending_fifo_frames(),
            fifo
        );
        // Productive pause/resume retains its chronological Native/FIFO owner.
        // It fences unadopted work but must not manufacture a replacement candidate.
        let resumed = oracle.frame;
        let end = resumed + 4096 + 1777;
        let mut partition = 0;
        while oracle.frame < end {
            oracle.render(
                [1, 127, 384, 96, 257, 512, 31][partition % 7].min((end - oracle.frame) as usize),
            );
            partition += 1;
            let active = voice(&oracle.loaded.callback.mixer);
            assert_eq!(active.stretch.native_state_address(), native);
            assert_eq!(active.stretch.native_origin_tuple(), origin);
            assert_eq!(active.stretch.adopted_request_id(), request);
            assert_eq!(
                active
                    .stretch
                    .productive_history()
                    .unwrap()
                    .fed_output_frames,
                fed + oracle.frame - resumed
            );
            for mixer in [&mut oracle.loaded.callback.mixer, oracle.complete.as_mut()] {
                assert!(!voice(mixer).stretch.source_preparation_pending());
                assert!(!voice_mut(mixer).stretch.source_preparation_ready());
            }
        }
        assert!(oracle.wet_audible);
        assert_eq!(oracle.adoptions, 5);
    }
}

#[test]
fn actual_worker_fractional_marker_seam_changes_right_tap_and_cuts_native_fifo_filter() {
    let mut oracle = SeekOracle::new(0.73);
    oracle.prepare_native();
    oracle.through_adoption();
    oracle.physical_edit(16, Some(112));
    oracle.prepare_native();
    oracle.through_adoption();
    // Arrange an actual fractional next read at 100; shrinking the exclusive end to
    // 101 preserves its left tap and coordinate but replaces right tap 101 with 16.
    let mut found = false;
    for _ in 0..512 {
        let position = voice(&oracle.loaded.callback.mixer)
            .source_playback
            .position();
        if position.frame == 100 && position.fraction != 0.0 {
            found = true;
            break;
        }
        oracle.render(1);
    }
    assert!(found);
    assert!(
        voice(&oracle.loaded.callback.mixer)
            .stretch
            .pending_fifo_frames()
            .0
            > 0
    );
    let position = voice(&oracle.loaded.callback.mixer)
        .source_playback
        .position();
    oracle.physical_edit(16, Some(101));
    assert_eq!(
        voice(&oracle.loaded.callback.mixer)
            .source_playback
            .position()
            .frame,
        position.frame
    );
    oracle.prepare_native();
    oracle.through_adoption();
    assert_eq!(oracle.adoptions, 3);
}

#[test]
fn actual_worker_source_cut_reserve_failure_rejects_seek_and_physical_ack_before_changing_audio() {
    for seek in [false, true] {
        let mut oracle = SeekOracle::new(0.73);
        oracle.prepare_native();
        oracle.through_adoption();
        let source = oracle.loaded.sample();
        let playback = voice(&oracle.loaded.callback.mixer).source_playback;
        let generation = voice(&oracle.loaded.callback.mixer).generation;
        let native = voice(&oracle.loaded.callback.mixer)
            .stretch
            .native_state_address();
        let fifo = voice(&oracle.loaded.callback.mixer)
            .stretch
            .pending_fifo_frames();
        let history = voice(&oracle.loaded.callback.mixer)
            .stretch
            .productive_history()
            .unwrap();
        let filter = oracle
            .loaded
            .callback
            .mixer
            .dsp_source_history_for_test(0)
            .unwrap();
        voice(&oracle.loaded.callback.mixer)
            .stretch
            .fail_preparation_worker();
        let ticket = oracle.loaded.prepare(if seek {
            WindowRequest {
                seek_position_s: Some(48.0 / 48_000.0),
                ..Default::default()
            }
        } else {
            WindowRequest {
                loop_region: Some((128.0 / 48_000.0, Some(192.0 / 48_000.0))),
                ..Default::default()
            }
        });
        wait_until(Duration::from_secs(5), || {
            oracle.drain();
            !matches!(ticket.publication_status(), "preparing" | "pending")
        });
        assert_ne!(ticket.publication_status(), "accepted");
        assert!(!ticket.is_current());
        reconcile(&oracle.loaded.engine).unwrap();
        assert!(oracle.loaded.sample().same_window(&source));
        assert_eq!(
            oracle.loaded.callback.mixer.loop_region_frames(0),
            (START, Some(END))
        );
        assert!(
            voice(&oracle.loaded.callback.mixer)
                .source_playback
                .matches_exact(&playback)
        );
        assert_eq!(voice(&oracle.loaded.callback.mixer).generation, generation);
        assert_eq!(
            voice(&oracle.loaded.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        assert_eq!(
            voice(&oracle.loaded.callback.mixer)
                .stretch
                .pending_fifo_frames(),
            fifo
        );
        assert_eq!(
            voice(&oracle.loaded.callback.mixer)
                .stretch
                .productive_history()
                .unwrap()
                .binding,
            history.binding
        );
        assert_eq!(
            voice(&oracle.loaded.callback.mixer)
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            history.fed_output_frames
        );
        assert_eq!(
            oracle
                .loaded
                .callback
                .mixer
                .dsp_source_history_for_test(0)
                .unwrap()
                .fed_output_frames,
            filter.fed_output_frames
        );
        let mut continued = 0;
        for frames in [1, 127, 384, 96, 257, 512, 31, 901] {
            oracle.render(frames); // Complete PCM AND the unchanged independent Raw Native/EQ owner.
            continued += frames as u64;
            assert_eq!(
                voice(&oracle.loaded.callback.mixer)
                    .stretch
                    .native_state_address(),
                native
            );
            assert_eq!(
                voice(&oracle.loaded.callback.mixer)
                    .stretch
                    .productive_history()
                    .unwrap()
                    .fed_output_frames,
                history.fed_output_frames + continued
            );
        }
        assert!(oracle.wet_audible);
    }
}
