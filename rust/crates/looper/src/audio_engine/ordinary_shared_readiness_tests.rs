//! Genuine shared physical window ACK with separate original Native attacks.
use super::super::*;
use crate::audio_engine::input_mapping::launch_admission_tests::RATE;
use crate::audio_engine::ordinary_start_tests::install_synthetic_complete_lease;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::pitch_runtime_tests::{Fixture, assert_tone};
use std::sync::{Condvar, mpsc};

const WAIT: Duration = Duration::from_secs(3);

struct ColdGate(Arc<(Mutex<bool>, Condvar)>);
impl ColdGate {
    fn new(fixture: &Fixture) -> Self {
        let gate = Self(Arc::new((Mutex::new(false), Condvar::new())));
        let (sender, receiver) = mpsc::channel();
        for _ in 0..crate::audio_engine::cold_jobs::WORKERS {
            let gate = gate.0.clone();
            let sender = sender.clone();
            fixture
                .control
                .engine
                .cold_jobs
                .submit(
                    fixture.control.engine.cold_jobs.reserve().unwrap(),
                    move || {
                        sender.send(()).unwrap();
                        let mut ready = gate.0.lock().unwrap();
                        while !*ready {
                            ready = gate.1.wait(ready).unwrap();
                        }
                    },
                )
                .unwrap();
        }
        for _ in 0..crate::audio_engine::cold_jobs::WORKERS {
            receiver.recv_timeout(WAIT).unwrap();
        }
        gate
    }
}
impl Drop for ColdGate {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

fn capture(fixture: &Fixture, volume: f32, from: f64) -> CapturedPadStart {
    fixture
        .control
        .engine
        .capture_pad_start(0, from, Some(2.75), volume, false, None, None)
        .unwrap()
}

fn fixture() -> (Fixture, tempfile::TempDir) {
    let mut fixture = Fixture::new(true);
    let files = install_synthetic_complete_lease(&mut fixture);
    fixture
        .mixer
        .set_pad_pitch_intent(0, PitchIntent::new(0, 7).unwrap())
        .unwrap();
    (fixture, files)
}

fn wait_ack(fixture: &mut Fixture, ticket: &ResidentWindowTicket) {
    let deadline = Instant::now() + WAIT;
    loop {
        fixture.drain();
        match ticket.publication_status() {
            "accepted" => break,
            "failed" | "cancelled" | "rejected" => {
                panic!("real window failed: {:?}", ticket.error())
            }
            _ => {}
        }
        assert!(Instant::now() < deadline, "genuine WindowACK timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait_work(fixture: &mut Fixture, starts: &[&CapturedPadStart]) {
    let deadline = Instant::now() + WAIT;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if starts
            .iter()
            .all(|start| runtime.has_ready_action(start.sequence()))
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "separate original Native Work timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn equal_original_gui_starts_share_one_real_window_but_keep_two_native_effects() {
    let (mut fixture, _files) = fixture();
    let gate = ColdGate::new(&fixture);
    let first = capture(&fixture, 0.25, 1.5);
    let second = capture(&fixture, 0.75, 1.5);
    fixture.drain();
    let a = fixture
        .control
        .engine
        .prepare_captured_pad_start(&first)
        .unwrap();
    let b = fixture
        .control
        .engine
        .prepare_captured_pad_start(&second)
        .unwrap();
    assert!(Arc::ptr_eq(&a.state, &b.state));
    assert!(!Arc::ptr_eq(
        a.member_cancelled.as_ref().unwrap(),
        b.member_cancelled.as_ref().unwrap()
    ));
    assert!(a.pad_start.as_ref().unwrap().same_owner(&first));
    assert!(b.pad_start.as_ref().unwrap().same_owner(&second));
    assert!(!a.pad_start.as_ref().unwrap().same_owner(&second));
    assert!(first.sequence() < second.sequence());
    assert_eq!(
        (first.original_frame(), second.original_frame()),
        (Some(0), Some(0))
    );
    assert_eq!(a.window_revision, b.window_revision);
    assert_ne!(a.publication_status(), "accepted");
    drop(gate);
    wait_ack(&mut fixture, &a);
    assert_eq!(b.publication_status(), "accepted");
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&first, Some(&a))
            .unwrap()
    );
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&second, Some(&b))
            .unwrap()
    );
    fixture.drain();
    assert_eq!(
        fixture.scheduler.len(),
        2,
        "sharing readiness cannot coalesce accepted attacks"
    );
    assert_eq!(
        (first.execution_frame(), second.execution_frame()),
        (Some(8192), Some(8192))
    );
    wait_work(&mut fixture, &[&first, &second]);
    assert!(fixture.render_to(8192).iter().all(|sample| *sample == 0.0));
    let mut pcm = fixture.render(1);
    pcm.extend(fixture.pcm(47_999));
    assert_eq!(fixture.starts(0), 2);
    assert_eq!((first.state(), second.state()), ("applied", "applied"));
    assert_eq!(
        (first.effective_frame(), second.effective_frame()),
        (Some(8192), Some(8192))
    );
    assert_eq!(
        fixture.voice(0).volume,
        0.75,
        "original input order retains the second effect last"
    );
    assert!(
        fixture
            .voice(0)
            .stretch
            .owns_pitch_contract(PitchIntent::new(0, 7).unwrap(), true)
    );
    assert_eq!(
        fixture
            .voice(0)
            .stretch
            .native_tap_observation()
            .unwrap()
            .missing_reads,
        0
    );
    assert_tone(&pcm, 0, 7, 1.0, true);
}

#[test]
fn fresh_member_after_real_runtime_projections_reuses_readiness_without_revoking_scheduled_sibling()
{
    use crate::audio_engine::constants::NUM_SAMPLES;
    use crate::audio_engine::pitch_runtime_tests::{LOOP_END, LOOP_START};

    let (mut fixture, _files) = fixture();
    let first = capture(&fixture, 0.25, 1.5);
    fixture.drain();
    let a = fixture
        .control
        .engine
        .prepare_captured_pad_start(&first)
        .unwrap();
    wait_ack(&mut fixture, &a);
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&first, Some(&a))
            .unwrap()
    );
    fixture.drain();
    wait_work(&mut fixture, &[&first]);
    assert_eq!(first.execution_frame(), Some(8192));
    assert!(first.accepted());
    let older_unpublished = capture(&fixture, 0.5, 1.5);
    let ownership = fixture.control.engine.input_runtime_ownership.clone();
    let before_runtime = ownership.runtime_revision(0);
    let before_intent = fixture.control.engine.resident_stem_cache.lock().unwrap()[0]
        .intent_epoch
        .load(Ordering::Acquire);
    let before_binding = fixture
        .control
        .engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap()
        .binding;

    // Use the production RuntimeState publication and genuine current bindings.
    // MultiLoop changes input routing only; actual bank/window/loop/timing stay fixed.
    for multi_loop in [false, true] {
        let mut loaded = vec![false; NUM_SAMPLES];
        let mut starts = vec![0.0; NUM_SAMPLES];
        let mut ends = vec![None; NUM_SAMPLES];
        let mut bindings = vec![None; NUM_SAMPLES];
        for id in [0, 1] {
            loaded[id] = true;
            (starts[id], ends[id]) = if id == 0 {
                (1.5, Some(2.75))
            } else {
                (
                    LOOP_START as f64 / f64::from(RATE),
                    Some(LOOP_END as f64 / f64::from(RATE)),
                )
            };
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
    assert!(ownership.runtime_revision(0) >= before_runtime + 2);
    let after_binding = fixture
        .control
        .engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap()
        .binding;
    assert!(before_binding.same_projection(&after_binding));
    assert!(a.publication.current());
    assert!(first.accepted());
    // Removing the cross-owner revision comparison does not refresh an older
    // unpublished member's own guards or allow it to create a new request.
    assert!(
        fixture
            .control
            .engine
            .prepare_captured_pad_start(&older_unpublished)
            .is_err()
    );
    older_unpublished.cancel();
    let second = capture(&fixture, 0.75, 1.5);
    let b = fixture
        .control
        .engine
        .prepare_captured_pad_start(&second)
        .unwrap();
    assert!(Arc::ptr_eq(&a.state, &b.state));
    assert_eq!(a.window_revision, b.window_revision);
    assert_eq!(
        fixture.control.engine.resident_stem_cache.lock().unwrap()[0]
            .intent_epoch
            .load(Ordering::Acquire),
        before_intent
    );
    assert!(
        a.publication.current(),
        "fresh member cannot supersede scheduled A"
    );
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&second, Some(&b))
            .unwrap()
    );
    fixture.drain();
    assert_eq!(fixture.scheduler.len(), 2);
    wait_work(&mut fixture, &[&first, &second]);
    assert!(fixture.render_to(8192).iter().all(|sample| *sample == 0.0));
    let mut pcm = fixture.render(1);
    pcm.extend(fixture.pcm(47_999));
    assert_eq!((first.state(), second.state()), ("applied", "applied"));
    assert_eq!(
        (first.effective_frame(), second.effective_frame()),
        (Some(8192), Some(8192))
    );
    assert_eq!(fixture.starts(0), 2);
    assert_eq!(fixture.voice(0).volume, 0.75);
    assert_tone(&pcm, 0, 7, 1.0, true);
}

#[test]
fn cancelling_one_real_member_before_or_after_ack_preserves_its_sibling() {
    for after_ack in [false, true] {
        let (mut fixture, _files) = fixture();
        let gate = ColdGate::new(&fixture);
        let first = capture(&fixture, 0.25, 1.5);
        let second = capture(&fixture, 0.75, 1.5);
        fixture.drain();
        let a = fixture
            .control
            .engine
            .prepare_captured_pad_start(&first)
            .unwrap();
        let b = fixture
            .control
            .engine
            .prepare_captured_pad_start(&second)
            .unwrap();
        if after_ack {
            drop(gate);
            wait_ack(&mut fixture, &a);
        } else {
            first.cancel();
            a.cancel_launch();
            a.cancel();
            drop(gate);
        }
        if after_ack {
            first.cancel();
            a.cancel_launch();
            a.cancel();
        }
        wait_ack(&mut fixture, &b);
        assert_eq!(a.publication_status(), "cancelled");
        assert!(b.is_current());
        assert!(!b.state.launch_cancelled.load(Ordering::Acquire));
        assert!(
            fixture
                .control
                .engine
                .publish_pad_start(&second, Some(&b))
                .unwrap()
        );
        fixture.drain();
        wait_work(&mut fixture, &[&second]);
        assert!(fixture.render_to(8192).iter().all(|sample| *sample == 0.0));
        fixture.render(2048);
        assert_eq!(fixture.starts(0), 1);
        assert_eq!(first.state(), "rejected");
        assert_eq!(second.state(), "applied");
    }
}

#[test]
fn a_pre_ack_original_can_join_after_real_ack_and_control_reconciliation() {
    let (mut fixture, _files) = fixture();
    let first = capture(&fixture, 0.25, 1.5);
    let second = capture(&fixture, 0.75, 1.5);
    fixture.drain();
    let a = fixture
        .control
        .engine
        .prepare_captured_pad_start(&first)
        .unwrap();
    wait_ack(&mut fixture, &a);
    reconcile(&fixture.control.engine).unwrap();
    assert!(
        fixture.control.engine.resident_stem_cache.lock().unwrap()[0]
            .relocation
            .is_none()
    );
    let b = fixture
        .control
        .engine
        .prepare_captured_pad_start(&second)
        .unwrap();
    assert!(Arc::ptr_eq(&a.state, &b.state));
    assert_eq!(b.publication_status(), "accepted");
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&second, Some(&b))
            .unwrap()
    );
    fixture.drain();
    wait_work(&mut fixture, &[&second]);
    fixture.render_to(8192);
    fixture.render(1);
    assert_eq!(second.state(), "applied");
    assert_eq!(fixture.starts(0), 1);
}

#[test]
fn different_region_and_replaced_source_cannot_join_the_original_physical_request() {
    let (mut fixture, _files) = fixture();
    let gate = ColdGate::new(&fixture);
    let original = capture(&fixture, 1.0, 1.5);
    let different = capture(&fixture, 1.0, 1.75);
    let ticket = fixture
        .control
        .engine
        .prepare_captured_pad_start(&original)
        .unwrap();
    assert!(
        join_captured_readiness(&fixture.control.engine, &different, None, None)
            .unwrap()
            .is_none()
    );
    fixture.control.publish(0, 0.2, 77);
    assert!(
        join_captured_readiness(&fixture.control.engine, &original, None, Some(&ticket)).is_err()
    );
    drop(gate);
    fixture.drain();
    assert_eq!(fixture.starts(0), 0);
    assert!(!fixture.mixer.voices.iter().any(|voice| voice.active));
}

#[test]
fn explicit_independent_loop_mode_request_keeps_own_ack_and_survives_member_cancellation() {
    let (mut fixture, _files) = fixture();
    let gate = ColdGate::new(&fixture);
    let edit = fixture
        .control
        .engine
        .prepare_resident_control(0, Some(1.5), Some(2.75), None, Some(true))
        .unwrap();
    let original = capture(&fixture, 0.75, 1.5);
    let ticket = fixture
        .control
        .engine
        ._prepare_captured_pad_start(&original, Some(&edit))
        .unwrap();
    assert!(Arc::ptr_eq(&edit.state, &ticket.state));
    assert!(edit.member_cancelled.is_none());
    assert!(ticket.member_cancelled.is_some());
    assert_eq!(ticket.key_lock_request_id, edit.key_lock_request_id);
    assert!(edit.key_lock_request_id.is_some());
    original.cancel();
    ticket.cancel_launch();
    ticket.cancel();
    assert_ne!(edit.publication_status(), "cancelled");
    assert!(!edit.state.launch_cancelled.load(Ordering::Acquire));
    drop(gate);
    wait_ack(&mut fixture, &edit);
    assert_eq!(ticket.publication_status(), "cancelled");
    assert!(edit.is_current());
    assert_eq!(fixture.starts(0), 0);
}
