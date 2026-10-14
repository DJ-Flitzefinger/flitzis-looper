//! The actual callback image retains deferred PCM through full retirement and close handoff.

use super::*;
use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirement, AudioBufferRetirementWorker, CallbackCloseOwner,
    RETIRED_AUDIO_BUFFER_QUEUE_CAPACITY, create_audio_buffer_retirement_with_callback_paused,
};
use crate::audio_engine::content_placement::PlacementError;
use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
use crate::messages::SampleBuffer;
use std::sync::atomic::{AtomicBool, AtomicU8};
use std::sync::{Weak, mpsc};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
const GENERATION: u64 = 7;
const FULL_RETIREMENT_SLOTS: usize = RETIRED_AUDIO_BUFFER_QUEUE_CAPACITY + 128;

struct DropThread {
    id: ThreadId,
    name: Option<String>,
}

struct DropProbe(mpsc::Sender<DropThread>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        let current = thread::current();
        let _ = self.0.send(DropThread {
            id: current.id(),
            name: current.name().map(str::to_owned),
        });
    }
}

struct ObservedCallback {
    callback: AudioCallbackState,
    // Field order reports only after all unique callback fields have been destroyed.
    _drop_probe: DropProbe,
}

struct Pins {
    pcm: Weak<[f32]>,
    adoption: Weak<AtomicU8>,
    cancellation: Weak<AtomicBool>,
    ownership: Weak<InputRuntimeOwnership>,
}

struct Fixture {
    callback: Option<CallbackCloseOwner<ObservedCallback>>,
    worker: Option<AudioBufferRetirementWorker>,
    release_worker: Option<mpsc::Sender<()>>,
    destroyed: mpsc::Receiver<DropThread>,
    producer: Producer<ControlMessage>,
    feedback: Consumer<AudioMessage>,
    ownership: Option<Arc<InputRuntimeOwnership>>,
}

impl Fixture {
    fn new() -> Self {
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        let (producer, consumer_in) = RingBuffer::new(8);
        let (_parameter_producer, parameter_consumer_in) = RingBuffer::new(8);
        let (producer_out, feedback) = RingBuffer::new(8);
        let mixer = create_stream_mixer(
            1,
            RATE,
            Arc::default(),
            ownership.clone(),
            (0..NUM_SAMPLES).map(|_| Arc::default()).collect(),
        )
        .unwrap();
        let (destroyed_tx, destroyed) = mpsc::channel();
        let (release_worker, worker_start) = mpsc::channel();
        let (callback, worker) = create_audio_buffer_retirement_with_callback_paused(
            move |retired_buffers| {
                Box::new(ObservedCallback {
                    callback: AudioCallbackState {
                        consumer_in,
                        parameter_consumer_in,
                        producer_out: ContentFeedbackProducer {
                            producer: producer_out,
                            ownership: mixer.input_ownership_for_effect(),
                        },
                        mixer,
                        transport: TransportTimeline::new(RATE),
                        scheduler: TransportScheduler::new(),
                        trigger_quantization: TriggerQuantization::Immediate,
                        callback_clock: Arc::new(SharedOutputClock::new()),
                        clock_mapper: OutputClockMapper::new(),
                        loop_acceptance: Arc::default(),
                        pad_peaks: [0.0; NUM_SAMPLES],
                        pad_activity: RtRenderPadActivity::default(),
                        last_pad_emit_frame: 0,
                        last_master_emit_frame: 0,
                        retired_buffers,
                    },
                    _drop_probe: DropProbe(destroyed_tx),
                })
            },
            move || {
                // This wait runs only on the off-RT worker, never in the callback owner.
                let _ = worker_start.recv();
            },
        )
        .unwrap();
        Self {
            callback: Some(callback),
            worker: Some(worker),
            release_worker: Some(release_worker),
            destroyed,
            producer,
            feedback,
            ownership: Some(ownership),
        }
    }

    fn state(&mut self) -> &mut AudioCallbackState {
        &mut self.callback.as_mut().unwrap().state_mut().callback
    }

    fn defer_cold(&mut self) -> Pins {
        let ownership = self.ownership.as_ref().unwrap().clone();
        let sample = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from(vec![0.25; 256 * 1024]),
        };
        let lifetime = crate::audio_engine::content_lifetime::reserve().unwrap();
        let original = ownership.slot_snapshot(0).unwrap();
        let write = ownership.prepare_slot_write(original, lifetime).unwrap();
        let publication = ownership
            .publish_source_write(&write, &sample, RATE, GENERATION, 2, None, true, || true)
            .unwrap();
        let adoption = Arc::new(AtomicU8::new(0));
        let cancellation = Arc::new(AtomicBool::new(false));
        let pins = Pins {
            pcm: Arc::downgrade(&sample.samples),
            adoption: Arc::downgrade(&adoption),
            cancellation: Arc::downgrade(&cancellation),
            ownership: Arc::downgrade(&ownership),
        };
        self.producer
            .push(ControlMessage::LoadColdSample {
                publication,
                id: 0,
                sample,
                source_generation: GENERATION,
                content_lifetime: lifetime,
                adoption,
                epoch: None,
                captured_epoch: 0,
                replace_assignment: false,
                loop_region: None,
                resident_cancelled: Some(cancellation),
                incoming_defaults: None,
            })
            .unwrap();
        let popped = self.state().consumer_in.pop().unwrap();
        self.producer.push(ControlMessage::Ping()).unwrap();
        let unrelated = ownership.slot_snapshot(215).unwrap();
        let write = ownership.prepare_slot_write_fixed(unrelated, 0).unwrap();
        let state = self.state();
        assert_eq!(
            ownership.commit_slot_write_fixed(
                &write,
                || {
                    process_control_message(
                        popped,
                        &mut state.scheduler,
                        0,
                        &mut state.trigger_quantization,
                        &mut state.transport,
                        &mut state.mixer,
                        &mut state.producer_out,
                        &mut state.retired_buffers,
                    );
                    assert!(matches!(
                        state.mixer.deferred_control(),
                        Some(ControlMessage::LoadColdSample { .. })
                    ));
                    false
                },
                |_| panic!("the rejected unrelated writer must not apply"),
            ),
            Err(PlacementError::ClaimRejected)
        );
        assert_eq!(pins.pcm.strong_count(), 1);
        assert_eq!(pins.adoption.strong_count(), 1);
        assert_eq!(pins.cancellation.strong_count(), 1);
        pins
    }

    fn fill_normal_lane(&mut self) -> Weak<[f32]> {
        let filler = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from([0.0; 16]),
        };
        let weak = Arc::downgrade(&filler.samples);
        let retirement = &mut self.state().retired_buffers;
        let mut filled = 0;
        while retirement.available_retirement_slots() != 0 {
            assert!(filled < FULL_RETIREMENT_SLOTS);
            retirement.retire_sample(filler.clone());
            filled += 1;
        }
        assert_eq!(filled, FULL_RETIREMENT_SLOTS);
        drop(filler);
        assert_eq!(weak.strong_count(), FULL_RETIREMENT_SLOTS);
        weak
    }

    fn drain(&mut self) -> usize {
        let state = self.state();
        drain_control_messages(
            &mut state.consumer_in,
            &mut state.scheduler,
            0,
            &mut state.trigger_quantization,
            &mut state.transport,
            &mut state.mixer,
            &mut state.producer_out,
            &mut state.retired_buffers,
        )
    }

    fn assert_blocked(&mut self, pins: &Pins) {
        assert_eq!(self.state().retired_buffers.available_retirement_slots(), 0);
        assert_eq!(self.drain(), 0);
        assert_eq!(self.drain(), 0);
        assert!(self.state().mixer.deferred_control().is_some());
        assert!(!self.state().mixer.can_play_sample(0, 1.0));
        assert!(matches!(
            self.state().consumer_in.peek(),
            Ok(ControlMessage::Ping())
        ));
        assert!(self.feedback.pop().is_err());
        assert_eq!(pins.pcm.strong_count(), 1);
        assert_eq!(pins.adoption.strong_count(), 1);
        assert_eq!(pins.cancellation.strong_count(), 1);
        assert_eq!(
            self.ownership.as_ref().unwrap().cold_status(0, GENERATION),
            Some(0)
        );
    }

    fn start_worker(&mut self) {
        if let Some(start) = self.release_worker.take() {
            let _ = start.send(());
        }
    }

    fn close_from_callback_thread(&mut self) -> ThreadId {
        // No test-side Native owner may mask a final owner release in the callback image.
        drop(self.ownership.take());
        let callback = self.callback.take().unwrap();
        let closer = thread::Builder::new()
            .name("test-callback-close".into())
            .spawn(move || drop(callback))
            .unwrap();
        let id = closer.thread().id();
        closer.join().unwrap();
        id
    }

    fn join_worker(&mut self) -> DropThread {
        assert!(self.callback.is_none());
        self.start_worker();
        drop(self.worker.take());
        self.destroyed.recv_timeout(Duration::from_secs(5)).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A failed assertion must not join a paused worker before its unique close handoff.
        drop(self.callback.take());
        self.start_worker();
        drop(self.worker.take());
    }
}

#[test]
fn full_retirement_lane_close_transfers_deferred_payload_and_backlog_off_callback() {
    let mut fixture = Fixture::new();
    let pins = fixture.defer_cold();
    let filler = fixture.fill_normal_lane();
    fixture.assert_blocked(&pins);

    let callback_thread = fixture.close_from_callback_thread();

    // The worker is still paused: even the last deferred PCM/token owner survives close.
    assert_eq!(pins.pcm.strong_count(), 1);
    assert_eq!(pins.adoption.strong_count(), 1);
    assert_eq!(pins.cancellation.strong_count(), 1);
    assert_eq!(filler.strong_count(), FULL_RETIREMENT_SLOTS);
    assert!(matches!(
        fixture.destroyed.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let destroyed = fixture.join_worker();
    assert_ne!(destroyed.id, callback_thread);
    assert_eq!(
        destroyed.name.as_deref(),
        Some("flitzis-audio-buffer-retirement")
    );
    assert_eq!(pins.pcm.strong_count(), 0);
    assert_eq!(pins.adoption.strong_count(), 0);
    assert_eq!(pins.cancellation.strong_count(), 0);
    assert_eq!(pins.ownership.strong_count(), 0);
    assert_eq!(filler.strong_count(), 0);
}

#[test]
fn full_retirement_lane_retries_deferred_cold_before_ping_and_closes_adopted_bank_off_rt() {
    let mut fixture = Fixture::new();
    let pins = fixture.defer_cold();
    let filler = fixture.fill_normal_lane();
    fixture.assert_blocked(&pins);
    fixture.start_worker();
    let needed = 2 * MAX_VOICES + 6;
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.state().retired_buffers.available_retirement_slots() < needed
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(1));
    }
    assert!(fixture.state().retired_buffers.available_retirement_slots() >= needed);

    assert_eq!(fixture.drain(), 2);

    assert!(fixture.state().mixer.deferred_control().is_none());
    assert!(fixture.state().consumer_in.peek().is_err());
    assert!(fixture.state().mixer.can_play_sample(0, 1.0));
    assert_eq!(
        fixture
            .ownership
            .as_ref()
            .unwrap()
            .cold_status(0, GENERATION),
        Some(2)
    );
    assert!(matches!(fixture.feedback.pop(), Ok(AudioMessage::Pong())));
    assert!(fixture.feedback.pop().is_err());
    assert_eq!(fixture.drain(), 0);
    assert!(pins.pcm.strong_count() > 0);
    let callback_thread = fixture.close_from_callback_thread();
    let destroyed = fixture.join_worker();
    assert_ne!(destroyed.id, callback_thread);
    assert_eq!(
        destroyed.name.as_deref(),
        Some("flitzis-audio-buffer-retirement")
    );
    assert_eq!(pins.pcm.strong_count(), 0);
    assert_eq!(pins.adoption.strong_count(), 0);
    assert_eq!(pins.cancellation.strong_count(), 0);
    assert_eq!(pins.ownership.strong_count(), 0);
    assert_eq!(filler.strong_count(), 0);
}
