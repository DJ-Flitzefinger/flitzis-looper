use super::*;
use crate::audio_engine::input_mapping::launch_admission_tests::RATE;
use crate::audio_engine::pitch_runtime_tests::{
    Fixture, LOOP_END, LOOP_START, SOURCE_HZ, assert_tone, publish_tone,
};
use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

fn wait(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline, "bounded host worker wait");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn start(fixture: &mut Fixture, extra: i32, lock: bool, ratio: f64) {
    fixture.mixer.set_pad_key_lock(0, lock);
    fixture
        .mixer
        .set_pad_pitch_intent(0, PitchIntent::new(0, extra).unwrap())
        .unwrap();
    fixture.mixer.set_speed(ratio);
    let origin = fixture
        .control
        .engine
        .capture_pad_start(
            0,
            LOOP_START as f64 / f64::from(RATE),
            Some(LOOP_END as f64 / f64::from(RATE)),
            1.0,
            false,
            None,
            None,
        )
        .unwrap();
    fixture
        .control
        .engine
        .publish_pad_start(&origin, None)
        .unwrap();
    fixture.drain();
    wait(|| {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        runtime.has_ready_action(origin.sequence())
    });
    fixture.render_to(origin.execution_frame().unwrap());
    fixture.render(1);
    assert!(origin.effective_frame().is_some(), "{:?}", origin.reason());
    fixture.pcm(8192);
}

fn complete_fixture(lock: bool) -> Fixture {
    let mut fixture = Fixture::new(lock);
    let sample = SampleBuffer {
        channels: 1,
        samples: (0..144_000)
            .map(|frame| {
                (0.25 * (frame as f64 * SOURCE_HZ * std::f64::consts::TAU / f64::from(RATE)).sin())
                    as f32
            })
            .collect::<Vec<_>>()
            .into(),
        residency: None,
    }
    .with_complete_source(RATE);
    publish_tone(&fixture.control, 0, sample.clone(), true);
    fixture.mixer.load_sample(0, sample);
    fixture.mixer.set_pad_loop_region(
        0,
        LOOP_START as f64 / f64::from(RATE),
        Some(LOOP_END as f64 / f64::from(RATE)),
    );
    fixture
}

fn submit_scalar(fixture: &mut Fixture, enabled: bool) -> u64 {
    let request = fixture.control.engine.set_pad_key_lock(0, enabled).unwrap();
    fixture.drain();
    assert_eq!(
        fixture.mixer.pad_key_lock_status[0].unwrap().state,
        "waiting"
    );
    fixture.render(1);
    request
}

fn wait_candidate(fixture: &mut Fixture) {
    wait(|| {
        fixture
            .mixer
            .voices
            .iter_mut()
            .filter(|voice| voice.active && voice.sample_id == 0)
            .all(|voice| voice.stretch.prospective_work_ready())
    });
}

/// Independent raw Native/FIFO oracle, without preparation, source-clock or reader helpers.
struct Raw {
    native: RubberBandLiveShifter,
    input: VecDeque<f32>,
    output: VecDeque<f32>,
    block: usize,
}

impl Raw {
    fn new(pitch: f64) -> Self {
        let mut native = RubberBandLiveShifter::new(RATE, 1).unwrap();
        native.set_pitch_scale(pitch).unwrap();
        native.reset_for_preparation();
        let block = native.block_size();
        Self {
            native,
            input: VecDeque::new(),
            output: std::iter::repeat_n(0.0, block - 1).collect(),
            block,
        }
    }

    fn render(&mut self, feed: &[f32]) -> Vec<f32> {
        self.input.extend(feed.iter().copied());
        while self.input.len() >= self.block {
            let input = vec![self.input.drain(..self.block).collect::<Vec<_>>()];
            let mut output = vec![vec![0.0; self.block]];
            self.native.shift(&input, &mut output).unwrap();
            self.output.extend(output[0].iter().copied());
        }
        (0..feed.len())
            .map(|_| self.output.pop_front().unwrap_or(0.0))
            .collect()
    }
}

fn source_pcm(phase: f64, ratio: f64, first: usize, frames: usize) -> Vec<f32> {
    let sine = |frame: usize| {
        (0.25 * (frame as f64 * SOURCE_HZ * std::f64::consts::TAU / f64::from(RATE)).sin()) as f32
    };
    (first..first + frames)
        .map(|offset| {
            let coordinate = LOOP_START as f64
                + (phase - LOOP_START as f64 + offset as f64 * ratio)
                    .rem_euclid((LOOP_END - LOOP_START) as f64);
            let left_frame = coordinate.floor() as usize;
            let right_frame = if left_frame + 1 == LOOP_END {
                LOOP_START
            } else {
                left_frame + 1
            };
            let left = sine(left_frame);
            left + (sine(right_frame) - left) * coordinate.fract() as f32
        })
        .collect()
}

fn compare(actual: &[f32], expected: &[f32], label: &str) {
    assert_eq!(actual.len(), expected.len());
    let error = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(error < 2.0e-5, "{label}: sample error {error}");
}

fn prove_mode_change(
    mut fixture: Fixture,
    mut reference: Fixture,
    extra: i32,
    old: bool,
    ratio: f64,
    resident: bool,
) {
    start(&mut fixture, extra, old, ratio);
    start(&mut reference, extra, old, ratio);
    let f = fixture.transport.output_frame();
    assert_eq!(reference.transport.output_frame(), f);
    let generation = fixture.voice(0).generation;
    let old_native = fixture.voice(0).stretch.native_state_address();
    let phase = fixture.voice(0).source_playback.position();
    let phase = phase.frame as f64 + phase.fraction;
    let mut dependent = None;
    let ticket = if resident {
        let ticket = fixture
            .control
            .engine
            .prepare_resident_control(0, None, None, None, Some(!old))
            .unwrap();
        let start = fixture
            .control
            .engine
            .capture_pad_start(
                0,
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap();
        let dependency = ticket.capture_start_dependency(&start).unwrap();
        assert!(dependency.resolve(&start).unwrap().is_none());
        dependent = Some((start, dependency));
        wait(|| {
            fixture.drain();
            fixture.mixer.pending_key_lock_tuples[0].is_some()
        });
        assert_eq!(ticket.publication_status(), "adopting");
        Some(ticket)
    } else {
        fixture.control.engine.set_pad_key_lock(0, !old).unwrap();
        fixture.drain();
        None
    };
    let first = fixture.render(1);
    let first_expected = reference.render(1);
    compare(&first, &first_expected, "old first sample");
    let e = fixture.mixer.pending_key_lock_tuples[0]
        .as_ref()
        .unwrap()
        .execution
        .unwrap();
    assert_eq!(e, f + 4096);
    wait_candidate(&mut fixture);
    let before = fixture.render_to(e);
    let before_expected = reference.render_to(e);
    compare(&before, &before_expected, "old continuous PCM before E");
    assert_eq!(fixture.mixer.pad_key_lock_enabled[0], old);
    assert_eq!(fixture.voice(0).stretch.native_state_address(), old_native);
    if let Some(ticket) = &ticket {
        assert_eq!(ticket.publication_status(), "adopting");
    }
    if let Some((start, dependency)) = &dependent {
        assert!(
            dependency.resolve(start).unwrap().is_none(),
            "GLOBAL dependency released before own mode E"
        );
        assert!(
            !fixture
                .control
                .engine
                .publish_pad_start(start, ticket.as_ref())
                .unwrap(),
            "ordinary start released before own mode E"
        );
    }
    let status = fixture.mixer.pad_key_lock_status[0].unwrap();
    assert_eq!(status.effective, old);
    assert!(!status.ready);
    let request = status.request_id;
    let mut actual = fixture.render(1);
    assert_eq!(fixture.mixer.pad_key_lock_enabled[0], !old);
    assert_eq!(fixture.voice(0).generation, generation);
    assert_ne!(fixture.voice(0).stretch.native_state_address(), old_native);
    if let Some(ticket) = &ticket {
        assert_eq!(ticket.publication_status(), "accepted");
    }
    let status = fixture.mixer.pad_key_lock_status[0].unwrap();
    assert_eq!(status.request_id, request);
    assert_eq!(status.effective, !old);
    assert!(status.ready);
    actual.extend(fixture.pcm(23_999));
    let p = 2.0_f64.powf(f64::from(extra) / 12.0) / if !old { ratio } else { 1.0 };
    let mut raw = Raw::new(p);
    raw.render(&source_pcm(phase, ratio, 0, 4096));
    let expected = raw.render(&source_pcm(phase, ratio, 4096, 24_000));
    compare(
        &actual,
        &expected,
        "prospective actual Native/FIFO PCM at E",
    );
    assert_tone(&actual, 0, extra, ratio, !old);
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(
        fixture.mixer.pad_pitch_intent[0],
        PitchIntent::new(0, extra).unwrap()
    );
}

#[test]
fn complete_public_mode_requests_adopt_exact_prospective_pcm_in_both_directions() {
    for extra in [-7, 7] {
        for old in [false, true] {
            for ratio in [1.0, 1.25] {
                prove_mode_change(
                    complete_fixture(old),
                    complete_fixture(old),
                    extra,
                    old,
                    ratio,
                    false,
                );
            }
        }
    }
    prove_mode_change(
        complete_fixture(false),
        complete_fixture(false),
        12,
        false,
        2.0,
        false,
    );
}

#[cfg(windows)]
#[test]
fn finite_public_mode_window_tickets_wait_for_same_request_prospective_pcm() {
    use crate::audio_engine::ordinary_start_tests::install_synthetic_complete_lease;
    for extra in [-7, 7] {
        for old in [false, true] {
            for ratio in [1.0, 1.25] {
                let mut fixture = Fixture::new(old);
                let mut reference = Fixture::new(old);
                let _files = install_synthetic_complete_lease(&mut fixture);
                let _reference_files = install_synthetic_complete_lease(&mut reference);
                prove_mode_change(fixture, reference, extra, old, ratio, true);
            }
        }
    }
    let mut fixture = Fixture::new(false);
    let mut reference = Fixture::new(false);
    let _files = install_synthetic_complete_lease(&mut fixture);
    let _reference_files = install_synthetic_complete_lease(&mut reference);
    prove_mode_change(fixture, reference, 12, false, 2.0, true);
}

/// This host-side sink advertises actual zero capacity and retains the exact Box it receives.
/// Any unadmitted retirement is a failure, rather than the production sink's final fallback.
struct RetirementGate {
    capacity: usize,
    retired: Option<Box<crate::messages::ResidentTransaction>>,
    calls: usize,
}

impl RetirementGate {
    fn closed() -> Self {
        Self {
            capacity: 0,
            retired: None,
            calls: 0,
        }
    }
}

impl AudioBufferRetirement for RetirementGate {
    fn retire_resident_transaction(&mut self, work: Box<crate::messages::ResidentTransaction>) {
        assert!(self.capacity > 0, "unadmitted resident retirement");
        assert!(self.retired.is_none(), "resident owner retired twice");
        self.capacity -= 1;
        self.calls += 1;
        self.retired = Some(work);
    }
    fn retire_resident_capture(
        &mut self,
        _: Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
        panic!("unexpected owner");
    }
    fn retire_resident_cancellation(&mut self, _: Arc<std::sync::atomic::AtomicBool>) {
        panic!("unexpected owner");
    }
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {
        panic!("unexpected owner");
    }
    fn retire_sample(&mut self, _: SampleBuffer) {
        panic!("unexpected sample retirement");
    }
    fn retire_prepared_stems(&mut self, _: PreparedStemSet) {
        panic!("unexpected stems retirement");
    }
    fn retire_constant_timing(&mut self, _: PreparedConstantTiming) {
        panic!("unexpected timing retirement");
    }
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        panic!("unexpected batch retirement");
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
        panic!("unexpected timing refresh retirement");
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.capacity
    }
}

#[cfg(windows)]
#[test]
fn finite_mode_retains_exact_owner_with_zero_retirement_capacity_until_completion_or_cancel_can_settle()
 {
    use crate::audio_engine::ordinary_start_tests::install_synthetic_complete_lease;
    for outcome in ["ready", "cancelled", "missed", "teardown"] {
        let mut fixture = Fixture::new(false);
        let _files = install_synthetic_complete_lease(&mut fixture);
        start(&mut fixture, 7, false, 1.0);
        let ticket = fixture
            .control
            .engine
            .prepare_resident_control(0, None, None, None, Some(true))
            .unwrap();
        wait(|| {
            fixture.drain();
            fixture.mixer.pending_key_lock_tuples[0].is_some()
        });
        fixture.render(1);
        wait_candidate(&mut fixture);
        let pending = fixture.mixer.pending_key_lock_tuples[0].as_ref().unwrap();
        let e = pending.execution.unwrap();
        let work = pending.resident.as_ref().unwrap();
        let owner_address = work.as_ref() as *const crate::messages::ResidentTransaction;
        let sample_pin = Arc::downgrade(&work.sample.samples);
        let native = fixture.voice(0).stretch.native_state_address();
        fixture.render_to(e);
        if outcome == "cancelled" {
            fixture.mixer.stop_sample_rt(0, &mut fixture.retirement);
        }
        let mut gate = RetirementGate::closed();
        fixture.mixer.poll_key_lock_tuples(Some(e), &mut gate);
        assert_eq!(gate.calls, 0);
        assert_eq!(ticket.publication_status(), "adopting");
        assert!(
            fixture
                .control
                .engine
                .input_runtime_ownership
                .resident_control_pending(0)
        );
        assert!(!fixture.mixer.pad_key_lock_enabled[0]);
        assert_eq!(
            fixture.mixer.pending_key_lock_tuples[0]
                .as_ref()
                .unwrap()
                .resident
                .as_ref()
                .unwrap()
                .as_ref() as *const _,
            owner_address
        );
        if outcome == "teardown" {
            // The production callback-close owner transfers the whole mixer box to its
            // existing retirement worker. This fixture's off-callback teardown likewise
            // destroys the retained envelope intact, without needing a normal queue slot.
            drop(ticket);
            drop(fixture);
            assert_eq!(gate.calls, 0);
            assert!(gate.retired.is_none());
            wait(|| sample_pin.upgrade().is_none());
            continue;
        }
        if outcome == "missed" {
            let mut output = [0.0; 64];
            fixture.mixer.render_rt_at_output_frame(
                &mut output,
                &mut [0.0; NUM_SAMPLES],
                e,
                &mut gate,
            );
            fixture.transport.advance_by_rendered_frames(64);
            assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
            assert_eq!(gate.calls, 0);
        }
        gate.capacity = 1;
        fixture
            .mixer
            .poll_key_lock_tuples(Some(fixture.transport.output_frame()), &mut gate);
        assert_eq!(gate.calls, 1);
        assert_eq!(
            gate.retired.as_ref().unwrap().as_ref() as *const _,
            owner_address
        );
        assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
        assert!(
            !fixture
                .control
                .engine
                .input_runtime_ownership
                .resident_control_pending(0)
        );
        assert_eq!(
            ticket.publication_status(),
            if outcome == "ready" {
                "accepted"
            } else {
                "rejected"
            }
        );
        assert_eq!(fixture.mixer.pad_key_lock_enabled[0], outcome == "ready");
        fixture
            .mixer
            .poll_key_lock_tuples(Some(fixture.transport.output_frame()), &mut gate);
        assert_eq!(gate.calls, 1, "terminal owner settled twice");
        drop(ticket);
        drop(fixture);
        assert!(
            sample_pin.upgrade().is_some(),
            "retirement sink must still own actual PCM"
        );
        drop(gate.retired.take());
        wait(|| sample_pin.upgrade().is_none());
    }
}

#[cfg(windows)]
#[test]
fn refused_mode_reconciles_its_actual_new_window_and_retry_prepares_real_pcm() {
    use crate::audio_engine::ordinary_start_tests::install_synthetic_complete_lease;
    use crate::audio_engine::resident_relocation;
    for race in ["stop", "timing"] {
        let mut fixture = Fixture::new(true);
        let _files = install_synthetic_complete_lease(&mut fixture);
        start(&mut fixture, 7, true, 1.0);
        let previous = fixture.control.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap();
        let ticket = fixture
            .control
            .engine
            .prepare_resident_control(0, None, None, None, Some(false))
            .unwrap();
        wait(|| {
            fixture.drain();
            fixture.mixer.pending_key_lock_tuples[0].is_some()
        });
        let actual = fixture.mixer.sample_bank[0].as_ref().unwrap().clone();
        assert_ne!(
            actual.resident_binding(),
            previous.resident_binding(),
            "real finite context/window change required"
        );
        assert_eq!(
            actual.residency.as_ref().unwrap().context,
            crate::messages::ResidentContext::FiniteLoop
        );
        assert_eq!(
            fixture.control.engine.sample_cache.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .resident_binding(),
            previous.resident_binding()
        );
        assert!(
            fixture.mixer.pending_key_lock_tuples[0]
                .as_ref()
                .unwrap()
                .resident
                .as_ref()
                .unwrap()
                .publication
                .resident_storage_adopted()
        );
        fixture.render(1);
        wait_candidate(&mut fixture);
        let e = fixture.mixer.pending_key_lock_tuples[0]
            .as_ref()
            .unwrap()
            .execution
            .unwrap();
        match race {
            "stop" => fixture.mixer.stop_sample_rt(0, &mut fixture.retirement),
            "timing" => fixture.mixer.set_speed(1.25),
            _ => unreachable!(),
        }
        fixture.render_to(e + 1);
        assert_eq!(ticket.publication_status(), "rejected");
        assert!(fixture.mixer.pad_key_lock_enabled[0]);
        resident_relocation::reconcile(&fixture.control.engine).unwrap();
        let cache = fixture.control.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap();
        assert_eq!(cache.resident_binding(), actual.resident_binding());
        assert!(Arc::ptr_eq(&cache.samples, &actual.samples));
        assert!(
            fixture
                .control
                .engine
                .current_input_runtime_pad_binding(0)
                .unwrap()
                .is_some(),
            "actual reconciled window must admit fresh capture"
        );
        if race == "stop" {
            start(&mut fixture, 7, true, 1.0);
        }
        fixture.pcm(8192);
        let ratio = if race == "timing" { 1.25 } else { 1.0 };
        let position = fixture.voice(0).source_playback.position();
        let phase = position.frame as f64 + position.fraction;
        let retry = fixture
            .control
            .engine
            .prepare_resident_control(0, None, None, None, Some(false))
            .unwrap();
        wait(|| {
            fixture.drain();
            fixture.mixer.pending_key_lock_tuples[0].is_some()
        });
        fixture.render(1);
        wait_candidate(&mut fixture);
        let e = fixture.mixer.pending_key_lock_tuples[0]
            .as_ref()
            .unwrap()
            .execution
            .unwrap();
        fixture.render_to(e);
        let mut pcm = fixture.render(1);
        assert_eq!(retry.publication_status(), "accepted");
        assert_eq!(
            ticket.publication_status(),
            "rejected",
            "retry cannot accept its predecessor"
        );
        pcm.extend(fixture.pcm(23_999));
        let mut raw = Raw::new(2.0_f64.powf(7.0 / 12.0));
        raw.render(&source_pcm(phase, ratio, 0, 4096));
        compare(
            &pcm,
            &raw.render(&source_pcm(phase, ratio, 4096, 24_000)),
            "retry from actual refused-mode window",
        );
        assert_tone(&pcm, 0, 7, ratio, false);
        resident_relocation::reconcile(&fixture.control.engine).unwrap();
        assert!(
            fixture
                .control
                .engine
                .current_input_runtime_pad_binding(0)
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn stale_timing_pitch_stop_and_source_never_adopt_a_prospective_mode_owner() {
    for race in ["timing", "pitch", "stop", "source", "window"] {
        let mut fixture = complete_fixture(false);
        start(&mut fixture, 7, false, 1.0);
        submit_scalar(&mut fixture, true);
        wait_candidate(&mut fixture);
        let e = fixture.mixer.pending_key_lock_tuples[0]
            .as_ref()
            .unwrap()
            .execution
            .unwrap();
        let native = fixture.voice(0).stretch.native_state_address();
        match race {
            "timing" => fixture.mixer.set_speed(1.25),
            "pitch" => fixture.mixer.pad_pitch_intent[0] = PitchIntent::new(0, -7).unwrap(),
            "stop" => fixture.mixer.stop_sample(0),
            "source" => {
                let source = SampleBuffer {
                    channels: 1,
                    samples: vec![0.125; 144_000].into(),
                    residency: None,
                }
                .with_complete_source(RATE);
                publish_tone(&fixture.control, 0, source, true);
            }
            "window" => {
                let old = fixture.mixer.sample_bank[0].as_ref().unwrap();
                let changed = old
                    .window(
                        LOOP_START,
                        LOOP_END,
                        old.window_revision() + 1,
                        crate::messages::ResidentContext::KeyLockFiniteLoop,
                    )
                    .unwrap();
                let ownership = &fixture.control.engine.input_runtime_ownership;
                ownership
                    .publish_window_for_cell(ownership.slot_snapshot(0).unwrap().cell(), &changed);
            }
            _ => unreachable!(),
        }
        fixture.render_to(e + 1);
        assert!(fixture.mixer.pending_key_lock_tuples[0].is_none(), "{race}");
        assert!(!fixture.mixer.pad_key_lock_enabled[0], "{race}");
        if race != "stop" {
            assert_eq!(
                fixture.voice(0).stretch.native_state_address(),
                native,
                "{race}"
            );
        }
    }
}

#[test]
fn variable_history_refusal_is_request_local_and_worker_can_prepare_a_later_request() {
    let mut fixture = complete_fixture(false);
    start(&mut fixture, 18, false, 1.0);
    // An actual new rate trajectory has no reconstructible expanded-pitch past.
    // Exercise the existing typed UnavailableSourceHistory, not an endpoint-rate guess.
    fixture.mixer.set_speed(1.5);
    submit_scalar(&mut fixture, true);
    wait(|| {
        fixture
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active && voice.sample_id == 0)
            .all(|voice| voice.stretch.prospective_work_finished())
    });
    let e = fixture.mixer.pending_key_lock_tuples[0]
        .as_ref()
        .unwrap()
        .execution
        .unwrap();
    fixture.render_to(e + 1);
    assert!(!fixture.mixer.pad_key_lock_enabled[0]);
    assert_eq!(fixture.mixer.pad_key_lock_status[0].unwrap().state, "error");
    assert!(
        fixture
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.stretch.wet_mode_failed())
    );
    // The existing source clock acquires actual constant history by playing it.
    fixture.pcm(24_000);
    let request = submit_scalar(&mut fixture, true);
    wait_candidate(&mut fixture);
    let e = fixture.mixer.pending_key_lock_tuples[0]
        .as_ref()
        .unwrap()
        .execution
        .unwrap();
    fixture.render_to(e + 1);
    assert!(fixture.mixer.pad_key_lock_enabled[0]);
    assert_eq!(
        fixture.mixer.pad_key_lock_status[0].unwrap().request_id,
        request
    );
}

#[test]
fn one_stale_participant_prevents_every_prospective_native_swap() {
    let mut fixture = complete_fixture(false);
    start(&mut fixture, 7, false, 1.0);
    let first = fixture
        .mixer
        .voices
        .iter()
        .position(|voice| voice.active && voice.sample_id == 0)
        .unwrap();
    let second = fixture
        .mixer
        .voices
        .iter()
        .position(|voice| !voice.active)
        .unwrap();
    let source = &fixture.mixer.voices[first];
    let config = VoiceStartConfig {
        sample_id: 0,
        sample: source.sample.clone().unwrap(),
        initial_frame_pos: source.frame_pos,
        volume: 0.0,
        initial_tempo_ratio: 1.0,
        start_output_frame: Some(fixture.transport.output_frame()),
        source_timing: source.source_timing,
        source_admission: source.source_admission.clone(),
    };
    fixture.mixer.voices[second].start_rt(config, &mut fixture.retirement);
    submit_scalar(&mut fixture, true);
    wait_candidate(&mut fixture);
    let first_native = fixture.mixer.voices[first].stretch.native_state_address();
    let second_native = fixture.mixer.voices[second].stretch.native_state_address();
    let e = fixture.mixer.pending_key_lock_tuples[0]
        .as_ref()
        .unwrap()
        .execution
        .unwrap();
    fixture.mixer.voices[second].stretch.invalidate_prepared();
    fixture.render_to(e + 1);
    assert!(!fixture.mixer.pad_key_lock_enabled[0]);
    assert_eq!(
        fixture.mixer.voices[first].stretch.native_state_address(),
        first_native
    );
    assert_eq!(
        fixture.mixer.voices[second].stretch.native_state_address(),
        second_native
    );
}

#[test]
fn superseding_mode_request_keeps_old_audio_and_cannot_reuse_the_old_candidate() {
    let mut fixture = complete_fixture(false);
    start(&mut fixture, 7, false, 1.0);
    let old_request = submit_scalar(&mut fixture, true);
    wait_candidate(&mut fixture);
    let native = fixture.voice(0).stretch.native_state_address();
    let new_request = fixture.control.engine.set_pad_key_lock(0, false).unwrap();
    assert_ne!(old_request, new_request);
    fixture.drain();
    fixture.render(1);
    assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
    assert!(!fixture.mixer.pad_key_lock_enabled[0]);
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(
        fixture.mixer.pad_key_lock_status[0].unwrap().request_id,
        new_request
    );
}
