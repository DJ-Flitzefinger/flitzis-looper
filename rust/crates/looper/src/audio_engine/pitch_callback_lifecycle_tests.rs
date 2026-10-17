//! Actual callback-box pitch custody, Native deletion, and worker-close ownership.
//!
//! The marked render scopes execute the existing drains/renderer on the actual
//! AudioCallbackState. They do not claim the outer CPAL timestamp/telemetry path,
//! C++ allocator freedom, a device deadline, acoustic quality, or parent acceptance.
//! Raw reference processing, snapshot decoding, barriers and assertions stay off RT.

use super::*;
use crate::audio_engine::buffer_retirement::RETIRED_AUDIO_BUFFER_QUEUE_CAPACITY;
use crate::audio_engine::dsp::PerPadDspChain;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::native_lifecycle_observation::{
    self as observation, CallbackRole, NativeEvent, NativeEventKind, Snapshot,
};
use crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_sources;
use crate::audio_engine::pitch_actions::{CapturedPitchAction, PitchActionReject};
use crate::audio_engine::pitch_publication::PitchWorkCapacity;
use crate::audio_engine::pitch_runtime_tests::source_feed_tests::RawRubberBand;
use crate::audio_engine::pitch_runtime_tests::{Fixture as PitchFixture, LOOP_END, LOOP_START};
use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
use crate::audio_engine::voice_slot::VoiceSlot;
use crate::messages::SampleBuffer;
use crate::test_memory::{ThreadAllocations, observe_current_thread};
use std::collections::BTreeSet;
use std::sync::Weak;
use std::sync::atomic::AtomicU32;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
const CAPACITY: usize = 1024;
const WAIT: Duration = Duration::from_secs(15);
const HISTORY: usize = 4096;
const FULL_LANE: usize = RETIRED_AUDIO_BUFFER_QUEUE_CAPACITY + 128;
const CHILD: &str = "FLITZI_PITCH_CALLBACK_LIFECYCLE_CHILD";
const CALIBRATION_TEST: &str = "audio_engine::audio_stream::pitch_callback_lifecycle_tests::native_lifecycle_observer_calibration_isolated";
const LIFECYCLE_TEST: &str = "audio_engine::audio_stream::pitch_callback_lifecycle_tests::actual_pitch_native_retirement_and_callback_close_are_off_callback";
const PARTITIONS: [usize; 7] = [73, 509, 7, 127, 3, 512, 31];

fn complete_snapshot() -> Snapshot {
    let deadline = Instant::now() + WAIT;
    loop {
        let snapshot = observation::snapshot();
        assert!(
            !snapshot.overflowed,
            "Native lifecycle observation overflowed"
        );
        if snapshot.incomplete == 0 {
            assert_eq!(snapshot.events.len(), snapshot.reserved);
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "Native lifecycle event did not finish"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn live_native(handle: usize) -> u64 {
    assert_ne!(handle, 0);
    let deadline = Instant::now() + WAIT;
    loop {
        let snapshot = complete_snapshot();
        if let Some(id) = snapshot.live_id(handle) {
            return id;
        }
        // A freed address can be reused before the previous delete hook resumes.
        // Only a completed, unambiguous lifetime may identify a currently held owner.
        assert!(
            Instant::now() < deadline,
            "held Native lifetime is ambiguous"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn event_for(snapshot: &Snapshot, id: u64, kind: NativeEventKind) -> NativeEvent {
    let matches: Vec<_> = snapshot
        .events
        .iter()
        .copied()
        .filter(|event| event.native_id == id && event.kind == kind)
        .collect();
    assert_eq!(matches.len(), 1, "one {kind:?} for Native lifetime {id}");
    matches[0]
}

fn is_deleted(snapshot: &Snapshot, id: u64) -> bool {
    snapshot
        .events
        .iter()
        .any(|event| event.native_id == id && event.kind == NativeEventKind::DeleteCompleted)
}

fn assert_no_callback_native(snapshot: &Snapshot) {
    assert!(
        snapshot
            .events
            .iter()
            .all(|event| { !matches!(event.role, CallbackRole::Render | CallbackRole::Close) })
    );
}

fn assert_deleted_on_retirement_worker(snapshot: &Snapshot, id: u64, retirement_thread: u32) {
    let created = event_for(snapshot, id, NativeEventKind::Created);
    let deleted = event_for(snapshot, id, NativeEventKind::DeleteCompleted);
    assert_eq!(created.opaque_handle, deleted.opaque_handle);
    assert!(created.sequence < deleted.sequence);
    assert_eq!(deleted.role, CallbackRole::Outside);
    assert_eq!(deleted.windows_thread_id, retirement_thread);
}

fn assert_every_native_closed(snapshot: &Snapshot) {
    assert_no_callback_native(snapshot);
    let created: BTreeSet<_> = snapshot
        .events
        .iter()
        .filter(|event| event.kind == NativeEventKind::Created)
        .map(|event| event.native_id)
        .collect();
    let deleted: BTreeSet<_> = snapshot
        .events
        .iter()
        .filter(|event| event.kind == NativeEventKind::DeleteCompleted)
        .map(|event| event.native_id)
        .collect();
    assert!(!created.is_empty());
    assert_eq!(
        created, deleted,
        "every observed wrapper owner really closes"
    );
    assert_eq!(snapshot.events.len(), 2 * created.len());
    for id in created {
        let born = event_for(snapshot, id, NativeEventKind::Created);
        let ended = event_for(snapshot, id, NativeEventKind::DeleteCompleted);
        assert_eq!(born.opaque_handle, ended.opaque_handle);
        assert!(born.sequence < ended.sequence);
        assert_eq!(ended.role, CallbackRole::Outside);
        assert_ne!(ended.windows_thread_id, 0);
    }
}

struct Source {
    // A copied independent oracle input, never a strong pin to productive PCM.
    samples: Vec<f32>,
    start: usize,
}

impl Source {
    fn sample(&self, age: i64) -> f32 {
        let frame = LOOP_START as i64 + age.rem_euclid((LOOP_END - LOOP_START) as i64);
        self.samples[frame as usize - self.start]
    }
}

struct Oracle {
    raw: RawRubberBand,
    filter: PerPadDspChain,
    age: u64,
    pitch: f64,
    handle: usize,
    native_id: u64,
    generation: u64,
}

impl Oracle {
    fn born(id: usize, note: i32, source: &Source, voice: &VoiceSlot) -> Self {
        let pitch = 2.0_f64.powf(f64::from(note) / 12.0);
        assert!((0.5..=2.0).contains(&pitch));
        assert_eq!(RawRubberBand::history_bound(pitch), HISTORY);
        let mut raw = RawRubberBand::new(pitch);
        for age in -(HISTORY as i64)..0 {
            raw.sample(source.sample(age), pitch);
        }
        let handle = voice.stretch.native_state_address();
        Self {
            raw,
            filter: PerPadDspChain::new(id, RATE as f32, 1),
            age: 0,
            pitch,
            handle,
            native_id: live_native(handle),
            generation: voice.generation,
        }
    }

    fn sample(&mut self, source: &Source) -> f32 {
        let wet = self.raw.sample(source.sample(self.age as i64), self.pitch);
        self.filter.begin_frame();
        let result = self.filter.process_sample(0, wet);
        self.age += 1;
        result
    }
}

struct Harness {
    control: Option<LaunchFixture>,
    callback: Option<CallbackCloseOwner<AudioCallbackState>>,
    worker: Option<AudioBufferRetirementWorker>,
    release: Option<mpsc::Sender<()>>,
    retirement_thread: Arc<AtomicU32>,
    feedback: Consumer<AudioMessage>,
    sources: [Source; 2],
    pcm: [Weak<[f32]>; 2],
    oracles: [Option<Oracle>; 2],
    receipts: Vec<CapturedPitchAction>,
    starts: [usize; 2],
    maximum_pcm_error: f32,
    compared_frames: usize,
    output_energy: f64,
    render_thread: u32,
}

impl Harness {
    fn new(lock: bool) -> Self {
        let mut setup = PitchFixture::new(lock);
        adopt_current_sources(&mut setup, lock);
        let sources = std::array::from_fn(|id| {
            let cache = setup.control.engine.sample_cache.lock().unwrap();
            let source = cache[id].as_ref().unwrap();
            Source {
                samples: source.samples.to_vec(),
                start: source.resident_start(),
            }
        });
        let pcm = std::array::from_fn(|id| {
            Arc::downgrade(
                &setup.control.engine.sample_cache.lock().unwrap()[id]
                    .as_ref()
                    .unwrap()
                    .samples,
            )
        });
        // Keep the same producer Arc used by Engine and InputRuntime. Only the
        // empty test queue storage is enlarged to the real production capacity.
        let (producer, consumer) = RingBuffer::new(CAPACITY);
        *setup.control.producer.lock().unwrap() = producer;
        setup.control.consumer = consumer;
        let PitchFixture {
            control,
            mixer,
            clock,
            retirement,
            _retirement_worker,
            ..
        } = setup;
        drop(retirement);
        drop(_retirement_worker); // SourceACK setup's empty old lane is terminal.
        assert!(Arc::ptr_eq(
            &mixer.input_ownership_for_effect(),
            &control.engine.input_runtime_ownership
        ));
        assert!(control.consumer.is_empty());
        let mut control = control;
        let (_, placeholder) = RingBuffer::new(1);
        let consumer = std::mem::replace(&mut control.consumer, placeholder);
        let (_parameters, parameter_consumer) = RingBuffer::new(CAPACITY);
        let (feedback_producer, feedback) = RingBuffer::new(CAPACITY);
        let (release, start) = mpsc::channel();
        let retirement_thread = Arc::new(AtomicU32::new(0));
        let observed_thread = retirement_thread.clone();
        let (callback, worker) = create_stream_callback_owner_with_start(
            (consumer, parameter_consumer, feedback_producer),
            mixer,
            RATE,
            clock,
            control.engine.loop_acceptance.clone(),
            move || {
                observed_thread.store(observation::current_windows_thread_id(), Ordering::Release);
                // This startup wait runs only on the actual retirement worker.
                let _ = start.recv();
            },
        )
        .unwrap();
        let result = Self {
            control: Some(control),
            callback: Some(callback),
            worker: Some(worker),
            release: Some(release),
            retirement_thread,
            feedback,
            sources,
            pcm,
            oracles: [None, None],
            receipts: Vec::new(),
            starts: [0, 0],
            maximum_pcm_error: 0.0,
            compared_frames: 0,
            output_energy: 0.0,
            render_thread: observation::current_windows_thread_id(),
        };
        // Install the failure-safe close/release/join guard before waiting.
        let deadline = Instant::now() + WAIT;
        while result.retirement_thread.load(Ordering::Acquire) == 0 {
            assert!(Instant::now() < deadline, "retirement worker did not start");
            std::thread::sleep(Duration::from_millis(1));
        }
        result
    }

    fn state(&mut self) -> &mut AudioCallbackState {
        self.callback.as_mut().unwrap().state_mut()
    }

    fn frame(&mut self) -> u64 {
        self.state().transport.output_frame()
    }

    fn voice(&mut self, id: usize) -> &VoiceSlot {
        self.state()
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(id))
            .unwrap()
    }

    fn used(&self) -> usize {
        self.control
            .as_ref()
            .unwrap()
            .engine
            .pitch_work_capacity
            .used()
    }

    fn publish_clock(&mut self) {
        let now = self
            .control
            .as_ref()
            .unwrap()
            .engine
            .input_clock
            .capture_ns();
        let state = self.state();
        let frame = state.transport.output_frame();
        state.callback_clock.publish(OutputClockSnapshot {
            valid: true,
            observed_at_ns: now,
            audible_at_ns: now,
            output_frame: frame,
            sample_rate_hz: RATE,
            master_period_seconds: Some(0.5),
            downbeat_frame: 0,
            master_beat: frame as f64 / (f64::from(RATE) * 0.5),
            freshness_ns: 2_000_000_000,
        });
    }

    fn capture(&mut self, id: usize, note: i32) -> CapturedPitchAction {
        self.publish_clock();
        let action = self
            .control
            .as_ref()
            .unwrap()
            .engine
            .capture_pad_pitch_action(id, &format!("pitch.note:{note}:set_retrigger"), None)
            .unwrap();
        assert!(
            self.control
                .as_ref()
                .unwrap()
                .engine
                .publish_pitch_action(&action)
                .unwrap()
        );
        self.receipts.push(action.clone());
        self.callback(0);
        assert!(action.accepted(), "{:?}", action.reason());
        action
    }

    fn ready(&mut self, action: &CapturedPitchAction) -> u64 {
        let frame = self.frame();
        let deadline = Instant::now() + WAIT;
        loop {
            self.callback(0);
            let ready = self
                .state()
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .ready_pitch_native_address(action.sequence());
            if let Some(handle) = ready {
                assert_eq!(self.frame(), frame);
                let id = live_native(handle);
                let created = event_for(&complete_snapshot(), id, NativeEventKind::Created);
                assert_eq!(created.role, CallbackRole::Outside);
                assert_ne!(created.windows_thread_id, self.render_thread);
                assert_ne!(
                    created.windows_thread_id,
                    self.retirement_thread.load(Ordering::Acquire)
                );
                return id;
            }
            assert_ne!(action.state(), "rejected", "{:?}", action.reason());
            assert!(
                Instant::now() < deadline,
                "original preparation did not become ready"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn callback(&mut self, frames: usize) {
        let frame = self.frame();
        let mut output = vec![0.0; frames];
        {
            let _role = observation::enter_callback_role(CallbackRole::Render);
            let state = self.state();
            drain_control_messages(
                &mut state.consumer_in,
                &mut state.scheduler,
                frame,
                &mut state.trigger_quantization,
                &mut state.transport,
                &mut state.mixer,
                &mut state.producer_out,
                &mut state.retired_buffers,
            );
            drain_parameter_messages(
                &mut state.parameter_consumer_in,
                &mut state.mixer,
                &mut state.transport,
            );
            render_scheduled_audio_tracking_pads(
                &mut state.mixer,
                &mut state.scheduler,
                &mut output,
                &mut state.pad_peaks,
                &mut state.pad_activity,
                frame,
                1,
                &mut state.transport,
                &mut state.producer_out,
                &mut state.retired_buffers,
            );
            state.transport.advance_by_rendered_frames(frames);
        }
        while let Ok(message) = self.feedback.pop() {
            match message {
                AudioMessage::ContentFeedback { id, kind: 1, .. } => {
                    assert!(id < 2);
                    self.starts[id] += 1;
                }
                unexpected => panic!("unexpected lifecycle feedback: {unexpected:?}"),
            }
        }
        if frames != 0 {
            let expected_ids: Vec<_> = self
                .oracles
                .iter()
                .enumerate()
                .filter_map(|(id, oracle)| oracle.as_ref().map(|_| id))
                .collect();
            let mut actual_ids: Vec<_> = self
                .state()
                .mixer
                .voices
                .iter()
                .filter(|voice| voice.active)
                .map(|voice| voice.sample_id)
                .collect();
            actual_ids.sort_unstable();
            assert_eq!(actual_ids, expected_ids);
            for actual in output {
                let mut expected = 0.0_f32;
                for id in 0..2 {
                    if let Some(oracle) = &mut self.oracles[id] {
                        expected += oracle.sample(&self.sources[id]);
                    }
                }
                assert!(actual.is_finite() && expected.is_finite());
                let error = (actual - expected).abs();
                self.maximum_pcm_error = self.maximum_pcm_error.max(error);
                self.output_energy += f64::from(actual).powi(2);
                assert!(error < 2.0e-6, "independent mixed PCM at {frame}: {error}");
                self.compared_frames += 1;
            }
            for id in 0..2 {
                let Some(oracle) = self.oracles[id].as_ref() else {
                    continue;
                };
                let expected = (
                    oracle.handle,
                    oracle.generation,
                    oracle.age,
                    oracle.raw.fifo(),
                );
                let voice = self.voice(id);
                assert_eq!(voice.stretch.native_state_address(), expected.0);
                assert_eq!(voice.generation, expected.1);
                assert_eq!(voice.stretch.pending_fifo_frames(), expected.3);
                let position = voice.source_playback.position();
                assert_eq!(
                    position.frame,
                    LOOP_START + expected.2 as usize % (LOOP_END - LOOP_START)
                );
                assert_eq!(position.fraction, 0.0);
            }
        }
        assert_no_callback_native(&complete_snapshot());
    }

    fn render_to(&mut self, target: u64) {
        let mut index = 0;
        assert!(self.frame() <= target);
        while self.frame() < target {
            let frames = PARTITIONS[index % PARTITIONS.len()].min((target - self.frame()) as usize);
            self.callback(frames);
            index += 1;
        }
        assert_eq!(self.frame(), target);
    }

    fn apply(&mut self, action: &CapturedPitchAction, id: usize, note: i32, candidate: u64) {
        let original = action.original_due_frame();
        let execution = action.execution_target();
        self.render_to(execution);
        let starts = self.starts[id];
        self.callback(0);
        assert!(action.effective(), "{:?}", action.reason());
        assert_eq!(action.original_due_frame(), original);
        assert_eq!(action.execution_target(), execution);
        assert_eq!(self.starts[id], starts + 1);
        let source = &self.sources[id];
        let voice = self
            .callback
            .as_mut()
            .unwrap()
            .state_mut()
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(id))
            .unwrap();
        let oracle = Oracle::born(id, note, source, voice);
        assert_eq!(oracle.native_id, candidate);
        assert_eq!(voice.stretch.pending_fifo_frames(), oracle.raw.fifo());
        self.oracles[id] = Some(oracle);
        self.callback(733);
    }

    fn start_voices(&mut self) {
        for (id, note) in [(0, 7), (1, -5)] {
            let action = self.capture(id, note);
            let candidate = self.ready(&action);
            self.apply(&action, id, note, candidate);
        }
        assert_eq!(self.starts, [1, 1]);
        assert_ne!(
            self.oracles[0].as_ref().unwrap().native_id,
            self.oracles[1].as_ref().unwrap().native_id
        );
    }

    fn fill_retirement(&mut self) -> Weak<[f32]> {
        let filler = SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from([0.0_f32; 16]),
        };
        let weak = Arc::downgrade(&filler.samples);
        let mut added = 0;
        while self.state().retired_buffers.available_retirement_slots() != 0 {
            assert!(added < FULL_LANE);
            self.state().retired_buffers.retire_sample(filler.clone());
            added += 1;
        }
        assert!(added > 0 && added <= FULL_LANE);
        drop(filler);
        assert_eq!(weak.strong_count(), added);
        assert_eq!(self.state().retired_buffers.available_retirement_slots(), 0);
        weak
    }

    fn start_retirement(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }

    fn wait_slots(&mut self) {
        let deadline = Instant::now() + WAIT;
        while self.state().retired_buffers.available_retirement_slots() < 128 {
            assert!(
                Instant::now() < deadline,
                "normal retirement lane did not drain"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn wait_work_released(&self, must_delete: &[u64]) {
        let deadline = Instant::now() + WAIT;
        while self.used() != 0 {
            assert!(
                Instant::now() < deadline,
                "terminal pitch storage did not retire"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        // Acquire of the final claim follows the actual C ABI completion hook.
        let snapshot = complete_snapshot();
        for id in must_delete {
            assert_deleted_on_retirement_worker(
                &snapshot,
                *id,
                self.retirement_thread.load(Ordering::Acquire),
            );
        }
    }

    fn close(&mut self, blocked: bool, held_ids: &[u64]) -> (u32, Snapshot) {
        let mut control = self.control.take().unwrap();
        // Retain the same claims' observation Arc, then remove all Engine/cache/
        // producer aliases before dropping the callback's unique owner. No later
        // action is published through the now unused replacement capacity.
        let capacity: PitchWorkCapacity = std::mem::take(&mut control.engine.pitch_work_capacity);
        let before = capacity.used();
        drop(control);
        let callback = self.callback.take().unwrap();
        let closer = std::thread::spawn(move || {
            let thread = observation::current_windows_thread_id();
            let _role = observation::enter_callback_role(CallbackRole::Close);
            drop(callback);
            thread
        });
        let close_thread = closer.join().unwrap();
        assert_ne!(close_thread, self.render_thread);
        if blocked {
            assert!(before >= 2);
            assert_eq!(
                capacity.used(),
                before,
                "close must hand off intact pitch work"
            );
            let snapshot = complete_snapshot();
            for id in held_ids {
                assert!(
                    !is_deleted(&snapshot, *id),
                    "held Native {id} died before worker release"
                );
            }
            assert!(self.pcm.iter().all(|pcm| pcm.strong_count() > 0));
            assert_no_callback_native(&snapshot);
        }
        self.start_retirement();
        drop(self.worker.take()); // Joins complete callback destruction and nested workers.
        assert_eq!(capacity.used(), 0);
        assert!(self.pcm.iter().all(|pcm| pcm.strong_count() == 0));
        let snapshot = complete_snapshot();
        assert_every_native_closed(&snapshot);
        (close_thread, snapshot)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // On any assertion failure, never join an intentionally blocked worker.
        // The callback handoff precedes release, exactly as in the existing close fixture.
        drop(self.callback.take());
        self.start_retirement();
        drop(self.worker.take());
    }
}

fn successful_retirement(lock: bool) {
    observation::enable();
    let mut harness = Harness::new(lock);
    harness.start_voices();
    let old_a = harness.oracles[0].as_ref().unwrap().native_id;
    let peer = harness.oracles[1].as_ref().unwrap().native_id;
    let action = harness.capture(0, 4);
    let candidate = harness.ready(&action);
    let original = (action.original_due_frame(), action.execution_target());
    let before = harness.used();
    assert!(
        before >= 3,
        "initial terminal work is held with the new original"
    );
    let filler = harness.fill_retirement();
    harness.callback(127);
    assert!(harness.frame() < original.1);
    assert_eq!(harness.used(), before);
    assert_eq!(harness.starts, [1, 1]);
    let blocked = complete_snapshot();
    for id in [old_a, peer, candidate] {
        assert!(!is_deleted(&blocked, id));
    }
    harness.start_retirement();
    harness.wait_slots();
    assert!(
        harness.frame() < original.1,
        "retirement is released BEFORE original E"
    );
    harness.apply(&action, 0, 4, candidate);
    assert_eq!(
        (action.original_due_frame(), action.execution_target()),
        original
    );
    harness.wait_work_released(&[old_a]);
    let snapshot = complete_snapshot();
    for id in [candidate, peer] {
        assert!(
            !is_deleted(&snapshot, id),
            "adopted/peer Native remains productive"
        );
    }
    assert_eq!(filler.strong_count(), 0);

    let cancelled = harness.capture(0, 10);
    let refused_candidate = harness.ready(&cancelled);
    let refused_timing = (cancelled.original_due_frame(), cancelled.execution_target());
    assert!(
        harness
            .control
            .as_ref()
            .unwrap()
            .engine
            .cancel_pitch_action(&cancelled)
            .unwrap()
    );
    assert_eq!(cancelled.state(), "rejected");
    harness.render_to(refused_timing.1);
    harness.callback(0);
    harness.callback(509);
    harness.wait_work_released(&[refused_candidate]);
    assert_eq!(harness.starts, [2, 1]);
    assert!(!cancelled.effective());
    assert_eq!(
        (cancelled.original_due_frame(), cancelled.execution_target()),
        refused_timing
    );
    let kept_receipts = harness.receipts.len();
    assert!(
        harness.output_energy > 0.01,
        "productive output must contain real audio"
    );
    let (_, snapshot) = harness.close(false, &[]);
    assert!(
        harness
            .receipts
            .iter()
            .all(|receipt| receipt.effective() || receipt.state() == "rejected")
    );
    eprintln!(
        "{}",
        serde_json::json!({
            "case":"successful_retirement", "key_lock":lock,
            "retained_receipts":kept_receipts, "claims_after_join":0,
            "native_events":snapshot.events.len(), "compared_frames":harness.compared_frames,
        "output_energy":harness.output_energy,
            "maximum_pcm_error":harness.maximum_pcm_error,
            "old_a_id":old_a,"adopted_id":candidate,"peer_id":peer,
            "cancelled_candidate_id":refused_candidate
        })
    );
}

fn blocked_close(lock: bool) {
    observation::enable();
    let mut harness = Harness::new(lock);
    harness.start_voices();
    let a = harness.oracles[0].as_ref().unwrap().native_id;
    let b = harness.oracles[1].as_ref().unwrap().native_id;
    let cancelled = harness.capture(0, 4);
    let cancelled_native = harness.ready(&cancelled);
    let stale = harness.capture(0, -2);
    let stale_native = harness.ready(&stale);
    let timing = [
        (cancelled.original_due_frame(), cancelled.execution_target()),
        (stale.original_due_frame(), stale.execution_target()),
    ];
    assert!(
        harness
            .control
            .as_ref()
            .unwrap()
            .engine
            .cancel_pitch_action(&cancelled)
            .unwrap()
    );
    let filler = harness.fill_retirement();
    let claims = harness.used();
    // The public Stop cancels the old launch generation immediately. Its actual
    // audio command remains in the original queue because retirement is full.
    harness
        .control
        .as_mut()
        .unwrap()
        .engine
        .stop_sample(0)
        .unwrap();
    assert!(matches!(
        stale.callback_guard(None),
        Err(PitchActionReject::LaunchCancelled)
    ));
    harness.callback(73);
    harness.callback(31);
    assert!(harness.frame() < timing[0].1.min(timing[1].1));
    assert_eq!(harness.starts, [1, 1]);
    assert_eq!(harness.used(), claims);
    assert!(harness.state().consumer_in.peek().is_ok());
    assert_eq!(
        harness.state().retired_buffers.available_retirement_slots(),
        0
    );
    assert!(!cancelled.effective() && !stale.effective());
    assert_eq!(
        (cancelled.original_due_frame(), cancelled.execution_target()),
        timing[0]
    );
    assert_eq!(
        (stale.original_due_frame(), stale.execution_target()),
        timing[1]
    );
    let held = [a, b, cancelled_native, stale_native];
    let kept_receipts = harness.receipts.len();
    assert!(
        harness.output_energy > 0.01,
        "productive output must contain real audio"
    );
    let (_, snapshot) = harness.close(true, &held);
    assert_eq!(filler.strong_count(), 0);
    assert!(!cancelled.effective() && !stale.effective());
    eprintln!(
        "{}",
        serde_json::json!({
            "case":"blocked_close", "key_lock":lock,
            "retained_receipts":kept_receipts,"claims_before_close":claims,"claims_after_join":0,
            "native_events":snapshot.events.len(),"compared_frames":harness.compared_frames,
        "output_energy":harness.output_energy,
            "maximum_pcm_error":harness.maximum_pcm_error,"held_native_ids":held,
            "stop_was_queued":true,"no_effect_retarget":true
        })
    );
}

fn run_child(test: &str, profile: &str) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, profile)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "isolated {profile} failed: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[test]
fn native_lifecycle_observer_calibration_isolated() {
    if std::env::var(CHILD).ok().as_deref() != Some("calibration") {
        run_child(CALIBRATION_TEST, "calibration");
        return;
    }
    observation::enable();
    let control_thread = observation::current_windows_thread_id();
    let first = {
        let _role = observation::enter_callback_role(CallbackRole::Calibration);
        RubberBandLiveShifter::new(RATE, 1).unwrap()
    };
    let first_handle = first.state_address();
    let first_id = live_native(first_handle);
    let ((), outside_allocations) = observe_current_thread(|| drop(first));
    assert_eq!(outside_allocations, ThreadAllocations::default());
    let first_snapshot = complete_snapshot();
    let outside = event_for(&first_snapshot, first_id, NativeEventKind::DeleteCompleted);
    assert_eq!(outside.role, CallbackRole::Outside);
    assert_eq!(outside.windows_thread_id, control_thread);
    assert_eq!(first_snapshot.live_id(first_handle), None);

    let negative = {
        let _role = observation::enter_callback_role(CallbackRole::Calibration);
        RubberBandLiveShifter::new(RATE, 1).unwrap()
    };
    let negative_handle = negative.state_address();
    let negative_id = live_native(negative_handle);
    assert_ne!(
        first_id, negative_id,
        "opaque addresses never define lifetimes"
    );
    // Deliberately bad ROLE during off-RT calibration, never a productive callback.
    // This proves detection using a real C ABI deletion and the same hook/guard.
    let ((), negative_allocations) = observe_current_thread(|| {
        let _role = observation::enter_callback_role(CallbackRole::Render);
        drop(negative);
    });
    assert_eq!(negative_allocations, ThreadAllocations::default());
    let snapshot = complete_snapshot();
    assert_eq!(snapshot.events.len(), 4);
    let bad = event_for(&snapshot, negative_id, NativeEventKind::DeleteCompleted);
    assert_eq!(bad.role, CallbackRole::Render);
    assert_eq!(bad.windows_thread_id, control_thread);
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.role == CallbackRole::Render)
            .count(),
        1
    );
    assert_eq!(snapshot.live_id(negative_handle), None);
    for id in [first_id, negative_id] {
        let created = event_for(&snapshot, id, NativeEventKind::Created);
        let deleted = event_for(&snapshot, id, NativeEventKind::DeleteCompleted);
        assert_eq!(created.role, CallbackRole::Calibration);
        assert_eq!(created.windows_thread_id, control_thread);
        assert_eq!(created.opaque_handle, deleted.opaque_handle);
        assert!(created.sequence < deleted.sequence);
    }
    eprintln!(
        "{}",
        serde_json::json!({
            "case":"observer_calibration", "events":4,"unique_lifetimes":2,
            "actual_os_thread":control_thread,"deliberate_off_rt_render_tag_detected":true,
            "observer_and_role_rust_allocation_requests":0,
            "opaque_address_reused":first_handle == negative_handle,
            "scope":"real Native deletion negative control on setup thread, never productive callback"
        })
    );
}

#[test]
fn actual_pitch_native_retirement_and_callback_close_are_off_callback() {
    match std::env::var(CHILD).ok().as_deref() {
        Some("success-off") => successful_retirement(false),
        Some("success-on") => successful_retirement(true),
        Some("close-off") => blocked_close(false),
        Some("close-on") => blocked_close(true),
        None => {
            for profile in ["success-off", "success-on", "close-off", "close-on"] {
                run_child(LIFECYCLE_TEST, profile);
            }
        }
        unexpected => panic!("unexpected lifecycle child profile: {unexpected:?}"),
    }
}
