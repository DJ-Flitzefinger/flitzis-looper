//! Exact independent edit fulfillment is not a later source-control edit.
//!
//! This is a child of resident_relocation::launch_publication_tests::
//! ordinary_shared_readiness. The existing synthetic complete-source fixture is
//! explicit; window jobs, memberships, publication, ACKs and Native preparation
//! are the production owners. No test constructs a positive receipt or ACK.

use super::*;
use crate::audio_engine::pitch_runtime_tests::{LOOP_END, LOOP_START, SOURCE_HZ};
use crate::audio_engine::resident_relocation::control_publication_tests::PendingWriter;
use crate::messages::ControlMessage;

const FROM: f64 = 1.5;
const TO: f64 = 2.75;
const FROM_FRAME: usize = 72_000;
const TO_FRAME: usize = 132_000;
const OUTPUT_FRAMES: usize = 48_000;

#[derive(Clone, Copy, Debug)]
enum FifoOrder {
    TargetThenAck,
    AckThenTarget,
}

struct DependencyCase {
    fixture: Fixture,
    root: ResidentWindowTicket,
    member: ResidentWindowTicket,
    start: CapturedPadStart,
    // Keep the actual complete-source files through engine/worker teardown.
    _files: tempfile::TempDir,
}

fn wait_root_queued(case: &DependencyCase) {
    let deadline = Instant::now() + WAIT;
    while !case.root.control_published_for_test()
        || case.fixture.control.producer.try_lock().is_err()
    {
        assert!(Instant::now() < deadline, "original root did not queue");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn pending_case(order: FifoOrder, wet: bool) -> (DependencyCase, Option<ColdGate>) {
    let (mut fixture, files) = fixture();
    if !wet {
        fixture
            .mixer
            .set_pad_pitch_intent(0, PitchIntent::default())
            .unwrap();
        fixture.mixer.set_pad_key_lock(0, false);
    }
    assert_eq!(
        fixture.mixer.loop_region_frames(0),
        (LOOP_START, Some(LOOP_END))
    );
    assert_ne!((LOOP_START, LOOP_END), (FROM_FRAME, TO_FRAME));
    let mut gate = Some(ColdGate::new(&fixture));
    let root = fixture
        .control
        .engine
        .prepare_resident_control(0, Some(FROM), Some(TO), None, Some(wet))
        .unwrap();
    assert_ne!(root.publication_status(), "accepted");
    if matches!(order, FifoOrder::AckThenTarget) {
        // The root transaction is genuinely queued first, but remains unACKed
        // when the user captures Play and retains this exact independent ticket.
        drop(gate.take());
        let deadline = Instant::now() + WAIT;
        while !root.control_published_for_test() || fixture.control.producer.try_lock().is_err() {
            assert!(Instant::now() < deadline, "original root did not queue");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(matches!(
            fixture.control.consumer.peek().unwrap(),
            ControlMessage::RelocateResident(_)
        ));
    }
    let start = capture(&fixture, 0.75, FROM);
    let member = fixture
        .control
        .engine
        ._prepare_captured_pad_start(&start, Some(&root))
        .unwrap();
    assert!(Arc::ptr_eq(&root.state, &member.state));
    assert!(root.member_cancelled.is_none());
    assert!(member.member_cancelled.is_some());
    assert!(member.pad_start.as_ref().unwrap().same_owner(&start));
    assert_eq!(member.key_lock_request_id, root.key_lock_request_id);
    assert!(root.key_lock_request_id.is_some());
    assert_ne!(root.publication_status(), "accepted");
    assert_eq!(start.original_frame(), None);
    assert_eq!(start.effective_frame(), None);
    if matches!(order, FifoOrder::TargetThenAck) {
        assert!(matches!(
            fixture.control.consumer.peek().unwrap(),
            ControlMessage::CapturePadStartTarget(_)
        ));
    }
    fixture.drain();
    assert_eq!(start.original_frame(), Some(0));
    assert_eq!(start.target_frame(), Some(0));
    assert_eq!(start.effective_frame(), None);
    assert_eq!(fixture.transport.output_frame(), 0);
    assert_eq!(fixture.starts(0), 0);
    assert!(fixture.scheduler.is_empty());
    match order {
        FifoOrder::TargetThenAck => {
            assert_ne!(root.publication_status(), "accepted");
            assert_eq!(
                fixture.mixer.loop_region_frames(0),
                (LOOP_START, Some(LOOP_END))
            );
        }
        FifoOrder::AckThenTarget => {
            assert_eq!(root.publication_status(), "accepted", "{:?}", root.error());
            assert_eq!(
                fixture.mixer.loop_region_frames(0),
                (FROM_FRAME, Some(TO_FRAME))
            );
        }
    }
    (
        DependencyCase {
            fixture,
            root,
            member,
            start,
            _files: files,
        },
        gate,
    )
}

fn finish_dependency(case: &mut DependencyCase, gate: Option<ColdGate>) {
    drop(gate);
    wait_ack(&mut case.fixture, &case.root);
    assert_eq!(case.member.publication_status(), "accepted");
    assert!(case.root.is_current());
    assert!(case.member.is_current());
    assert!(Arc::ptr_eq(&case.root.state, &case.member.state));
    assert_eq!(
        case.fixture.mixer.loop_region_frames(0),
        (FROM_FRAME, Some(TO_FRAME))
    );
    assert_eq!(case.start.original_frame(), Some(0));
    assert_eq!(case.start.effective_frame(), None);
    assert_eq!(case.fixture.transport.output_frame(), 0);
}

fn publish_original(case: &mut DependencyCase) {
    assert!(
        case.fixture
            .control
            .engine
            .publish_pad_start(&case.start, Some(&case.member))
            .unwrap()
    );
    case.fixture.drain();
}

fn prove_original_attack(case: &mut DependencyCase, wet: bool) {
    publish_original(case);
    assert!(case.start.accepted(), "{:?}", case.start.reason());
    assert_ne!(case.start.state(), "rejected", "{:?}", case.start.reason());
    let execution = case.start.execution_frame().unwrap();
    assert_eq!(execution, if wet { 8192 } else { 0 });
    if wet {
        wait_work(&mut case.fixture, &[&case.start]);
    } else {
        // The real dry E=0 effect executes synchronously. There is no retained
        // Native request to await, and no output sample has been consumed yet.
        assert_eq!(case.start.state(), "applied");
        assert_eq!(case.start.effective_frame(), Some(0));
        assert_eq!(case.fixture.starts(0), 1);
        assert!(case.fixture.scheduler.is_empty());
        assert!(
            !case
                .fixture
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .has_ready_action(case.start.sequence())
        );
        let voice = case.fixture.voice(0);
        assert_eq!(voice.source_playback.position().frame, FROM_FRAME);
        assert_eq!(voice.stretch.adopted_request_id(), None);
        assert_eq!(voice.stretch.native_primed_output_frames(), 0);
    }
    case.fixture.render(0);
    let execution = case.start.execution_frame().unwrap();
    assert!(
        case.fixture
            .render_to(execution)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let mut pcm = case.fixture.render(1);
    pcm.extend(case.fixture.pcm(OUTPUT_FRAMES - 1));
    assert_eq!(case.start.state(), "applied", "{:?}", case.start.reason());
    assert_eq!(case.start.original_frame(), Some(0));
    assert_eq!(case.start.effective_frame(), Some(execution));
    assert_eq!(case.fixture.starts(0), 1);
    assert_eq!(case.root.publication_status(), "accepted");
    assert_eq!(case.member.publication_status(), "accepted");
    let voice = case.fixture.voice(0);
    assert_eq!(voice.volume, 0.75);
    assert_eq!(
        voice.source_playback.position().frame,
        FROM_FRAME + OUTPUT_FRAMES
    );
    assert_eq!(voice.source_playback.position().fraction, 0.0);
    assert_eq!(voice.source_playback.tempo_ratio(), 1.0);
    if wet {
        assert_eq!(
            voice.stretch.adopted_request_id(),
            Some(case.start.sequence())
        );
        assert_ne!(voice.stretch.native_state_address(), 0);
        assert!(
            voice
                .stretch
                .owns_pitch_contract(PitchIntent::new(0, 7).unwrap(), true)
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
    assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
    if wet {
        assert_physical_birth_tone(&pcm, 0, 7, 1.0, true, 0.75);
    } else {
        // Neutral dry playback bypasses Native startup. The complete-source
        // fixture's analytic samples, with neutral EQ/trim and original volume,
        // are its independent reference from the very first emitted sample.
        assert_eq!(pcm.len(), OUTPUT_FRAMES);
        assert!(pcm.iter().all(|sample| sample.is_finite()));
        let maximum = pcm
            .iter()
            .enumerate()
            .map(|(frame, actual)| {
                let source_frame = FROM_FRAME + frame;
                let source = (0.25
                    * (source_frame as f64 * SOURCE_HZ * std::f64::consts::TAU / f64::from(RATE))
                        .sin()) as f32;
                (*actual - source * 0.75).abs()
            })
            .fold(0.0_f32, f32::max);
        assert!(
            maximum < 2.0e-6,
            "complete independent dry PCM mismatch: {maximum}"
        );
    }
}

#[test]
fn changed_loop_exact_dependency_accepts_non_neutral_original_in_both_actual_fifo_orders() {
    for order in [FifoOrder::AckThenTarget, FifoOrder::TargetThenAck] {
        let (mut case, gate) = pending_case(order, true);
        finish_dependency(&mut case, gate);
        prove_original_attack(&mut case, true);
    }
}

#[test]
fn neutral_original_keeps_the_same_exact_dependency_without_a_native_delay() {
    let (mut case, gate) = pending_case(FifoOrder::TargetThenAck, false);
    finish_dependency(&mut case, gate);
    prove_original_attack(&mut case, false);
}

#[test]
fn actual_unrelated_odd_writer_defers_exact_ack_without_replacing_original() {
    let (mut case, gate) = pending_case(FifoOrder::TargetThenAck, true);
    drop(gate);
    wait_root_queued(&case);
    let writer = PendingWriter::hold(case.fixture.control.engine.input_runtime_ownership.clone());
    case.fixture.drain();
    assert_ne!(case.root.publication_status(), "accepted");
    assert_eq!(case.start.original_frame(), Some(0));
    assert_eq!(case.start.effective_frame(), None);
    assert_eq!(case.fixture.starts(0), 0);
    assert!(case.fixture.scheduler.is_empty());
    writer.complete();
    finish_dependency(&mut case, None);
    prove_original_attack(&mut case, true);
}

fn set_loop(fixture: &mut Fixture, id: usize, from: usize, to: usize) {
    fixture.mixer.set_pad_loop_region(
        id,
        from as f64 / f64::from(RATE),
        Some(to as f64 / f64::from(RATE)),
    );
    assert_eq!(fixture.mixer.loop_region_frames(id), (from, Some(to)));
}

#[test]
fn other_pad_loop_edits_before_and_after_exact_ack_do_not_consume_its_authority() {
    let (mut case, gate) = pending_case(FifoOrder::TargetThenAck, true);
    set_loop(&mut case.fixture, 1, LOOP_START + 2400, LOOP_END - 2400);
    finish_dependency(&mut case, gate);
    set_loop(&mut case.fixture, 1, LOOP_START, LOOP_END);
    prove_original_attack(&mut case, true);
    assert_eq!(case.fixture.starts(1), 0);
}

#[test]
fn independent_same_pad_loop_aba_before_or_after_exact_ack_still_rejects_original() {
    for before_ack in [true, false] {
        let (mut case, gate) = pending_case(FifoOrder::TargetThenAck, true);
        if before_ack {
            set_loop(&mut case.fixture, 0, LOOP_START + 2400, LOOP_END - 2400);
            set_loop(&mut case.fixture, 0, LOOP_START, LOOP_END);
        }
        finish_dependency(&mut case, gate);
        if !before_ack {
            set_loop(&mut case.fixture, 0, FROM_FRAME + 2400, TO_FRAME - 2400);
            set_loop(&mut case.fixture, 0, FROM_FRAME, TO_FRAME);
        }
        // Before publication, control_guard still refers to the original old
        // window. The actual member API must resolve its own adopted binding;
        // that earlier WindowPending is not evidence that the ABA was refused.
        let publication = case
            .fixture
            .control
            .engine
            .publish_pad_start(&case.start, Some(&case.member));
        match publication {
            Ok(true) => {
                if case.start.state() != "rejected" {
                    // The exact ACK installed a current window projection. The
                    // remaining refusal must concern the changed source-control
                    // program, rather than an unjoined old RuntimeCell window.
                    assert!(case.start.control_guard().is_ok());
                }
                case.fixture.drain();
                assert_eq!(case.start.state(), "rejected", "{:?}", case.start.reason());
                assert_eq!(case.start.reason(), Some("timing_changed"));
            }
            Ok(false) => {
                // A pending window alone cannot satisfy this negative oracle.
                assert_eq!(case.start.state(), "rejected", "{:?}", case.start.reason());
                assert_eq!(case.start.reason(), Some("timing_changed"));
            }
            Err(error) => {
                // An earlier authoritative refusal is equally valid, but must
                // explicitly identify the control discontinuity, not readiness.
                assert!(error.to_string().contains("timing_changed"), "{error}");
            }
        }
        assert!(!case.start.accepted());
        assert_eq!(case.start.original_frame(), Some(0));
        assert_eq!(case.start.effective_frame(), None);
        assert_eq!(case.fixture.starts(0), 0);
        assert!(case.fixture.scheduler.is_empty());
        assert!(case.fixture.pcm(4096).iter().all(|sample| *sample == 0.0));
        assert_eq!(case.root.publication_status(), "accepted");
    }
}

#[test]
fn cancelled_root_or_actual_replaced_source_cannot_supply_successful_fulfillment() {
    for cancel_root in [true, false] {
        let (mut case, gate) = pending_case(FifoOrder::TargetThenAck, true);
        if cancel_root {
            assert!(case.root.cancel());
        } else {
            // Existing fixture publication installs a genuinely new source/lifetime
            // through InputRuntimeOwnership, rather than changing a numeric guard.
            case.fixture.control.publish(0, 0.2, 77);
        }
        drop(gate);
        let deadline = Instant::now() + WAIT;
        while case.fixture.control.engine.cold_jobs.counts_for_test() != (0, 0, 0) {
            case.fixture.drain();
            assert!(Instant::now() < deadline, "cancelled root did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
        case.fixture.drain();
        assert!(matches!(
            case.root.publication_status(),
            "failed" | "cancelled" | "rejected"
        ));
        assert!(matches!(
            case.member.publication_status(),
            "failed" | "cancelled" | "rejected"
        ));
        assert!(!matches!(
            case.fixture
                .control
                .engine
                .publish_pad_start(&case.start, Some(&case.member)),
            Ok(true)
        ));
        assert_eq!(case.start.original_frame(), Some(0));
        assert_eq!(case.start.effective_frame(), None);
        assert_eq!(case.fixture.starts(0), 0);
        assert!(case.fixture.scheduler.is_empty());
        assert!(case.fixture.pcm(4096).iter().all(|sample| *sample == 0.0));
    }
}

#[test]
fn sibling_membership_cannot_publish_another_original_even_over_the_same_real_root() {
    let (mut case, gate) = pending_case(FifoOrder::TargetThenAck, true);
    let other = capture(&case.fixture, 0.5, FROM);
    let other_member = case
        .fixture
        .control
        .engine
        ._prepare_captured_pad_start(&other, Some(&case.root))
        .unwrap();
    assert!(Arc::ptr_eq(&case.member.state, &other_member.state));
    assert!(!Arc::ptr_eq(
        case.member.member_cancelled.as_ref().unwrap(),
        other_member.member_cancelled.as_ref().unwrap()
    ));
    case.fixture.drain();
    finish_dependency(&mut case, gate);
    assert!(!matches!(
        case.fixture
            .control
            .engine
            .publish_pad_start(&case.start, Some(&other_member)),
        Ok(true)
    ));
    assert_eq!(case.start.effective_frame(), None);
    case.start.cancel();
    case.member.cancel_launch();
    case.member.cancel();
    assert_eq!(case.root.publication_status(), "accepted");
    assert_eq!(other_member.publication_status(), "accepted");
    assert!(
        case.fixture
            .control
            .engine
            .publish_pad_start(&other, Some(&other_member))
            .unwrap()
    );
    case.fixture.drain();
    assert!(other.accepted(), "{:?}", other.reason());
    wait_work(&mut case.fixture, &[&other]);
    let execution = observe_ready_due_effects(&mut case.fixture, &[&other]);
    assert!(
        case.fixture
            .render_to(execution)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let pcm = case.fixture.render(4096);
    assert_eq!(other.state(), "applied", "{:?}", other.reason());
    assert_eq!(other.effective_frame(), Some(execution));
    assert_eq!(case.fixture.starts(0), 1);
    assert_eq!(
        case.fixture.voice(0).stretch.adopted_request_id(),
        Some(other.sequence())
    );
    assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn global_original_group_retains_changed_loop_dependency_before_its_real_ack() {
    let (mut fixture, files) = fixture();
    fixture
        .mixer
        .set_pad_pitch_intent(1, PitchIntent::new(0, -3).unwrap())
        .unwrap();
    let gate = ColdGate::new(&fixture);
    let root = fixture
        .control
        .engine
        .prepare_resident_control(0, Some(FROM), Some(TO), None, Some(true))
        .unwrap();
    let (root, captured) = Python::attach(|py| {
        let root = Py::new(py, root).unwrap();
        let entries = [0, 1]
            .into_iter()
            .map(|id| {
                let binding = fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                if id == 0 {
                    (
                        Py::new(py, binding).unwrap(),
                        FROM,
                        Some(TO),
                        Some(root.clone_ref(py)),
                    )
                } else {
                    (
                        Py::new(py, binding).unwrap(),
                        LOOP_START as f64 / f64::from(RATE),
                        Some(LOOP_END as f64 / f64::from(RATE)),
                        None,
                    )
                }
            })
            .collect();
        let captured = fixture
            .control
            .engine
            .capture_global_start(py, entries, None)
            .unwrap();
        (root, captured)
    });
    fixture.drain();
    assert_eq!(captured.original_frame(), Some(0));
    assert!(
        fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture.mixer.loop_region_frames(0),
        (LOOP_START, Some(LOOP_END))
    );
    drop(gate);
    let deadline = Instant::now() + WAIT;
    let ticket = loop {
        fixture.drain();
        Python::attach(|py| {
            let root = root.borrow(py);
            assert!(
                !matches!(
                    root.publication_status(),
                    "failed" | "cancelled" | "rejected"
                ),
                "{:?}",
                root.error()
            );
        });
        if let Some(ticket) = fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .unwrap()
        {
            break ticket;
        }
        assert!(
            Instant::now() < deadline,
            "original global dependency did not ACK"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    fixture.drain();
    assert_ne!(captured.state(), "rejected");
    let execution = fixture.scheduler.peek_next_target_frame().unwrap();
    assert_eq!(execution, 8192);
    let originals = fixture
        .mixer
        .pitch_runtime
        .as_ref()
        .unwrap()
        .ordinary_group_originals();
    assert_eq!(originals.len(), 2);
    assert_eq!(originals[0].diagnostic_slot(), Some(0));
    assert_eq!(originals[1].diagnostic_slot(), Some(1));
    assert_eq!(originals[0].loop_region(), (FROM, Some(TO)));
    assert_ne!(originals[0].sequence(), originals[1].sequence());
    let deadline = Instant::now() + WAIT;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if runtime.ordinary_group_ready(2) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "actual original Native group did not prepare"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    fixture.render(0);
    let execution = originals[0].execution_frame().unwrap();
    assert!(
        fixture
            .render_to(execution)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let pcm = fixture.render(4096);
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(captured.original_frame(), Some(0));
    assert_eq!((fixture.starts(0), fixture.starts(1)), (1, 1));
    assert_eq!(
        fixture.voice(0).source_playback.position().frame,
        FROM_FRAME + 4096
    );
    for (id, original) in originals.iter().enumerate() {
        assert_eq!(original.original_frame(), Some(0));
        assert_eq!(original.effective_frame(), Some(execution));
        assert_eq!(
            fixture.voice(id).stretch.adopted_request_id(),
            Some(original.sequence())
        );
    }
    assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
    // Keep the cold artifacts until this fixture has actually released its workers.
    drop(fixture);
    drop(files);
}
