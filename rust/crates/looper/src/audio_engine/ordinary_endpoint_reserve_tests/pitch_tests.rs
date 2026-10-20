//! Device-free pitch retriggers through the public Native GUI-origin seam and the
//! real InputRuntime MIDI dispatcher. These do not exercise ImGui frames, hardware,
//! CPAL, or Human listening acceptance.

use super::*;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_mapping::{InputRuntimeEvent, PitchPadProjection};
use crate::audio_engine::pitch_actions::{CapturedPitchAction, PitchActionVariant, PitchKeyMode};
use crate::audio_engine::source_reader::ExplicitSeekMode;

#[path = "mixed_burst_tests.rs"]
mod mixed_burst_tests;

const EXTRA_SEVEN: &str = "pitch.extra:7:set_retrigger";

#[derive(Clone, Copy)]
enum Origin {
    PublicGui,
    RoutedMidi,
}

fn runtime_rows(
    fixture: &Fixture,
    quantize_step_64ths: Option<u32>,
    loop_start: usize,
    loop_end: usize,
) {
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    for id in [0, 1] {
        loaded[id] = true;
        starts[id] = loop_start as f64 / f64::from(RATE);
        ends[id] = Some(loop_end as f64 / f64::from(RATE));
        bindings[id] = fixture
            .control
            .engine
            .current_input_runtime_pad_binding(id)
            .unwrap();
        rows[id] = PitchPadProjection {
            source_root: Some(0),
            source_mode: Some(PitchKeyMode::Major),
            intent: fixture.mixer.pad_pitch_intent(id).unwrap(),
            key_lock: true,
            quantize_step_64ths,
            sync: false,
            request_sequence: runtime.pitch_request_sequence(id).unwrap(),
            ..PitchPadProjection::default()
        };
    }
    runtime
        .set_runtime_state_with_pitch(
            true,
            loaded,
            starts,
            ends,
            bindings.iter().map(Option::as_ref).collect(),
            rows,
        )
        .unwrap();
    runtime.replace_mappings(vec![
        ("midi:note:1:40".into(), "ui.select_pad:0".into()),
        ("midi:note:2:60".into(), EXTRA_SEVEN.into()),
    ]);
    runtime.set_enabled(true);
}

fn routed_event(fixture: &Fixture) -> InputRuntimeEvent {
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(event) = runtime.poll_event() {
            return event;
        }
        assert!(Instant::now() < deadline, "routed MIDI event timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn select_ready(fixture: &mut Fixture, origin: Origin) -> usize {
    if matches!(origin, Origin::PublicGui) {
        return select_and_wait(fixture);
    }
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    assert!(runtime.inject_midi_message(&[0x90, 40, 127]));
    let selected = routed_event(fixture);
    assert_eq!(selected.action_key.as_deref(), Some("ui.select_pad:0"));
    assert!(selected.dispatched && !selected.direct);
    assert!(selected.selection_revision.is_some());
    assert!(selected.captured_pitch.is_none());
    assert_eq!(runtime.selected_projection().unwrap().1, Some(0));
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + WAIT;
    loop {
        fixture.render(0);
        let runtime = fixture.mixer.pitch_runtime.as_ref().unwrap();
        assert_eq!(runtime.unused_action_capacity(), (64, 64));
        if runtime.selected_endpoint_ready() {
            assert_eq!(fixture.transport.output_frame(), frame);
            return runtime.selected_endpoint_native_address();
        }
        assert!(
            Instant::now() < deadline,
            "routed selection endpoint timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn capture_pitch(fixture: &Fixture, origin: Origin) -> CapturedPitchAction {
    if matches!(origin, Origin::PublicGui) {
        return fixture.capture(0, EXTRA_SEVEN);
    }
    fixture.refresh_clock();
    let runtime = fixture.control.engine.input_runtime.as_ref().unwrap();
    assert!(runtime.inject_midi_message(&[0x91, 60, 127]));
    let event = routed_event(fixture);
    assert_eq!(event.binding_key, "midi:note:2:60");
    assert_eq!(event.action_key.as_deref(), Some(EXTRA_SEVEN));
    assert!(event.direct && !event.dispatched && event.pitch_rejection.is_none());
    let action = event.captured_pitch.unwrap();
    assert_eq!(action.received_timestamp(), event.received_at_ns);
    assert!(!runtime.inject_midi_message(&[0x81, 60, 0]));
    assert!(!runtime.inject_midi_message(&[0x91, 60, 0]));
    assert!(runtime.poll_event().is_none());
    action
}

fn publish_pitch(fixture: &mut Fixture, action: &CapturedPitchAction) {
    assert!(fixture.control.engine.publish_pitch_action(action).unwrap());
    fixture.drain();
    assert!(
        action.accepted(),
        "state={} reason={:?}",
        action.state(),
        action.reason()
    );
}

fn ordinary_miss(fixture: &mut Fixture) -> CapturedPadStart {
    let start = capture(fixture, LOOP_START, LOOP_END, 1.0);
    fixture.drain();
    publish(fixture, &start);
    assert_eq!(start.original_frame(), Some(0));
    assert_eq!(start.execution_frame(), Some(8192));
    wait_action(fixture, &start);
    start
}

fn ordinary_ready(fixture: &mut Fixture) -> CapturedPadStart {
    let frame = fixture.transport.output_frame();
    select_and_wait(fixture);
    fixture.refresh_clock();
    let start = capture(fixture, LOOP_START, LOOP_END, 1.0);
    fixture.drain();
    publish(fixture, &start);
    assert_eq!(start.original_frame(), Some(frame));
    assert_eq!(start.execution_frame(), Some(frame));
    // A current-frame cache hit can already have completed its genuine effect
    // during the control drain; no ready worker slot remains after retirement.
    if start.state() != "applied" {
        wait_action(fixture, &start);
    }
    start
}

fn start_ordinary_and_progress(fixture: &mut Fixture) -> CapturedPadStart {
    let start = ordinary_miss(fixture);
    assert!(fixture.render_to(8192).iter().all(|sample| *sample == 0.0));
    let mut pcm = fixture.render(1);
    pcm.extend(fixture.render_to(17_000));
    assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
    assert_eq!(start.state(), "applied");
    assert_eq!(fixture.starts(0), 1);
    let emitted_frames = fixture.transport.output_frame() - start.effective_frame().unwrap();
    assert_eq!(emitted_frames, pcm.len() as u64);
    assert_eq!(
        fixture.voice(0).source_playback.position().frame,
        LOOP_START + emitted_frames as usize
    );
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(start.sequence())
    );
    start
}

fn assert_initial_pcm(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), 48_000);
    assert_eq!(actual.len(), expected.len());
    assert!(actual.iter().all(|sample| sample.is_finite()));
    assert!(expected.iter().all(|sample| sample.is_finite()));
    let maximum = actual
        .iter()
        .zip(expected)
        .map(|(actual, expected)| (actual - expected).abs())
        .fold(0.0_f32, f32::max);
    assert!(maximum < 2.0e-5, "complete initial PCM mismatch: {maximum}");
    // The full cold beginning must equal explicit-silence/raw Native; startup
    // frequency transients have their own independently fixed settled window.
    assert_physical_birth_tone(actual, 0, 7, 1.0, true, 1.0);
}

fn assert_one_emitted_birth(fixture: &Fixture) {
    let voice = fixture.voice(0);
    assert_eq!(voice.source_playback.position().frame, LOOP_START + 1);
    assert_eq!(voice.source_playback.position().fraction, 0.0);
    assert_eq!(
        voice.source_playback.position().seek_mode,
        ExplicitSeekMode::Normal
    );
    // Explicit mono/48k/Short geometry: H=4096 is block aligned, then one
    // physical sample enters the one 511-frame adapter, independently of E-T.
    assert_eq!(voice.stretch.pending_fifo_frames(), (1, 510));
}

fn assert_native_attack(
    fixture: &Fixture,
    action: &CapturedPitchAction,
    native: usize,
    frames: usize,
) {
    assert_eq!(action.state(), "applied", "reason={:?}", action.reason());
    assert!(action.effective());
    assert_eq!(action.variant(), PitchActionVariant::SetRetrigger);
    assert_eq!(action.render().pitch, PitchIntent::new(0, 7).unwrap());
    assert!(action.render().key_lock);
    let voice = fixture.voice(0);
    assert_eq!(voice.stretch.native_state_address(), native);
    assert_eq!(voice.stretch.adopted_request_id(), Some(action.sequence()));
    assert!(
        voice
            .stretch
            .owns_pitch_contract(action.render().pitch, true)
    );
    assert_eq!(voice.source_playback.position().frame, LOOP_START + frames);
    assert_eq!(voice.source_playback.position().fraction, 0.0);
    assert_eq!(
        voice.source_playback.position().seek_mode,
        ExplicitSeekMode::Normal
    );
    let primed = voice.stretch.native_primed_output_frames();
    assert!(primed >= super::super::prepared_native_history::PREPARED_HISTORY_FRAMES);
    assert_eq!(primed % 512, 0);
    let pending_input = frames % 512;
    assert_eq!(
        voice.stretch.pending_fifo_frames(),
        (pending_input, 511 - pending_input)
    );
    assert_eq!(
        voice
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames,
        (primed + frames) as u64
    );
    assert_eq!(
        voice
            .stretch
            .native_tap_observation()
            .unwrap()
            .missing_reads,
        0
    );
}

fn ready_hit_profile(origin: Origin, playing: bool) {
    let mut hit = fixture();
    let mut reference = fixture();
    set_pitch(&mut hit, 7);
    set_pitch(&mut reference, 7);
    runtime_rows(&hit, None, LOOP_START, LOOP_END);
    let previous = playing.then(|| start_ordinary_and_progress(&mut hit));
    let before = previous.as_ref().map(|_| {
        let voice = hit.voice(0);
        (
            voice.source_playback,
            voice.generation,
            voice.stretch.native_state_address(),
            voice.stretch.pending_fifo_frames(),
            voice.stretch.adopted_request_id(),
        )
    });
    let cached = select_ready(&mut hit, origin);
    assert_ne!(cached, 0);
    if let Some((playback, generation, native, fifo, request)) = before {
        assert_ne!(
            cached, native,
            "warm a distinct Normal endpoint beside actual playback"
        );
        assert!(hit.voice(0).source_playback.matches_exact(&playback));
        assert_eq!(hit.voice(0).generation, generation);
        assert_eq!(hit.voice(0).stretch.native_state_address(), native);
        assert_eq!(hit.voice(0).stretch.pending_fifo_frames(), fifo);
        assert_eq!(hit.voice(0).stretch.adopted_request_id(), request);
    } else {
        assert!(!hit.mixer.voices.iter().any(|voice| voice.active));
    }
    let action = capture_pitch(&hit, origin);
    let original = (
        action.content_key(),
        action.sequence(),
        action.received_timestamp(),
        action.render(),
        action.original_due_frame(),
        action.loop_region(),
    );
    assert_eq!(
        original.0,
        hit.control
            .engine
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap()
            .content_key()
            .unwrap()
    );
    assert_eq!(original.4, if playing { 17_000 } else { 0 });
    assert!(!action.sync() && !action.exclusive());
    assert!(!action.accepted() && !action.effective());
    publish_pitch(&mut hit, &action);
    assert_eq!(
        action.execution_target(),
        action.original_due_frame(),
        "a ready original pitch attack must not acquire the worker's 8192-frame lead"
    );
    assert!(
        !hit.mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .selected_endpoint_ready()
    );
    assert!(
        reference
            .render_to(action.original_due_frame())
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let expected = ordinary_ready(&mut reference);
    let oracle = complete_pcm(&mut reference, action.original_due_frame());
    assert_eq!(expected.state(), "applied");
    let mut pcm = hit.render(1);
    pcm.extend(hit.pcm(47_999));
    assert_native_attack(&hit, &action, cached, pcm.len());
    assert_eq!(hit.starts(0), if playing { 2 } else { 1 });
    assert_eq!(hit.voice(0).generation, if playing { 2 } else { 1 });
    assert_eq!(
        hit.voice(0).stretch.pending_fifo_frames(),
        reference.voice(0).stretch.pending_fifo_frames()
    );
    assert_eq!(
        (
            action.content_key(),
            action.sequence(),
            action.received_timestamp(),
            action.render(),
            action.original_due_frame(),
            action.loop_region()
        ),
        original
    );
    assert_initial_pcm(&pcm, &oracle);

    // An ordinary worker miss preserves original T and emits its complete new
    // beginning at its own E. Compare every emitted sample with the independent
    // ready beginning above, without matching different absolute output times.
    let mut uncached = fixture();
    set_pitch(&mut uncached, 7);
    assert!(
        uncached
            .render_to(action.original_due_frame())
            .iter()
            .all(|sample| *sample == 0.0)
    );
    uncached.refresh_clock();
    let miss = capture(&uncached, LOOP_START, LOOP_END, 1.0);
    uncached.drain();
    publish(&mut uncached, &miss);
    assert_eq!(miss.original_frame(), Some(action.original_due_frame()));
    assert_eq!(
        miss.execution_frame(),
        Some(action.original_due_frame() + 8192)
    );
    wait_action(&mut uncached, &miss);
    let uncached_pcm = complete_pcm(&mut uncached, miss.execution_frame().unwrap());
    assert_eq!(
        reference.transport.output_frame(),
        action.original_due_frame() + oracle.len() as u64
    );
    assert_eq!(
        uncached.transport.output_frame(),
        miss.execution_frame().unwrap() + uncached_pcm.len() as u64
    );
    assert_eq!(miss.state(), "applied");
    assert_eq!(miss.original_frame(), Some(action.original_due_frame()));
    assert_eq!(
        miss.execution_frame(),
        Some(action.original_due_frame() + 8192)
    );
    assert_eq!(
        uncached.voice(0).source_playback.position().frame,
        LOOP_START + uncached_pcm.len()
    );
    assert_eq!(
        uncached.voice(0).stretch.adopted_request_id(),
        Some(miss.sequence())
    );
    assert_eq!(
        uncached.voice(0).stretch.pending_fifo_frames(),
        reference.voice(0).stretch.pending_fifo_frames()
    );
    assert_ne!(
        uncached.voice(0).stretch.native_state_address(),
        reference.voice(0).stretch.native_state_address()
    );
    let primed = uncached.voice(0).stretch.native_primed_output_frames();
    assert_eq!(
        primed,
        reference.voice(0).stretch.native_primed_output_frames()
    );
    assert_eq!(
        uncached
            .voice(0)
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames,
        (primed + uncached_pcm.len()) as u64
    );
    assert_eq!(
        uncached
            .voice(0)
            .stretch
            .native_tap_observation()
            .unwrap()
            .missing_reads,
        0
    );
    assert_initial_pcm(&uncached_pcm, &oracle);
}

#[test]
fn public_gui_origin_stopped_ready_pitch_retrigger_reuses_original_endpoint_at_t() {
    ready_hit_profile(Origin::PublicGui, false);
}

#[test]
fn public_gui_origin_playing_ready_pitch_retrigger_reuses_independent_endpoint_at_t() {
    ready_hit_profile(Origin::PublicGui, true);
}

#[test]
fn routed_midi_stopped_ready_pitch_retrigger_reuses_original_endpoint_at_t() {
    ready_hit_profile(Origin::RoutedMidi, false);
}

#[test]
fn routed_midi_playing_ready_pitch_retrigger_reuses_independent_endpoint_at_t() {
    ready_hit_profile(Origin::RoutedMidi, true);
}

#[test]
fn future_ready_pitch_hit_keeps_old_pcm_native_and_fifo_until_own_t() {
    let mut hit = fixture();
    let mut background = fixture();
    set_pitch(&mut hit, 7);
    set_pitch(&mut background, 7);
    let previous = start_ordinary_and_progress(&mut hit);
    start_ordinary_and_progress(&mut background);
    runtime_rows(&hit, Some(16), LOOP_START, LOOP_END);
    let cached = select_ready(&mut hit, Origin::RoutedMidi);
    let native = hit.voice(0).stretch.native_state_address();
    let generation = hit.voice(0).generation;
    assert_ne!(cached, native);
    let action = capture_pitch(&hit, Origin::RoutedMidi);
    let identity = (
        action.content_key(),
        action.sequence(),
        action.received_timestamp(),
        action.render(),
    );
    assert_eq!(action.original_due_frame(), 24_000);
    publish_pitch(&mut hit, &action);
    assert_eq!(action.execution_target(), 24_000);
    hit.wait_worker(&[&action]);
    // Revoking cache selection after admission must not revoke the fresh action permit.
    hit.control.engine.select_input_pad(1).unwrap();
    hit.render(0);
    assert_eq!(hit.render_to(24_000), background.render_to(24_000));
    assert!(!action.effective());
    assert_eq!(hit.starts(0), 1);
    assert_eq!(hit.voice(0).generation, generation);
    assert_eq!(hit.voice(0).stretch.native_state_address(), native);
    assert_eq!(
        hit.voice(0).stretch.adopted_request_id(),
        Some(previous.sequence())
    );
    assert!(
        hit.voice(0)
            .source_playback
            .matches_exact(&background.voice(0).source_playback)
    );
    assert_eq!(
        hit.voice(0).stretch.pending_fifo_frames(),
        background.voice(0).stretch.pending_fifo_frames()
    );
    hit.render(1);
    assert_native_attack(&hit, &action, cached, 1);
    assert_eq!(hit.starts(0), 2);
    assert_eq!(hit.voice(0).generation, generation + 1);
    assert_eq!(
        (
            action.content_key(),
            action.sequence(),
            action.received_timestamp(),
            action.render()
        ),
        identity
    );
    assert_eq!(action.original_due_frame(), 24_000);
    assert_eq!(action.execution_target(), 24_000);
}

#[test]
fn equal_ready_pitch_attacks_consume_one_endpoint_and_preserve_both_originals() {
    let mut fixture = fixture();
    set_pitch(&mut fixture, 7);
    runtime_rows(&fixture, None, LOOP_START, LOOP_END);
    let cached = select_ready(&mut fixture, Origin::PublicGui);
    let first = capture_pitch(&fixture, Origin::PublicGui);
    let second = capture_pitch(&fixture, Origin::PublicGui);
    let identities = [
        (
            first.content_key(),
            first.sequence(),
            first.received_timestamp(),
        ),
        (
            second.content_key(),
            second.sequence(),
            second.received_timestamp(),
        ),
    ];
    assert!(first.sequence() < second.sequence());
    publish_pitch(&mut fixture, &first);
    publish_pitch(&mut fixture, &second);
    assert_eq!(first.original_due_frame(), 0);
    assert_eq!(second.original_due_frame(), 0);
    assert_eq!(first.execution_target(), 0);
    assert_eq!(second.execution_target(), 8192);
    fixture.wait_worker(&[&second]);
    fixture.render(1);
    assert_native_attack(&fixture, &first, cached, 1);
    assert_eq!(fixture.starts(0), 1);
    fixture.render_to(8192);
    fixture.render(1);
    assert!(first.effective() && second.effective());
    assert_eq!(fixture.starts(0), 2);
    assert_ne!(fixture.voice(0).stretch.native_state_address(), cached);
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(second.sequence())
    );
    assert_one_emitted_birth(&fixture);
    assert_eq!(
        [
            (
                first.content_key(),
                first.sequence(),
                first.received_timestamp()
            ),
            (
                second.content_key(),
                second.sequence(),
                second.received_timestamp()
            )
        ],
        identities
    );
}

#[test]
fn rapid_unselected_pitch_misses_keep_every_equal_t_attack_while_workers_prepare() {
    let mut fixture = fixture();
    set_pitch(&mut fixture, 7);
    runtime_rows(&fixture, None, LOOP_START, LOOP_END);
    let actions = (0..4)
        .map(|_| capture_pitch(&fixture, Origin::PublicGui))
        .collect::<Vec<_>>();
    for pair in actions.windows(2) {
        assert!(pair[0].sequence() < pair[1].sequence());
    }
    for action in &actions {
        publish_pitch(&mut fixture, action);
        assert_eq!(action.original_due_frame(), 0);
        assert_eq!(action.execution_target(), 8192);
    }
    fixture.wait_worker(&actions.iter().collect::<Vec<_>>());
    assert!(fixture.render_to(8192).iter().all(|sample| *sample == 0.0));
    fixture.render(1);
    assert!(actions.iter().all(CapturedPitchAction::effective));
    assert_eq!(fixture.starts(0), 4);
    assert_eq!(fixture.voice(0).generation, 4);
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(actions[3].sequence())
    );
    assert_one_emitted_birth(&fixture);
}

fn ordinary_pitch_competition(pitch_first: bool) {
    let mut fixture = fixture();
    set_pitch(&mut fixture, 7);
    runtime_rows(&fixture, None, LOOP_START, LOOP_END);
    let cached = select_ready(&mut fixture, Origin::PublicGui);
    let pitch = capture_pitch(&fixture, Origin::PublicGui);
    let ordinary = capture(&fixture, LOOP_START, LOOP_END, 1.0);
    fixture.drain();
    if pitch_first {
        publish_pitch(&mut fixture, &pitch);
        publish(&mut fixture, &ordinary);
    } else {
        publish(&mut fixture, &ordinary);
        publish_pitch(&mut fixture, &pitch);
    }
    assert_eq!(pitch.original_due_frame(), 0);
    assert_eq!(ordinary.original_frame(), Some(0));
    assert_eq!(pitch.execution_target(), if pitch_first { 0 } else { 8192 });
    assert_eq!(
        ordinary.execution_frame(),
        Some(if pitch_first { 8192 } else { 0 })
    );
    if pitch_first {
        wait_action(&mut fixture, &ordinary);
    } else {
        fixture.wait_worker(&[&pitch]);
    }
    fixture.render(1);
    assert_eq!(fixture.starts(0), 1);
    assert_eq!(fixture.voice(0).stretch.native_state_address(), cached);
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(if pitch_first {
            pitch.sequence()
        } else {
            ordinary.sequence()
        })
    );
    fixture.render_to(8192);
    fixture.render(1);
    assert_eq!(fixture.starts(0), 2);
    assert!(pitch.effective());
    assert_eq!(ordinary.state(), "applied");
    assert_ne!(fixture.voice(0).stretch.native_state_address(), cached);
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(if pitch_first {
            ordinary.sequence()
        } else {
            pitch.sequence()
        })
    );
    // Both are fresh births. Original T=0 and own E=8192 remain distinct, while
    // neither ordinary nor SET_RETRIGGER consumes the un-emitted E-T interval.
    if pitch_first {
        assert_eq!(ordinary.original_frame(), Some(0));
        assert_eq!(ordinary.execution_frame(), Some(8192));
    } else {
        assert_eq!(pitch.original_due_frame(), 0);
        assert_eq!(pitch.execution_target(), 8192);
    }
    assert_one_emitted_birth(&fixture);
}

#[test]
fn pitch_then_ordinary_compete_for_one_physical_selected_endpoint() {
    ordinary_pitch_competition(true);
}

#[test]
fn ordinary_then_pitch_compete_for_one_physical_selected_endpoint() {
    ordinary_pitch_competition(false);
}

#[test]
fn changed_pitch_loop_or_rate_misses_with_original_pitch_action_and_target_intact() {
    for change in ["pitch", "loop", "rate"] {
        let mut fixture = fixture();
        set_pitch(&mut fixture, 7);
        runtime_rows(&fixture, None, LOOP_START, LOOP_END);
        let cached = select_ready(&mut fixture, Origin::PublicGui);
        match change {
            "loop" => {
                fixture.mixer.set_pad_loop_region(
                    0,
                    (LOOP_START + 128) as f64 / f64::from(RATE),
                    Some((LOOP_END - 128) as f64 / f64::from(RATE)),
                );
                runtime_rows(&fixture, None, LOOP_START + 128, LOOP_END - 128);
            }
            "rate" => fixture.mixer.set_speed(1.25),
            _ => {}
        }
        let action = fixture.capture(
            0,
            if change == "pitch" {
                "pitch.extra:5:set_retrigger"
            } else {
                EXTRA_SEVEN
            },
        );
        let identity = (
            action.content_key(),
            action.sequence(),
            action.received_timestamp(),
            action.render(),
            action.loop_region(),
        );
        publish_pitch(&mut fixture, &action);
        assert_eq!(action.original_due_frame(), 0, "change={change}");
        assert_eq!(action.execution_target(), 8192, "change={change}");
        fixture.wait_worker(&[&action]);
        assert!(fixture.render_to(8192).iter().all(|sample| *sample == 0.0));
        fixture.render(1);
        assert!(
            action.effective(),
            "change={change} reason={:?}",
            action.reason()
        );
        assert_eq!(fixture.starts(0), 1);
        assert_ne!(
            fixture.voice(0).stretch.native_state_address(),
            cached,
            "change={change}"
        );
        assert_eq!(
            fixture.voice(0).stretch.adopted_request_id(),
            Some(action.sequence())
        );
        assert_eq!(
            (
                action.content_key(),
                action.sequence(),
                action.received_timestamp(),
                action.render(),
                action.loop_region()
            ),
            identity
        );
        assert!(
            fixture
                .voice(0)
                .stretch
                .owns_pitch_contract(action.render().pitch, true)
        );
    }
}

#[test]
fn selection_epoch_revoked_before_pitch_admission_cannot_lend_its_old_endpoint() {
    let mut fixture = fixture();
    set_pitch(&mut fixture, 7);
    runtime_rows(&fixture, None, LOOP_START, LOOP_END);
    assert_ne!(select_ready(&mut fixture, Origin::PublicGui), 0);
    let allocations = fixture.control.engine.pitch_work_capacity.allocations();
    let action = capture_pitch(&fixture, Origin::PublicGui);
    fixture.control.engine.select_input_pad(1).unwrap();
    fixture.render(0);
    publish_pitch(&mut fixture, &action);
    assert_eq!(action.original_due_frame(), 0);
    assert_eq!(action.execution_target(), 8192);
    fixture.wait_worker(&[&action]);
    fixture.render_to(8192);
    fixture.render(1);
    assert!(action.effective());
    assert_eq!(fixture.starts(0), 1);
    // The retired cache may already have been freed, so an allocator may reuse
    // its raw address. This original work must construct its own actual Native.
    assert_eq!(
        fixture.control.engine.pitch_work_capacity.allocations(),
        (allocations.0 + 1, allocations.1 + 1)
    );
    let voice = fixture.voice(0);
    assert_ne!(voice.stretch.native_state_address(), 0);
    assert_eq!(
        voice.source_admission.as_ref().unwrap().content_key(),
        Some(action.content_key())
    );
    assert!(
        voice
            .stretch
            .owns_pitch_contract(action.render().pitch, true)
    );
    assert_one_emitted_birth(&fixture);
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(action.sequence())
    );
}

#[test]
fn future_ready_pitch_action_keeps_own_source_incarnation_and_stop_fences() {
    for revoke in ["source", "refill", "stop"] {
        let mut fixture = fixture();
        set_pitch(&mut fixture, 7);
        runtime_rows(&fixture, Some(16), LOOP_START, LOOP_END);
        fixture.render_to(17_000);
        select_ready(&mut fixture, Origin::PublicGui);
        let action = capture_pitch(&fixture, Origin::PublicGui);
        let identity = (
            action.content_key(),
            action.sequence(),
            action.received_timestamp(),
            action.render(),
        );
        publish_pitch(&mut fixture, &action);
        assert_eq!(action.original_due_frame(), 24_000);
        assert_eq!(action.execution_target(), 24_000);
        fixture.wait_worker(&[&action]);
        if revoke == "stop" {
            fixture.control.engine.stop_sample(0).unwrap();
            fixture.drain();
        } else {
            let sample = fixture.control.engine.sample_cache.lock().unwrap()[0]
                .clone()
                .unwrap();
            super::super::pitch_runtime_tests::publish_tone(
                &fixture.control,
                0,
                sample,
                revoke == "refill",
            );
        }
        assert!(
            fixture
                .render_to(24_001)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert_eq!(action.state(), "rejected", "revoke={revoke}");
        assert!(!action.effective());
        assert_eq!(fixture.starts(0), 0);
        assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
        assert_eq!(
            (
                action.content_key(),
                action.sequence(),
                action.received_timestamp(),
                action.render()
            ),
            identity
        );
        assert_eq!(action.original_due_frame(), 24_000);
        assert_eq!(action.execution_target(), 24_000);
        if revoke == "stop" {
            assert_eq!(action.reason(), Some("launch_cancelled"));
        }
    }
}
