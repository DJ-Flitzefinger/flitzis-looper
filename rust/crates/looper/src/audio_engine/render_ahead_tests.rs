//! Device-free render-ahead evidence at the real shared stream renderer.

use super::*;
use crate::audio_engine::buffer_retirement::close_callback_and_join;
use crate::audio_engine::global_rate_status::RateState;
use crate::audio_engine::input_runtime_binding::{
    InputRuntimeOwnership, enqueue_stop_with_producer,
};
use crate::audio_engine::mix_capture::publication::CapturePublication;
use crate::audio_engine::mix_capture::{
    CaptureConfig, CaptureControl, CaptureFailure, CaptureStatus, SPOOL_HEADER_BYTES, prepare,
};
use crate::audio_engine::native_render_observation::Scope;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::messages::{MixCaptureTarget, SampleBuffer};
use std::path::PathBuf;
use std::time::{Duration, Instant};

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
        super::super::push_global_rate_message(
            &self.ownership.global_timing_revision,
            &self.ownership.global_timing_publishing,
            &self.ownership.global_rate_status,
            &mut self.parameters,
            Some(1.3),
            Some(0.4),
            "render-ahead probe rate",
        )
        .unwrap();
    }

    fn record(&mut self, first: u64, end: u64) -> Take {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("take.capture");
        let (control, work) = prepare(
            &path,
            CaptureConfig {
                sample_rate_hz: RATE,
                channels: 2,
                block_frames: FRAMES,
                block_count: 4,
                max_pool_pcm_bytes: 128 * 1024,
            },
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
        let status = self.joined();
        assert!(status.successful_spool(), "{status:?}");
        assert_eq!(status.first_frame, Some(block.first + 17));
        assert_eq!(status.end_frame, Some(block.first + 83));
        assert_eq!(status.written_frames, 66);
        let spool = std::fs::read(&self.path).unwrap();
        assert_eq!(spool.len(), SPOOL_HEADER_BYTES as usize + 66 * 2 * 4);
        let actual = spool[SPOOL_HEADER_BYTES as usize..]
            .chunks_exact(4)
            .map(|bits| u32::from_le_bytes(bits.try_into().unwrap()));
        assert!(actual.eq(block.pcm[34..166].iter().map(|value| value.to_bits())));
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
