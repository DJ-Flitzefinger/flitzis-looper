//! Active copied-pad loop edit and immediate original launch, as used by waveform left click.
//! Real public preparation/launch/ACK only; no injected owner, phase or positive receipt.

use super::*;
use pyo3::types::PyInt;

fn wait_window(fixture: &mut Fixture, ticket: &ResidentWindowTicket) {
    wait_for_with_diagnostics(
        || {
            collect(fixture);
            fixture.callback.drain();
            collect(fixture);
            fixture.callback.render(64);
            fixture.with_engine(|engine| resident_relocation::reconcile(engine).unwrap());
            collect(fixture);
            match ticket.publication_status() {
                "accepted" => true,
                "preparing" | "pending" | "adopting" => false,
                status => panic!(
                    "active copied-pad window failed: {status}: {:?}",
                    ticket.error()
                ),
            }
        },
        "active copied-pad window did not receive its actual Native ACK",
        || format!("{}: {:?}", ticket.publication_status(), ticket.error()),
    );
    assert!(ticket.is_current());
    assert!(ticket.error().unwrap().is_none());
}

fn request_window(
    fixture: &Fixture,
    slot: usize,
    start: usize,
    end: usize,
    key_lock: bool,
) -> ResidentWindowTicket {
    fixture.with_engine(|engine| {
        let binding = engine
            .current_input_runtime_pad_binding(slot)
            .unwrap()
            .unwrap();
        assert!(binding.is_current());
        engine
            .prepare_resident_control_with_source(
                slot,
                Some(start as f64 / f64::from(RATE)),
                Some(end as f64 / f64::from(RATE)),
                None,
                Some(key_lock),
                Some(binding.source_generation),
            )
            .unwrap()
    })
}

fn voice_generations(fixture: &Fixture) -> [u64; 3] {
    SLOTS.map(|slot| {
        let mut voices = fixture
            .callback
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.is_playing_sample(slot));
        let generation = voices.next().unwrap().generation;
        assert!(
            voices.next().is_none(),
            "slot {slot} unexpectedly has two active voices"
        );
        generation
    })
}

fn edit_and_restart(
    fixture: &mut Fixture,
    expected: &mut [SlotState; 3],
    slot: usize,
    start: usize,
    key_lock: bool,
) {
    let end = expected[slot].window.1;
    let previous_voices = voice_generations(fixture);
    let starts_before = fixture.callback.feedback.starts[slot];
    // UI captures this original Native timestamp before the loop edit, then
    // trigger_pad_keep_others captures the start and retains the edit's ticket.
    let received = fixture.with_engine(|engine| engine.capture_input_timestamp_ns());
    let root = request_window(fixture, slot, start, end, key_lock);
    assert_ne!(root.publication_status(), "accepted");
    let original = fixture.with_engine(|engine| {
        let binding = engine
            .current_input_runtime_pad_binding(slot)
            .unwrap()
            .unwrap();
        assert!(binding.is_current());
        Python::attach(|py| {
            engine
                .capture_pad_start(
                    slot,
                    start as f64 / f64::from(RATE),
                    Some(end as f64 / f64::from(RATE)),
                    1.0,
                    false,
                    Some(PyInt::new(py, received).as_any()),
                    Some(binding.source_generation),
                )
                .unwrap()
        })
    });
    assert_eq!(original.received_timestamp(), received);
    fixture.callback.feedback.expect_start(&original);
    let member = fixture.with_engine(|engine| {
        engine
            .python_prepare_captured_pad_start(&original, Some(&root))
            .unwrap()
    });
    // No callback has run since the edit. This is the genuine unACKed request,
    // not a later equal window discovered after the source has changed.
    assert_ne!(root.publication_status(), "accepted");
    assert_ne!(member.publication_status(), "accepted");
    assert_eq!(original.original_frame(), None);
    assert_eq!(original.effective_frame(), None);
    collect(fixture);
    wait_window(fixture, &root);
    assert_eq!(member.publication_status(), "accepted");
    assert!(member.is_current());
    assert_eq!(original.received_timestamp(), received);
    assert_eq!(original.effective_frame(), None);
    assert_eq!(fixture.callback.feedback.starts[slot], starts_before);
    let target = original.original_frame().unwrap();
    fixture.with_engine(|engine| {
        assert!(engine.publish_pad_start(&original, Some(&member)).unwrap());
    });
    fixture.callback.drain();
    fixture.callback.complete_start(&original, starts_before);
    assert_eq!(original.received_timestamp(), received);
    assert_eq!(original.original_frame(), Some(target));
    assert_eq!(original.state(), "applied");
    assert!(root.error().unwrap().is_none());
    assert!(member.error().unwrap().is_none());
    expected[slot].window = (start, end);
    expected[slot].loop_region = (start, Some(end));
    assert_eq!(states(fixture), *expected);
    let current_voices = voice_generations(fixture);
    for neighbor in SLOTS.into_iter().filter(|neighbor| *neighbor != slot) {
        assert_eq!(
            current_voices[neighbor], previous_voices[neighbor],
            "editing/restarting copied slot {slot} replaced neighbor {neighbor} voice"
        );
    }
    for id in SLOTS {
        assert_eq!(
            fixture.callback.mixer.key_lock_for_measurement(id),
            key_lock
        );
        fixture.with_engine(|engine| {
            assert_eq!(
                engine
                    .input_runtime_ownership
                    .key_lock_status(id)
                    .unwrap()
                    .effective,
                key_lock
            );
        });
    }
    drop(original);
    drop(member);
    drop(root);
    collect(fixture);
    materialize_current_readers(fixture, expected);
}

fn active_copy_case(key_lock: bool) {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let parent =
        workspace.join("test-artifacts/rearrange-human-fixes-20261021/copied-loop-launch-native");
    fs::create_dir_all(&parent).unwrap();
    fs::write(
        parent.join("OWNER.md"),
        b"Owned synthetic active copied-pad loop/launch regression. FullMix source, Key Lock OFF/ON. No Human audio, installed runtime or device work. Root owns execution/review/cleanup.\n",
    )
    .unwrap();
    let mut fixture =
        Fixture::with_pair_registration_at(false, true, key_lock, "tap", Some(parent));
    let initial = request_window(&fixture, 0, START, END, key_lock);
    wait_window(&mut fixture, &initial);
    drop(initial);
    copy_to(&mut fixture, 1);
    copy_to(&mut fixture, 2);
    fixture.with_engine(|engine| {
        let cache = engine.sample_cache.lock().unwrap();
        let source = cache[0].as_ref().unwrap();
        for slot in [1, 2] {
            let copied = cache[slot].as_ref().unwrap();
            assert!(Arc::ptr_eq(&source.samples, &copied.samples));
            assert!(!Arc::ptr_eq(
                source.residency.as_ref().unwrap(),
                copied.residency.as_ref().unwrap()
            ));
        }
    });
    fixture.start(1);
    fixture.start(2);
    let mut expected = states(&fixture);
    assert_ne!(expected[0].assignment, expected[1].assignment);
    assert_ne!(expected[0].assignment, expected[2].assignment);
    assert_ne!(expected[1].assignment, expected[2].assignment);
    voice_generations(&fixture);
    // First copied-pad edit happens while all original/copy voices are active,
    // before any helper materializes a complete reader for the copied source.
    edit_and_restart(&mut fixture, &mut expected, 1, START + 64, key_lock);
    edit_and_restart(&mut fixture, &mut expected, 2, START + 128, key_lock);
    edit_and_restart(&mut fixture, &mut expected, 1, START + 192, key_lock);
}

#[test]
fn active_copied_loop_start_and_original_restart_keep_custody_with_key_lock_off() {
    isolated(
        "copied_loop_custody_tests::launch_tests::active_copied_loop_start_and_original_restart_keep_custody_with_key_lock_off",
        || active_copy_case(false),
    );
}

#[test]
fn active_copied_loop_start_and_original_restart_keep_custody_with_key_lock_on() {
    isolated(
        "copied_loop_custody_tests::launch_tests::active_copied_loop_start_and_original_restart_keep_custody_with_key_lock_on",
        || active_copy_case(true),
    );
}
