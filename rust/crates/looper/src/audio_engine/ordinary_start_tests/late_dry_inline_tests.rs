//! Complete delayed dry births through the actual public command drain and render path.
//! Rust allocator observations exclude C++ and the outer CPAL/device wrapper.
#![cfg(windows)]

use super::*;
use crate::audio_engine::audio_stream::render_scheduled_audio;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_mapping::PitchPadProjection;
use crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_source_ids;
use crate::test_memory::{ThreadAllocations, observe_current_thread};
use rtrb::{Consumer, Producer, RingBuffer};

const ORIGINAL_T: u64 = 64;
const PCM_FRAMES: usize = 257;
const EQ: [f32; 3] = [-12.0, -3.0, 4.0];
const TRIM_DB: f32 = -4.5;

fn configured(lock: bool, channels: usize, ids: &[usize]) -> Fixture {
    let mut fixture = Fixture::new_format(lock, RATE, channels);
    let source = fixture.control.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    for &id in ids.iter().filter(|id| **id >= 2) {
        // Shared physical PCM does not share the PAD's actual content authority.
        publish_tone(&fixture.control, id, source.clone(), true);
    }
    adopt_current_source_ids(&mut fixture, lock, ids);
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    for &id in ids {
        loaded[id] = true;
        starts[id] = LOOP_START as f64 / f64::from(RATE);
        ends[id] = Some(LOOP_END as f64 / f64::from(RATE));
        bindings[id] = fixture
            .control
            .engine
            .current_input_runtime_pad_binding(id)
            .unwrap();
        rows[id].key_lock = lock;
        fixture.mixer.set_pad_eq(id, EQ[0], EQ[1], EQ[2]);
        fixture.mixer.set_pad_gain(id, TRIM_DB);
    }
    fixture
        .control
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
    // Observe the unchanged trim/EQ epoch before T while every PAD is stopped.
    // No active warming voice or test-only gain settlement is used.
    assert!(
        fixture
            .render(ORIGINAL_T as usize)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
    fixture
}

fn capture_id(fixture: &Fixture, id: usize) -> CapturedPadStart {
    fixture
        .control
        .engine
        .capture_pad_start(
            id,
            LOOP_START as f64 / f64::from(RATE),
            Some(LOOP_END as f64 / f64::from(RATE)),
            1.0,
            false,
            None,
            None,
        )
        .unwrap()
}

fn publish(fixture: &Fixture, start: &CapturedPadStart) {
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(start, None)
            .unwrap()
    );
}

// This counter covers PitchAction work, not OrdinaryStart Native construction.
// It proves that this path does not allocate unrelated pitch-action Native work.
fn pitch_native_allocations(fixture: &Fixture) -> usize {
    fixture.control.engine.pitch_work_capacity.allocations().1
}

fn callback_into(fixture: &mut Fixture, output: &mut [f32], feedback: &mut Producer<AudioMessage>) {
    let frame = fixture.transport.output_frame();
    drain_control_messages(
        &mut fixture.control.consumer,
        &mut fixture.scheduler,
        frame,
        &mut TriggerQuantization::Immediate,
        &mut fixture.transport,
        &mut fixture.mixer,
        feedback,
        &mut fixture.retirement,
    );
    render_scheduled_audio(
        &mut fixture.mixer,
        &mut fixture.scheduler,
        output,
        &mut [0.0; NUM_SAMPLES],
        frame,
        fixture.channels,
        &mut fixture.transport,
        feedback,
        &mut fixture.retirement,
    );
    fixture
        .transport
        .advance_by_rendered_frames(output.len() / fixture.channels);
}

fn measured_callback(
    fixture: &mut Fixture,
    output: &mut [f32],
    feedback: &mut Producer<AudioMessage>,
) {
    let ((), allocations) = observe_current_thread(|| callback_into(fixture, output, feedback));
    assert_eq!(
        allocations,
        ThreadAllocations::default(),
        "actual dry drain/render callback"
    );
}

fn start_ids(messages: &mut Consumer<AudioMessage>) -> Vec<usize> {
    let mut ids = Vec::new();
    while let Ok(message) = messages.pop() {
        match message {
            AudioMessage::ContentFeedback { id, kind: 1, .. }
            | AudioMessage::SampleStarted { id } => ids.push(id),
            _ => {}
        }
    }
    ids
}

fn assert_pcm(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    assert!(actual.iter().all(|sample| sample.is_finite()));
    assert_eq!(
        actual
            .iter()
            .zip(expected)
            .position(|(actual, expected)| actual != expected),
        None,
        "every emitted sample, including the first, follows the independent on-time chain"
    );
    assert!(actual.iter().any(|sample| sample.abs() > 1.0e-5));
}

fn compare_late_dry(lock: bool, channels: usize, delay: usize) {
    let mut actual = configured(lock, channels, &[0, 1]);
    let mut reference = configured(lock, channels, &[0, 1]);
    let actual_native = pitch_native_allocations(&actual);
    let reference_native = pitch_native_allocations(&reference);
    let late = capture_id(&actual, 0);
    actual.drain();
    let early = capture_id(&reference, 0);
    reference.drain();
    assert_eq!(late.target_frame(), Some(ORIGINAL_T));
    assert_eq!(early.target_frame(), Some(ORIGINAL_T));
    publish(&reference, &early);
    reference.drain();
    reference.render(0);
    assert_eq!(early.state(), "applied", "{:?}", early.reason());
    assert_eq!(early.effective_frame(), Some(ORIGINAL_T));
    assert!(actual.render(delay).iter().all(|sample| *sample == 0.0));
    let execution = actual.transport.output_frame();
    publish(&actual, &late);
    let (mut feedback, mut messages) = RingBuffer::new(64);
    let mut output = [0.0; PCM_FRAMES * 2];
    // Ready dry work applies in this callback, including delay above the former
    // inline-history bound. It needs neither unheard DSP history nor a worker lead.
    measured_callback(
        &mut actual,
        &mut output[..PCM_FRAMES * channels],
        &mut feedback,
    );
    assert_eq!(late.state(), "applied", "{:?}", late.reason());
    assert_eq!(late.target_frame(), Some(ORIGINAL_T));
    assert_eq!(late.original_frame(), Some(ORIGINAL_T));
    assert_eq!(late.execution_frame(), Some(execution));
    assert_eq!(late.effective_frame(), Some(execution));
    assert_eq!(start_ids(&mut messages), vec![0]);
    let expected = reference.render(PCM_FRAMES);
    assert_pcm(&output[..PCM_FRAMES * channels], &expected);
    for (fixture, before) in [(&actual, actual_native), (&reference, reference_native)] {
        assert_eq!(
            pitch_native_allocations(fixture),
            before,
            "ordinary dry launch adds no pitch-action Native work"
        );
        assert_eq!(fixture.voice(0).stretch.adopted_request_id(), None);
        assert_eq!(fixture.voice(0).stretch.native_primed_output_frames(), 0);
        assert_eq!(
            fixture.voice(0).source_playback.position().frame,
            LOOP_START + PCM_FRAMES
        );
        assert_eq!(fixture.voice(0).source_playback.position().fraction, 0.0);
        assert!(
            !fixture
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 1)
        );
        assert!(fixture.scheduler.is_empty());
    }
}

#[test]
fn delayed_dry_birth_keeps_its_full_beginning_without_callback_allocation_in_both_lock_profiles() {
    for lock in [false, true] {
        for channels in [1, 2] {
            for delay in [1, 32] {
                compare_late_dry(lock, channels, delay);
            }
        }
    }
}

#[test]
fn delayed_dry_birth_above_old_history_bound_has_no_worker_lead_or_native() {
    for lock in [false, true] {
        for delay in [33, 1274, 8192] {
            compare_late_dry(lock, 2, delay);
        }
    }
}

#[test]
fn all_ready_stereo_dry_births_apply_together_without_consuming_history_budget() {
    let mut fixture = configured(true, 2, &[0, 1, 2, 3]);
    let mut reference = configured(true, 2, &[0, 1, 2, 3]);
    let native = pitch_native_allocations(&fixture);
    let reference_native = pitch_native_allocations(&reference);
    let starts: Vec<_> = (0..4).map(|id| capture_id(&fixture, id)).collect();
    let early: Vec<_> = (0..4).map(|id| capture_id(&reference, id)).collect();
    fixture.drain();
    reference.drain();
    for start in starts.iter().chain(&early) {
        assert_eq!(start.target_frame(), Some(ORIGINAL_T));
    }
    for start in &early {
        publish(&reference, start);
    }
    reference.drain();
    reference.render(0);
    for start in &early {
        assert_eq!(start.state(), "applied", "{:?}", start.reason());
        assert_eq!(start.effective_frame(), Some(ORIGINAL_T));
    }
    assert!(fixture.render(1274).iter().all(|sample| *sample == 0.0));
    let frame = fixture.transport.output_frame();
    for start in &starts {
        publish(&fixture, start);
    }
    let (mut feedback, mut messages) = RingBuffer::new(64);
    // All four original owners are ready dry births. Their unpublished delay
    // consumes no source/DSP samples and cannot exhaust a catchup budget.
    measured_callback(&mut fixture, &mut [], &mut feedback);
    for start in &starts {
        assert_eq!(start.state(), "applied", "{:?}", start.reason());
        assert_eq!(start.original_frame(), Some(ORIGINAL_T));
        assert_eq!(start.execution_frame(), Some(frame));
        assert_eq!(start.effective_frame(), Some(frame));
    }
    assert_eq!(start_ids(&mut messages), vec![0, 1, 2, 3]);
    assert_eq!(fixture.transport.output_frame(), frame);
    for id in 0..4 {
        assert_eq!(
            fixture.voice(id).source_playback.position().frame,
            LOOP_START
        );
    }
    let mut output = [0.0; PCM_FRAMES * 2];
    measured_callback(&mut fixture, &mut output, &mut feedback);
    assert_pcm(&output, &reference.render(PCM_FRAMES));
    assert!(start_ids(&mut messages).is_empty());
    for id in 0..4 {
        let voice = fixture.voice(id);
        assert_eq!(
            voice.source_playback.position().frame,
            LOOP_START + PCM_FRAMES
        );
        assert_eq!(voice.stretch.adopted_request_id(), None);
        assert_eq!(voice.stretch.native_primed_output_frames(), 0);
    }
    assert_eq!(pitch_native_allocations(&fixture), native);
    assert_eq!(pitch_native_allocations(&reference), reference_native);
    assert!(fixture.scheduler.is_empty());
}

#[derive(Clone, Copy, Debug)]
enum Invalidation {
    SourceAba,
    EqAba,
    GainAba,
}

fn edit(fixture: &mut Fixture, invalidation: Invalidation) {
    match invalidation {
        Invalidation::SourceAba => {
            fixture.mixer.set_pad_loop_region(
                0,
                (LOOP_START + 1) as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
            );
            fixture.mixer.set_pad_loop_region(
                0,
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
            );
        }
        Invalidation::EqAba => {
            fixture.mixer.set_pad_eq(0, -3.0, 2.0, -9.0);
            fixture.mixer.set_pad_eq(0, EQ[0], EQ[1], EQ[2]);
        }
        Invalidation::GainAba => {
            fixture.mixer.set_pad_gain(0, -9.0);
            fixture.mixer.set_pad_gain(0, TRIM_DB);
        }
    }
}

#[test]
fn tiny_dry_source_eq_gain_aba_refuse_old_t_preserve_voice_and_allow_fresh_action() {
    for invalidation in [
        Invalidation::SourceAba,
        Invalidation::EqAba,
        Invalidation::GainAba,
    ] {
        let mut actual = configured(false, 1, &[0, 1]);
        let mut reference = configured(false, 1, &[0, 1]);
        for fixture in [&mut actual, &mut reference] {
            let start = capture_id(fixture, 0);
            fixture.drain();
            publish(fixture, &start);
            fixture.drain();
            fixture.render(0);
            assert_eq!(start.state(), "applied", "{:?}", start.reason());
            fixture.render(64);
        }
        let native = pitch_native_allocations(&actual);
        let generation = actual.voice(0).generation;
        let stale = capture_id(&actual, 0);
        actual.drain();
        let original = actual.transport.output_frame();
        assert_eq!(stale.target_frame(), Some(original));
        assert_eq!(actual.render(1), reference.render(1));
        // Real setters edit the authoritative source/DSP epoch after T. Restoring
        // its values cannot grant a proof that this replacement epoch covered T.
        edit(&mut actual, invalidation);
        edit(&mut reference, invalidation);
        publish(&actual, &stale);
        let (mut feedback, mut messages) = RingBuffer::new(64);
        let mut output = [0.0; PCM_FRAMES];
        measured_callback(&mut actual, &mut output, &mut feedback);
        assert_eq!(
            stale.state(),
            "rejected",
            "{invalidation:?}: {:?}",
            stale.reason()
        );
        assert!(stale.reason().is_some());
        assert_eq!(stale.effective_frame(), None);
        assert_eq!(actual.voice(0).generation, generation);
        assert_eq!(pitch_native_allocations(&actual), native);
        assert!(start_ids(&mut messages).is_empty());
        assert!(actual.scheduler.is_empty());
        assert_pcm(&output, &reference.render(PCM_FRAMES));
        assert!(
            actual
                .voice(0)
                .source_playback
                .matches_exact(&reference.voice(0).source_playback)
        );
        let fresh = capture_id(&actual, 0);
        measured_callback(&mut actual, &mut [], &mut feedback);
        let fresh_frame = actual.transport.output_frame();
        publish(&actual, &fresh);
        measured_callback(&mut actual, &mut [], &mut feedback);
        assert_eq!(
            fresh.state(),
            "applied",
            "{invalidation:?}: {:?}",
            fresh.reason()
        );
        assert_eq!(fresh.effective_frame(), Some(fresh_frame));
        assert_ne!(fresh.sequence(), stale.sequence());
        assert_eq!(actual.voice(0).generation, generation + 1);
        assert_eq!(start_ids(&mut messages), vec![0]);
        assert_eq!(pitch_native_allocations(&actual), native);
    }
}
