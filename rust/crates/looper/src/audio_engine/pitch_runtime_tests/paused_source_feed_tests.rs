//! Public paused SETs replay genuine active past; silent global time supplies no reads.
//! The independent finite A-reset contract is separate from continuous-O/listening gates.

use super::*;
use crate::audio_engine::content_placement::ContentKey;
use crate::audio_engine::dsp::{DspNodeSlot, DspParameterId, DspParameterSlot, PerPadDspChain};
use crate::audio_engine::source_playback::SourcePlayback;

const PARTITIONS: [usize; 11] = [1, 73, 511, 127, 512, 379, 512, 512, 512, 512, 446];
const RESUMED: usize = 4_097 + 8_193;

fn pitch(frame: ScalarFrame, intent: PitchIntent, lock: bool) -> f64 {
    2.0_f64.powf(f64::from(intent.total_semitones()) / 12.0) / if lock { frame.rate } else { 1.0 }
}

/// Query the direct C ABI reset profile, without production history/reader helpers.
fn reset_origin(
    timeline: &[ScalarFrame],
    endpoint: usize,
    intent: PitchIntent,
    lock: bool,
) -> (usize, usize) {
    let mut queried = BTreeMap::new();
    let mut bound = RawRubberBand::history_bound(pitch(timeline[endpoint], intent, lock));
    loop {
        let origin = (endpoint - bound) / BLOCK * BLOCK;
        let mut required = bound;
        for frame in &timeline[origin..endpoint] {
            let native_pitch = pitch(*frame, intent, lock);
            let history = *queried
                .entry(native_pitch.to_bits())
                .or_insert_with(|| RawRubberBand::history_bound(native_pitch));
            required = required.max(history);
        }
        if required == bound {
            return (origin, bound);
        }
        bound = required;
    }
}

struct PausedSetOracle {
    reference: RawRubberBand,
    backwards: RawRubberBand,
    origin: usize,
    bound: usize,
    endpoint: usize,
    intent: PitchIntent,
    lock: bool,
    rendered: usize,
    maximum: f32,
    wrong_maximum: f32,
}

impl PausedSetOracle {
    fn new(
        timeline: &[ScalarFrame],
        source: &SampleBuffer,
        endpoint: usize,
        intent: PitchIntent,
        lock: bool,
    ) -> Self {
        let (origin, bound) = reset_origin(timeline, endpoint, intent, lock);
        assert!(bound > 4_096 && origin < endpoint);
        let first_pitch = pitch(timeline[origin + BLOCK - 1], intent, lock);
        let mut reference = RawRubberBand::new(first_pitch);
        let mut backwards = RawRubberBand::new(first_pitch);
        let end = timeline[endpoint];
        for (active, frame) in timeline
            .iter()
            .copied()
            .enumerate()
            .take(endpoint)
            .skip(origin)
        {
            reference.sample(frame.pcm(&source.samples), pitch(frame, intent, lock));
            let coordinate = (end.frame as f64 + end.fraction
                - START as f64
                - (endpoint - active) as f64 * end.rate)
                .rem_euclid((END - START) as f64);
            let wrong = ScalarFrame {
                frame: START + coordinate.floor() as usize,
                fraction: coordinate.fract(),
                passage: Passage::Normal,
                rate: end.rate,
            };
            backwards.sample(wrong.pcm(&source.samples), pitch(wrong, intent, lock));
        }
        assert_eq!(
            reference.fifo(),
            (
                (endpoint - origin) % BLOCK,
                BLOCK - 1 - (endpoint - origin) % BLOCK
            )
        );
        assert_eq!(reference.fifo(), backwards.fifo());
        Self {
            reference,
            backwards,
            origin,
            bound,
            endpoint,
            intent,
            lock,
            rendered: 0,
            maximum: 0.0,
            wrong_maximum: 0.0,
        }
    }

    fn compare(
        &mut self,
        fixture: &mut Fixture,
        source: &SampleBuffer,
        timeline: &[ScalarFrame],
        frames: usize,
        label: &str,
    ) {
        let actual = fixture.render(frames);
        for (offset, sample) in actual.iter().enumerate() {
            let frame = timeline[self.endpoint + self.rendered + offset];
            let native_pitch = pitch(frame, self.intent, self.lock);
            let expected = self
                .reference
                .sample(frame.pcm(&source.samples), native_pitch);
            let wrong = self
                .backwards
                .sample(frame.pcm(&source.samples), native_pitch);
            let error = (*sample - expected).abs();
            self.maximum = self.maximum.max(error);
            self.wrong_maximum = self.wrong_maximum.max((*sample - wrong).abs());
            assert!(
                error < 2.0e-6,
                "{label}: resumed_offset={} actual={sample} independent_A_reset={expected} error={error}",
                self.rendered + offset
            );
        }
        self.rendered += frames;
        assert_phase(fixture, timeline[self.endpoint + self.rendered]);
        assert_eq!(
            fixture.voice(0).stretch.pending_fifo_frames(),
            self.reference.fifo(),
            "{label}"
        );
    }
}

struct OriginalReceipt {
    sequence: u64,
    timestamp: u64,
    content: ContentKey,
    original_t: u64,
    execution: u64,
}

impl OriginalReceipt {
    fn capture(action: &CapturedPitchAction, execution: u64) -> Self {
        Self {
            sequence: action.sequence(),
            timestamp: action.received_timestamp(),
            content: action.content_key(),
            original_t: action.original_due_frame(),
            execution,
        }
    }

    fn assert_unchanged(&self, action: &CapturedPitchAction) {
        assert_eq!(action.sequence(), self.sequence);
        assert_eq!(action.received_timestamp(), self.timestamp);
        assert_eq!(action.content_key(), self.content);
        assert_eq!(action.original_due_frame(), self.original_t);
        assert_eq!(action.execution_target(), self.execution);
    }
}

fn active_clock(fixture: &Fixture, id: usize) -> (u64, u64) {
    let mut snapshot = crate::audio_engine::source_feed_history::SourceFeedSnapshot::new();
    assert!(
        fixture
            .voice(id)
            .source_feed_history
            .capture_into(&mut snapshot)
    );
    (snapshot.active_origin(), snapshot.active_frames())
}

/// Unequal probe gains expose warm filter state while the actual chain stays neutral.
fn filter_probe(chain: &PerPadDspChain) -> Vec<f32> {
    let mut probe = chain.clone();
    let low = DspParameterId::per_pad(0, DspNodeSlot::Slot0, DspParameterSlot::Slot0).unwrap();
    assert!(probe.set_parameter(low, 0.0));
    (0..73)
        .map(|frame| {
            probe.begin_frame();
            probe.process_sample(0, if frame == 0 { 0.37 } else { 0.0 })
        })
        .collect()
}

fn current_filter(fixture: &Fixture) -> Vec<f32> {
    filter_probe(&fixture.mixer.dsp_chain_snapshot_for_test(0).unwrap())
}

struct FrozenVoice {
    playback: SourcePlayback,
    history: (u64, u64),
    filter: Vec<f32>,
    generation: u64,
    address: usize,
    starts: usize,
}

impl FrozenVoice {
    fn capture(fixture: &Fixture) -> Self {
        let voice = fixture.voice(0);
        Self {
            playback: voice.source_playback,
            history: active_clock(fixture, 0),
            filter: current_filter(fixture),
            generation: voice.generation,
            address: voice.sample.as_ref().unwrap().source_address(),
            starts: fixture.starts(0),
        }
    }

    fn assert_paused(&self, fixture: &Fixture) {
        let voice = fixture.voice(0);
        assert!(voice.paused);
        assert!(voice.source_playback.matches_exact(&self.playback));
        assert_eq!(active_clock(fixture, 0), self.history);
        assert_eq!(
            current_filter(fixture),
            self.filter,
            "paused SET must retain actual filter history"
        );
        assert_eq!(voice.generation, self.generation);
        assert_eq!(
            voice.sample.as_ref().unwrap().source_address(),
            self.address
        );
        assert_eq!(fixture.starts(0), self.starts);
    }

    fn render_silent(&self, fixture: &mut Fixture, frames: usize) {
        let native = fixture.voice(0).stretch.native_state_address();
        let fifo = fixture.voice(0).stretch.pending_fifo_frames();
        assert!(fixture.render(frames).iter().all(|sample| *sample == 0.0));
        self.assert_paused(fixture);
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(fixture.voice(0).stretch.pending_fifo_frames(), fifo);
    }

    fn render_silent_to(&self, fixture: &mut Fixture, target: u64) {
        while fixture.transport.output_frame() < target {
            let frames = (target - fixture.transport.output_frame()).min(127) as usize;
            self.render_silent(fixture, frames);
        }
        assert_eq!(fixture.transport.output_frame(), target);
    }
}

fn pause_public(fixture: &mut Fixture) {
    let frame = fixture.transport.output_frame();
    let phase = fixture.voice(0).source_playback;
    fixture.control.engine.pause_sample(0).unwrap();
    fixture.drain();
    assert_eq!(fixture.transport.output_frame(), frame);
    assert!(fixture.voice(0).paused);
    assert!(fixture.voice(0).source_playback.matches_exact(&phase));
}

fn resume_public(fixture: &mut Fixture) {
    let frame = fixture.transport.output_frame();
    let phase = fixture.voice(0).source_playback;
    fixture.control.engine.resume_sample(0).unwrap();
    fixture.drain();
    assert_eq!(fixture.transport.output_frame(), frame);
    assert!(!fixture.voice(0).paused);
    assert!(fixture.voice(0).source_playback.matches_exact(&phase));
}

fn start_silent_peer(fixture: &mut Fixture) {
    let frame = fixture.transport.output_frame();
    let start = fixture
        .control
        .engine
        .capture_pad_start(
            1,
            LOOP_START as f64 / f64::from(RATE),
            Some(LOOP_END as f64 / f64::from(RATE)),
            0.0,
            false,
            None,
            None,
        )
        .unwrap();
    fixture
        .control
        .engine
        .publish_pad_start(&start, None)
        .unwrap();
    fixture.drain();
    assert_eq!(start.original_frame(), Some(frame));
    assert_eq!(start.execution_frame(), Some(frame));
    let deadline = Instant::now() + WORKER_WAIT;
    while start.effective_frame().is_none() {
        assert_ne!(start.state(), "rejected", "{:?}", start.reason());
        assert!(
            Instant::now() < deadline,
            "silent peer public start host wait exceeded 2 s"
        );
        fixture.mixer.pitch_runtime.as_mut().unwrap().poll();
        fixture.drain();
        assert_eq!(fixture.transport.output_frame(), frame);
        std::thread::sleep(Duration::from_millis(1));
    }
    fixture.render(1);
    assert_eq!(start.effective_frame(), Some(frame));
    assert_eq!(fixture.voice(1).volume, 0.0);
    assert!(!fixture.voice(1).paused);
}

fn capture_set(
    fixture: &Fixture,
    intent: PitchIntent,
    lock: bool,
    grid_origin: Option<u64>,
) -> CapturedPitchAction {
    fixture.rows(PitchPadProjection {
        intent: PitchIntent::new(intent.base_shift(), 0).unwrap(),
        key_lock: lock,
        quantize_step_64ths: grid_origin.map(|_| 16),
        ..PitchPadProjection::default()
    });
    fixture.refresh_clock();
    if let Some(origin) = grid_origin {
        let mut clock = fixture.clock.read().unwrap();
        clock.downbeat_frame = origin;
        clock.master_beat = (fixture.transport.output_frame() - origin) as f64 / 24_000.0;
        fixture.clock.publish(clock);
    }
    let action = fixture
        .control
        .engine
        .capture_pad_pitch_action(
            0,
            &format!("pitch.extra:{}:set", intent.extra_shift()),
            None,
        )
        .unwrap();
    assert_eq!(action.render().pitch, intent);
    assert_eq!(action.render().key_lock, lock);
    action
}

fn wait_rejected(fixture: &mut Fixture, action: &CapturedPitchAction) {
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + WORKER_WAIT;
    while action.state() != "rejected" {
        fixture.mixer.pitch_runtime.as_mut().unwrap().poll();
        assert!(
            Instant::now() < deadline,
            "paused SET refusal host wait exceeded 2 s"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), frame);
    assert_eq!(action.reason(), Some("preparation_failed"));
    assert!(!action.effective());
}

fn compare_resumed(
    fixture: &mut Fixture,
    source: &SampleBuffer,
    timeline: &[ScalarFrame],
    oracle: &mut PausedSetOracle,
    receipt: &OriginalReceipt,
    frozen: &FrozenVoice,
    label: &str,
) {
    let adopted = fixture.voice(0).stretch.native_state_address();
    resume_public(fixture);
    assert_eq!(current_filter(fixture), frozen.filter);
    assert_eq!(fixture.voice(0).stretch.native_state_address(), adopted);
    assert_eq!(
        fixture.voice(0).stretch.pending_fifo_frames(),
        oracle.reference.fifo()
    );
    assert!(!fixture.voice(0).stretch.wet_current_source_output_ready());
    for frames in PARTITIONS
        .into_iter()
        .chain(std::iter::repeat_n(512, 16))
        .chain(std::iter::once(1))
    {
        oracle.compare(fixture, source, timeline, frames, label);
        let voice = fixture.voice(0);
        assert_eq!(
            voice.stretch.native_state_address(),
            adopted,
            "{label}: Resume replaced the claimed Native/FIFO owner"
        );
        assert_eq!(voice.stretch.adopted_request_id(), Some(receipt.sequence));
        assert!(
            !voice.stretch.source_preparation_pending(),
            "{label}: Resume queued an automatic replacement"
        );
        assert_eq!(voice.generation, frozen.generation);
        assert_eq!(
            voice.sample.as_ref().unwrap().source_address(),
            frozen.address
        );
        assert_eq!(fixture.starts(0), frozen.starts);
        assert_eq!(fixture.mixer.pad_pitch_intent(0), Some(oracle.intent));
        if oracle.rendered == 1 {
            assert!(voice.stretch.wet_current_source_output_ready());
        }
    }
    assert_eq!(oracle.rendered, RESUMED);
    assert!(
        oracle.wrong_maximum > 0.005,
        "{label}: backwards-constant history must disagree: {}",
        oracle.wrong_maximum
    );
    assert_eq!(
        fixture
            .voice(0)
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames,
        (oracle.endpoint - oracle.origin + RESUMED) as u64
    );
    println!(
        "paused_set_A_reset {label} active_F={} relative_A={} H={} original_T={} original_global_E={} sequence={} resumed_samples={} every_sample_max_abs={} backwards_constant_max_abs={}; continuous_O_and_acoustic_acceptance_are_separate",
        oracle.endpoint,
        oracle.origin,
        oracle.bound,
        receipt.original_t,
        receipt.execution,
        receipt.sequence,
        oracle.rendered,
        oracle.maximum,
        oracle.wrong_maximum
    );
}

#[test]
fn public_paused_sets_replay_genuine_past_and_resume_every_sample_without_owner_replacement() {
    for passage in [Passage::Intro, Passage::Tail] {
        for intent in [
            PitchIntent::new(6, 18).unwrap(),
            PitchIntent::new(-5, -18).unwrap(),
        ] {
            for lock in [false, true] {
                let (mut fixture, source, output_origin) = setup_pitch(passage, lock, 0);
                start_silent_peer(&mut fixture);
                run_controls_to_capture(&mut fixture, output_origin);
                let timeline = chronology(passage, F + RESUMED);
                assert_phase(&fixture, timeline[F]);
                let mut oracle = PausedSetOracle::new(&timeline, &source, F, intent, lock);
                let crossing = timeline
                    .iter()
                    .position(|frame| frame.passage == Passage::Normal)
                    .unwrap();
                assert!(
                    oracle.origin < crossing && crossing < F,
                    "{passage:?} crossing={crossing} must lie in genuine past {}..{F}",
                    oracle.origin
                );
                let mut cold = fixture.mixer.dsp_chain_snapshot_for_test(0).unwrap();
                cold.reset();
                assert!(
                    current_filter(&fixture)
                        .iter()
                        .zip(filter_probe(&cold))
                        .any(|(warm, cold)| (*warm - cold).abs() > 1.0e-7),
                    "filter preservation needs a nonvacuous warm state"
                );
                pause_public(&mut fixture);
                let frozen = FrozenVoice::capture(&fixture);
                assert_eq!(frozen.history.1 - frozen.history.0, F as u64);
                let old_native = fixture.voice(0).stretch.native_state_address();
                let peer_clock = active_clock(&fixture, 1).1;
                let peer_generation = fixture.voice(1).generation;
                let peer_starts = fixture.starts(1);
                let capture = fixture.transport.output_frame();
                let action = capture_set(&fixture, intent, lock, Some(output_origin));
                assert_eq!(action.original_due_frame(), output_origin + E as u64);
                let receipt = OriginalReceipt::capture(&action, action.original_due_frame());
                fixture.publish(&action);
                receipt.assert_unchanged(&action);
                assert_eq!(receipt.execution - capture, 4_096);
                fixture.wait_worker(&[&action]);
                assert_eq!(fixture.transport.output_frame(), capture);
                frozen.render_silent_to(&mut fixture, receipt.execution);
                assert!(!action.effective());
                assert_eq!(fixture.voice(0).stretch.native_state_address(), old_native);
                assert_eq!(
                    fixture.mixer.pad_pitch_intent(0),
                    Some(PitchIntent::default())
                );
                assert_eq!(fixture.render(1), vec![0.0]);
                assert!(
                    action.effective(),
                    "{} {:?}",
                    action.state(),
                    action.reason()
                );
                receipt.assert_unchanged(&action);
                frozen.assert_paused(&fixture);
                assert_ne!(fixture.voice(0).stretch.native_state_address(), old_native);
                assert_eq!(
                    fixture.voice(0).stretch.adopted_request_id(),
                    Some(receipt.sequence)
                );
                assert!(fixture.voice(0).stretch.owns_pitch_contract(intent, lock));
                assert_eq!(
                    fixture.voice(0).stretch.pending_fifo_frames(),
                    oracle.reference.fifo()
                );
                assert!(!fixture.voice(0).stretch.wet_render_ready());
                assert!(!fixture.voice(0).stretch.wet_current_source_output_ready());
                for frames in [73, 511, 127, 512, 379] {
                    frozen.render_silent(&mut fixture, frames);
                    assert!(!fixture.voice(0).stretch.wet_current_source_output_ready());
                }
                assert_eq!(
                    active_clock(&fixture, 1).1 - peer_clock,
                    fixture.transport.output_frame() - capture
                );
                assert_eq!(fixture.voice(1).generation, peer_generation);
                assert_eq!(fixture.starts(1), peer_starts);
                assert!(!fixture.voice(1).paused);
                let label = format!("{passage:?} k={} lock={lock}", intent.total_semitones());
                compare_resumed(
                    &mut fixture,
                    &source,
                    &timeline,
                    &mut oracle,
                    &receipt,
                    &frozen,
                    &label,
                );
                receipt.assert_unchanged(&action);
                assert!(fixture.scheduler.is_empty());
            }
        }
    }
}

#[test]
fn paused_set_resume_pause_alias_and_rate_restore_refuse_original_then_retry_on_same_worker() {
    for race in ["resume_pause", "rate_restore"] {
        // Dry old route avoids independently timed automatic compensation owners.
        let (mut fixture, source, output_origin) = setup_pitch(Passage::Intro, false, 0);
        run_controls_to_capture(&mut fixture, output_origin);
        let timeline = chronology(Passage::Intro, F + RESUMED);
        pause_public(&mut fixture);
        let frozen = FrozenVoice::capture(&fixture);
        let old_native = fixture.voice(0).stretch.native_state_address();
        let old_fifo = fixture.voice(0).stretch.pending_fifo_frames();
        let worker = fixture.mixer.pitch_runtime.as_ref().unwrap() as *const _ as usize;
        let intent = PitchIntent::new(6, 18).unwrap();
        let old = capture_set(&fixture, intent, false, Some(output_origin));
        let receipt = OriginalReceipt::capture(&old, output_origin + E as u64);
        fixture.publish(&old);
        fixture.wait_worker(&[&old]);
        match race {
            "resume_pause" => {
                resume_public(&mut fixture);
                pause_public(&mut fixture);
            }
            "rate_restore" => {
                fixture.mixer.set_master_period(0.5 / 0.91);
                fixture.mixer.set_master_period(0.5 / 1.29);
            }
            _ => unreachable!(),
        }
        // No reads occurred: equal physical phase cannot legitimize the old capture.
        assert_phase(&fixture, timeline[F]);
        frozen.render_silent_to(&mut fixture, receipt.execution);
        frozen.render_silent(&mut fixture, 1);
        wait_rejected(&mut fixture, &old);
        receipt.assert_unchanged(&old);
        assert_eq!(fixture.voice(0).stretch.native_state_address(), old_native);
        assert_eq!(fixture.voice(0).stretch.pending_fifo_frames(), old_fifo);
        assert_eq!(
            fixture.mixer.pad_pitch_intent(0),
            Some(PitchIntent::default())
        );
        assert!(fixture.scheduler.is_empty());
        let fresh = capture_set(&fixture, intent, false, None);
        let fresh_receipt = OriginalReceipt::capture(&fresh, fresh.original_due_frame() + 8_192);
        assert!(fresh.sequence() > old.sequence());
        fixture.publish(&fresh);
        fresh_receipt.assert_unchanged(&fresh);
        fixture.wait_worker(&[&fresh]);
        assert_eq!(
            fixture.mixer.pitch_runtime.as_ref().unwrap() as *const _ as usize,
            worker
        );
        frozen.render_silent_to(&mut fixture, fresh_receipt.execution);
        assert_eq!(fixture.render(1), vec![0.0]);
        assert!(
            fresh.effective(),
            "{race}: {} {:?}",
            fresh.state(),
            fresh.reason()
        );
        frozen.assert_paused(&fixture);
        let mut oracle = PausedSetOracle::new(&timeline, &source, F, intent, false);
        assert_eq!(
            fixture.voice(0).stretch.pending_fifo_frames(),
            oracle.reference.fifo()
        );
        compare_resumed(
            &mut fixture,
            &source,
            &timeline,
            &mut oracle,
            &fresh_receipt,
            &frozen,
            race,
        );
        fresh_receipt.assert_unchanged(&fresh);
        receipt.assert_unchanged(&old);
        assert_eq!(old.state(), "rejected");
        assert!(!old.effective());
        assert!(fixture.scheduler.is_empty());
    }
}

#[test]
fn paused_set_silent_lead_cannot_replace_missing_past_and_same_worker_accepts_genuine_retry() {
    let (mut fixture, source, output_origin) = setup_pitch(Passage::Intro, false, 0);
    fixture.render_to(output_origin + EVENTS[0].0 as u64);
    fixture.mixer.set_speed(EVENTS[0].1);
    fixture.render_to(output_origin + 2_000);
    pause_public(&mut fixture);
    let early = FrozenVoice::capture(&fixture);
    assert_eq!(early.history.1 - early.history.0, 2_000);
    let expected_early = chronology(Passage::Intro, 2_000)[2_000];
    let actual_early = fixture.voice(0).source_playback.position();
    assert_eq!(expected_early.passage, Passage::Intro);
    assert_eq!(actual_early.frame, expected_early.frame);
    assert_eq!(
        actual_early.fraction.to_bits(),
        expected_early.fraction.to_bits()
    );
    assert_eq!(actual_early.seek_mode, ExplicitSeekMode::BeforeLoop);
    // Unlike the legitimate constant pre-origin startup profile, this short
    // actual past includes two smoothing steps after a real off-grid edit.
    let mut captured = crate::audio_engine::source_feed_history::SourceFeedSnapshot::new();
    assert!(
        fixture
            .voice(0)
            .source_feed_history
            .capture_into(&mut captured)
    );
    assert!(captured.spans().len() >= 3);
    assert!(
        captured
            .spans()
            .zip(captured.spans().skip(1))
            .any(|(before, after)| {
                before.playback.tempo_ratio().to_bits() != after.playback.tempo_ratio().to_bits()
            })
    );
    let old_native = fixture.voice(0).stretch.native_state_address();
    let old_fifo = fixture.voice(0).stretch.pending_fifo_frames();
    let worker = fixture.mixer.pitch_runtime.as_ref().unwrap() as *const _ as usize;
    let intent = PitchIntent::new(6, 18).unwrap();
    let refused = capture_set(&fixture, intent, false, None);
    let refused_receipt = OriginalReceipt::capture(&refused, refused.original_due_frame() + 8_192);
    // This request can fail before publish() observes accepted; enqueue the original directly.
    assert!(
        fixture
            .control
            .engine
            .publish_pitch_action(&refused)
            .unwrap()
    );
    fixture.drain();
    wait_rejected(&mut fixture, &refused);
    refused_receipt.assert_unchanged(&refused);
    early.render_silent_to(&mut fixture, refused_receipt.execution);
    early.render_silent(&mut fixture, 1);
    assert_eq!(fixture.voice(0).stretch.native_state_address(), old_native);
    assert_eq!(fixture.voice(0).stretch.pending_fifo_frames(), old_fifo);
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::default())
    );
    assert!(
        RawRubberBand::history_bound(4.0) > 2_000,
        "silent global lead cannot fill actual active past"
    );
    assert!(fixture.scheduler.is_empty());
    // Rebase only the test's global-to-active coordinate after the real public Resume.
    // Source history O/F and the original refused action are never rewritten.
    resume_public(&mut fixture);
    let resumed_origin = fixture.transport.output_frame() - 2_000;
    for (event, rate) in EVENTS.into_iter().skip(1) {
        fixture.render_to(resumed_origin + event as u64);
        if event == EVENTS[1].0 {
            fixture.mixer.set_bpm_lock(true);
        }
        fixture.mixer.set_master_period(0.5 / rate);
    }
    fixture.render_to(resumed_origin + F as u64);
    let timeline = chronology(Passage::Intro, F + RESUMED);
    assert_phase(&fixture, timeline[F]);
    pause_public(&mut fixture);
    let frozen = FrozenVoice::capture(&fixture);
    assert_eq!(frozen.history.1 - frozen.history.0, F as u64);
    let fresh = capture_set(&fixture, intent, false, None);
    let fresh_receipt = OriginalReceipt::capture(&fresh, fresh.original_due_frame() + 8_192);
    assert!(fresh.sequence() > refused.sequence());
    fixture.publish(&fresh);
    fixture.wait_worker(&[&fresh]);
    assert_eq!(
        fixture.mixer.pitch_runtime.as_ref().unwrap() as *const _ as usize,
        worker
    );
    frozen.render_silent_to(&mut fixture, fresh_receipt.execution);
    assert_eq!(fixture.render(1), vec![0.0]);
    assert!(fresh.effective(), "{} {:?}", fresh.state(), fresh.reason());
    frozen.assert_paused(&fixture);
    let mut oracle = PausedSetOracle::new(&timeline, &source, F, intent, false);
    assert_eq!(
        fixture.voice(0).stretch.pending_fifo_frames(),
        oracle.reference.fifo()
    );
    compare_resumed(
        &mut fixture,
        &source,
        &timeline,
        &mut oracle,
        &fresh_receipt,
        &frozen,
        "insufficient_history_fresh_retry",
    );
    fresh_receipt.assert_unchanged(&fresh);
    refused_receipt.assert_unchanged(&refused);
    assert_eq!(refused.state(), "rejected");
    assert!(!refused.effective());
    assert!(fixture.scheduler.is_empty());
}
