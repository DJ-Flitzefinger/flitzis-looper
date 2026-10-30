//! An original ordinary attack outlives worker delay and unrelated placement publication.
//! Gates affect one real worker owner only; no virtual-time wait certifies a device deadline.
#![cfg(windows)]

use super::*;
use crate::audio_engine::input_mapping::PitchPadProjection;
use crate::audio_engine::ordinary_start::OrdinaryStartWork;
use crate::audio_engine::pitch_actions::{
    CapturedPitchAction, PitchActionReject, PitchKeyMode, PreparedActionWork,
};
use pyo3::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};

const CAPTURE_FRAME: u64 = 1_000;
const FIRST_TARGET: u64 = 12_000;
const SECOND_TARGET: u64 = 24_000;
const DELAY_AFTER_TARGET: u64 = 257;
const EMITTED_FRAMES: usize = 24_000;
const GATE_WAIT: Duration = Duration::from_secs(10);

type GateKey = (usize, u64);
type GateRegistry = Mutex<HashMap<GateKey, Weak<WorkerGateState>>>;

#[derive(Default)]
struct GateObservation {
    entered: bool,
    released: bool,
    finished: bool,
    reason: Option<PitchActionReject>,
    return_held: bool,
    prepared_native: Option<usize>,
}

#[derive(Default)]
struct WorkerGateState {
    observation: Mutex<GateObservation>,
    changed: Condvar,
}

fn registry() -> &'static GateRegistry {
    static REGISTRY: OnceLock<GateRegistry> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

fn work_key(work: &PreparedActionWork) -> Option<GateKey> {
    Some(match work {
        PreparedActionWork::Ordinary(work) => (
            Arc::as_ptr(work.origin().ownership()) as usize,
            work.sequence(),
        ),
        PreparedActionWork::Pitch(work) => (
            Arc::as_ptr(work.action.preparation_epoch()) as usize,
            work.action.sequence(),
        ),
    })
}

fn registered(work: &PreparedActionWork) -> Option<Arc<WorkerGateState>> {
    registry()
        .lock()
        .unwrap()
        .get(&work_key(work)?)
        .and_then(Weak::upgrade)
}

/// Test-only worker seam. The registry lock is released before this one-owner wait.
pub(crate) fn before_worker(work: &PreparedActionWork) {
    let Some(gate) = registered(work) else {
        return;
    };
    let mut observation = gate.observation.lock().unwrap();
    observation.entered = true;
    gate.changed.notify_all();
    while !observation.released {
        let waited = gate.changed.wait_timeout(observation, GATE_WAIT).unwrap();
        observation = waited.0;
        assert!(
            !waited.1.timed_out(),
            "owned action worker gate was not released"
        );
    }
}

pub(crate) fn after_worker(work: &PreparedActionWork, reason: Option<PitchActionReject>) {
    let Some(gate) = registered(work) else {
        return;
    };
    let mut observation = gate.observation.lock().unwrap();
    observation.finished = true;
    observation.reason = reason;
    observation.prepared_native = match work {
        PreparedActionWork::Pitch(work) if work.is_ready() => work
            .candidate
            .as_ref()
            .and_then(|candidate| candidate.rubberband.as_ref())
            .map(|native| native.state_address()),
        _ => None,
    };
    gate.changed.notify_all();
    while observation.return_held {
        let waited = gate.changed.wait_timeout(observation, GATE_WAIT).unwrap();
        observation = waited.0;
        assert!(
            !waited.1.timed_out(),
            "owned prepared worker return was not released"
        );
    }
}

pub(in crate::audio_engine) struct WorkerGate {
    key: GateKey,
    state: Arc<WorkerGateState>,
}

impl WorkerGate {
    pub(in crate::audio_engine) fn for_ordinary(start: &CapturedPadStart) -> Self {
        Self::with_key((Arc::as_ptr(start.ownership()) as usize, start.sequence()))
    }
    pub(super) fn new(fixture: &Fixture, start: &CapturedPadStart) -> Self {
        Self::with_key((
            Arc::as_ptr(&fixture.control.engine.input_runtime_ownership) as usize,
            start.sequence(),
        ))
    }

    pub(in crate::audio_engine) fn for_pitch(action: &CapturedPitchAction) -> Self {
        Self::with_key((
            Arc::as_ptr(action.preparation_epoch()) as usize,
            action.sequence(),
        ))
    }

    /// The real worker completes Native preparation before its return is held.
    /// This gate is armed before publication and never runs on the callback.
    pub(in crate::audio_engine) fn for_pitch_return(action: &CapturedPitchAction) -> Self {
        let gate = Self::for_pitch(action);
        {
            let mut observation = gate.state.observation.lock().unwrap();
            observation.released = true;
            observation.return_held = true;
        }
        gate
    }

    pub(in crate::audio_engine) fn prepared_native(&self) -> usize {
        let observation = self.state.observation.lock().unwrap();
        assert!(observation.finished && observation.reason.is_none());
        observation
            .prepared_native
            .filter(|address| *address != 0)
            .expect("genuine prepared Native owner")
    }

    pub(in crate::audio_engine) fn release_pitch_into(
        &self,
        fixture: &mut Fixture,
        action: &CapturedPitchAction,
    ) {
        let native = self.prepared_native();
        self.release();
        fixture.wait_worker(&[action]);
        assert_eq!(
            fixture
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .ready_pitch_native_address(action.sequence()),
            Some(native)
        );
    }

    fn with_key(key: GateKey) -> Self {
        let state = Arc::new(WorkerGateState::default());
        assert!(
            registry()
                .lock()
                .unwrap()
                .insert(key, Arc::downgrade(&state))
                .is_none()
        );
        Self { key, state }
    }

    pub(in crate::audio_engine) fn wait_for(&self, finished: bool) -> Option<PitchActionReject> {
        let mut observation = self.state.observation.lock().unwrap();
        while if finished {
            !observation.finished
        } else {
            !observation.entered
        } {
            let waited = self
                .state
                .changed
                .wait_timeout(observation, GATE_WAIT)
                .unwrap();
            observation = waited.0;
            assert!(
                !waited.1.timed_out(),
                "owned action worker did not reach its gate"
            );
        }
        observation.reason
    }

    pub(in crate::audio_engine) fn release(&self) {
        let mut observation = self.state.observation.lock().unwrap();
        observation.released = true;
        observation.return_held = false;
        self.state.changed.notify_all();
    }

    fn arm_next_attempt(&self) {
        let mut observation = self.state.observation.lock().unwrap();
        assert!(observation.finished);
        *observation = GateObservation::default();
    }
}

impl Drop for WorkerGate {
    fn drop(&mut self) {
        self.release();
        registry().lock().unwrap().remove(&self.key);
    }
}

fn plan_marker_registry() -> &'static GateRegistry {
    static REGISTRY: OnceLock<GateRegistry> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

/// One scoped test transaction enters the actual publication marker only after
/// the callback reaches this particular member's late-plan boundary.
pub(crate) fn before_group_late_plan(work: &OrdinaryStartWork) {
    let key = (
        Arc::as_ptr(work.origin().ownership()) as usize,
        work.sequence(),
    );
    let gate = plan_marker_registry()
        .lock()
        .unwrap()
        .get(&key)
        .and_then(Weak::upgrade);
    let Some(gate) = gate else { return };
    let mut observation = gate.observation.lock().unwrap();
    observation.entered = true;
    gate.changed.notify_all();
    while !observation.finished {
        let waited = gate.changed.wait_timeout(observation, GATE_WAIT).unwrap();
        observation = waited.0;
        assert!(!waited.1.timed_out(), "own late-plan marker did not enter");
    }
}

struct GroupPlanMarker {
    key: GateKey,
    state: Arc<WorkerGateState>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl GroupPlanMarker {
    fn new(fixture: &Fixture, member: &CapturedPadStart) -> Self {
        let ownership = fixture.control.engine.input_runtime_ownership.clone();
        let unrelated = ownership.slot_snapshot(2).unwrap();
        let write = ownership
            .prepare_slot_write_fixed(unrelated, unrelated.lifetime())
            .unwrap();
        let key = (Arc::as_ptr(&ownership) as usize, member.sequence());
        let state = Arc::new(WorkerGateState::default());
        assert!(
            plan_marker_registry()
                .lock()
                .unwrap()
                .insert(key, Arc::downgrade(&state))
                .is_none()
        );
        let marker_state = state.clone();
        let thread = std::thread::spawn(move || {
            let mut observation = marker_state.observation.lock().unwrap();
            while !observation.entered && !observation.released {
                let waited = marker_state
                    .changed
                    .wait_timeout(observation, GATE_WAIT)
                    .unwrap();
                observation = waited.0;
                assert!(!waited.1.timed_out(), "own group never reached late plan");
            }
            if observation.released {
                return;
            }
            drop(observation);
            ownership
                .commit_slot_write_fixed(
                    &write,
                    || {
                        let mut observation = marker_state.observation.lock().unwrap();
                        assert!(ownership.placement_pending());
                        observation.finished = true;
                        marker_state.changed.notify_all();
                        while !observation.released {
                            let waited = marker_state
                                .changed
                                .wait_timeout(observation, GATE_WAIT)
                                .unwrap();
                            observation = waited.0;
                            assert!(
                                !waited.1.timed_out(),
                                "own group late-plan marker was not released"
                            );
                        }
                        true
                    },
                    |_| {},
                )
                .unwrap();
        });
        Self {
            key,
            state,
            thread: Some(thread),
        }
    }

    fn finish(&mut self) {
        self.state.observation.lock().unwrap().released = true;
        self.state.changed.notify_all();
        plan_marker_registry().lock().unwrap().remove(&self.key);
        self.thread.take().unwrap().join().unwrap();
    }
}

impl Drop for GroupPlanMarker {
    fn drop(&mut self) {
        self.state.observation.lock().unwrap().released = true;
        self.state.changed.notify_all();
        plan_marker_registry().lock().unwrap().remove(&self.key);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn configured(policy: &str) -> Fixture {
    let mut fixture = Fixture::new(true);
    fixture.mixer.set_speed(1.43);
    fixture.mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    fixture.mixer.set_pad_gain(0, -4.5);
    fixture.drain();
    assert!(fixture.render(1).iter().all(|sample| *sample == 0.0));
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior(policy)
            .unwrap()
    );
    assert!(
        fixture
            .render_to(CAPTURE_FRAME)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    fixture
}

fn publish_at_grid(fixture: &mut Fixture, start: &CapturedPadStart, step: u16, target: u64) {
    drain_with_mode(fixture, TriggerQuantization::Grid { step_64ths: step });
    assert_eq!(start.target_frame(), Some(target));
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(start, None)
            .unwrap()
    );
    drain_with_mode(fixture, TriggerQuantization::Grid { step_64ths: step });
    assert!(start.accepted(), "{:?}", start.reason());
    assert_eq!(start.execution_frame(), Some(target));
}

fn render_partitioned(fixture: &mut Fixture, frames: usize) -> Vec<f32> {
    let mut output = Vec::with_capacity(frames);
    let mut index = 0;
    while output.len() < frames {
        let count = [127, 1, 509, 31, 256][index % 5].min(frames - output.len());
        output.extend(fixture.render(count));
        index += 1;
    }
    output
}

fn assert_full_birth_phase(fixture: &Fixture, execution: u64, emitted_frames: usize) {
    assert_eq!(
        fixture.transport.output_frame(),
        execution + emitted_frames as u64
    );
    let position = fixture.voice(0).source_playback.position();
    let expected = LOOP_START as f64
        + (emitted_frames as f64 * 1.43).rem_euclid((LOOP_END - LOOP_START) as f64);
    let observed = position.frame as f64 + position.fraction;
    assert!(
        (observed - expected).abs() < 1.0e-6,
        "fresh source phase must count emitted frames from actual E: observed={observed} expected={expected} E={execution}"
    );
    assert_eq!(fixture.voice(0).source_playback.tempo_ratio(), 1.43);
    assert_eq!(
        position.seek_mode,
        crate::audio_engine::source_reader::ExplicitSeekMode::Normal
    );
}

fn wait_burst_ready(fixture: &mut Fixture, originals: &[CapturedPadStart]) {
    let frame = fixture.transport.output_frame();
    // This host budget covers the genuine sequential Debug jobs. Product lead,
    // original-T delay and attempt bounds remain unchanged.
    let deadline = Instant::now() + WORKER_WAIT * originals.len() as u32;
    loop {
        fixture.mixer.pitch_runtime.as_mut().unwrap().poll();
        for original in originals {
            assert_ne!(
                original.state(),
                "rejected",
                "burst original rejected: sequence={} T={:?} E={:?} reason={:?}",
                original.sequence(),
                original.original_frame(),
                original.execution_frame(),
                original.reason()
            );
        }
        if originals.iter().all(|original| {
            fixture
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .has_ready_action(original.sequence())
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "burst host preparation exceeded {} own jobs: {:?}",
            originals.len(),
            originals
                .iter()
                .map(|original| (
                    original.sequence(),
                    original.state(),
                    original.reason(),
                    original.execution_frame()
                ))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), frame);
}

fn with_unrelated_marker(fixture: &mut Fixture, operation: impl FnOnce(&mut Fixture)) {
    let ownership = fixture.control.engine.input_runtime_ownership.clone();
    let unrelated = ownership.slot_snapshot(1).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(unrelated, unrelated.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(
            &write,
            || {
                assert!(ownership.placement_pending());
                operation(fixture);
                assert!(ownership.placement_pending());
                true
            },
            |_| {},
        )
        .unwrap();
}

fn late_ordinary_burst(policy: &str, pending_marker: bool) {
    let mut actual = configured(policy);
    let mut background = configured(policy);
    let mut reference = configured(policy);
    let first = capture(&actual, 1.0);
    let background_first = capture(&background, 1.0);
    publish_at_grid(&mut actual, &first, 8, FIRST_TARGET);
    publish_at_grid(&mut background, &background_first, 8, FIRST_TARGET);
    wait_ready(&mut actual, &[first.sequence()]);
    wait_ready(&mut background, &[background_first.sequence()]);
    let second = capture(&actual, 1.0);
    let expected = capture(&reference, 1.0);
    let gate = WorkerGate::new(&actual, &second);
    publish_at_grid(&mut actual, &second, 16, SECOND_TARGET);
    publish_at_grid(&mut reference, &expected, 16, SECOND_TARGET);
    gate.wait_for(false);
    wait_ready(&mut reference, &[expected.sequence()]);
    let identity = (
        second.content_key(),
        second.sequence(),
        second.received_timestamp(),
        second.target_frame(),
        second.source_control_revision(),
    );
    assert!(first.sequence() < second.sequence());
    let after_missed_target = SECOND_TARGET + DELAY_AFTER_TARGET;
    assert_eq!(
        actual.render_to(after_missed_target),
        background.render_to(after_missed_target)
    );
    assert_eq!(first.state(), "applied", "{:?}", first.reason());
    assert_eq!(actual.starts(0), 1);
    assert_eq!(second.state(), "scheduled", "{:?}", second.reason());
    let first_native = actual.voice(0).stretch.native_state_address();
    let first_generation = actual.voice(0).generation;
    assert_ne!(first_native, 0);
    gate.release();
    assert_eq!(
        gate.wait_for(true),
        None,
        "an intact own source must prepare off realtime"
    );
    wait_ready(&mut actual, &[second.sequence()]);
    if pending_marker {
        let old_execution = second.execution_frame();
        with_unrelated_marker(&mut actual, |fixture| {
            assert_eq!(fixture.render(1), background.render(1));
            assert_ne!(
                second.state(),
                "rejected",
                "a transient marker during own late replan must retain the ordinary original; reason={:?}",
                second.reason()
            );
            assert_eq!(second.execution_frame(), old_execution);
            assert_eq!(fixture.starts(0), 1);
            assert_eq!(second.effective_frame(), None);
        });
    }

    // A returned old-E result must initiate only bounded off-thread continuation/reprepare.
    // The first callback after readiness must not turn ordinary worker delay into mismatch.
    let before_effect = actual.render(1);
    assert_ne!(
        second.state(),
        "applied",
        "a stale endpoint must not be blindly adopted on the callback"
    );
    assert_eq!(before_effect, background.render(1));
    assert_ne!(
        second.state(),
        "rejected",
        "a late intact ordinary original must remain owned; reason={:?}",
        second.reason()
    );
    let deadline = Instant::now() + GATE_WAIT;
    while second.state() != "applied" {
        actual.mixer.pitch_runtime.as_mut().unwrap().poll();
        let frame = actual.transport.output_frame();
        let execution = second
            .execution_frame()
            .expect("the original retains its own execution plan");
        let ready = actual
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_ready_action(second.sequence());
        if ready && execution >= frame {
            assert_eq!(actual.render_to(execution), background.render_to(execution));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "late ordinary preparation did not return with its own future endpoint"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let execution = second.execution_frame().unwrap();
    assert!(execution > SECOND_TARGET);
    assert_eq!(actual.transport.output_frame(), execution);
    let mut actual_pcm = actual.render(1);
    actual_pcm.extend(render_partitioned(&mut actual, EMITTED_FRAMES - 1));
    assert_eq!(second.state(), "applied", "{:?}", second.reason());
    assert_eq!(second.effective_frame(), Some(execution));
    assert_eq!(actual.starts(0), 2);
    assert_eq!(actual.voice(0).generation, first_generation + 1);
    assert_ne!(actual.voice(0).stretch.native_state_address(), first_native);
    assert_eq!(
        actual.voice(0).stretch.adopted_request_id(),
        Some(second.sequence())
    );
    assert_eq!(
        (
            second.content_key(),
            second.sequence(),
            second.received_timestamp(),
            second.target_frame(),
            second.source_control_revision()
        ),
        identity
    );
    assert!(
        reference
            .render_to(SECOND_TARGET)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let expected_pcm = render_partitioned(&mut reference, EMITTED_FRAMES);
    assert_eq!(expected.state(), "applied", "{:?}", expected.reason());
    assert_eq!(
        actual_pcm, expected_pcm,
        "every emitted first/continuing Native/EQ sample must equal the independent complete source beginning"
    );
    assert!(actual_pcm.iter().any(|sample| sample.abs() > 0.001));
    assert_full_birth_phase(&actual, execution, EMITTED_FRAMES);
    assert_full_birth_phase(&reference, SECOND_TARGET, EMITTED_FRAMES);
    assert!(
        actual
            .voice(0)
            .source_playback
            .matches_exact(&reference.voice(0).source_playback)
    );
    assert_eq!(
        actual.voice(0).stretch.pending_fifo_frames(),
        reference.voice(0).stretch.pending_fifo_frames()
    );
}

#[test]
fn ordinary_same_content_worker_delay_preserves_both_originals_first_sound() {
    late_ordinary_burst("first_sound_at_grid", false);
}

#[test]
fn ordinary_same_content_worker_delay_preserves_both_originals_attack() {
    late_ordinary_burst("attack_at_grid", false);
}

#[test]
fn ordinary_late_replan_retains_its_original_during_transient_placement_marker() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        late_ordinary_burst(policy, true);
    }
}

fn late_wet_piano_worker(policy: &str, pending_marker: bool) {
    let mut actual = configured(policy);
    let mut reference = configured(policy);
    for fixture in [&mut actual, &mut reference] {
        fixture.rows(PitchPadProjection {
            source_root: Some(0),
            source_mode: Some(PitchKeyMode::Major),
            key_lock: true,
            quantize_step_64ths: Some(16),
            ..PitchPadProjection::default()
        });
        assert!(
            fixture
                .render_to(SECOND_TARGET - 2_048)
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }
    // Each original is a public Piano note gesture. The reference owns a
    // separate source, permit, Native and EQ chain and really starts at T.
    let original = actual.capture(0, "pitch.note:7:set_retrigger");
    let expected = reference.capture(0, "pitch.note:7:set_retrigger");
    let identity = (
        original.content_key(),
        original.sequence(),
        original.received_timestamp(),
        original.original_due_frame(),
        original.render(),
    );
    assert_eq!(original.original_due_frame(), SECOND_TARGET);
    assert_eq!(expected.original_due_frame(), SECOND_TARGET);
    assert_eq!(original.render().pitch, PitchIntent::new(7, 0).unwrap());
    assert!(original.render().key_lock);
    let gate = WorkerGate::for_pitch(&original);
    actual.publish(&original);
    reference.publish(&expected);
    assert_eq!(original.execution_target(), SECOND_TARGET);
    assert_eq!(expected.execution_target(), SECOND_TARGET);
    gate.wait_for(false);
    wait_ready(&mut reference, &[expected.sequence()]);
    assert!(
        actual
            .render_to(SECOND_TARGET + DELAY_AFTER_TARGET)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!(actual.starts(0), 0);
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    wait_ready(&mut actual, &[original.sequence()]);
    if pending_marker {
        let old_execution = original.execution_target();
        with_unrelated_marker(&mut actual, |fixture| {
            assert_eq!(fixture.render(1), vec![0.0]);
            assert_ne!(
                original.state(),
                "rejected",
                "a transient marker during own late replan must retain the wet Piano original; reason={:?}",
                original.reason()
            );
            assert_eq!(original.execution_target(), old_execution);
            assert_eq!(fixture.starts(0), 0);
            assert!(!original.effective());
        });
    }
    let native = actual
        .mixer
        .pitch_runtime
        .as_ref()
        .unwrap()
        .ready_pitch_native_address(original.sequence())
        .expect("the delayed Piano original owns its genuine prepared Native");
    assert_ne!(native, 0);
    let constructions = actual.control.engine.pitch_work_capacity.allocations().1;
    gate.arm_next_attempt();
    assert_eq!(actual.render(1), vec![0.0]);
    assert_ne!(original.state(), "rejected", "{:?}", original.reason());
    assert!(!original.effective());
    let execution = original.execution_target();
    assert!(execution > actual.transport.output_frame());
    gate.wait_for(false);
    assert_eq!(actual.starts(0), 0);
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    wait_ready(&mut actual, &[original.sequence()]);
    assert_eq!(
        actual
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .ready_pitch_native_address(original.sequence()),
        Some(native),
        "own reprepare must retain its original Native handle"
    );
    assert_eq!(
        actual.control.engine.pitch_work_capacity.allocations().1,
        constructions,
        "the same owned Piano candidate is reused rather than reconstructed"
    );
    assert!(
        actual
            .render_to(execution)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let mut actual_pcm = actual.render(1);
    actual_pcm.extend(render_partitioned(&mut actual, EMITTED_FRAMES - 1));
    assert!(original.effective(), "{:?}", original.reason());
    assert_eq!(original.execution_target(), execution);
    assert_eq!(actual.starts(0), 1);
    assert_eq!(actual.voice(0).stretch.native_state_address(), native);
    assert_eq!(
        actual.voice(0).stretch.adopted_request_id(),
        Some(original.sequence())
    );
    assert!(
        actual
            .voice(0)
            .stretch
            .owns_pitch_contract(original.render().pitch, true)
    );
    assert!(
        reference
            .render_to(SECOND_TARGET)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let expected_pcm = render_partitioned(&mut reference, EMITTED_FRAMES);
    assert!(expected.effective(), "{:?}", expected.reason());
    assert_eq!(
        actual_pcm, expected_pcm,
        "the replanned wet Piano Native/FIFO/EQ beginning must equal its independent complete launch under {policy}"
    );
    assert!(actual_pcm.iter().any(|sample| sample.abs() > 0.001));
    assert_full_birth_phase(&actual, execution, EMITTED_FRAMES);
    assert_full_birth_phase(&reference, SECOND_TARGET, EMITTED_FRAMES);
    assert!(
        actual
            .voice(0)
            .source_playback
            .matches_exact(&reference.voice(0).source_playback)
    );
    assert_eq!(actual.voice(0).source_playback.tempo_ratio(), 1.43);
    assert_eq!(
        actual.voice(0).stretch.pending_fifo_frames(),
        reference.voice(0).stretch.pending_fifo_frames()
    );
    assert_eq!(
        actual.voice(0).stretch.native_primed_output_frames(),
        reference.voice(0).stretch.native_primed_output_frames(),
        "fresh preparation retains Native Hbase without unheard E-T output"
    );
    assert_eq!(
        (
            original.content_key(),
            original.sequence(),
            original.received_timestamp(),
            original.original_due_frame(),
            original.render()
        ),
        identity
    );
}

#[test]
fn wet_piano_own_worker_misses_e_and_reuses_native_first_sound() {
    late_wet_piano_worker("first_sound_at_grid", false);
}

#[test]
fn wet_piano_own_worker_misses_e_and_reuses_native_attack() {
    late_wet_piano_worker("attack_at_grid", false);
}

#[test]
fn wet_piano_late_replan_retains_its_original_during_transient_placement_marker() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        late_wet_piano_worker(policy, true);
    }
}

#[test]
fn immutable_ordinary_worker_does_not_require_unrelated_publication_marker() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        let mut fixture = configured(policy);
        let original = capture(&fixture, 1.0);
        let gate = WorkerGate::new(&fixture, &original);
        publish_at_grid(&mut fixture, &original, 16, SECOND_TARGET);
        gate.wait_for(false);
        let ownership = fixture.control.engine.input_runtime_ownership.clone();
        let unrelated = ownership.slot_snapshot(1).unwrap();
        let write = ownership
            .prepare_slot_write_fixed(unrelated, unrelated.lifetime())
            .unwrap();
        let mut observed_reason = None;
        ownership
            .commit_slot_write_fixed(
                &write,
                || {
                    assert!(ownership.placement_pending());
                    gate.release();
                    observed_reason = gate.wait_for(true);
                    assert!(ownership.placement_pending());
                    true
                },
                |_| {},
            )
            .unwrap();
        assert_eq!(
            observed_reason, None,
            "immutable original worker preparation must survive an unrelated scalar marker under {policy}"
        );
        wait_ready(&mut fixture, &[original.sequence()]);
        fixture.render_to(SECOND_TARGET);
        assert!(fixture.render(1).iter().all(|sample| sample.is_finite()));
        assert_eq!(original.state(), "applied", "{:?}", original.reason());
        assert_eq!(original.original_frame(), Some(SECOND_TARGET));
        assert_eq!(original.execution_frame(), Some(SECOND_TARGET));
        assert_eq!(fixture.starts(0), 1);
    }
}

fn blocked_second_attempt(policy: &str) -> (Fixture, Fixture, CapturedPadStart, WorkerGate) {
    let mut actual = configured(policy);
    let mut background = configured(policy);
    let first = capture(&actual, 1.0);
    let background_first = capture(&background, 1.0);
    publish_at_grid(&mut actual, &first, 8, FIRST_TARGET);
    publish_at_grid(&mut background, &background_first, 8, FIRST_TARGET);
    wait_ready(&mut actual, &[first.sequence()]);
    wait_ready(&mut background, &[background_first.sequence()]);
    let second = capture(&actual, 1.0);
    let gate = WorkerGate::new(&actual, &second);
    publish_at_grid(&mut actual, &second, 16, SECOND_TARGET);
    gate.wait_for(false);
    let late = SECOND_TARGET + DELAY_AFTER_TARGET;
    assert_eq!(actual.render_to(late), background.render_to(late));
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    wait_ready(&mut actual, &[second.sequence()]);
    gate.arm_next_attempt();
    assert_eq!(actual.render(1), background.render(1));
    assert_ne!(second.state(), "rejected", "{:?}", second.reason());
    assert!(second.execution_frame().unwrap() > actual.transport.output_frame());
    gate.wait_for(false);
    assert_eq!(actual.starts(0), 1);
    (actual, background, second, gate)
}

#[test]
fn source_control_aba_during_the_new_own_attempt_refuses_without_second_birth() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        let (mut actual, mut background, second, gate) = blocked_second_attempt(policy);
        let identity = (
            second.sequence(),
            second.target_frame(),
            second.received_timestamp(),
        );
        let generation = actual.voice(0).generation;
        actual.mixer.set_speed(1.37);
        actual.mixer.set_speed(1.43);
        gate.release();
        gate.wait_for(true);
        let execution = second.execution_frame().unwrap();
        assert_eq!(actual.render_to(execution), background.render_to(execution));
        assert_eq!(actual.render(1), background.render(1));
        assert_eq!(second.state(), "rejected", "{:?}", second.reason());
        assert_ne!(second.reason(), Some("prepared_mismatch"));
        assert_eq!(
            (
                second.sequence(),
                second.target_frame(),
                second.received_timestamp()
            ),
            identity
        );
        assert_eq!(actual.starts(0), 1);
        assert_eq!(actual.voice(0).generation, generation);
        assert_ne!(
            actual.voice(0).stretch.adopted_request_id(),
            Some(second.sequence())
        );
    }
}

#[test]
fn original_t_catchup_bound_cancels_a_waiting_new_attempt_without_changing_old_audio() {
    let (mut actual, mut background, second, gate) = blocked_second_attempt("first_sound_at_grid");
    let generation = actual.voice(0).generation;
    let bound = SECOND_TARGET
        + crate::audio_engine::source_feed_history::SOURCE_FEED_HISTORY_MAX_FRAMES as u64
        + 1;
    assert_eq!(actual.render_to(bound), background.render_to(bound));
    assert_eq!(actual.render(1), background.render(1));
    assert_eq!(second.state(), "rejected");
    assert_eq!(second.reason(), Some("preparation_failed"));
    assert_eq!(second.original_frame(), Some(SECOND_TARGET));
    assert_eq!(actual.starts(0), 1);
    assert_eq!(actual.voice(0).generation, generation);
    gate.release();
    gate.wait_for(true);
    assert_eq!(actual.render(1), background.render(1));
    assert_eq!(actual.starts(0), 1);
    assert_ne!(
        actual.voice(0).stretch.adopted_request_id(),
        Some(second.sequence())
    );
}

fn dry_configured() -> Fixture {
    let mut fixture = Fixture::new(false);
    fixture.mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    fixture.mixer.set_pad_gain(0, -4.5);
    fixture.drain();
    assert!(
        fixture
            .render_to(CAPTURE_FRAME)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    fixture
}

fn older_native_piano_birth(fixture: &mut Fixture) -> (CapturedPitchAction, WorkerGate) {
    // A genuine nonneutral Piano owner needs its own Native worker. A younger
    // unity birth must retain its distinct source request and original ordering.
    let first = fixture.capture(0, "pitch.extra:7:set_retrigger");
    assert_eq!(first.original_due_frame(), CAPTURE_FRAME);
    let gate = WorkerGate::for_pitch(&first);
    fixture.publish(&first);
    assert_eq!(first.execution_target(), CAPTURE_FRAME + 8192);
    gate.wait_for(false);
    assert_eq!(fixture.starts(0), 0);
    (first, gate)
}

fn older_native_ordinary_birth(fixture: &mut Fixture) -> (CapturedPadStart, WorkerGate) {
    // This real nonneutral ordinary owner requires Native preparation while
    // stopped. A later scalar SET must not borrow its birth ordering or tuple.
    fixture
        .mixer
        .set_pad_pitch_intent(0, PitchIntent::new(0, 3).unwrap())
        .unwrap();
    let first = capture(fixture, 1.0);
    fixture.drain();
    assert_eq!(first.target_frame(), Some(CAPTURE_FRAME));
    let gate = WorkerGate::new(fixture, &first);
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&first, None)
            .unwrap()
    );
    fixture.drain();
    assert_eq!(first.execution_frame(), Some(CAPTURE_FRAME + 8192));
    gate.wait_for(false);
    assert_eq!(fixture.starts(0), 0);
    (first, gate)
}

#[test]
fn terminal_worker_owned_predecessors_do_not_delay_an_independent_unity_piano_birth() {
    use crate::audio_engine::audio_stream::render_scheduled_audio;
    use crate::audio_engine::constants::NUM_SAMPLES;
    use crate::audio_engine::ordinary_endpoint_reserve_tests::{
        adopt_current_sources, refresh_actual_runtime_bindings,
    };
    use crate::test_memory::{ThreadAllocations, observe_current_thread};
    use rtrb::RingBuffer;

    for piano in [false, true] {
        let mut actual = dry_configured();
        let mut reference = dry_configured();
        for fixture in [&mut actual, &mut reference] {
            // The production ring publishes only source-owned feedback, unlike
            // the diagnostic Vec sink used by the older reference fixtures.
            adopt_current_sources(fixture, false);
            refresh_actual_runtime_bindings(fixture);
        }
        let (sequence, gate) = if piano {
            let (first, gate) = older_native_piano_birth(&mut actual);
            assert!(first.reject(PitchActionReject::LaunchCancelled));
            (first.sequence(), gate)
        } else {
            let (first, gate) = older_native_ordinary_birth(&mut actual);
            assert!(first.cancel());
            (first.sequence(), gate)
        };
        let second = actual.capture(0, "pitch.extra:0:set_retrigger");
        let expected = reference.capture(0, "pitch.extra:0:set_retrigger");
        actual.publish(&second);
        reference.publish(&expected);
        let (mut feedback, mut messages) = RingBuffer::new(64);
        let ((), allocations) = observe_current_thread(|| {
            render_scheduled_audio(
                &mut actual.mixer,
                &mut actual.scheduler,
                &mut [],
                &mut [0.0; NUM_SAMPLES],
                CAPTURE_FRAME,
                1,
                &mut actual.transport,
                &mut feedback,
                &mut actual.retirement,
            )
        });
        assert_eq!(allocations, ThreadAllocations::default());
        while let Ok(message) = messages.pop() {
            actual.feedback.push(message);
        }
        assert!(
            second.effective(),
            "piano predecessor={piano}, {:?}",
            second.reason()
        );
        assert_eq!(second.execution_target(), CAPTURE_FRAME);
        assert_eq!(actual.starts(0), 1);
        assert_eq!(actual.scheduler.len(), 1);
        let runtime = actual.mixer.pitch_runtime.as_ref().unwrap();
        assert_eq!(
            runtime.unused_action_capacity().0,
            63,
            "worker slot remains reserved"
        );
        assert!(
            !runtime.has_ready_action(sequence),
            "worker still owns predecessor"
        );
        // Release only after the younger effect. The genuine rejected Work must
        // return and reach its old queued cleanup without changing current audio.
        gate.release();
        assert_eq!(gate.wait_for(true), Some(PitchActionReject::StaleContent));
        let actual_pcm = render_partitioned(&mut actual, EMITTED_FRAMES);
        let expected_pcm = render_partitioned(&mut reference, EMITTED_FRAMES);
        assert_eq!(
            actual_pcm, expected_pcm,
            "full dry/EQ beginning and old cleanup"
        );
        assert_eq!(actual.starts(0), 1);
        assert_eq!(
            actual
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .unused_action_capacity()
                .0,
            64
        );
    }
}

#[test]
fn ontime_unity_piano_birth_retains_its_own_dry_request_behind_older_native_birth() {
    let mut actual = dry_configured();
    let mut reference = dry_configured();
    let (first, gate) = older_native_piano_birth(&mut actual);
    let original = actual.transport.output_frame();
    assert!(
        reference
            .render_to(original)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let second = actual.capture(0, "pitch.extra:0:set_retrigger");
    let expected = reference.capture(0, "pitch.extra:0:set_retrigger");
    assert_eq!(second.original_due_frame(), original);
    assert_eq!(expected.original_due_frame(), original);
    actual.publish(&second);
    reference.publish(&expected);
    assert_eq!(second.execution_target(), original);
    let identity = (
        second.content_key(),
        second.sequence(),
        second.received_timestamp(),
        second.original_due_frame(),
    );
    // The real due callback must first observe the older unfinished worker.
    assert_eq!(actual.render(1), vec![0.0]);
    assert!(!first.effective() && !second.effective());
    assert!(second.execution_target() > original);
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    wait_ready(&mut actual, &[first.sequence()]);
    let deadline = Instant::now() + GATE_WAIT;
    let mut actual_pcm = Vec::new();
    while !second.effective() {
        actual.mixer.pitch_runtime.as_mut().unwrap().poll();
        assert_ne!(
            second.state(),
            "rejected",
            "unity original behind an older Native birth must retain its own dry request; reason={:?}",
            second.reason()
        );
        let frame = actual.transport.output_frame();
        let ready = actual
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_ready_action(second.sequence());
        if ready && second.execution_target() >= frame {
            actual.render_to(second.execution_target());
            let emitted = actual.render(1);
            if second.effective() {
                actual_pcm.extend(emitted);
            }
        } else if ready {
            let emitted = actual.render(1);
            if second.effective() {
                actual_pcm.extend(emitted);
            }
        } else {
            assert!(
                Instant::now() < deadline,
                "unity original did not return from its own ordered preparation"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert_eq!(first.state(), "applied", "{:?}", first.reason());
    assert_eq!(actual.starts(0), 2);
    assert!(second.execution_target() > original);
    assert_eq!(actual_pcm.len(), 1, "retain the first actual unity sample");
    actual_pcm.extend(render_partitioned(&mut actual, EMITTED_FRAMES - 1));
    let expected_pcm = render_partitioned(&mut reference, EMITTED_FRAMES);
    assert!(expected.effective(), "{:?}", expected.reason());
    assert_eq!(
        actual_pcm, expected_pcm,
        "the entire emitted unity beginning must equal its independent complete birth"
    );
    assert_eq!(
        actual.transport.output_frame(),
        second.execution_target() + EMITTED_FRAMES as u64
    );
    assert_eq!(
        actual.voice(0).source_playback.position().frame,
        LOOP_START + EMITTED_FRAMES
    );
    assert_eq!(
        (
            second.content_key(),
            second.sequence(),
            second.received_timestamp(),
            second.original_due_frame()
        ),
        identity
    );
    assert!(!actual.voice(0).stretch.loop_acceptance_state().0);
}

#[test]
fn stopped_scalar_set_keeps_its_original_effect_in_front_of_an_older_native_birth() {
    let mut fixture = dry_configured();
    let (first, gate) = older_native_ordinary_birth(&mut fixture);
    let original = fixture.transport.output_frame();
    let older_execution = first.execution_frame();
    let scalar = fixture.capture(0, "pitch.extra:7:set");
    assert!(first.sequence() < scalar.sequence());
    assert_eq!(scalar.original_due_frame(), original);
    fixture.publish(&scalar);
    assert_eq!(scalar.execution_target(), original);
    assert_eq!(fixture.render(1), vec![0.0]);
    assert!(
        scalar.effective(),
        "a stopped SET must keep its original scalar effect rather than enter fresh-birth ordering; reason={:?}",
        scalar.reason()
    );
    assert_eq!(scalar.execution_target(), original);
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(0, 7).unwrap())
    );
    assert_eq!(fixture.starts(0), 0);
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
    gate.release();
    gate.wait_for(true);
    fixture.render_to(first.execution_frame().unwrap());
    fixture.render(1);
    assert_eq!(
        first.state(),
        "rejected",
        "the earlier ordinary birth retains its genuine frozen-tuple guard"
    );
    assert_eq!(fixture.starts(0), 0);
    assert_eq!(first.target_frame(), Some(CAPTURE_FRAME));
    assert_eq!(first.execution_frame(), older_execution);
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(0, 7).unwrap())
    );
}

#[test]
fn eight_equal_t_originals_pipeline_their_own_late_preparation_before_ordered_effects() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        let mut actual = configured(policy);
        let mut reference = configured(policy);
        let mut originals = Vec::new();
        let mut gates = Vec::new();
        for _ in 0..8 {
            let original = capture(&actual, 1.0);
            gates.push(WorkerGate::new(&actual, &original));
            publish_at_grid(&mut actual, &original, 16, SECOND_TARGET);
            originals.push(original);
        }
        gates[0].wait_for(false);
        let expected = capture(&reference, 1.0);
        publish_at_grid(&mut reference, &expected, 16, SECOND_TARGET);
        wait_ready(&mut reference, &[expected.sequence()]);
        assert!(
            actual
                .render_to(SECOND_TARGET + DELAY_AFTER_TARGET)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        for gate in &gates {
            gate.release();
        }
        for gate in &gates {
            assert_eq!(gate.wait_for(true), None);
        }
        let sequences = originals
            .iter()
            .map(CapturedPadStart::sequence)
            .collect::<Vec<_>>();
        wait_burst_ready(&mut actual, &originals);
        assert_eq!(actual.render(1), vec![0.0]);
        let planned = originals.iter().map(|original| {
            assert_ne!(original.state(), "rejected", "{:?}", original.reason());
            let execution = original.execution_frame().unwrap();
            assert!(execution > SECOND_TARGET, "every returned original must already own its new prepared endpoint, including younger events");
            execution
        }).collect::<Vec<_>>();
        let first = *planned.iter().min().unwrap();
        let last = *planned.iter().max().unwrap();
        assert!(
            last - first <= 8192,
            "ordering must not add one preparation lead per accepted attack: {planned:?}"
        );
        wait_burst_ready(&mut actual, &originals);
        actual.render_to(last);
        let mut actual_pcm = actual.render(1);
        actual_pcm.extend(render_partitioned(&mut actual, EMITTED_FRAMES - 1));
        assert_eq!(actual.starts(0), originals.len());
        for original in &originals {
            assert_eq!(original.state(), "applied", "{:?}", original.reason());
            assert_eq!(original.original_frame(), Some(SECOND_TARGET));
            assert_eq!(original.effective_frame(), original.execution_frame());
        }
        assert_eq!(
            actual.voice(0).stretch.adopted_request_id(),
            Some(*sequences.last().unwrap())
        );
        assert!(
            reference
                .render_to(SECOND_TARGET)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        let expected_pcm = render_partitioned(&mut reference, EMITTED_FRAMES);
        assert_eq!(
            actual_pcm, expected_pcm,
            "the eighth actual Native/FIFO/EQ owner must retain the independent complete source beginning under {policy}"
        );
        assert_full_birth_phase(&actual, last, EMITTED_FRAMES);
        assert_full_birth_phase(&reference, SECOND_TARGET, EMITTED_FRAMES);
        assert_eq!(
            actual.voice(0).stretch.pending_fifo_frames(),
            reference.voice(0).stretch.pending_fifo_frames()
        );
    }
}

fn public_group_start(
    fixture: &Fixture,
) -> crate::audio_engine::global_playback_batch::GlobalPlaybackBatchTicket {
    Python::attach(|py| {
        let entries = [0, 1]
            .into_iter()
            .map(|id| {
                let binding = fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                (
                    Py::new(py, binding).unwrap(),
                    LOOP_START as f64 / f64::from(RATE),
                    Some(LOOP_END as f64 / f64::from(RATE)),
                )
            })
            .collect();
        fixture
            .control
            .engine
            .start_global_playback_batch(py, entries, None)
            .unwrap()
    })
}

fn wait_own_group(fixture: &mut Fixture, originals: &[CapturedPadStart]) {
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + WORKER_WAIT * originals.len() as u32;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        for original in originals {
            assert_ne!(original.state(), "rejected", "{:?}", original.reason());
        }
        if runtime.ordinary_group_ready(originals.len()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "own group preparation host bound"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), frame);
}

#[test]
fn global_late_plan_marker_after_first_member_preserves_all_original_plans_until_commit() {
    for policy in ["first_sound_at_grid", "attack_at_grid"] {
        let mut fixture = configured(policy);
        fixture.mixer.set_pad_eq(1, -12.0, -3.0, 4.0);
        fixture.mixer.set_pad_gain(1, -4.5);
        let ticket = public_group_start(&fixture);
        fixture.drain();
        let originals = fixture
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .ordinary_group_originals();
        assert_eq!(originals.len(), 2);
        wait_own_group(&mut fixture, &originals);
        let execution = originals[0].execution_frame().unwrap();
        assert_eq!(originals[1].execution_frame(), Some(execution));
        // Genuine callback refusal at E leaves the ready group for E+1, where
        // the existing late-plan loop can be observed between its two members.
        with_unrelated_marker(&mut fixture, |fixture| {
            assert!(
                fixture
                    .render_to(execution)
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
            assert_eq!(fixture.render(1), vec![0.0]);
            assert_eq!(ticket.publication_status(), "pending");
            assert_eq!((fixture.starts(0), fixture.starts(1)), (0, 0));
        });
        let before = originals
            .iter()
            .map(|original| {
                (
                    original.sequence(),
                    original.execution_frame(),
                    original.state(),
                    original.reason(),
                )
            })
            .collect::<Vec<_>>();
        let mut marker = GroupPlanMarker::new(&fixture, &originals[1]);
        assert_eq!(fixture.render(1), vec![0.0]);
        assert!(
            fixture
                .control
                .engine
                .input_runtime_ownership
                .placement_pending()
        );
        for (original, expected) in originals.iter().zip(&before) {
            assert_eq!(
                (
                    original.sequence(),
                    original.execution_frame(),
                    original.state(),
                    original.reason()
                ),
                *expected,
                "no member may commit a new E/status before all genuine plans validate under {policy}"
            );
        }
        assert_eq!(ticket.publication_status(), "pending");
        assert_eq!((fixture.starts(0), fixture.starts(1)), (0, 0));
        assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
        marker.finish();
        assert_eq!(fixture.render(1), vec![0.0]);
        let common = originals[0].execution_frame().unwrap();
        assert!(common > execution);
        assert_eq!(originals[1].execution_frame(), Some(common));
        wait_own_group(&mut fixture, &originals);
        assert!(
            fixture
                .render_to(common)
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert!(fixture.render(1).iter().all(|sample| sample.is_finite()));
        assert_eq!(ticket.publication_status(), "accepted");
        assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));
        for (id, original) in originals.iter().enumerate() {
            assert_eq!(original.state(), "applied", "{:?}", original.reason());
            assert_eq!(original.original_frame(), Some(CAPTURE_FRAME));
            assert_eq!(original.effective_frame(), Some(common));
            assert_eq!(
                fixture.voice(id).stretch.adopted_request_id(),
                Some(original.sequence())
            );
            assert_ne!(fixture.voice(id).stretch.native_state_address(), 0);
        }
        assert!(fixture.scheduler.is_empty());
    }
}
