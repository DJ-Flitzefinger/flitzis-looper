//! Device-free public capture/publication through the real boxed mixer and scheduler.

#[path = "pitch_runtime_tests/source_feed_tests.rs"]
pub(in crate::audio_engine) mod source_feed_tests;

#[path = "pitch_runtime_tests/custody_tests.rs"]
mod custody_tests;

#[path = "pitch_runtime_tests/routed_midi_tests.rs"]
mod routed_midi_tests;

use super::*;
use crate::audio_engine::audio_stream::{
    create_stream_mixer, drain_control_messages, render_scheduled_audio,
};
use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirementWorker, RtAudioBufferRetirement, create_audio_buffer_retirement,
};
use crate::audio_engine::input_mapping::launch_admission_tests::{LaunchFixture, RATE};
use crate::audio_engine::input_mapping::{InputRuntime, PitchPadProjection};
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::pitch_actions::PitchKeyMode;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::source_playback::FractionalSourcePosition;
use crate::audio_engine::source_reader::ExplicitSeekMode;
use crate::audio_engine::timing::{OutputClockSnapshot, SharedOutputClock};
use crate::audio_engine::transport::TransportTimeline;
use crate::audio_engine::voice_slot::VoiceSlot;
use crate::messages::{ResidentContext, SampleBuffer};
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use rtrb::RingBuffer;
use std::time::{Duration, Instant};

pub(super) const LOOP_START: usize = 4_800;
pub(super) const LOOP_END: usize = 100_800;
pub(super) const SOURCE_HZ: f64 = 160.0;
const WORKER_WAIT: Duration = Duration::from_secs(2);

fn tone(hz: f64) -> SampleBuffer {
    let complete = SampleBuffer {
        channels: 1,
        samples: Arc::from(
            (0..144_000)
                .map(|frame| {
                    (0.25 * (frame as f64 * hz * std::f64::consts::TAU / f64::from(RATE)).sin())
                        as f32
                })
                .collect::<Vec<_>>(),
        ),
        residency: None,
    }
    .with_complete_source(RATE);
    complete
        .window(LOOP_START, LOOP_END, 2, ResidentContext::KeyLockFiniteLoop)
        .unwrap()
}

/// Publish actual source/lifetime/authority before the mixer adopts its bank image.
pub(super) fn publish_tone(control: &LaunchFixture, id: usize, sample: SampleBuffer, refill: bool) {
    let engine = &control.engine;
    let ownership = &engine.input_runtime_ownership;
    let before = ownership.slot_snapshot(id).unwrap();
    let lifetime = if refill {
        crate::audio_engine::content_lifetime::reserve().unwrap()
    } else {
        before.lifetime()
    };
    let write = ownership
        .prepare_slot_write_fixed(before, lifetime)
        .unwrap();
    let generation = engine.pad_request_ids.lock().unwrap()[id] + 1;
    let publication = ownership
        .publish_source_write(
            &write,
            &sample,
            RATE,
            generation,
            ownership.next_authority(id).unwrap(),
            Some(TimingIntent::Manual),
            false,
            || true,
        )
        .unwrap();
    assert!(publication.current(&sample, RATE));
    assert_eq!(publication.key().unwrap().lifetime(), lifetime);
    engine.sample_cache.lock().unwrap()[id] = Some(sample);
    engine.pad_request_ids.lock().unwrap()[id] = generation;
    engine.loaded_source_generations.lock().unwrap()[id] = (generation, RATE);
    engine.loaded_source_digests.lock().unwrap()[id] = Some(format!("{generation:064x}"));
    engine.timing_intents.lock().unwrap()[id] = TimingIntent::Manual;
    engine
        .prepared_source_epoch(id)
        .unwrap()
        .store(generation, Ordering::Release);
}

pub(super) struct Fixture {
    pub(super) control: LaunchFixture,
    pub(super) mixer: Box<RtMixer>,
    pub(super) scheduler: FixedCapacityScheduler<16>,
    pub(super) transport: TransportTimeline,
    clock: Arc<SharedOutputClock>,
    pub(super) feedback: Vec<AudioMessage>,
    pub(super) retirement: RtAudioBufferRetirement,
    _retirement_worker: AudioBufferRetirementWorker,
}

impl Fixture {
    pub(super) fn new(lock: bool) -> Self {
        let mut control = LaunchFixture::new(32);
        let (_, feedback) = RingBuffer::<AudioMessage>::new(64);
        control.engine.test_control_channels =
            Some((control.producer.clone(), Arc::new(Mutex::new(feedback))));
        control.engine.input_runtime = Some(InputRuntime::new_with_ownership(
            control.producer.clone(),
            control.engine.input_clock,
            control.engine.input_runtime_ownership.clone(),
        ));
        publish_tone(&control, 0, tone(SOURCE_HZ), true);
        publish_tone(&control, 1, tone(320.0), true);
        let clock = Arc::new(SharedOutputClock::new());
        control
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .attach_pitch_clock(clock.clone())
            .unwrap();
        let mut mixer = create_stream_mixer(
            1,
            RATE,
            control.engine.current_timing_acknowledgements.clone(),
            control.engine.input_runtime_ownership.clone(),
            control.engine.prepared_source_epochs.clone(),
        )
        .unwrap();
        for id in [0, 1] {
            mixer.load_sample(
                id,
                control.engine.sample_cache.lock().unwrap()[id]
                    .clone()
                    .unwrap(),
            );
            mixer.set_pad_loop_region(
                id,
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
            );
            mixer.set_pad_key_lock(id, lock);
        }
        let mut loaded = vec![false; NUM_SAMPLES];
        let mut starts = vec![0.0; NUM_SAMPLES];
        let mut ends = vec![None; NUM_SAMPLES];
        let mut bindings = vec![None; NUM_SAMPLES];
        for id in [0, 1] {
            loaded[id] = true;
            starts[id] = LOOP_START as f64 / f64::from(RATE);
            ends[id] = Some(LOOP_END as f64 / f64::from(RATE));
            bindings[id] = input_runtime_binding::capture(&control.engine, id).unwrap();
        }
        let rows = vec![
            PitchPadProjection {
                key_lock: lock,
                source_root: Some(0),
                source_mode: Some(PitchKeyMode::Major),
                ..PitchPadProjection::default()
            };
            NUM_SAMPLES
        ];
        control
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_runtime_state_with_pitch(
                true,
                loaded,
                starts,
                ends,
                bindings.iter().map(Option::as_ref).collect(),
                rows,
            )
            .unwrap();
        let (retirement, worker) = create_audio_buffer_retirement();
        let fixture = Self {
            control,
            mixer,
            scheduler: FixedCapacityScheduler::new(),
            transport: TransportTimeline::new(RATE),
            clock,
            feedback: Vec::new(),
            retirement,
            _retirement_worker: worker,
        };
        fixture.refresh_clock();
        fixture
    }

    fn refresh_clock(&self) {
        let frame = self.transport.output_frame();
        let now = self.control.engine.input_clock.capture_ns();
        self.clock.publish(OutputClockSnapshot {
            valid: true,
            observed_at_ns: now,
            audible_at_ns: now,
            output_frame: frame,
            sample_rate_hz: RATE,
            master_period_seconds: Some(0.5),
            downbeat_frame: 0,
            master_beat: frame as f64 / 24_000.0,
            freshness_ns: 2_000_000_000,
        });
    }

    pub(super) fn capture(&self, id: usize, key: &str) -> CapturedPitchAction {
        self.refresh_clock();
        self.control
            .engine
            .capture_pad_pitch_action(id, key, None)
            .unwrap()
    }

    pub(super) fn rows(&self, row: PitchPadProjection) {
        let runtime = self.control.engine.input_runtime.as_ref().unwrap();
        let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
        rows[0] = PitchPadProjection {
            request_sequence: runtime.pitch_request_sequence(0).unwrap(),
            ..row
        };
        rows[1].key_lock = false;
        runtime.set_pitch_runtime_state(rows).unwrap();
    }

    fn midi_capture(&self, key: &str) -> CapturedPitchAction {
        self.refresh_clock();
        let runtime = self.control.engine.input_runtime.as_ref().unwrap();
        runtime.set_selected_pad(0).unwrap();
        runtime.replace_mappings(vec![("midi:note:1:60".into(), key.into())]);
        runtime.set_enabled(true);
        assert!(runtime.inject_midi_message(&[0x90, 60, 127]));
        let deadline = Instant::now() + WORKER_WAIT;
        loop {
            if let Some(event) = runtime.poll_event() {
                assert_eq!(event.action_key.as_deref(), Some(key));
                assert!(!event.dispatched && event.direct);
                assert!(event.pitch_rejection.is_none());
                let action = event.captured_pitch.unwrap();
                assert_eq!(action.received_timestamp(), event.received_at_ns);
                return action;
            }
            assert!(
                Instant::now() < deadline,
                "MIDI capture host wait exceeded 2 s"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub(super) fn publish(&mut self, action: &CapturedPitchAction) {
        assert!(self.control.engine.publish_pitch_action(action).unwrap());
        self.drain();
        assert!(
            action.accepted(),
            "state={} reason={:?}",
            action.state(),
            action.reason()
        );
        assert!(!action.effective());
    }

    /// Host preparation gets a bounded wait while the virtual source/output clock stays still.
    /// Successful adoption below proves readiness; this does not measure a device deadline.
    pub(super) fn wait_worker(&mut self, actions: &[&CapturedPitchAction]) {
        let frame = self.transport.output_frame();
        let deadline = Instant::now() + WORKER_WAIT;
        loop {
            self.mixer.pitch_runtime.as_mut().unwrap().poll();
            let ready = actions.iter().all(|action| {
                self.mixer
                    .pitch_runtime
                    .as_ref()
                    .unwrap()
                    .has_ready_action(action.sequence())
            });
            if ready {
                break;
            }
            for action in actions {
                assert_ne!(action.state(), "rejected", "reason={:?}", action.reason());
            }
            assert!(
                Instant::now() < deadline,
                "pitch worker host wait exceeded 2 s"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            Instant::now() < deadline,
            "pitch worker host wait exceeded 2 s"
        );
        assert_eq!(self.transport.output_frame(), frame);
    }

    pub(super) fn render_to(&mut self, target: u64) -> Vec<f32> {
        assert!(target >= self.transport.output_frame());
        let mut output = Vec::new();
        while self.transport.output_frame() < target {
            let frames = (target - self.transport.output_frame()).min(512) as usize;
            output.extend(self.render(frames));
        }
        assert_eq!(self.transport.output_frame(), target);
        output
    }

    fn apply(&mut self, action: &CapturedPitchAction) -> Vec<f32> {
        self.wait_worker(&[action]);
        let before = self.render_to(action.execution_target());
        self.render(1);
        assert!(
            action.effective(),
            "state={} reason={:?}",
            action.state(),
            action.reason()
        );
        assert!(self.scheduler.is_empty());
        assert_eq!(self.transport.output_frame(), action.execution_target() + 1);
        before
    }

    pub(super) fn pcm(&mut self, frames: usize) -> Vec<f32> {
        let mut pcm = Vec::with_capacity(frames);
        while pcm.len() < frames {
            pcm.extend(self.render((frames - pcm.len()).min(512)));
        }
        pcm
    }

    pub(super) fn voice(&self, id: usize) -> &VoiceSlot {
        self.mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == id)
            .unwrap()
    }

    pub(super) fn starts(&self, id: usize) -> usize {
        self.feedback
            .iter()
            .filter(|message| {
                matches!(message,
                AudioMessage::ContentFeedback { id: actual, kind: 1, .. }
                | AudioMessage::SampleStarted { id: actual } if *actual == id)
            })
            .count()
    }

    pub(super) fn drain(&mut self) {
        drain_control_messages(
            &mut self.control.consumer,
            &mut self.scheduler,
            self.transport.output_frame(),
            &mut TriggerQuantization::Immediate,
            &mut self.transport,
            &mut self.mixer,
            &mut self.feedback,
            &mut self.retirement,
        );
    }

    pub(super) fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames];
        let frame = self.transport.output_frame();
        render_scheduled_audio(
            &mut self.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            frame,
            1,
            &mut self.transport,
            &mut self.feedback,
            &mut self.retirement,
        );
        self.transport.advance_by_rendered_frames(frames);
        output
    }
}

fn assert_source_phase(
    position: FractionalSourcePosition,
    start_phase: f64,
    frames: u64,
    rate: f64,
) {
    let expected = LOOP_START as f64
        + (start_phase - LOOP_START as f64 + frames as f64 * rate)
            .rem_euclid((LOOP_END - LOOP_START) as f64);
    let observed = position.frame as f64 + position.fraction;
    assert!(
        (observed - expected).abs() < 1.0e-6,
        "source phase={observed} expected={expected}"
    );
    assert_eq!(position.seek_mode, ExplicitSeekMode::Normal);
}

pub(super) fn assert_tone(pcm: &[f32], base: i32, extra: i32, rate: f64, lock: bool) {
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    let energy = pcm
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / pcm.len() as f64;
    assert!(
        energy > 0.001,
        "silent pitch route: base={base} extra={extra} energy={energy}"
    );
    let crossings: Vec<_> = pcm
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] <= 0.0 && pair[1] > 0.0)
        .map(|(index, pair)| index as f64 - f64::from(pair[0]) / f64::from(pair[1] - pair[0]))
        .collect();
    assert!(crossings.len() > 8, "insufficient audible tone cycles");
    let measured =
        (crossings.len() - 1) as f64 * f64::from(RATE) / (crossings.last().unwrap() - crossings[0]);
    // Independent raw integer oracle, deliberately outside production pitch helpers.
    let expected =
        SOURCE_HZ * 2.0_f64.powf(f64::from(base + extra) / 12.0) * if lock { 1.0 } else { rate };
    let cents = 1200.0 * (measured / expected).log2();
    assert!(
        cents.abs() < 15.0,
        "base={base} extra={extra} rate={rate} lock={lock} measured={measured} expected={expected} cents={cents}"
    );
}

fn assert_native_source(fixture: &Fixture, id: usize, action: &CapturedPitchAction) {
    let voice = fixture.voice(id);
    assert_eq!(voice.stretch.adopted_request_id(), Some(action.sequence()));
    assert_ne!(voice.stretch.native_state_address(), 0);
    assert!(
        voice
            .stretch
            .owns_pitch_contract(action.render().pitch, action.render().key_lock)
    );
    let history = voice.stretch.productive_history().unwrap();
    let sample = voice.sample.as_ref().unwrap();
    assert_eq!(history.binding.source_address, sample.source_address());
    assert_eq!(history.binding.resident, sample.resident_binding());
    assert_eq!(history.binding.sample_count, 144_000);
    assert_eq!(history.binding.channels, 1);
    assert_eq!(history.binding.sample_rate_hz, RATE);
    assert_eq!(history.binding.accepted, voice.source_timing.accepted);
    assert!(history.fed_output_frames >= 4096);
    let taps = voice.stretch.native_tap_observation().unwrap();
    assert!(taps.left_reads > 0);
    assert_eq!(taps.missing_reads, 0);
    assert!(taps.min_frame.unwrap() >= LOOP_START);
    assert!(taps.max_frame.unwrap() < LOOP_END);
}

#[test]
fn stopped_set_is_immediate_preserves_stop_and_equal_unity_retriggers_are_distinct_at_original_t() {
    let mut fixture = Fixture::new(false);
    let set = fixture.capture(0, "pitch.extra:0:set");
    assert!(fixture.control.engine.publish_pitch_action(&set).unwrap());
    assert!(
        !set.accepted(),
        "producer queueing alone is not an accepted native effect"
    );
    fixture.drain();
    assert!(
        set.accepted(),
        "existing scheduler slot owns actual action admission"
    );
    fixture.render(1);
    assert!(
        set.effective(),
        "state={} reason={:?}",
        set.state(),
        set.reason()
    );
    assert_eq!(set.execution_target(), set.original_due_frame());
    assert!(
        !fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    let first = fixture.capture(0, "pitch.extra:0:set_retrigger");
    let second = fixture.capture(0, "pitch.extra:0:set_retrigger");
    // Publish in the opposite order: original capture order still owns equal-T attacks.
    fixture
        .control
        .engine
        .publish_pitch_action(&second)
        .unwrap();
    fixture.control.engine.publish_pitch_action(&first).unwrap();
    fixture.drain();
    fixture.render(1);
    assert!(
        first.effective() && second.effective(),
        "first={} {:?}, second={} {:?}",
        first.state(),
        first.reason(),
        second.state(),
        second.reason()
    );
    assert_eq!(first.execution_target(), first.original_due_frame());
    assert_eq!(second.execution_target(), second.original_due_frame());
    assert_eq!(
        fixture
            .feedback
            .iter()
            .filter(|message| matches!(
                message,
                AudioMessage::ContentFeedback { kind: 1, .. } | AudioMessage::SampleStarted { .. }
            ))
            .count(),
        2
    );
    assert_eq!(
        fixture
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == 0)
            .unwrap()
            .generation,
        2
    );
    assert!(fixture.control.engine.publish_pitch_action(&first).unwrap());
    fixture.drain();
    assert!(fixture.scheduler.is_empty());
}

#[test]
fn original_capture_cannot_target_a_refilled_source_or_a_foreign_engine() {
    let mut fixture = Fixture::new(false);
    let action = fixture.capture(0, "pitch.extra:7:set_retrigger");
    let other = Fixture::new(false);
    assert!(other.control.engine.publish_pitch_action(&action).is_err());
    fixture.control.publish(0, 0.8, 20);
    assert!(
        fixture
            .control
            .engine
            .publish_pitch_action(&action)
            .is_err()
    );
    fixture.drain();
    fixture.render(1);
    assert!(
        !fixture
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    assert!(!action.effective());
}

#[test]
fn publication_clone_never_creates_a_second_heavy_owner_or_attack() {
    let fixture = Fixture::new(false);
    let action = fixture.capture(0, "pitch.extra:0:set_retrigger");
    let work = pitch_actions::PitchActionWork::new(
        action,
        RATE,
        1,
        fixture.control.engine.pitch_work_capacity.claim().unwrap(),
    )
    .unwrap();
    let publication = Arc::new(pitch_publication::PitchActionPublication::new(work));
    let clone = publication.clone();
    assert!(publication.take().is_some());
    assert!(clone.take().is_none());
}

#[test]
fn every_numeric_extra_publication_is_audible_from_its_own_normal_start_for_both_locks() {
    for lock in [false, true] {
        let mut fixture = Fixture::new(lock);
        for extra in -18..=18 {
            fixture.mixer.stop_sample_rt(0, &mut fixture.retirement);
            let action = fixture.capture(0, &format!("pitch.extra:{extra}:set_retrigger"));
            let original = action.original_due_frame();
            let timestamp = action.received_timestamp();
            assert_eq!(action.render().pitch, PitchIntent::new(0, extra).unwrap());
            assert_eq!(action.render().key_lock, lock);
            let previous_handle = fixture.mixer.voices[0].stretch.native_state_address();
            let previous_starts = fixture.starts(0);
            fixture.publish(&action);
            assert_eq!(action.original_due_frame(), original);
            assert_eq!(action.received_timestamp(), timestamp);
            assert_eq!(
                action.execution_target(),
                original + if extra == 0 { 0 } else { 8192 }
            );
            let before = fixture.apply(&action);
            assert!(
                before.iter().all(|sample| *sample == 0.0),
                "stopped RET started before E"
            );
            assert_eq!(fixture.starts(0), previous_starts + 1);
            assert_source_phase(
                fixture.voice(0).source_playback.position(),
                LOOP_START as f64,
                1,
                1.0,
            );
            assert_eq!(
                fixture.voice(0).explicit_seek_mode,
                ExplicitSeekMode::Normal
            );
            if extra != 0 {
                assert_native_source(&fixture, 0, &action);
                assert_ne!(
                    fixture.voice(0).stretch.native_state_address(),
                    previous_handle
                );
            }
            let pcm = fixture.pcm(48_000);
            assert_tone(&pcm[12_000..], 0, extra, 1.0, lock);
            assert_source_phase(
                fixture.voice(0).source_playback.position(),
                LOOP_START as f64,
                48_001,
                1.0,
            );
        }
    }
}

#[test]
fn public_base_resolution_total_edges_and_reset_preserve_the_numeric_extra() {
    for lock in [false, true] {
        let mut fixture = Fixture::new(lock);
        for (root, expected_base, extra) in [(7, -5, -18), (6, 6, 18)] {
            fixture.rows(PitchPadProjection {
                source_root: Some(0),
                source_mode: Some(PitchKeyMode::Major),
                intent: PitchIntent::new(0, extra).unwrap(),
                key_lock: lock,
                ..PitchPadProjection::default()
            });
            let base = fixture.capture(0, &format!("pitch.base:{root}:major:set_retrigger"));
            assert_eq!(
                base.render().pitch,
                PitchIntent::new(expected_base, extra).unwrap()
            );
            fixture.publish(&base);
            fixture.apply(&base);
            assert_native_source(&fixture, 0, &base);
            let pcm = fixture.pcm(48_000);
            assert_tone(&pcm[12_000..], expected_base, extra, 1.0, lock);
            let reset = fixture.capture(0, "pitch.base.reset:set_retrigger");
            assert_eq!(reset.render().pitch, PitchIntent::new(0, extra).unwrap());
            fixture.publish(&reset);
            fixture.apply(&reset);
            assert_native_source(&fixture, 0, &reset);
            let pcm = fixture.pcm(48_000);
            assert_tone(&pcm[12_000..], 0, extra, 1.0, lock);
            fixture.mixer.stop_sample_rt(0, &mut fixture.retirement);
        }
    }
}

#[test]
fn hot_set_retunes_without_restart_and_source_clock_keeps_varispeed_independent_of_pitch() {
    for rate in [0.5, 1.0, 2.0] {
        for lock in [false, true] {
            let mut fixture = Fixture::new(lock);
            fixture.mixer.set_speed(rate);
            let start = fixture.capture(0, "pitch.extra:0:set_retrigger");
            fixture.publish(&start);
            fixture.apply(&start);
            fixture.pcm(2_048);
            let before = fixture.voice(0);
            let generation = before.generation;
            let position = before.source_playback.position();
            let phase = position.frame as f64 + position.fraction;
            let starts = fixture.starts(0);
            let capture_frame = fixture.transport.output_frame();
            let action = fixture.capture(0, "pitch.extra:7:set");
            fixture.publish(&action);
            assert_eq!(action.original_due_frame(), capture_frame);
            assert_eq!(action.execution_target(), capture_frame + 8192);
            fixture.apply(&action);
            assert_eq!(fixture.voice(0).generation, generation);
            assert_eq!(fixture.starts(0), starts);
            assert!(!fixture.voice(0).paused);
            assert_source_phase(
                fixture.voice(0).source_playback.position(),
                phase,
                8193,
                rate,
            );
            assert_native_source(&fixture, 0, &action);
            let pcm = fixture.pcm(48_000);
            assert_tone(&pcm[12_000..], 0, 7, rate, lock);
            assert_source_phase(
                fixture.voice(0).source_playback.position(),
                phase,
                56_193,
                rate,
            );
        }
    }
}

fn assert_live_scalar_set_continuity(extra: i32, lock: bool) {
    let mut fixture = Fixture::new(lock);
    let mut reference = Fixture::new(lock);
    for current in [&mut fixture, &mut reference] {
        let initial = current.capture(0, &format!("pitch.extra:{extra}:set_retrigger"));
        assert!(
            current
                .control
                .engine
                .publish_pitch_action(&initial)
                .unwrap()
        );
        current.drain();
        if extra != 0 {
            // Only the initial independent nonzero attack needs Native preparation.
            // The live SET below has no worker wait or host-time readiness escape.
            current.wait_worker(&[&initial]);
            assert!(
                current
                    .render_to(initial.execution_target())
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
        }
        current.render(1);
        assert!(
            initial.effective(),
            "state={} reason={:?}",
            initial.state(),
            initial.reason()
        );
        current.pcm(733); // Leave a live Native/FIFO checkpoint off a block boundary.
    }
    assert_eq!(
        fixture.transport.output_frame(),
        reference.transport.output_frame()
    );
    let before = fixture.voice(0);
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let request = before.stretch.adopted_request_id();
    let cursor = before.source_playback;
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history().map(|history| {
        (
            history.binding,
            history.next_position,
            history.domain,
            history.fed_output_frames,
        )
    });
    if extra != 0 {
        assert!(
            before
                .stretch
                .owns_pitch_contract(PitchIntent::new(0, extra).unwrap(), lock)
        );
        assert!(history.is_some());
        assert!(
            fifo.0 + fifo.1 > 0,
            "exercise a populated live Native/FIFO checkpoint"
        );
    } else {
        assert!(
            !before.stretch.loop_acceptance_state().0,
            "exercise the actual dry-unity branch"
        );
        assert!(history.is_none());
    }
    let starts = fixture.starts(0);
    let frame = fixture.transport.output_frame();
    let native_creations = fixture.control.engine.pitch_work_capacity.allocations().1;
    let action = fixture.capture(0, &format!("pitch.extra:{extra}:set"));
    let timestamp = action.received_timestamp();
    assert_eq!(action.original_due_frame(), frame);
    assert!(
        fixture
            .control
            .engine
            .publish_pitch_action(&action)
            .unwrap()
    );
    fixture.drain();
    assert!(
        action.accepted(),
        "state={} reason={:?}",
        action.state(),
        action.reason()
    );
    assert_eq!(action.execution_target(), frame);
    assert_eq!(action.original_due_frame(), frame);
    assert_eq!(action.received_timestamp(), timestamp);
    // The real scheduler drains E=F before rendering, without progressing audio or
    // waiting for an unnecessary worker. A deferred/preparing SET fails here.
    fixture.render(0);
    assert_eq!(
        fixture.control.engine.pitch_work_capacity.allocations().1,
        native_creations
    );
    assert!(
        action.effective(),
        "state={} reason={:?}",
        action.state(),
        action.reason()
    );
    assert_eq!(fixture.transport.output_frame(), frame);
    assert!(fixture.scheduler.is_empty());
    let after = fixture.voice(0);
    assert_eq!(after.generation, generation);
    assert_eq!(fixture.starts(0), starts);
    assert_eq!(after.stretch.native_state_address(), native);
    assert_eq!(
        after.stretch.adopted_request_id(),
        request,
        "SET must retain the live history owner"
    );
    assert!(after.source_playback.matches_exact(&cursor));
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    assert_eq!(
        after.stretch.productive_history().map(|history| (
            history.binding,
            history.next_position,
            history.domain,
            history.fed_output_frames,
        )),
        history
    );
    assert!(!after.paused);
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(0, extra).unwrap())
    );

    // Independent uninterrupted rendering is the sample/FIFO continuity oracle.
    let mut pcm = fixture.render(1);
    pcm.extend(fixture.pcm(4095));
    let mut expected = reference.render(1);
    expected.extend(reference.pcm(4095));
    let maximum = pcm
        .iter()
        .zip(&expected)
        .map(|(actual, expected)| (actual - expected).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        maximum < 2.0e-5,
        "live scalar SET changed continuous PCM: {maximum}"
    );
    assert!(
        pcm.iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>()
            / pcm.len() as f64
            > 0.001
    );
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(fixture.voice(0).stretch.adopted_request_id(), request);
    assert_eq!(
        fixture.voice(0).stretch.pending_fifo_frames(),
        reference.voice(0).stretch.pending_fifo_frames()
    );
    assert!(
        fixture
            .voice(0)
            .source_playback
            .matches_exact(&reference.voice(0).source_playback)
    );
    assert_source_phase(
        fixture.voice(0).source_playback.position(),
        cursor.position().frame as f64 + cursor.position().fraction,
        pcm.len() as u64,
        1.0,
    );
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), starts);
}

#[test]
fn live_dry_unity_set_applies_at_original_frame_without_native_or_fifo_reset() {
    assert_live_scalar_set_continuity(0, false);
}

#[test]
fn live_same_owned_nonzero_set_applies_at_original_frame_without_native_or_fifo_reset() {
    assert_live_scalar_set_continuity(7, true);
}

#[test]
fn stopped_and_paused_set_preserve_transport_while_ret_applies_one_own_nonzero_attack() {
    for lock in [false, true] {
        let mut fixture = Fixture::new(lock);
        let stopped = fixture.capture(0, "pitch.extra:7:set");
        fixture.publish(&stopped);
        assert_eq!(stopped.execution_target(), stopped.original_due_frame());
        fixture.apply(&stopped);
        assert!(
            !fixture
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert!(fixture.pcm(512).iter().all(|sample| *sample == 0.0));
        assert_eq!(fixture.starts(0), 0);
        let first = fixture.capture(0, "pitch.extra:7:set_retrigger");
        fixture.publish(&first);
        fixture.apply(&first);
        fixture.pcm(1024);
        fixture
            .mixer
            .pause_sample_at_output_frame(0, fixture.transport.output_frame());
        let before = fixture.voice(0);
        let position = before.source_playback.position();
        let generation = before.generation;
        let paused = fixture.capture(0, "pitch.extra:-7:set");
        fixture.publish(&paused);
        let before_e = fixture.apply(&paused);
        assert!(before_e.iter().all(|sample| *sample == 0.0));
        assert_eq!(fixture.voice(0).source_playback.position(), position);
        assert_eq!(fixture.voice(0).generation, generation);
        assert!(fixture.voice(0).paused);
        assert_eq!(fixture.starts(0), 1);
        assert_native_source(&fixture, 0, &paused);
        assert!(fixture.pcm(1024).iter().all(|sample| *sample == 0.0));
        fixture
            .mixer
            .resume_sample_at_output_frame(0, fixture.transport.output_frame());
        let resumed = fixture.pcm(48_000);
        assert_tone(&resumed[12_000..], 0, -7, 1.0, lock);
        let repeat = fixture.capture(0, "pitch.extra:-7:set_retrigger");
        let previous_handle = fixture.voice(0).stretch.native_state_address();
        let previous_generation = fixture.voice(0).generation;
        fixture.publish(&repeat);
        fixture.apply(&repeat);
        assert_eq!(fixture.starts(0), 2);
        assert_eq!(fixture.voice(0).generation, previous_generation + 1);
        assert_ne!(
            fixture.voice(0).stretch.native_state_address(),
            previous_handle
        );
        assert_native_source(&fixture, 0, &repeat);
        assert_source_phase(
            fixture.voice(0).source_playback.position(),
            LOOP_START as f64,
            1,
            1.0,
        );
        let repeated = fixture.pcm(48_000);
        assert_tone(&repeated[12_000..], 0, -7, 1.0, lock);
    }
}

#[test]
fn captured_quantize_sync_gui_and_midi_keep_original_t_tuple_content_and_timestamp() {
    for quantize in [false, true] {
        for sync in [false, true] {
            for midi in [false, true] {
                for lock in [false, true] {
                    let mut fixture = Fixture::new(lock);
                    fixture.render_to(17_000);
                    // Existing global BPM_LOCK controls real source time. The per-input SYNC
                    // projection is frozen metadata and cannot toggle a later global setting.
                    fixture.mixer.set_pad_bpm(0, Some(80.0));
                    fixture.mixer.set_bpm_lock(sync);
                    if sync {
                        fixture.mixer.set_master_bpm(120.0);
                    }
                    let source_rate = if sync { 120.0 / 80.0 } else { 1.0 };
                    fixture.rows(PitchPadProjection {
                        source_root: Some(0),
                        source_mode: Some(PitchKeyMode::Major),
                        intent: PitchIntent::new(-5, 3).unwrap(),
                        key_lock: lock,
                        quantize_step_64ths: quantize.then_some(16),
                        sync,
                        ..PitchPadProjection::default()
                    });
                    let action = if midi {
                        fixture.midi_capture("pitch.extra:7:set_retrigger")
                    } else {
                        fixture.capture(0, "pitch.extra:7:set_retrigger")
                    };
                    let content = action.content_key();
                    let timestamp = action.received_timestamp();
                    let original = if quantize { 24_000 } else { 17_000 };
                    assert_eq!(action.original_due_frame(), original);
                    assert_eq!(action.render().pitch, PitchIntent::new(-5, 7).unwrap());
                    assert_eq!(action.render().key_lock, lock);
                    assert_eq!(action.sync(), sync);
                    // Later GUI projection and clock grid edits cannot relabel this original input.
                    fixture.rows(PitchPadProjection {
                        source_root: Some(8),
                        source_mode: Some(PitchKeyMode::Minor),
                        intent: PitchIntent::new(6, -18).unwrap(),
                        key_lock: !lock,
                        quantize_step_64ths: Some(1),
                        sync: !sync,
                        ..PitchPadProjection::default()
                    });
                    let mut edited_clock = fixture.clock.read().unwrap();
                    edited_clock.master_period_seconds = Some(0.75);
                    edited_clock.master_beat = 100.0;
                    fixture.clock.publish(edited_clock);
                    fixture.publish(&action);
                    assert_eq!(
                        action.execution_target(),
                        if quantize { original } else { original + 8192 }
                    );
                    fixture.wait_worker(&[&action]);
                    assert_eq!(fixture.transport.output_frame(), 17_000);
                    let before_e = fixture.apply(&action);
                    assert!(before_e.iter().all(|sample| *sample == 0.0));
                    assert_eq!(action.original_due_frame(), original);
                    assert_eq!(action.content_key(), content);
                    assert_eq!(action.received_timestamp(), timestamp);
                    assert_eq!(
                        fixture.mixer.pad_pitch_intent(0),
                        Some(PitchIntent::new(-5, 7).unwrap())
                    );
                    assert_native_source(&fixture, 0, &action);
                    assert_eq!(fixture.voice(0).source_playback.tempo_ratio(), source_rate);
                    assert_source_phase(
                        fixture.voice(0).source_playback.position(),
                        LOOP_START as f64,
                        1,
                        source_rate,
                    );
                    let pcm = fixture.pcm(48_000);
                    assert_tone(&pcm[12_000..], -5, 7, source_rate, lock);
                    assert_source_phase(
                        fixture.voice(0).source_playback.position(),
                        LOOP_START as f64,
                        48_001,
                        source_rate,
                    );
                }
            }
        }
    }
}

#[test]
fn nonzero_equal_t_retriggers_each_own_a_worker_candidate_and_attack() {
    let mut fixture = Fixture::new(true);
    let first = fixture.capture(0, "pitch.extra:7:set_retrigger");
    let second = fixture.capture(0, "pitch.extra:7:set_retrigger");
    assert!(first.sequence() < second.sequence());
    assert_eq!(first.original_due_frame(), second.original_due_frame());
    fixture.publish(&second);
    fixture.publish(&first);
    assert_eq!(fixture.scheduler.len(), 2);
    assert_eq!(first.execution_target(), second.execution_target());
    fixture.wait_worker(&[&first, &second]);
    fixture.render_to(first.execution_target());
    fixture.render(1);
    assert!(
        first.effective() && second.effective(),
        "first={} {:?} second={} {:?}",
        first.state(),
        first.reason(),
        second.state(),
        second.reason()
    );
    assert_eq!(fixture.starts(0), 2);
    assert_eq!(fixture.voice(0).generation, 2);
    assert_native_source(&fixture, 0, &second);
    assert_source_phase(
        fixture.voice(0).source_playback.position(),
        LOOP_START as f64,
        1,
        1.0,
    );
    let pcm = fixture.pcm(48_000);
    assert_tone(&pcm[12_000..], 0, 7, 1.0, true);
    assert!(fixture.control.engine.publish_pitch_action(&first).unwrap());
    fixture.drain();
    assert!(fixture.scheduler.is_empty());
    assert_eq!(fixture.starts(0), 2);
}

#[test]
fn another_pad_keeps_its_bytes_and_source_clock_through_hot_set_and_retrigger() {
    let mut edited = Fixture::new(false);
    let mut baseline = Fixture::new(false);
    for fixture in [&mut edited, &mut baseline] {
        for id in [0, 1] {
            let action = fixture.capture(id, "pitch.extra:0:set_retrigger");
            fixture.publish(&action);
            fixture.apply(&action);
        }
        fixture.pcm(1_024);
    }
    let other_generation = edited.voice(1).generation;
    let other_phase = edited.voice(1).source_playback.position();
    let other_handle = edited.voice(1).stretch.native_state_address();
    for variant in ["set", "set_retrigger"] {
        let action = edited.capture(0, &format!("pitch.extra:7:{variant}"));
        edited.publish(&action);
        edited.wait_worker(&[&action]);
        let target = action.execution_target();
        edited.render_to(target);
        baseline.render_to(target);
        edited.render(1);
        baseline.render(1);
        assert!(action.effective());
        assert_eq!(edited.voice(1).generation, other_generation);
        assert_eq!(edited.voice(1).stretch.native_state_address(), other_handle);
        assert!(
            edited
                .voice(1)
                .source_playback
                .matches_exact(&baseline.voice(1).source_playback)
        );
        // Pause the changed pad in both fixtures to isolate actual unaffected-pad PCM.
        edited
            .mixer
            .pause_sample_at_output_frame(0, edited.transport.output_frame());
        baseline
            .mixer
            .pause_sample_at_output_frame(0, baseline.transport.output_frame());
        assert_eq!(edited.pcm(4_096), baseline.pcm(4_096));
        assert_eq!(edited.starts(1), 1);
        assert_source_phase(
            edited.voice(1).source_playback.position(),
            other_phase.frame as f64 + other_phase.fraction,
            edited.transport.output_frame() - 1_026,
            1.0,
        );
        edited
            .mixer
            .resume_sample_at_output_frame(0, edited.transport.output_frame());
        baseline
            .mixer
            .resume_sample_at_output_frame(0, baseline.transport.output_frame());
    }
}

#[test]
fn own_source_lifetime_ack_and_epoch_permit_cannot_be_relabelled_by_equal_pcm() {
    let fixture = Fixture::new(true);
    let foreign = Fixture::new(true);
    let action = fixture.capture(0, "pitch.extra:7:set_retrigger");
    let sample = fixture.control.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let permit = action.capture_history_permit(&sample, RATE, None).unwrap();
    let ownership = &fixture.control.engine.input_runtime_ownership;
    let ack = &fixture.control.engine.current_timing_acknowledgements;
    assert!(permit.matches_action_owner(
        ownership,
        ack,
        action.content_key(),
        action.preparation_epoch()
    ));
    assert!(!permit.matches_action_owner(
        &foreign.control.engine.input_runtime_ownership,
        ack,
        action.content_key(),
        action.preparation_epoch()
    ));
    assert!(!permit.matches_action_owner(
        ownership,
        &Arc::new(constant_timing::CurrentTimingAcknowledgements::default()),
        action.content_key(),
        action.preparation_epoch()
    ));
    assert!(!permit.matches_action_owner(
        ownership,
        ack,
        action.content_key(),
        &Arc::new(AtomicU64::new(1))
    ));
    let other_action = fixture.capture(1, "pitch.extra:7:set_retrigger");
    assert!(!permit.matches_action_owner(
        ownership,
        ack,
        other_action.content_key(),
        action.preparation_epoch()
    ));
    let copied = tone(SOURCE_HZ);
    assert_eq!(sample.samples.as_ref(), copied.samples.as_ref());
    assert!(!permit.current(&copied, RATE));
    assert!(!permit.current(&sample, RATE / 2));
}

#[test]
fn prepared_source_change_and_remove_refill_reject_without_pitch_attack_or_cursor_mutation() {
    for refill in [false, true] {
        let mut fixture = Fixture::new(false);
        let start = fixture.capture(0, "pitch.extra:0:set_retrigger");
        fixture.publish(&start);
        fixture.apply(&start);
        fixture.pcm(1024);
        let action = fixture.capture(0, "pitch.extra:7:set_retrigger");
        fixture.publish(&action);
        fixture.wait_worker(&[&action]);
        let original_t = action.original_due_frame();
        let execution = action.execution_target();
        let before = fixture.voice(0);
        let generation = before.generation;
        let position = before.source_playback.position();
        let phase = position.frame as f64 + position.fraction;
        let capture_frame = fixture.transport.output_frame();
        let native = before.stretch.native_state_address();
        let ownership = fixture.control.engine.input_runtime_ownership.clone();
        if refill {
            let current = ownership.slot_snapshot(0).unwrap();
            let empty = ownership.prepare_slot_write_fixed(current, 0).unwrap();
            ownership
                .retire_source_write(&empty, ownership.next_authority(0).unwrap(), || true)
                .unwrap();
        }
        publish_tone(&fixture.control, 0, tone(SOURCE_HZ), refill);
        fixture.render_to(execution);
        fixture.render(1);
        assert!(!action.effective());
        assert_eq!(action.state(), "rejected");
        assert_eq!(action.original_due_frame(), original_t);
        assert_eq!(action.execution_target(), execution);
        assert_eq!(fixture.starts(0), 1);
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(
            fixture.mixer.pad_pitch_intent(0),
            Some(PitchIntent::default())
        );
        assert_source_phase(
            fixture.voice(0).source_playback.position(),
            phase,
            execution + 1 - capture_frame,
            1.0,
        );
        assert!(fixture.scheduler.is_empty());
        let pcm = fixture.pcm(48_000);
        assert_tone(&pcm[12_000..], 0, 0, 1.0, false);
    }
}

#[test]
fn public_stop_and_global_cancel_reject_prepared_pitch_attack_but_preserve_a_nonstarting_set() {
    for cancel_all in [false, true] {
        let mut fixture = Fixture::new(false);
        fixture.render_to(17_000);
        fixture.rows(PitchPadProjection {
            quantize_step_64ths: Some(16),
            ..PitchPadProjection::default()
        });
        let attack = fixture.capture(0, "pitch.extra:7:set_retrigger");
        let set = fixture.capture(0, "pitch.extra:3:set");
        assert_eq!(attack.original_due_frame(), 24_000);
        assert_eq!(set.original_due_frame(), 24_000);
        fixture.publish(&attack);
        fixture.publish(&set);
        fixture.wait_worker(&[&attack, &set]);
        if cancel_all {
            // This is the same native cancellation called before Python's GLOBAL STOP
            // returns early for an empty active/paused set. Pending-only attacks count too.
            assert!(
                fixture
                    .control
                    .engine
                    .cancel_all_launches()
                    .unwrap()
                    .is_empty()
            );
        } else {
            fixture.control.engine.stop_sample(0).unwrap();
        }
        fixture.drain();
        fixture.render_to(24_000);
        fixture.render(1);
        assert_eq!(attack.state(), "rejected");
        assert_eq!(attack.reason(), Some("launch_cancelled"));
        assert!(!attack.effective());
        assert_eq!(attack.original_due_frame(), 24_000);
        assert_eq!(attack.execution_target(), 24_000);
        assert!(set.effective());
        assert_eq!(fixture.starts(0), 0);
        assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
        assert_eq!(
            fixture.mixer.pad_pitch_intent(0),
            Some(PitchIntent::new(0, 3).unwrap())
        );
        assert!(fixture.scheduler.is_empty());
    }
}
