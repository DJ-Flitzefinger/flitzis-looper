//! Real occupied Copy versus the removed content's original scheduled pitch owner.
//! Host worker waits freeze virtual time; no device or musical acceptance is claimed.

use super::*;
use crate::audio_engine::committed_stem_residency::{CommittedStemSetTicket, ResidentStemTicket};
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
    selected: Option<CommittedStemSetTicket>,
}

impl PitchFixture {
    fn new(lock: bool) -> Self {
        let copy = Fixture::new(false, lock, "manual");
        let mut fixture = Self::from_copy(copy);
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

    fn from_copy(copy: Fixture) -> Self {
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
        Self {
            copy,
            clock,
            selected: None,
        }
    }

    fn new_selected(lock: bool) -> Self {
        // Reuse the already accepted own registration. The unregistered pair
        // prepared before the fixture's ordinary start is not a fresh candidate.
        // Fixed original targets below align the independent later histories.
        let copy = Fixture::new(true, lock, "manual");
        let selection = copy.with_engine(|engine| {
            let source = engine
                .capture_prepared_source(0, copy.version.clone())
                .unwrap();
            engine
                .register_committed_stem_pair_with_producer(
                    copy.pair.as_ref().unwrap(),
                    &source,
                    &copy.producer,
                )
                .unwrap()
        });
        let mut fixture = Self::from_copy(copy);
        fixture.copy.callback.drain();
        assert_eq!(
            selection.publication_status(),
            "accepted",
            "{:?}",
            selection.error()
        );
        assert!(selection.is_current());
        let mode = fixture.components(&selection, 15);
        fixture.selected = Some(selection);
        fixture.render_to(18_000);
        assert_eq!(
            mode.effective_mode_status(),
            "effective",
            "{:?}",
            mode.error()
        );
        // Fixed original targets align both independent musical histories. A's
        // later normal-start retrigger also resets its own Native/DSP history.
        for (capture_at, key, retrigger, due) in [
            (18_000, "pitch.extra:-5:set", false, 24_000),
            (42_000, "pitch.note:12:set_retrigger", true, 48_000),
        ] {
            fixture.render_to(capture_at);
            let action = fixture.capture(0, key, true);
            assert_eq!(action.original_due_frame(), due);
            if retrigger {
                fixture.expect_pitch_start(&action);
            }
            fixture.publish(&action);
            fixture.wait_ready(&action);
            assert_eq!(action.execution_target(), due);
            fixture.render_to(due);
            fixture.copy.callback.render(1);
            assert!(action.effective(), "{:?}", action.reason());
        }
        assert_eq!(
            fixture.copy.callback.mixer.pad_pitch_intent(0),
            Some(copied_pitch())
        );
        fixture
    }

    fn components(&mut self, selection: &CommittedStemSetTicket, mask: u8) -> ResidentStemTicket {
        assert!(selection.is_current());
        let ticket = self.copy.with_engine(|engine| {
            engine
                .request_committed_stem_residency_with_producer(
                    selection,
                    "all_stems",
                    mask,
                    &self.copy.producer,
                )
                .unwrap()
        });
        let frame = self.copy.callback.frame;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.copy.callback.drain();
            if ticket.residency_status() == "accepted" {
                break;
            }
            assert_ne!(
                ticket.residency_status(),
                "rejected",
                "{:?}",
                ticket.error()
            );
            assert!(
                Instant::now() < deadline,
                "own selected components did not acknowledge"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        self.copy
            .with_engine(|engine| resident_relocation::reconcile(engine).unwrap());
        assert_eq!(self.copy.callback.frame, frame);
        assert!(ticket.is_current(), "{:?}", ticket.error());
        ticket
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
        if self.selected.is_some() {
            envelope["snapshot"]["stem_intent"]["enabled_mask"] = json!(15);
            envelope["snapshot"]["stem_intent"]["display_mode"] = json!("all");
        }
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
        let id = action.current_control_slot().unwrap();
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
            self.copy.callback.feedback.expected_starts[id]
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
    run_case(order, lock, false);
}

fn assert_selected_view(
    fixture: &PitchFixture,
    id: usize,
    selection: &CommittedStemSetTicket,
    mask: u8,
    live: bool,
) {
    assert_eq!(selection.publication_status(), "accepted");
    assert!(selection.is_current());
    let mixer = &fixture.copy.callback.mixer;
    assert_eq!(
        mixer.stem_demand_for_measurement(id),
        (
            StemMixMode::AllStems,
            mask,
            source_version_hash(&fixture.copy.version)
        )
    );
    let stems = mixer.stems_for_measurement()[id].as_ref().unwrap();
    assert_eq!(stems.available_mask, 15);
    assert_eq!(
        *stems.complete_set_identity,
        *fixture.copy.pair.as_ref().unwrap()._identity
    );
    assert!(
        stems
            .stems
            .iter()
            .all(|stem| stem.samples.iter().any(|value| value.abs() > 0.0001))
    );
    fixture.copy.with_engine(|engine| {
        let cache = engine.resident_stem_cache.lock().unwrap();
        let pair = cache[id].accepted.as_ref().unwrap().pair.as_ref().unwrap();
        assert_eq!(
            pair.descriptor_reference,
            fixture.copy.pair.as_ref().unwrap().descriptor_reference
        );
        let leases = engine.cold_leases.lock().unwrap();
        assert_eq!(
            pair.source_lease.assignment_id(),
            leases[id].as_ref().unwrap().assignment_id()
        );
    });
    if live {
        let voice = mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(id))
            .unwrap();
        assert_ne!(voice.stretch.native_state_address(), 0);
        assert!(voice.stretch.wet_render_ready());
        assert!(voice.stretch.wet_output_ready());
        assert!(voice.stretch.wet_current_source_output_ready());
        assert!(
            voice
                .stretch
                .output_buffers()
                .iter()
                .flatten()
                .any(|value| value.abs() > 0.0001)
        );
    }
}

fn current_wet_ready(fixture: &PitchFixture, id: usize) -> bool {
    fixture
        .copy
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(id))
        .is_some_and(|voice| voice.stretch.wet_current_source_output_ready())
}

fn run_case(order: Order, lock: bool, selected: bool) {
    let mut actual = if selected {
        PitchFixture::new_selected(lock)
    } else {
        PitchFixture::new(lock)
    };
    let mut reference = if selected {
        PitchFixture::new_selected(lock)
    } else {
        PitchFixture::new(lock)
    };
    let source_modes = if selected {
        let selection = actual.selected.as_ref().unwrap().clone();
        let reference_selection = reference.selected.as_ref().unwrap().clone();
        let mode = actual.components(&selection, 15);
        let reference_mode = reference.components(&reference_selection, 15);
        let deadline = Instant::now() + Duration::from_secs(10);
        while mode.effective_mode_status() != "effective"
            || reference_mode.effective_mode_status() != "effective"
            || !current_wet_ready(&actual, 0)
            || !current_wet_ready(&reference, 0)
        {
            actual.copy.callback.drain();
            reference.copy.callback.drain();
            assert_eq!(
                actual.copy.callback.render(17),
                reference.copy.callback.render(17)
            );
            assert!(
                Instant::now() < deadline,
                "source selected modes lack own effect ACK"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_selected_view(&actual, 0, &selection, 15, true);
        assert_selected_view(&reference, 0, &reference_selection, 15, true);
        Some((mode, reference_mode))
    } else {
        None
    };
    assert_eq!(actual.copy.callback.frame, reference.copy.callback.frame);
    let original_b = actual.copy_to_target(false);
    actual
        .copy
        .with_engine(|engine| engine.publish_pad_copy(&original_b).unwrap());
    actual.copy.callback.drain();
    actual.settle(&original_b);
    actual.sync_rows(None);
    actual.copy.start(TARGET);
    reference.render_to(actual.copy.callback.frame);
    let old_selection = original_b.committed_stem_ticket().unwrap();
    let old_mode = if let Some(selection) = &old_selection {
        let mode = actual.components(selection, 5);
        let deadline = Instant::now() + Duration::from_secs(10);
        while mode.effective_mode_status() != "effective" || !current_wet_ready(&actual, TARGET) {
            actual.copy.callback.drain();
            actual.copy.callback.render(17);
            reference.copy.callback.render(17);
            assert!(
                Instant::now() < deadline,
                "B lacks its own selected effect ACK"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_selected_view(&actual, TARGET, selection, 5, true);
        assert_ne!(
            actual.copy.source_voice().stretch.native_state_address(),
            actual
                .copy
                .callback
                .mixer
                .voices
                .iter()
                .find(|voice| voice.is_playing_sample(TARGET))
                .unwrap()
                .stretch
                .native_state_address()
        );
        Some(mode)
    } else {
        None
    };
    let original_e = if selected {
        (actual.copy.callback.frame / 24_000 + 2) * 24_000
    } else {
        ORIGINAL_E
    };
    let capture_frame = if selected {
        original_e - 6_000
    } else {
        CAPTURE_FRAME
    };
    actual.render_to(capture_frame);
    reference.render_to(capture_frame);
    assert_source_chronology(&actual, &reference);

    let action = actual.capture(TARGET, "pitch.note:0:set_retrigger", true);
    let original = (
        action.sequence(),
        action.content_key(),
        action.received_timestamp(),
    );
    assert_eq!(action.render().pitch, PitchIntent::new(0, -5).unwrap());
    assert_eq!(action.original_due_frame(), original_e);
    if matches!(order, Order::Prepared) {
        actual.publish(&action);
        actual.wait_ready(&action);
        assert_eq!(action.execution_target(), original_e);
    }
    let source_native = actual.copy.source_voice().stretch.native_state_address();
    let (source_assignment, old_assignment) = actual.copy.with_engine(|engine| {
        let leases = engine.cold_leases.lock().unwrap();
        (
            leases[0].as_ref().unwrap().assignment_id().to_owned(),
            leases[TARGET].as_ref().unwrap().assignment_id().to_owned(),
        )
    });
    assert_ne!(source_assignment, old_assignment);
    let source_reader = selected.then(|| {
        actual.copy.with_engine(|engine| {
            engine.resident_stem_cache.lock().unwrap()[0]
                .accepted
                .as_ref()
                .unwrap()
                .pair
                .as_ref()
                .unwrap()
                .reader
                .clone()
        })
    });
    let source_components = actual.copy.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .map(|stems| stems.stems.clone());
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
    actual.copy.with_engine(|engine| {
        let leases = engine.cold_leases.lock().unwrap();
        assert_eq!(
            leases[0].as_ref().unwrap().assignment_id(),
            source_assignment
        );
        let copied_assignment = leases[TARGET].as_ref().unwrap().assignment_id();
        assert_ne!(copied_assignment, source_assignment);
        assert_ne!(copied_assignment, old_assignment);
    });
    assert_eq!(actual.copy.callback.frame, capture_frame);
    if let Some(selection) = &old_selection {
        assert!(!selection.is_current());
    }
    if let Some(mode) = &old_mode {
        assert!(!mode.is_current());
    }
    let new_selection = replacement.committed_stem_ticket().unwrap();
    let stopped_mode = if let Some(selection) = &new_selection {
        let mode = actual.components(selection, 15);
        assert_eq!(mode.effective_mode_status(), "waiting");
        assert_selected_view(&actual, TARGET, selection, 15, false);
        Some(mode)
    } else {
        None
    };
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
        if let Some((mode, reference_mode)) = &source_modes {
            assert_eq!(mode.effective_mode_status(), "effective");
            assert_eq!(reference_mode.effective_mode_status(), "effective");
            assert_selected_view(&actual, 0, actual.selected.as_ref().unwrap(), 15, true);
            actual.copy.with_engine(|engine| {
                let cache = engine.resident_stem_cache.lock().unwrap();
                assert!(Arc::ptr_eq(
                    &cache[0]
                        .accepted
                        .as_ref()
                        .unwrap()
                        .pair
                        .as_ref()
                        .unwrap()
                        .reader,
                    source_reader.as_ref().unwrap()
                ));
            });
            for (current, original) in actual.copy.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap()
                .stems
                .iter()
                .zip(source_components.as_ref().unwrap())
            {
                assert!(Arc::ptr_eq(&current.samples, &original.samples));
                assert!(Arc::ptr_eq(
                    &current.residency.as_ref().unwrap().source,
                    &original.residency.as_ref().unwrap().source
                ));
            }
        }
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
    assert!(actual.copy.callback.frame > original_e + 8192);
    assert_eq!(action.state(), "rejected");
    assert_eq!(action.reason(), Some("stale_content"));
    assert!(!action.effective());
    if matches!(order, Order::CopyFirst) {
        assert!(!action.accepted());
    } else {
        assert!(action.accepted());
        assert_eq!(action.execution_target(), original_e);
    }
    assert_eq!(
        (
            action.sequence(),
            action.content_key(),
            action.received_timestamp()
        ),
        original
    );
    assert_eq!(action.original_due_frame(), original_e);
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
    if let Some(selection) = &new_selection {
        // Admit advances transport intent but preserves the cancellation revision.
        // This same-content component receipt can remain current across a pitch
        // attack; its earlier stopped "waiting" state was never an audible ACK.
        assert!(stopped_mode.as_ref().unwrap().is_current());
        assert_eq!(
            stopped_mode.as_ref().unwrap().residency_status(),
            "accepted"
        );
        let mode = actual.components(selection, 15);
        let deadline = Instant::now() + Duration::from_secs(10);
        while mode.effective_mode_status() != "effective" || !current_wet_ready(&actual, TARGET) {
            actual.copy.callback.drain();
            actual.copy.callback.render(17);
            reference.copy.callback.render(17);
            assert_source_chronology(&actual, &reference);
            assert!(
                Instant::now() < deadline,
                "C lacks its own current effective-mode ACK"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_selected_view(&actual, TARGET, selection, 15, true);
    }
    assert_eq!(
        actual.copy.source_voice().stretch.native_state_address(),
        source_native
    );
}

#[test]
fn selected_copy_fences_prepared_pitch_with_own_components_and_wet_native() {
    isolated(
        "overwrite_pitch_tests::selected_copy_fences_prepared_pitch_with_own_components_and_wet_native",
        || {
            for lock in [false, true] {
                run_case(Order::Prepared, lock, true);
            }
        },
    );
}

#[test]
fn selected_copy_fences_pitch_first_queue_and_preserves_productive_source() {
    isolated(
        "overwrite_pitch_tests::selected_copy_fences_pitch_first_queue_and_preserves_productive_source",
        || {
            for lock in [false, true] {
                run_case(Order::PitchFirst, lock, true);
            }
        },
    );
}

#[test]
fn selected_copy_fences_copy_first_queue_and_adopts_fresh_wet_pitch() {
    isolated(
        "overwrite_pitch_tests::selected_copy_fences_copy_first_queue_and_adopts_fresh_wet_pitch",
        || {
            for lock in [false, true] {
                run_case(Order::CopyFirst, lock, true);
            }
        },
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
