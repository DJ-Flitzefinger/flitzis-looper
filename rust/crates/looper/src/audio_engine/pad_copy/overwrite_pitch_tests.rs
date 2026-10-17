//! Real occupied Copy versus the removed content's original scheduled pitch owner.
//! Host worker waits freeze virtual time; no device or musical acceptance is claimed.

use super::*;
use crate::audio_engine::input_mapping::{InputRuntime, PitchPadProjection};
use crate::audio_engine::pitch_actions::{CapturedPitchAction, PitchKeyMode};
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::timing::{OutputClockSnapshot, SharedOutputClock};

const TARGET: usize = 36;
// The existing nearest-grid policy chooses 48k from 42k, before the effect.
const CAPTURE_FRAME: u64 = 42_000;
const ORIGINAL_E: u64 = 48_000;

struct PitchFixture {
    copy: Fixture,
    clock: Arc<SharedOutputClock>,
}

impl PitchFixture {
    fn new(lock: bool) -> Self {
        let copy = Fixture::new(false, lock, "manual");
        let clock = Arc::new(SharedOutputClock::new());
        copy.with_engine(|engine| {
            engine.input_runtime = Some(InputRuntime::new_with_ownership(
                copy.producer.clone(),
                engine.input_clock,
                engine.input_runtime_ownership.clone(),
            ));
            let runtime = engine.input_runtime.as_ref().unwrap();
            runtime.set_render_format(RATE, 2).unwrap();
            runtime.attach_pitch_clock(clock.clone()).unwrap();
        });
        let mut fixture = Self { copy, clock };
        // Both components are real actions, not an injected mixer scalar. The
        // copied octave and offset must remain distinct even though total = +7.
        fixture.apply_set("pitch.note:12:set");
        fixture.apply_set("pitch.extra:-5:set");
        assert_eq!(
            fixture.copy.callback.mixer.pad_pitch_intent(0),
            Some(copied_pitch())
        );
        assert_ne!(
            fixture.copy.source_voice().stretch.native_state_address(),
            0
        );
        fixture
    }

    fn sync_rows(&self, quantized: Option<usize>) {
        self.copy.with_engine(|engine| {
            let mut loaded = vec![false; NUM_SAMPLES];
            let mut starts = vec![0.0; NUM_SAMPLES];
            let mut ends = vec![None; NUM_SAMPLES];
            let mut bindings: Vec<_> = (0..NUM_SAMPLES).map(|_| None).collect();
            let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
            for id in [0, TARGET] {
                let binding = engine.current_input_runtime_pad_binding(id).unwrap();
                if let Some(binding) = binding {
                    loaded[id] = true;
                    starts[id] = START as f64 / f64::from(RATE);
                    ends[id] = Some(END as f64 / f64::from(RATE));
                    bindings[id] = Some(binding);
                    rows[id] = PitchPadProjection {
                        source_root: Some(0),
                        source_mode: Some(PitchKeyMode::Major),
                        intent: self.copy.callback.mixer.pad_pitch_intent(id).unwrap(),
                        key_lock: self.copy.key_lock,
                        quantize_step_64ths: (quantized == Some(id)).then_some(16),
                        ..PitchPadProjection::default()
                    };
                }
            }
            engine
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
            let now = engine.input_clock.capture_ns();
            let frame = self.copy.callback.frame;
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
        });
    }

    fn capture(&self, id: usize, key: &str, quantized: bool) -> CapturedPitchAction {
        self.sync_rows(quantized.then_some(id));
        self.copy
            .with_engine(|engine| engine.capture_pad_pitch_action(id, key, None).unwrap())
    }

    fn publish(&mut self, action: &CapturedPitchAction) {
        self.copy
            .with_engine(|engine| assert!(engine.publish_pitch_action(action).unwrap()));
        self.copy.callback.drain();
        assert!(action.accepted(), "{:?}", action.reason());
    }

    fn wait_ready(&mut self, action: &CapturedPitchAction) {
        let frame = self.copy.callback.frame;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let runtime = self.copy.callback.mixer.pitch_runtime.as_mut().unwrap();
            runtime.poll();
            if runtime.has_ready_action(action.sequence()) {
                break;
            }
            assert_ne!(action.state(), "rejected", "{:?}", action.reason());
            assert!(
                Instant::now() < deadline,
                "original pitch worker did not become ready"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(self.copy.callback.frame, frame);
        assert_eq!(self.copy.callback.transport.output_frame(), frame);
    }

    fn render_to(&mut self, target: u64) {
        assert!(target >= self.copy.callback.frame);
        while self.copy.callback.frame < target {
            let frames = (target - self.copy.callback.frame).min(257) as usize;
            self.copy.callback.render(frames);
        }
    }

    fn apply_set(&mut self, key: &str) {
        let action = self.capture(0, key, false);
        let starts = self.copy.callback.feedback.starts[0];
        self.publish(&action);
        self.wait_ready(&action);
        self.render_to(action.execution_target());
        self.copy.callback.render(1);
        assert!(action.effective(), "{:?}", action.reason());
        assert_eq!(self.copy.callback.feedback.starts[0], starts);
    }

    fn envelope(&self) -> String {
        let mut envelope: Value = serde_json::from_str(&self.copy.envelope()).unwrap();
        envelope["snapshot"]["key_intent"] = json!({
            "schema_version":1,"source":null,"correction":null,
            "analysis_epoch":0,"correction_epoch":0,
            "base_shift":12,"extra_shift":-5,"retrigger":true,"extra_retrigger":false,
        });
        envelope.to_string()
    }

    fn copy_to_target(&mut self, overwrite: bool) -> PadCopyTicket {
        let captured = self.copy.with_engine(|engine| {
            let source = engine
                .current_input_runtime_pad_binding(0)
                .unwrap()
                .unwrap();
            let target = overwrite.then(|| {
                engine
                    .current_input_runtime_pad_binding(TARGET)
                    .unwrap()
                    .unwrap()
            });
            engine
                .capture_pad_copy(&source, TARGET, &self.envelope(), target.as_ref())
                .unwrap()
        });
        self.copy.prepare(&captured)
    }

    fn settle(&mut self, ticket: &PadCopyTicket) {
        assert_eq!(ticket.publication_status(), "accepted");
        self.copy.with_engine(|engine| {
            assert!(engine.reconcile_pad_copy(ticket).unwrap());
            engine.finish_pad_copy(ticket).unwrap();
        });
    }

    fn expect_pitch_start(&mut self, action: &CapturedPitchAction) {
        let key = action.content_key();
        let ownership = &self.copy.callback.feedback.ownership;
        let expected = StartFeedback {
            owner: ownership.owner_identity().diagnostic(),
            cell: key.cell().index(),
            lifetime: key.lifetime(),
            transport_revision: ownership
                .transport_intent_for_cell(key.cell())
                .load(Ordering::Acquire)
                .checked_add(1)
                .unwrap(),
        };
        assert!(
            self.copy.callback.feedback.expected_starts[TARGET]
                .replace(expected)
                .is_none()
        );
    }
}

fn copied_pitch() -> PitchIntent {
    PitchIntent::new(12, -5).unwrap()
}

fn assert_source_chronology(actual: &PitchFixture, reference: &PitchFixture) {
    let a = actual.copy.source_voice();
    let b = reference.copy.source_voice();
    assert!(a.source_playback.matches_exact(&b.source_playback));
    assert_eq!(
        a.stretch.pending_fifo_frames(),
        b.stretch.pending_fifo_frames()
    );
    let history = a.stretch.productive_history().unwrap();
    let reference_history = b.stretch.productive_history().unwrap();
    assert_eq!(history.next_position, reference_history.next_position);
    assert_eq!(history.domain, reference_history.domain);
    assert_eq!(
        history.fed_output_frames,
        reference_history.fed_output_frames
    );
    let dsp = actual
        .copy
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let reference_dsp = reference
        .copy
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(dsp.next_position, reference_dsp.next_position);
    assert_eq!(dsp.fed_output_frames, reference_dsp.fed_output_frames);
}

#[derive(Clone, Copy, Debug)]
enum Order {
    Prepared,
    PitchFirst,
    CopyFirst,
}

fn case(order: Order, lock: bool) {
    let mut actual = PitchFixture::new(lock);
    let mut reference = PitchFixture::new(lock);
    assert_eq!(actual.copy.callback.frame, reference.copy.callback.frame);
    let original_b = actual.copy_to_target(false);
    actual
        .copy
        .with_engine(|engine| engine.publish_pad_copy(&original_b).unwrap());
    actual.copy.callback.drain();
    actual.settle(&original_b);
    actual.sync_rows(None);
    actual.copy.start(TARGET);
    actual.render_to(CAPTURE_FRAME);
    reference.render_to(CAPTURE_FRAME);
    assert_source_chronology(&actual, &reference);

    let action = actual.capture(TARGET, "pitch.note:0:set_retrigger", true);
    let original = (
        action.sequence(),
        action.content_key(),
        action.received_timestamp(),
    );
    assert_eq!(action.render().pitch, PitchIntent::new(0, -5).unwrap());
    assert_eq!(action.original_due_frame(), ORIGINAL_E);
    if matches!(order, Order::Prepared) {
        actual.publish(&action);
        actual.wait_ready(&action);
        assert_eq!(action.execution_target(), ORIGINAL_E);
    }
    let source_native = actual.copy.source_voice().stretch.native_state_address();
    let source_key = actual.copy.with_engine(|engine| {
        engine
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap()
            .content_key()
            .unwrap()
    });
    let source_generation = actual.copy.source_voice().generation;
    let source_adopted = actual.copy.source_voice().stretch.adopted_request_id();
    let source_position = actual.copy.source_voice().source_playback;
    let source_history = actual
        .copy
        .source_voice()
        .stretch
        .productive_history()
        .unwrap();
    let source_dsp = actual
        .copy
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let source_sample = actual.copy.source_voice().sample.as_ref().unwrap().clone();
    let starts_before = actual.copy.callback.feedback.starts;
    let replacement = actual.copy_to_target(true);
    actual.copy.with_engine(|engine| {
        if matches!(order, Order::PitchFirst) {
            assert!(engine.publish_pitch_action(&action).unwrap());
        }
        engine.publish_pad_copy(&replacement).unwrap();
        if matches!(order, Order::CopyFirst) {
            assert!(engine.publish_pitch_action(&action).unwrap());
        }
    });
    actual.copy.callback.drain();
    actual.settle(&replacement);
    assert_eq!(actual.copy.callback.frame, CAPTURE_FRAME);
    assert_eq!(actual.copy.callback.feedback.starts, starts_before);
    let new_binding = actual.copy.with_engine(|engine| {
        engine
            .current_input_runtime_pad_binding(TARGET)
            .unwrap()
            .unwrap()
    });
    let new_key = new_binding
        .binding
        .placement
        .unwrap()
        .content_key()
        .unwrap();
    assert_ne!(new_key, original.1);
    assert_ne!(new_key.lifetime(), original.1.lifetime());
    assert_ne!(new_key.lifetime(), source_key.lifetime());
    assert_eq!(
        actual.copy.callback.mixer.pad_pitch_intent(TARGET),
        Some(copied_pitch())
    );
    assert!(
        !actual
            .copy
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(TARGET))
    );
    assert!(
        actual
            .copy
            .source_voice()
            .source_playback
            .matches_exact(&source_position)
    );
    let retained = actual
        .copy
        .source_voice()
        .stretch
        .productive_history()
        .unwrap();
    assert_eq!(retained.binding, source_history.binding);
    assert_eq!(retained.next_position, source_history.next_position);
    assert_eq!(retained.domain, source_history.domain);
    assert_eq!(retained.fed_output_frames, source_history.fed_output_frames);

    // Run across old E and beyond a complete preparation lead. Different block
    // sizes cross Native FIFO and loop boundaries without resetting either owner.
    let mut audible = false;
    for frames in [1, 127, 257, 509].into_iter().cycle().take(140) {
        let pcm = actual.copy.callback.render(frames);
        assert_eq!(
            pcm,
            reference.copy.callback.render(frames),
            "order={order:?} lock={lock}"
        );
        audible |= pcm.iter().any(|sample| sample.abs() > 0.0001);
        assert_source_chronology(&actual, &reference);
        let source = actual.copy.source_voice();
        assert_eq!(source.generation, source_generation);
        assert_eq!(source.stretch.native_state_address(), source_native);
        assert_eq!(source.stretch.adopted_request_id(), source_adopted);
        assert_eq!(
            source.stretch.productive_history().unwrap().binding,
            source_history.binding
        );
        assert!(Arc::ptr_eq(
            &source.sample.as_ref().unwrap().samples,
            &source_sample.samples
        ));
        assert_eq!(
            actual
                .copy
                .callback
                .mixer
                .dsp_source_history_for_test(0)
                .unwrap()
                .binding,
            source_dsp.binding
        );
        assert_eq!(actual.copy.callback.feedback.starts, starts_before);
        assert_eq!(
            actual.copy.callback.mixer.pad_pitch_intent(TARGET),
            Some(copied_pitch())
        );
    }
    assert!(audible);
    assert!(actual.copy.callback.frame > ORIGINAL_E + 8192);
    assert_eq!(action.state(), "rejected");
    assert_eq!(action.reason(), Some("stale_content"));
    assert!(!action.effective());
    if matches!(order, Order::CopyFirst) {
        assert!(!action.accepted());
    } else {
        assert!(action.accepted());
        assert_eq!(action.execution_target(), ORIGINAL_E);
    }
    assert_eq!(
        (
            action.sequence(),
            action.content_key(),
            action.received_timestamp()
        ),
        original
    );
    assert_eq!(action.original_due_frame(), ORIGINAL_E);
    assert!(actual.copy.callback.scheduler.is_empty());

    let fresh = actual.capture(TARGET, "pitch.extra.step:1:set_retrigger", false);
    assert_eq!(fresh.content_key(), new_key);
    assert_ne!(fresh.content_key(), original.1);
    assert_eq!(fresh.render().pitch, PitchIntent::new(12, -4).unwrap());
    actual.expect_pitch_start(&fresh);
    actual.publish(&fresh);
    actual.wait_ready(&fresh);
    while actual.copy.callback.frame < fresh.execution_target() {
        let frames = (fresh.execution_target() - actual.copy.callback.frame).min(257) as usize;
        assert_eq!(
            actual.copy.callback.render(frames),
            reference.copy.callback.render(frames)
        );
    }
    actual.copy.callback.render(1);
    reference.copy.callback.render(1);
    assert!(fresh.effective(), "{:?}", fresh.reason());
    assert_eq!(
        actual.copy.callback.feedback.starts[TARGET],
        starts_before[TARGET] + 1
    );
    let c = actual
        .copy
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(TARGET))
        .unwrap();
    assert_ne!(c.stretch.native_state_address(), 0);
    assert_ne!(c.stretch.native_state_address(), source_native);
    assert_eq!(c.stretch.adopted_request_id(), Some(fresh.sequence()));
    assert_eq!(
        c.stretch.native_origin_tuple(),
        Some((fresh.render().pitch, lock, fresh.sequence()))
    );
    assert!(c.stretch.productive_history().unwrap().fed_output_frames >= 4096);
    assert!(Arc::ptr_eq(
        &c.sample.as_ref().unwrap().samples,
        &source_sample.samples
    ));
    assert_productive_native_source(c);
    assert_eq!(
        actual.copy.callback.mixer.pad_pitch_intent(TARGET),
        Some(fresh.render().pitch)
    );
    let mixed = actual.copy.callback.render(512);
    let source_only = reference.copy.callback.render(512);
    assert!(
        mixed
            .iter()
            .zip(&source_only)
            .any(|(a, b)| (a - b).abs() > 0.0001)
    );
    assert_source_chronology(&actual, &reference);
    assert_eq!(
        actual.copy.source_voice().stretch.native_state_address(),
        source_native
    );
}

#[test]
fn occupied_copy_fences_prepared_pitch_and_preserves_fresh_octave_intent() {
    isolated(
        "overwrite_pitch_tests::occupied_copy_fences_prepared_pitch_and_preserves_fresh_octave_intent",
        || {
            for lock in [false, true] {
                case(Order::Prepared, lock);
            }
        },
    );
}

#[test]
fn occupied_copy_fences_pitch_first_queue_without_touching_source_chronology() {
    isolated(
        "overwrite_pitch_tests::occupied_copy_fences_pitch_first_queue_without_touching_source_chronology",
        || {
            for lock in [false, true] {
                case(Order::PitchFirst, lock);
            }
        },
    );
}

#[test]
fn occupied_copy_fences_copy_first_queue_without_attacking_replacement() {
    isolated(
        "overwrite_pitch_tests::occupied_copy_fences_copy_first_queue_without_attacking_replacement",
        || {
            for lock in [false, true] {
                case(Order::CopyFirst, lock);
            }
        },
    );
}
