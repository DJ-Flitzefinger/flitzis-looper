//! Actual selection, callback capture, worker, original action and Native/FIFO adoption.

use super::audio_stream::drain_control_messages;
use super::input_mapping::launch_admission_tests::RATE;
use super::ordinary_start::OrdinaryStartWork;
use super::pad_start::CapturedPadStart;
use super::pitch_intent::PitchIntent;
use super::pitch_runtime_tests::{Fixture, LOOP_END, LOOP_START, assert_tone};
use super::rubberband_backend::RubberBandError;
use crate::messages::{ControlMessage, TriggerQuantization};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(3);

fn fixture() -> Fixture {
    let mut fixture = Fixture::new(true);
    adopt_current_sources(&mut fixture);
    refresh_actual_runtime_bindings(&fixture);
    fixture
}

/// The shared pitch fixture installs its mixer bank directly. Endpoint preparation
/// additionally requires the real callback SourceACK, so publish and adopt the exact
/// retained source through the production complete LoadSample claim first.
fn adopt_current_sources(fixture: &mut Fixture) {
    for id in [0, 1] {
        let ownership = fixture.control.engine.input_runtime_ownership.clone();
        let placement = ownership.slot_snapshot(id).unwrap();
        if ownership.callback_feedback_key(id) == placement.content_key() {
            continue;
        }
        let sample = fixture.control.engine.sample_cache.lock().unwrap()[id]
            .clone()
            .unwrap();
        let (generation, rate) = fixture
            .control
            .engine
            .loaded_source_generations
            .lock()
            .unwrap()[id];
        let pitch = fixture.mixer.pad_pitch_intent(id).unwrap();
        let write = ownership
            .prepare_slot_write_fixed(placement, placement.lifetime())
            .unwrap();
        let publication = ownership
            .publish_source_write(
                &write,
                &sample,
                rate,
                generation,
                ownership.authority_revision(id),
                None,
                false,
                || true,
            )
            .unwrap();
        fixture
            .control
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::LoadSample {
                id,
                sample,
                publication: Some(publication),
            })
            .unwrap();
        fixture.drain();
        assert_eq!(
            ownership.callback_feedback_key(id),
            placement.content_key(),
            "only the complete actual callback adoption grants this SourceACK"
        );
        fixture.mixer.set_pad_loop_region(
            id,
            LOOP_START as f64 / f64::from(RATE),
            Some(LOOP_END as f64 / f64::from(RATE)),
        );
        fixture.mixer.set_pad_key_lock(id, true);
        fixture.mixer.set_pad_pitch_intent(id, pitch).unwrap();
    }
}

fn set_pitch(fixture: &mut Fixture, extra: i32) {
    fixture
        .mixer
        .set_pad_pitch_intent(0, PitchIntent::new(0, extra).unwrap())
        .unwrap();
}

fn capture(fixture: &Fixture, start: usize, end: usize, volume: f32) -> CapturedPadStart {
    fixture
        .control
        .engine
        .capture_pad_start(
            0,
            start as f64 / f64::from(RATE),
            Some(end as f64 / f64::from(RATE)),
            volume,
            false,
            None,
            None,
        )
        .unwrap()
}

fn publish(fixture: &mut Fixture, start: &CapturedPadStart) {
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(start, None)
            .unwrap()
    );
    fixture.drain();
    assert!(
        start.accepted(),
        "state={} reason={:?}",
        start.state(),
        start.reason()
    );
}

fn select_and_wait(fixture: &mut Fixture) -> usize {
    fixture.control.engine.select_input_pad(0).unwrap();
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + WAIT;
    // This is the production render entry, with no output advancement or arming API.
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
            "selected endpoint did not become ready"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait_action(fixture: &mut Fixture, start: &CapturedPadStart) {
    let deadline = Instant::now() + WAIT;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if runtime.has_ready_action(start.sequence()) {
            return;
        }
        assert_ne!(start.state(), "rejected", "reason={:?}", start.reason());
        assert!(
            Instant::now() < deadline,
            "original attack did not become ready"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn complete_pcm(fixture: &mut Fixture, execution: u64) -> Vec<f32> {
    assert!(
        fixture
            .render_to(execution)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let mut pcm = fixture.render(1);
    pcm.extend(fixture.pcm(47_999));
    pcm
}

fn refresh_actual_runtime_bindings(fixture: &Fixture) {
    publish_actual_runtime_bindings(fixture, true);
}

fn publish_actual_runtime_bindings(fixture: &Fixture, multi_loop: bool) {
    use super::constants::NUM_SAMPLES;
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    for id in [0, 1] {
        loaded[id] = true;
        starts[id] = LOOP_START as f64 / f64::from(RATE);
        ends[id] = Some(LOOP_END as f64 / f64::from(RATE));
        bindings[id] = fixture
            .control
            .engine
            .current_input_runtime_pad_binding(id)
            .unwrap();
    }
    fixture
        .control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state(
            multi_loop,
            loaded,
            starts,
            ends,
            bindings.iter().map(Option::as_ref).collect(),
        )
        .unwrap();
}

#[cfg(windows)]
fn wait_real_window(
    fixture: &mut Fixture,
    ticket: &super::resident_relocation::ResidentWindowTicket,
) {
    let deadline = Instant::now() + WAIT;
    loop {
        let frame = fixture.transport.output_frame();
        drain_control_messages(
            &mut fixture.control.consumer,
            &mut fixture.scheduler,
            frame,
            &mut TriggerQuantization::Grid { step_64ths: 16 },
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut fixture.feedback,
            &mut fixture.retirement,
        );
        if ticket.publication_status() == "accepted" {
            assert!(ticket.is_current());
            return;
        }
        assert!(
            !matches!(
                ticket.publication_status(),
                "failed" | "cancelled" | "rejected"
            ),
            "real WindowACK failed: {:?}",
            ticket.error()
        );
        assert!(Instant::now() < deadline, "real WindowACK timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(windows)]
#[test]
fn original_prepared_window_start_reuses_same_physical_cache_with_its_real_own_ack() {
    for late in [false, true] {
        let mut fixture = fixture();
        let _files = super::ordinary_start_tests::install_synthetic_complete_lease(&mut fixture);
        adopt_current_sources(&mut fixture);
        refresh_actual_runtime_bindings(&fixture);
        set_pitch(&mut fixture, 7);
        fixture.render_to(1000);
        let cached_native = select_and_wait(&mut fixture);
        let cached_runtime = fixture
            .control
            .engine
            .input_runtime_ownership
            .runtime_revision(0);
        // These real routing publications advance only the control projection.
        // The fresh original below owns the resulting current revision; cached
        // physical history still belongs to the pre-publication projection.
        for multi_loop in [false, true] {
            publish_actual_runtime_bindings(&fixture, multi_loop);
        }
        let original_binding = fixture
            .control
            .engine
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .unwrap()
            .binding;
        let original_runtime = fixture
            .control
            .engine
            .input_runtime_ownership
            .runtime_revision(0);
        assert!(original_runtime >= cached_runtime + 2);
        let original = capture(&fixture, LOOP_START, LOOP_END, 1.0);
        let ticket = fixture
            .control
            .engine
            .prepare_captured_pad_start(&original)
            .unwrap();
        assert_ne!(ticket.publication_status(), "accepted");
        wait_real_window(&mut fixture, &ticket);
        assert_eq!(original.original_frame(), Some(24_000));
        assert_eq!(original.execution_frame(), None);
        let actual_binding = fixture
            .control
            .engine
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .unwrap()
            .binding;
        assert!(
            original_binding.same_projection(&actual_binding),
            "the real WindowACK preserved physical source, window and timing"
        );
        assert!(
            fixture
                .control
                .engine
                .input_runtime_ownership
                .runtime_revision(0)
                >= original_runtime,
            "a genuine same-window ACK need not advance the runtime projection"
        );
        if late {
            assert!(
                fixture
                    .render_to(30_000)
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
        }
        assert!(
            fixture
                .control
                .engine
                .publish_pad_start(&original, Some(&ticket))
                .unwrap()
        );
        fixture.drain();
        assert!(original.accepted(), "reason={:?}", original.reason());
        let execution = if late { 30_000 } else { 24_000 };
        assert_eq!(original.original_frame(), Some(24_000));
        assert_eq!(original.execution_frame(), Some(execution));
        if late {
            // E is already due at this drain's F. It performs the complete effect
            // immediately, while rendering has not consumed the first PCM frame.
            assert_eq!(
                original.state(),
                "applied",
                "reason={:?}",
                original.reason()
            );
            assert_eq!(original.effective_frame(), Some(execution));
            assert_eq!(fixture.starts(0), 1);
            assert_eq!(
                fixture.voice(0).stretch.native_state_address(),
                cached_native
            );
            assert_eq!(
                fixture.voice(0).stretch.adopted_request_id(),
                Some(original.sequence())
            );
            assert_eq!(
                fixture.voice(0).source_playback.position().frame,
                LOOP_START
            );
            assert_eq!(fixture.voice(0).source_playback.position().fraction, 0.0);
        } else {
            assert!(
                fixture
                    .mixer
                    .pitch_runtime
                    .as_ref()
                    .unwrap()
                    .has_ready_action(original.sequence()),
                "future original is immediately prepared: state={} reason={:?} T={:?} E={:?}",
                original.state(),
                original.reason(),
                original.original_frame(),
                original.execution_frame()
            );
            assert_eq!(original.effective_frame(), None);
            assert_eq!(fixture.starts(0), 0);
        }
        let pcm = complete_pcm(&mut fixture, execution);
        assert_eq!(
            original.state(),
            "applied",
            "reason={:?}",
            original.reason()
        );
        assert_eq!(original.effective_frame(), Some(execution));
        assert_eq!(
            fixture.voice(0).stretch.native_state_address(),
            cached_native
        );
        assert_eq!(
            fixture.voice(0).stretch.adopted_request_id(),
            Some(original.sequence())
        );
        assert_eq!(
            fixture.voice(0).source_playback.position().frame,
            LOOP_START + pcm.len()
        );
        assert!(fixture.voice(0).source_playback.position().fraction.abs() < 1.0e-9);
        assert!(
            pcm[..2048]
                .iter()
                .map(|sample| f64::from(*sample).powi(2))
                .sum::<f64>()
                / 2048.0
                > 0.001
        );
        assert_tone(&pcm, 0, 7, 1.0, true);
    }
}

#[cfg(windows)]
#[test]
fn genuinely_changed_prepared_window_does_not_borrow_old_endpoint_native() {
    let mut fixture = fixture();
    let _files = super::ordinary_start_tests::install_synthetic_complete_lease(&mut fixture);
    adopt_current_sources(&mut fixture);
    refresh_actual_runtime_bindings(&fixture);
    set_pitch(&mut fixture, 7);
    let old_native = select_and_wait(&mut fixture);
    let old_binding = fixture
        .control
        .engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap()
        .binding;
    let original = capture(&fixture, 72_000, 132_000, 1.0);
    fixture.drain(); // original free T=0, before its genuine new-window request.
    let ticket = fixture
        .control
        .engine
        .prepare_captured_pad_start(&original)
        .unwrap();
    wait_real_window(&mut fixture, &ticket);
    let new_binding = fixture
        .control
        .engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap()
        .binding;
    assert_ne!(old_binding.resident, new_binding.resident);
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&original, Some(&ticket))
            .unwrap()
    );
    fixture.drain();
    assert!(original.accepted(), "reason={:?}", original.reason());
    assert_eq!(original.original_frame(), Some(0));
    assert_eq!(original.execution_frame(), Some(8192));
    wait_action(&mut fixture, &original);
    let pcm = complete_pcm(&mut fixture, 8192);
    assert_eq!(
        original.state(),
        "applied",
        "reason={:?}",
        original.reason()
    );
    assert_ne!(fixture.voice(0).stretch.native_state_address(), old_native);
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(original.sequence())
    );
    assert_eq!(
        fixture.voice(0).source_playback.position().frame,
        72_000 + pcm.len()
    );
    assert_tone(&pcm, 0, 7, 1.0, true);
}

#[test]
fn selected_stopped_ready_hit_uses_original_t_fresh_action_and_complete_initial_native_pcm() {
    let mut hit = fixture();
    let mut reference = fixture();
    set_pitch(&mut hit, 7);
    set_pitch(&mut reference, 7);
    assert!(
        super::input_runtime_binding::capture(&hit.control.engine, 0)
            .unwrap()
            .unwrap()
            .current()
    );
    assert!(
        !hit.mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .selected_endpoint_ready()
    );
    let native = select_and_wait(&mut hit);
    assert!(!hit.mixer.voices.iter().any(|voice| voice.active));
    assert_ne!(native, 0);
    let original = capture(&hit, LOOP_START, LOOP_END, 1.0);
    hit.drain();
    publish(&mut hit, &original);
    assert_eq!(original.original_frame(), Some(0));
    assert_eq!(original.execution_frame(), Some(0));
    assert!(
        !hit.mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .selected_endpoint_ready()
    );

    let expected = capture(&reference, LOOP_START, LOOP_END, 1.0);
    reference.drain();
    publish(&mut reference, &expected);
    assert_eq!(expected.original_frame(), Some(0));
    assert_eq!(expected.execution_frame(), Some(8192));
    wait_action(&mut reference, &expected);
    super::source_reader::reset_tap_observation_for_test();
    let pcm = complete_pcm(&mut hit, 0);
    let rendered_taps = super::source_reader::tap_observation_for_test();
    let oracle = complete_pcm(&mut reference, 8192);
    assert_eq!(
        original.state(),
        "applied",
        "reason={:?}",
        original.reason()
    );
    assert_eq!(original.effective_frame(), Some(0));
    assert_eq!(hit.starts(0), 1);
    let voice = hit.voice(0);
    assert_eq!(
        voice.stretch.native_state_address(),
        native,
        "adopt the actual prepared Native owner"
    );
    assert_eq!(
        voice.stretch.adopted_request_id(),
        Some(original.sequence())
    );
    assert_eq!(
        voice.source_playback.position().frame,
        LOOP_START + pcm.len()
    );
    assert!(voice.source_playback.position().fraction.abs() < 1.0e-9);
    // A fresh Normal epoch can prime its entire past with silence. The retained
    // preparation observation is distinct from actual productive callback reads.
    let priming_taps = voice.stretch.native_tap_observation().unwrap();
    assert_eq!(priming_taps.missing_reads, 0);
    let primed_frames = voice.stretch.native_primed_output_frames();
    assert!(primed_frames >= super::prepared_native_history::PREPARED_HISTORY_FRAMES);
    assert_eq!(
        primed_frames,
        reference.voice(0).stretch.native_primed_output_frames()
    );
    assert_eq!(
        voice
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames,
        (primed_frames + pcm.len()) as u64
    );
    assert!(rendered_taps.left_reads >= pcm.len());
    assert_eq!(rendered_taps.missing_reads, 0);
    assert!(rendered_taps.min_frame.unwrap() >= LOOP_START);
    assert!(rendered_taps.max_frame.unwrap() < LOOP_END);
    let maximum = pcm
        .iter()
        .zip(&oracle)
        .map(|(actual, expected)| (actual - expected).abs())
        .fold(0.0_f32, f32::max);
    assert!(maximum < 2.0e-5, "complete initial PCM mismatch: {maximum}");
    assert!(
        pcm[..2048]
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>()
            / 2048.0
            > 0.001
    );
    assert_tone(&pcm, 0, 7, 1.0, true);
}

#[test]
fn selected_endpoint_has_one_consumable_native_and_equal_attacks_remain_distinct() {
    let mut fixture = fixture();
    set_pitch(&mut fixture, 7);
    let native = select_and_wait(&mut fixture);
    let first = capture(&fixture, LOOP_START, LOOP_END, 0.4);
    let second = capture(&fixture, LOOP_START, LOOP_END, 0.7);
    fixture.drain();
    publish(&mut fixture, &first);
    publish(&mut fixture, &second);
    assert!(first.sequence() < second.sequence());
    assert_eq!(first.original_frame(), Some(0));
    assert_eq!(second.original_frame(), Some(0));
    assert_eq!(first.execution_frame(), Some(0));
    assert_eq!(second.execution_frame(), Some(8192));
    wait_action(&mut fixture, &second);
    fixture.render(1);
    assert_eq!(first.state(), "applied");
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(fixture.voice(0).volume, 0.4);
    fixture.render_to(8192);
    fixture.render(1);
    assert_eq!(second.state(), "applied", "reason={:?}", second.reason());
    assert_eq!(fixture.starts(0), 2);
    assert_eq!(fixture.voice(0).volume, 0.7);
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(second.sequence())
    );
    assert_ne!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(
        fixture.voice(0).source_playback.position().frame,
        LOOP_START + 1
    );
}

#[test]
fn selected_playing_repeat_prepares_independent_normal_history_and_adopts_at_original_t() {
    let mut fixture = fixture();
    set_pitch(&mut fixture, 7);
    let first_native = select_and_wait(&mut fixture);
    let first = capture(&fixture, LOOP_START, LOOP_END, 1.0);
    fixture.drain();
    publish(&mut fixture, &first);
    let mut initial = fixture.render(1);
    initial.extend(fixture.pcm(2047));
    assert_eq!(first.state(), "applied");
    let before = fixture.voice(0).source_playback;
    let generation = fixture.voice(0).generation;
    let next_native = select_and_wait(&mut fixture);
    assert_ne!(next_native, first_native);
    assert!(fixture.voice(0).source_playback.matches_exact(&before));
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(
        fixture.voice(0).stretch.native_state_address(),
        first_native
    );
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(first.sequence())
    );

    let repeat = capture(&fixture, LOOP_START, LOOP_END, 1.0);
    drain_control_messages(
        &mut fixture.control.consumer,
        &mut fixture.scheduler,
        2048,
        &mut TriggerQuantization::Grid { step_64ths: 16 },
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
    publish(&mut fixture, &repeat);
    assert_eq!(repeat.original_frame(), Some(24_000));
    assert_eq!(repeat.execution_frame(), Some(24_000));
    let old_pcm = fixture.render_to(24_000);
    assert!(old_pcm.iter().any(|sample| sample.abs() > 0.01));
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(
        fixture.voice(0).stretch.native_state_address(),
        first_native
    );
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(first.sequence())
    );
    assert_eq!(
        fixture.voice(0).source_playback.position().frame,
        LOOP_START + 24_000
    );
    let mut repeated = fixture.render(1);
    repeated.extend(fixture.pcm(2047));
    assert_eq!(repeat.state(), "applied", "reason={:?}", repeat.reason());
    assert_eq!(repeat.effective_frame(), Some(24_000));
    assert_eq!(fixture.starts(0), 2);
    assert_eq!(fixture.voice(0).stretch.native_state_address(), next_native);
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(repeat.sequence())
    );
    assert_eq!(
        fixture.voice(0).source_playback.position().frame,
        LOOP_START + 2048
    );
    let maximum = initial
        .iter()
        .zip(&repeated)
        .map(|(first, repeated)| (first - repeated).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        maximum < 2.0e-5,
        "fresh complete repeat PCM differs: {maximum}"
    );
}

#[test]
fn ready_hit_keeps_actual_source_and_incarnation_fences_after_admission() {
    for refill in [false, true] {
        let mut fixture = fixture();
        set_pitch(&mut fixture, 7);
        fixture.render_to(1000);
        select_and_wait(&mut fixture);
        let original = capture(&fixture, LOOP_START, LOOP_END, 1.0);
        drain_control_messages(
            &mut fixture.control.consumer,
            &mut fixture.scheduler,
            1000,
            &mut TriggerQuantization::Grid { step_64ths: 16 },
            &mut fixture.transport,
            &mut fixture.mixer,
            &mut fixture.feedback,
            &mut fixture.retirement,
        );
        publish(&mut fixture, &original);
        assert_eq!(original.execution_frame(), Some(24_000));
        let sample = fixture.control.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap();
        // Same immutable PCM pin does not grant the old attack a new Native source generation
        // or replacement incarnation. The actual callback still holds its original bank image.
        super::pitch_runtime_tests::publish_tone(&fixture.control, 0, sample, refill);
        assert!(
            fixture
                .render_to(24_001)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert_eq!(original.state(), "rejected", "refill={refill}");
        assert_eq!(original.original_frame(), Some(24_000));
        assert_eq!(original.execution_frame(), Some(24_000));
        assert_eq!(original.effective_frame(), None);
        assert_eq!(fixture.starts(0), 0);
        assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
    }
}

#[test]
fn future_ready_hit_survives_later_selection_and_cache_epoch_revocation() {
    let mut fixture = fixture();
    set_pitch(&mut fixture, 7);
    fixture.render_to(1000);
    let native = select_and_wait(&mut fixture);
    let original = capture(&fixture, LOOP_START, LOOP_END, 1.0);
    drain_control_messages(
        &mut fixture.control.consumer,
        &mut fixture.scheduler,
        1000,
        &mut TriggerQuantization::Grid { step_64ths: 16 },
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
    publish(&mut fixture, &original);
    assert_eq!(original.original_frame(), Some(24_000));
    assert_eq!(original.execution_frame(), Some(24_000));
    fixture.control.engine.select_input_pad(1).unwrap();
    fixture.render(0); // invalidates only the cache epoch; original action has a fresh permit.
    assert!(
        !fixture
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .selected_endpoint_ready()
    );
    let pcm = complete_pcm(&mut fixture, 24_000);
    assert_eq!(
        original.state(),
        "applied",
        "reason={:?}",
        original.reason()
    );
    assert_eq!(original.effective_frame(), Some(24_000));
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(original.sequence())
    );
    assert_tone(&pcm, 0, 7, 1.0, true);
}

#[test]
fn changed_actual_loop_pitch_or_rate_misses_without_replacing_original_target() {
    for change in ["loop", "pitch", "rate"] {
        let mut fixture = fixture();
        set_pitch(&mut fixture, 7);
        let cached = select_and_wait(&mut fixture);
        let (start, end) = if change == "loop" {
            (LOOP_START + 128, LOOP_END - 128)
        } else {
            (LOOP_START, LOOP_END)
        };
        match change {
            "loop" => fixture.mixer.set_pad_loop_region(
                0,
                start as f64 / f64::from(RATE),
                Some(end as f64 / f64::from(RATE)),
            ),
            "pitch" => {
                set_pitch(&mut fixture, 5);
            }
            "rate" => fixture.mixer.set_speed(1.25),
            _ => unreachable!(),
        }
        let original = capture(&fixture, start, end, 1.0);
        fixture.drain();
        publish(&mut fixture, &original);
        assert_eq!(original.original_frame(), Some(0), "change={change}");
        assert_eq!(original.execution_frame(), Some(8192), "change={change}");
        wait_action(&mut fixture, &original);
        complete_pcm(&mut fixture, 8192);
        assert_eq!(
            original.state(),
            "applied",
            "change={change} reason={:?}",
            original.reason()
        );
        assert_ne!(
            fixture.voice(0).stretch.native_state_address(),
            cached,
            "change={change}"
        );
        assert_eq!(
            fixture.voice(0).stretch.adopted_request_id(),
            Some(original.sequence())
        );
    }
}

#[test]
fn stopping_a_future_ready_hit_keeps_original_cancel_fence() {
    let mut fixture = fixture();
    set_pitch(&mut fixture, 7);
    fixture.render_to(1000);
    select_and_wait(&mut fixture);
    let original = capture(&fixture, LOOP_START, LOOP_END, 1.0);
    drain_control_messages(
        &mut fixture.control.consumer,
        &mut fixture.scheduler,
        1000,
        &mut TriggerQuantization::Grid { step_64ths: 16 },
        &mut fixture.transport,
        &mut fixture.mixer,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
    publish(&mut fixture, &original);
    assert_eq!(original.execution_frame(), Some(24_000));
    fixture.control.engine.stop_sample(0).unwrap();
    fixture.drain();
    assert!(
        fixture
            .render_to(24_001)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!(original.state(), "rejected");
    assert_eq!(original.reason(), Some("launch_cancelled"));
    assert_eq!(fixture.starts(0), 0);
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
}

#[test]
fn ordinary_capture_skeleton_preserves_format_validation_without_constructing_native() {
    assert!(matches!(
        OrdinaryStartWork::new(0, 1),
        Err(RubberBandError::InvalidSampleRate)
    ));
    assert!(matches!(
        OrdinaryStartWork::new(RATE, 0),
        Err(RubberBandError::InvalidChannelCount)
    ));
    assert!(matches!(
        OrdinaryStartWork::new(RATE, usize::MAX),
        Err(RubberBandError::InvalidChannelCount)
    ));
    let work = OrdinaryStartWork::new(RATE, 1).unwrap();
    assert!(work.candidate.is_none());
    assert!(work.validates_render_format(RATE, 1));
    assert!(!work.validates_render_format(RATE, 2));
}
