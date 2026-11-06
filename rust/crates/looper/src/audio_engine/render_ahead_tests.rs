//! Device-free render-ahead evidence at the real shared stream renderer.

use super::*;
use crate::audio_engine::buffer_retirement::close_callback_and_join;
use crate::audio_engine::global_rate_status::RateState;
use crate::audio_engine::input_runtime_binding::{
    InputRuntimeOwnership, enqueue_stop_with_producer,
};
use crate::audio_engine::mix_capture::publication::CapturePublication;
use crate::audio_engine::mix_capture::{
    CaptureConfig, CaptureControl, CaptureFailure, CaptureStatus, SPOOL_HEADER_BYTES,
    prepare_for_output,
};
use crate::audio_engine::native_render_observation::Scope;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::messages::{MixCaptureTarget, SampleBuffer};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[cfg(windows)]
#[path = "wasapi_submission_tests.rs"]
mod wasapi_submission_tests;

const RATE: u32 = 96_000;
const FRAMES: usize = 96;
const LEAD: usize = 6;
const PERIOD: Duration = Duration::from_millis(1);
const WAIT: Duration = Duration::from_secs(5);

struct Block {
    first: u64,
    pcm: [f32; FRAMES * 2],
}

struct Stream {
    owner: CallbackCloseOwner<AudioCallbackState>,
    retirement: AudioBufferRetirementWorker,
    controls: Producer<ControlMessage>,
    parameters: Producer<ControlParameterMessage>,
    _feedback: Consumer<AudioMessage>,
    ownership: Arc<InputRuntimeOwnership>,
}

impl Stream {
    fn new(sources: &[SampleBuffer], wet: bool) -> Self {
        Self::with_source_preparation(sources, wet, false)
    }

    fn with_source_preparation(sources: &[SampleBuffer], wet: bool, prepare: bool) -> Self {
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        let mut mixer = create_stream_mixer(
            2,
            RATE,
            Arc::default(),
            ownership.clone(),
            (0..NUM_SAMPLES).map(|_| Arc::default()).collect(),
        )
        .unwrap();
        if wet {
            mixer.set_speed(1.2);
        }
        for (id, sample) in sources.iter().enumerate() {
            ownership.publish_test_source(id, sample, RATE, 1);
            mixer.load_sample(id, sample.clone());
            mixer.set_pad_key_lock(id, wet);
            assert!(mixer.play_sample(id, 1.0));
        }
        // As in the continuous Native partition test, fence asynchronous
        // replacement to compare equivalent unused warm Native owners.
        if wet && !prepare {
            mixer.set_input_runtime_ownership(Arc::new(InputRuntimeOwnership::tracked()));
        }
        let (controls, control_rx) = RingBuffer::new(16);
        let (parameters, parameter_rx) = RingBuffer::new(16);
        let (feedback_tx, feedback_rx) = RingBuffer::new(128);
        let (owner, retirement) = create_stream_callback_owner(
            control_rx,
            parameter_rx,
            feedback_tx,
            mixer,
            RATE,
            Arc::new(SharedOutputClock::new()),
            Arc::default(),
        )
        .unwrap();
        Self {
            owner,
            retirement,
            controls,
            parameters,
            _feedback: feedback_rx,
            ownership,
        }
    }

    fn frame(&mut self) -> u64 {
        self.owner.state_mut().transport.output_frame()
    }

    fn render(&mut self) -> Block {
        let first = self.frame();
        let mut pcm = [0.0; FRAMES * 2];
        render_output_block(
            self.owner.state_mut(),
            &mut pcm,
            RenderFormat {
                channels: 2,
                sample_rate_hz: RATE,
            },
            &AtomicBool::new(false),
            None,
        );
        Block { first, pcm }
    }

    fn stop_pad(&mut self, id: usize) {
        assert!(enqueue_stop_with_producer(
            &self.ownership,
            &mut self.controls,
            Some(id)
        ));
    }

    fn change_rate(&mut self) {
        self.set_rate(1.3);
    }

    fn set_rate(&mut self, ratio: f64) {
        super::super::push_global_rate_message(
            &self.ownership.global_timing_revision,
            &self.ownership.global_timing_publishing,
            &self.ownership.global_rate_status,
            &mut self.parameters,
            Some(ratio),
            Some(0.4),
            "render-ahead probe rate",
        )
        .unwrap();
    }

    fn record(&mut self, first: u64, end: u64) -> Take {
        self.record_for_output(first, end, None)
    }

    fn record_for_output(
        &mut self,
        first: u64,
        end: u64,
        submission: Option<Arc<OutputSubmission>>,
    ) -> Take {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("take.capture");
        let (control, work) = prepare_for_output(
            &path,
            CaptureConfig {
                sample_rate_hz: RATE,
                channels: 2,
                block_frames: FRAMES,
                block_count: 4,
                max_pool_pcm_bytes: 128 * 1024,
            },
            submission,
        )
        .unwrap();
        let take = Take {
            control,
            path,
            directory: Some(directory),
        };
        let until = Instant::now() + WAIT;
        while !take.control.status().writer_ready {
            assert!(Instant::now() < until);
            std::thread::sleep(PERIOD);
        }
        let publication = Arc::new(CapturePublication::new(work));
        let session_id = publication.session_id();
        self.controls
            .push(ControlMessage::StartMixCapture {
                publication,
                target_frame: MixCaptureTarget::At(first),
            })
            .unwrap();
        self.controls
            .push(ControlMessage::StopMixCapture {
                session_id,
                target_frame: MixCaptureTarget::At(end),
            })
            .unwrap();
        take
    }
}

fn sources(count: usize) -> Vec<SampleBuffer> {
    (0..count)
        .map(|id| {
            SampleBuffer {
                channels: 2,
                residency: None,
                samples: (0..2 * RATE)
                    .flat_map(|frame| {
                        let phase = f64::from(frame) * (0.027 + id as f64 * 0.0061);
                        [0.08 * phase.sin() as f32, 0.06 * (phase + 0.2).cos() as f32]
                    })
                    .collect::<Vec<_>>()
                    .into(),
            }
            .with_complete_source(RATE)
        })
        .collect()
}

struct Take {
    control: CaptureControl,
    path: PathBuf,
    directory: Option<tempfile::TempDir>,
}

impl Take {
    fn joined(&mut self) -> CaptureStatus {
        let until = Instant::now() + WAIT;
        loop {
            let joined = self.control.join_if_finished().unwrap();
            let status = self.control.status();
            if joined && status.worker_joined && status.worker_closed && status.rt_released {
                return status;
            }
            assert!(Instant::now() < until, "{status:?}");
            std::thread::sleep(PERIOD);
        }
    }

    fn assert_recording(&mut self, block: &Block) {
        self.assert_recording_span(block, 17, 83);
    }

    fn assert_recording_span(&mut self, block: &Block, first: usize, end: usize) {
        let status = self.joined();
        let frames = end - first;
        assert!(status.successful_spool(), "{status:?}");
        assert_eq!(status.first_frame, Some(block.first + first as u64));
        assert_eq!(status.end_frame, Some(block.first + end as u64));
        assert_eq!(status.written_frames, frames as u64);
        let spool = std::fs::read(&self.path).unwrap();
        assert_eq!(spool.len(), SPOOL_HEADER_BYTES as usize + frames * 2 * 4);
        let actual = spool[SPOOL_HEADER_BYTES as usize..]
            .chunks_exact(4)
            .map(|bits| u32::from_le_bytes(bits.try_into().unwrap()));
        assert!(
            actual.eq(block.pcm[first * 2..end * 2]
                .iter()
                .map(|value| value.to_bits()))
        );
    }
}

impl Drop for Take {
    fn drop(&mut self) {
        let status = self.control.status();
        if !(status.worker_joined && status.rt_released)
            && let Some(directory) = self.directory.take()
        {
            eprintln!(
                "unsettled render-ahead capture retained: {}",
                directory.keep().display()
            );
        }
    }
}

#[test]
fn committed_mix_cannot_apply_new_stop_rate_or_capture_to_unconsumed_frames() {
    let samples = sources(2);
    let mut inline = Stream::new(&samples, false);
    let mut ahead = Stream::new(&samples, false);
    assert_eq!(inline.render().pcm, ahead.render().pcm);
    let consumed = inline.frame();
    let queued: Vec<_> = (0..LEAD).map(|_| ahead.render()).collect();
    assert_eq!(queued[0].first, consumed);
    let frontier = consumed + (LEAD * FRAMES) as u64;
    assert_eq!(ahead.frame(), frontier);

    // Same public ingress, published only after the prefix is committed.
    for stream in [&mut inline, &mut ahead] {
        stream.stop_pad(0);
        stream.change_rate();
    }
    let mut expected_capture = inline.record(consumed + 17, consumed + 83);
    let mut late_capture = ahead.record(consumed + 17, consumed + 83);
    let expected = inline.render();
    let too_late = ahead.render();
    assert_eq!(expected.first, consumed);
    assert_eq!(too_late.first, frontier);
    assert!(expected.pcm.iter().any(|value| value.abs() > 1.0e-3));
    assert_ne!(expected.pcm, queued[0].pcm);
    for (stream, frame) in [(&mut inline, consumed), (&mut ahead, frontier)] {
        let state = stream.owner.state_mut();
        assert!(!state.mixer.voices.iter().any(|v| v.is_playing_sample(0)));
        assert!(state.mixer.voices.iter().any(|v| v.is_playing_sample(1)));
        assert_eq!(state.transport.master_period_seconds(), Some(0.4));
        let effect = stream.ownership.global_rate_status.read().unwrap();
        assert_eq!(effect.request_revision, effect.effect.revision);
        assert_eq!(effect.effect.state, RateState::Applied);
        assert_eq!(effect.effect.execution_frame, Some(frame));
    }
    expected_capture.assert_recording(&expected);
    let late_status = late_capture.joined();
    assert!(!late_status.started);
    assert_eq!(late_status.failure, Some(CaptureFailure::Rejected));
    assert_eq!(late_status.written_frames, 0);

    // A pre-admitted take has exact PCM, but the current writer can finish
    // while its entire interval is still in the speculative output queue.
    let future = ahead.frame();
    let mut early_capture = ahead.record(future + 17, future + 83);
    let unconsumed = ahead.render();
    early_capture.assert_recording(&unconsumed);
    assert_eq!(inline.frame(), consumed + FRAMES as u64);
    assert!(unconsumed.first > inline.frame());
}

#[test]
fn wet_fifo_prefix_survives_same_owner_rate_but_cannot_cover_due_native_shift() {
    struct ReadyWet {
        slot: usize,
        generation: u64,
        native: usize,
        pcm: Vec<Vec<f32>>,
    }

    let samples = sources(2);
    let readers: Vec<_> = samples
        .iter()
        .map(|sample| Arc::downgrade(&sample.samples))
        .collect();
    let mut stream = Stream::with_source_preparation(&samples, true, true);
    stream.render();
    // Let the preparation worker publish both lanes before crossing their
    // original adoption frame. The render clock stays at 96 while waiting.
    let deadline = Instant::now() + WAIT;
    for voice in stream.owner.state_mut().mixer.voices.iter_mut().take(2) {
        assert!(voice.active);
        while !voice.stretch.source_preparation_ready() {
            assert!(
                Instant::now() < deadline,
                "source-owned Wet preparation timed out"
            );
            std::thread::sleep(PERIOD);
        }
        assert_eq!(voice.stretch.prepared_target_output_frame(), Some(4096));
    }

    // Warm beyond adoption, ending with 480 of 512 inputs and 31 ready outputs.
    const CUT: u64 = 69 * FRAMES as u64;
    while stream.frame() < CUT {
        stream.render();
    }
    assert_eq!(stream.frame(), CUT);
    let voices = &stream.owner.state_mut().mixer.voices;
    let [stopped, continuing] = [0, 1].map(|id| {
        let (index, voice) = voices
            .iter()
            .enumerate()
            .find(|(_, voice)| voice.is_playing_sample(id))
            .unwrap();
        let stretch = &voice.stretch;
        assert!(stretch.wet_current_source_output_ready());
        assert!(stretch.owns_pitch_contract(PitchIntent::default(), true));
        assert_eq!(stretch.rubberband_block_size(), 512);
        assert_eq!(stretch.pending_fifo_frames(), (480, 31));
        let pcm = stretch.copy_ready_wet_pcm_for_test();
        assert_eq!((pcm[0].len(), pcm[1].len()), (31, 31));
        assert!(
            pcm.iter()
                .all(|channel| channel.iter().any(|v| v.abs() > 0.001))
        );
        ReadyWet {
            slot: index,
            generation: voice.generation,
            native: stretch.native_identity(),
            pcm,
        }
    });
    assert_ne!(stopped.native, continuing.native);
    assert_ne!(stopped.pcm, continuing.pcm);

    let stopped_key = stream.ownership.callback_content_key(0).unwrap();
    let before_stop = stream.ownership.ordinary_snapshot(stopped_key).unwrap();
    stream.stop_pad(0);
    stream.change_rate();
    let mut take = stream.record(CUT + 17, CUT + 83);
    let scope = Scope::begin();
    let mixed = stream.render();
    let observation = scope.finish();
    assert_eq!(mixed.first, CUT);
    assert_eq!(stream.frame(), CUT + FRAMES as u64);
    let effect = stream.ownership.global_rate_status.read().unwrap();
    assert_eq!(effect.request_revision, effect.effect.revision);
    assert_eq!(effect.effect.state, RateState::Applied);
    assert_eq!(effect.effect.execution_frame, Some(CUT));
    let state = stream.owner.state_mut();
    assert_eq!(state.transport.master_period_seconds(), Some(0.4));
    let stopped_voice = &state.mixer.voices[stopped.slot];
    assert!(!stopped_voice.active);
    assert_ne!(stopped_voice.generation, stopped.generation);
    assert_eq!(stopped_voice.stretch.pending_fifo_frames(), (0, 0));
    let continuing_voice = &state.mixer.voices[continuing.slot];
    assert!(continuing_voice.is_playing_sample(1));
    assert_eq!(continuing_voice.generation, continuing.generation);
    assert_eq!(
        continuing_voice.stretch.native_identity(),
        continuing.native
    );
    assert_eq!(continuing_voice.source_playback.rate_target(), 1.3);
    assert_eq!(continuing_voice.source_playback.tempo_ratio(), 1.25);
    assert_eq!(continuing_voice.stretch.pending_fifo_frames(), (64, 447));
    assert_eq!(observation.shifts, 1);
    assert_eq!(observation.adapter_shortfall_channel_frames, 0);
    assert_eq!(observation.source_refusals, 0);
    let after_stop = stream.ownership.ordinary_snapshot(stopped_key).unwrap();
    assert_eq!(after_stop.kind, super::super::ordinary_transport::STOPPED);
    assert_eq!(after_stop.source, before_stop.source);
    assert!(after_stop.sequence > before_stop.sequence);
    assert!(!after_stop.active);

    // Both channels are neutral downstream. Only the continuing owner's
    // already-generated prefix can reach the final mix after STOP and rate.
    for (frame, stereo) in mixed.pcm.chunks_exact(2).take(31).enumerate() {
        assert_eq!(stereo[0].to_bits(), continuing.pcm[0][frame].to_bits());
        assert_eq!(stereo[1].to_bits(), continuing.pcm[1][frame].to_bits());
    }
    assert!(mixed.pcm[62..].iter().any(|sample| sample.abs() > 0.001));
    take.assert_recording(&mixed);
    drop(samples);
    close_callback_and_join(stream.owner, stream.retirement).unwrap();
    assert!(readers.iter().all(|reader| reader.upgrade().is_none()));
    eprintln!(
        "Wet FIFO cut at {CUT}: 31 retained frames, 65 from one due Native shift, capture [17,83) relative to cut"
    );
}

#[test]
fn wet_source_continuation_transfers_a_worker_shift_after_due_controls() {
    use crate::audio_engine::source_feed_history::SourceFeedSnapshot;

    const E: u64 = 6624;
    const F: u64 = E - FRAMES as u64;
    let samples = sources(2);
    let readers: Vec<_> = samples
        .iter()
        .map(|sample| Arc::downgrade(&sample.samples))
        .collect();

    for stale in [false, true] {
        let mut inline = Stream::with_source_preparation(&samples, true, true);
        let mut ahead = Stream::with_source_preparation(&samples, true, true);
        let mut owners = [[0; 2]; 2];
        let mut slots = [[0; 2]; 2];
        for (side, stream) in [&mut inline, &mut ahead].into_iter().enumerate() {
            stream.render();
            let deadline = Instant::now() + WAIT;
            for voice in stream.owner.state_mut().mixer.voices.iter_mut().take(2) {
                while !voice.stretch.source_preparation_ready() {
                    assert!(Instant::now() < deadline, "initial source preparation");
                    std::thread::sleep(PERIOD);
                }
            }
            while stream.frame() < F - FRAMES as u64 {
                stream.render();
            }
            stream.change_rate();
            stream.render();
            assert_eq!(stream.frame(), F);
            let mixer = &mut stream.owner.state_mut().mixer;
            for id in 0..2 {
                let index = mixer
                    .voices
                    .iter()
                    .position(|voice| voice.is_playing_sample(id))
                    .unwrap();
                slots[side][id] = index;
                owners[side][id] = mixer.voices[index].stretch.native_identity();
                let probe = if stale { side == 1 } else { id == 1 };
                if !probe {
                    continue;
                }
                mixer
                    .arm_wet_continuation_probe(index, F, E, side == 1)
                    .unwrap();
                let stretch = &mut mixer.voices[index].stretch;
                let deadline = Instant::now() + WAIT;
                while !stretch.prospective_work_ready() {
                    assert!(
                        !stretch.prospective_work_finished(),
                        "worker refused E for PAD {id}"
                    );
                    assert!(
                        Instant::now() < deadline,
                        "worker continuation at E for PAD {id}"
                    );
                    std::thread::sleep(PERIOD);
                }
                assert_eq!(stretch.prepared_target_output_frame(), Some(E));
            }
            stream.render();
            assert_eq!(stream.frame(), E);
        }

        let stopped_key = ahead.ownership.callback_content_key(0).unwrap();
        let stopped_source = ahead
            .ownership
            .ordinary_snapshot(stopped_key)
            .unwrap()
            .source;
        let stopped_generation = ahead.owner.state_mut().mixer.voices[slots[1][0]].generation;
        let prepared = ahead.owner.state_mut().mixer.voices[slots[1][1]]
            .stretch
            .prepared_native_address();
        assert_ne!(prepared, owners[1][1]);
        assert_ne!(prepared, 0);
        let mut take = ahead.record(E, E + FRAMES as u64);
        let [reference, candidate] = [&mut inline, &mut ahead].map(|stream| {
            stream.stop_pad(0);
            if stale {
                stream.set_rate(1.4);
            }
            let scope = Scope::begin();
            let block = stream.render();
            let observation = scope.finish();
            assert_eq!(observation.source_refusals, 0);
            assert_eq!(observation.adapter_shortfall_channel_frames, 0);
            if stale {
                let effect = stream.ownership.global_rate_status.read().unwrap();
                assert_eq!(effect.request_revision, effect.effect.revision);
                assert_eq!(effect.effect.state, RateState::Applied);
                assert_eq!(effect.effect.execution_frame, Some(E));
            }
            (block, observation)
        });
        assert_eq!(
            reference.0.pcm.map(f32::to_bits),
            candidate.0.pcm.map(f32::to_bits)
        );
        assert_eq!(reference.1.shifts, 1);
        assert_eq!(candidate.1.shifts, u64::from(stale));
        // The first new shift of this finite replay emits a quiet onset.
        // Require real signal in both channels without an arbitrary amplitude floor.
        let fresh = &candidate.0.pcm[62..];
        assert!(fresh.iter().all(|value| value.is_finite()));
        assert!((0..2).all(|channel| fresh.iter().skip(channel).step_by(2).any(|v| *v != 0.0)));
        let fresh_peak = fresh
            .iter()
            .fold(0.0_f32, |peak, value| peak.max(value.abs()));
        let state = ahead.owner.state_mut();
        let stopped = &mut state.mixer.voices[slots[1][0]];
        assert!(!stopped.active);
        assert_ne!(stopped.generation, stopped_generation);
        assert_eq!(stopped.stretch.pending_fifo_frames(), (0, 0));
        assert!(!stopped.stretch.prospective_work_ready());
        let stop = ahead.ownership.ordinary_snapshot(stopped_key).unwrap();
        assert_eq!(stop.kind, super::super::ordinary_transport::STOPPED);
        assert_eq!(stop.source, stopped_source);
        let voice = &mut state.mixer.voices[slots[1][1]];
        assert!(voice.stretch.wet_current_source_output_ready());
        assert_eq!(voice.stretch.pending_fifo_frames(), (64, 447));
        if stale {
            assert_eq!(voice.source_playback.rate_target(), 1.4);
            assert_eq!(voice.stretch.native_identity(), owners[1][1]);
            assert!(voice.stretch.accepted_wet_probe().is_none());
            assert!(!voice.stretch.prospective_work_ready());
        } else {
            assert_eq!(voice.stretch.native_identity(), prepared);
            let span = voice.stretch.accepted_wet_probe().unwrap();
            assert_eq!(span.shifts, 1);
            assert_eq!(span.before, (480, 31));
            assert_eq!(span.after, (64, 447));
            let mut history = SourceFeedSnapshot::new();
            assert!(voice.source_feed_history.capture_into(&mut history));
            assert_eq!(history.active_frames(), E + FRAMES as u64);
            let last = history.spans().next_back().unwrap();
            let recorded = last
                .slice((E - last.active_start) as usize, FRAMES)
                .unwrap();
            assert!(recorded.playback.matches_exact(&span.receipt.playback));
            assert!(recorded.plan.matches_exact(span.receipt.plan));
            assert_eq!(recorded.context, span.receipt.context);
        }

        let scope = Scope::begin();
        for _ in 0..5 {
            assert_eq!(
                inline.render().pcm.map(f32::to_bits),
                ahead.render().pcm.map(f32::to_bits)
            );
        }
        assert_eq!(
            scope.finish().shifts,
            2,
            "both owners must reach the next shift"
        );
        take.assert_recording_span(&candidate.0, 0, FRAMES);
        close_callback_and_join(inline.owner, inline.retirement).unwrap();
        close_callback_and_join(ahead.owner, ahead.retirement).unwrap();
        eprintln!(
            "Wet continuation E={E} stale={stale}: inline shifts={}, callback shifts={}, fresh peak={fresh_peak}",
            reference.1.shifts, candidate.1.shifts,
        );
    }
    drop(samples);
    assert!(readers.iter().all(|reader| reader.upgrade().is_none()));
}

#[test]
#[ignore = "paced device-free real-mixer producer diagnostic at 96 kHz / 96 frames"]
fn real_wet_mixer_render_ahead_at_96_frames() {
    const PERIODS: usize = 1024;
    const PADS: usize = 6;
    let samples = sources(PADS);
    let mut inline = Stream::new(&samples, true);
    let reference_scope = Scope::begin();
    let expected: Vec<_> = (0..PERIODS).map(|_| inline.render()).collect();
    let reference = reference_scope.finish();
    let mixer = &inline.owner.state_mut().mixer;
    for id in 0..PADS {
        let voice = mixer
            .voices
            .iter()
            .find(|v| v.is_playing_sample(id))
            .unwrap();
        assert!(voice.stretch.wet_render_ready());
        assert!(!voice.stretch.wet_mode_failed());
        assert_eq!(voice.stretch.rubberband_block_size(), 512);
    }
    assert_eq!(reference.shifts, (PADS * (PERIODS * FRAMES / 512)) as u64);
    assert_eq!(reference.adapter_shortfall_channel_frames, 0);
    assert!(
        expected
            .iter()
            .any(|b| b.pcm.iter().any(|v| v.abs() > 0.001))
    );
    drop(inline);

    std::thread::scope(|threads| {
        let (mut producer, mut consumer) = RingBuffer::new(LEAD);
        let worker = threads.spawn(move || {
            let mut stream = Stream::new(&samples, true);
            let scope = Scope::begin();
            let mut total_ns = 0;
            let mut max_ns = 0;
            for _ in 0..PERIODS {
                while producer.slots() == 0 {
                    assert!(!producer.is_abandoned());
                    std::thread::sleep(Duration::from_micros(100));
                }
                let began = Instant::now();
                let block = stream.render();
                let elapsed = began.elapsed().as_nanos();
                total_ns += elapsed;
                max_ns = max_ns.max(elapsed);
                assert!(producer.push(block).is_ok());
            }
            (scope.finish(), total_ns, max_ns)
        });

        let limit = Instant::now() + WAIT;
        while consumer.slots() < LEAD {
            assert!(Instant::now() < limit && !consumer.is_abandoned());
            std::thread::sleep(PERIOD);
        }
        let began = Instant::now();
        let mut empty_reads = 0;
        let scope = Scope::begin();
        for (index, expected) in expected.into_iter().enumerate() {
            let due = began + PERIOD * index as u32;
            if let Some(delay) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(delay);
            }
            let deadline = Instant::now() + WAIT;
            let actual = consumer.pop().unwrap_or_else(|_| {
                empty_reads += 1;
                loop {
                    if let Ok(block) = consumer.pop() {
                        break block;
                    }
                    assert!(Instant::now() < deadline && !consumer.is_abandoned());
                    std::thread::sleep(Duration::from_micros(100));
                }
            });
            assert_eq!(actual.first, expected.first);
            assert_eq!(actual.pcm.map(f32::to_bits), expected.pcm.map(f32::to_bits));
        }
        let consumer = scope.finish();
        let (producer, total_ns, max_ns) = worker.join().unwrap();
        assert_eq!(consumer.shifts, 0);
        assert_eq!(producer.shifts, reference.shifts);
        assert_eq!(producer.adapter_shortfall_channel_frames, 0);
        eprintln!(
            "real render-ahead: pads=6 periods={PERIODS} lead_frames={} empty_reads={empty_reads} worker_shifts={} worker_render_ns={total_ns} worker_max_ns={max_ns}",
            LEAD * FRAMES,
            producer.shifts,
        );
    });
}
