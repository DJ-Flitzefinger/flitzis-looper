//! Actual complete Copy followed by independent finite loop edits and collection.
//! No registry/phase mutation, manufactured ACK, device or Human acceptance.

#[path = "copied_loop_custody_tests/launch_tests.rs"]
mod launch_tests;

#[path = "copied_loop_custody_tests/controller_tests.rs"]
mod controller_tests;

#[path = "copied_loop_custody_tests/seek_pair_tests.rs"]
mod seek_pair_tests;

use super::*;
use crate::audio_engine::native_owned_work::CompleteSampleWork;
use crate::audio_engine::resident_relocation::ResidentWindowTicket;

const SLOTS: [usize; 3] = [0, 1, 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SlotState {
    assignment: u64,
    generation: u64,
    lifetime: u64,
    window: (usize, usize),
    loop_region: (usize, Option<usize>),
}

fn collect(fixture: &Fixture) {
    // Only the service is retained, never an additional sample/voice/lease owner.
    fixture
        .with_engine(|engine| engine.project_assets.clone())
        .collect_for_test();
}

fn state(fixture: &Fixture, slot: usize) -> SlotState {
    let (assignment, generation, lifetime, window) = fixture.with_engine(|engine| {
        let binding = engine
            .current_input_runtime_pad_binding(slot)
            .unwrap()
            .unwrap();
        assert!(
            binding.is_current(),
            "slot {slot} lost its actual source binding"
        );
        let lifetime = binding
            .binding
            .placement
            .unwrap()
            .content_key()
            .unwrap()
            .lifetime();
        let leases = engine.cold_leases.lock().unwrap();
        let lease = leases[slot].as_ref().unwrap();
        assert!(
            !lease.assignment_retired(),
            "slot {slot} assignment retired"
        );
        let assignment = lease.assignment_id();
        drop(leases);
        let cache = engine.sample_cache.lock().unwrap();
        let sample = cache[slot].as_ref().unwrap();
        assert_eq!(sample.frame_count(), FRAMES);
        (
            assignment,
            binding.source_generation,
            lifetime,
            (sample.resident_start(), sample.resident_end()),
        )
    });
    SlotState {
        assignment,
        generation,
        lifetime,
        window,
        loop_region: fixture.callback.mixer.loop_region_frames(slot),
    }
}

fn states(fixture: &Fixture) -> [SlotState; 3] {
    SLOTS.map(|slot| state(fixture, slot))
}

fn settle_window(fixture: &mut Fixture, slot: usize, start: usize, end: usize) {
    let before_regions = SLOTS.map(|id| fixture.callback.mixer.loop_region_frames(id));
    let generation = fixture.with_engine(|engine| {
        let binding = engine
            .current_input_runtime_pad_binding(slot)
            .unwrap()
            .unwrap();
        assert!(binding.is_current());
        binding.source_generation
    });
    collect(fixture);
    let ticket: ResidentWindowTicket = fixture.with_engine(|engine| {
        engine
            .prepare_resident_control_with_source(
                slot,
                Some(start as f64 / f64::from(RATE)),
                Some(end as f64 / f64::from(RATE)),
                None,
                Some(false),
                Some(generation),
            )
            .unwrap()
    });
    collect(fixture);
    wait_for_with_diagnostics(
        || {
            collect(fixture);
            fixture.callback.drain();
            // Exercise collection after Native can adopt, before control mirror.
            collect(fixture);
            fixture.with_engine(|engine| resident_relocation::reconcile(engine).unwrap());
            collect(fixture);
            match ticket.publication_status() {
                "accepted" => true,
                "preparing" | "pending" | "adopting" => false,
                status => panic!(
                    "slot {slot} loop {start}..{end} failed: {status}: {:?}",
                    ticket.error()
                ),
            }
        },
        "copied loop did not receive its original Native window ACK",
        || {
            format!(
                "slot={slot} {}: {:?}",
                ticket.publication_status(),
                ticket.error()
            )
        },
    );
    assert!(ticket.is_current());
    assert!(ticket.error().unwrap().is_none());
    let after = state(fixture, slot);
    assert_eq!(after.generation, generation);
    assert_eq!(after.window, (start, end));
    assert_eq!(after.loop_region, (start, Some(end)));
    for neighbor in SLOTS.into_iter().filter(|id| *id != slot) {
        assert_eq!(
            fixture.callback.mixer.loop_region_frames(neighbor),
            before_regions[neighbor],
            "editing slot {slot} changed slot {neighbor} loop intent"
        );
    }
    drop(ticket);
    collect(fixture);
}

fn copy_to(fixture: &mut Fixture, target: usize) {
    let source_before = state(fixture, 0);
    let captured = fixture.capture(target);
    let ticket = fixture.prepare(&captured);
    assert_ne!(ticket.publication_status(), "accepted");
    collect(fixture);
    fixture.with_engine(|engine| engine.publish_pad_copy(&ticket).unwrap());
    collect(fixture);
    assert_eq!(fixture.callback.drain(), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    collect(fixture);
    fixture.with_engine(|engine| {
        assert!(engine.reconcile_pad_copy(&ticket).unwrap());
        engine.finish_pad_copy(&ticket).unwrap();
        assert_eq!(engine.input_runtime_ownership.copy_reservation(target), 0);
    });
    assert_eq!(state(fixture, 0), source_before);
    assert!(
        !fixture
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == target)
    );
    drop(ticket);
    drop(captured);
    collect(fixture);
}

fn materialize_current_readers(fixture: &Fixture, expected: &[SlotState; 3]) {
    for slot in SLOTS {
        {
            let work = fixture.with_engine(|engine| {
                let binding = engine
                    .current_input_runtime_pad_binding(slot)
                    .unwrap()
                    .unwrap();
                assert!(binding.is_current());
                assert_eq!(binding.source_generation, expected[slot].generation);
                let cache = engine.sample_cache.lock().unwrap();
                CompleteSampleWork::capture(engine, slot, cache[slot].as_ref().unwrap()).unwrap()
            });
            let complete = work
                .materialize(crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES)
                .unwrap_or_else(|error| {
                    panic!("slot {slot} exact-assignment materialization: {error}")
                });
            assert_eq!(
                (complete.resident_start(), complete.resident_end()),
                (0, FRAMES)
            );
            assert_eq!(complete.samples.len(), FRAMES * 2);
            // Both real temporary PCM owners end before collection. No helper
            // keeps old finite backing alive to mask an absent assignment row.
        }
        collect(fixture);
        assert_eq!(states(fixture), *expected);
    }
}

fn edit_independently(
    fixture: &mut Fixture,
    expected: &mut [SlotState; 3],
    slot: usize,
    start: usize,
    end: usize,
) {
    assert_eq!(states(fixture), *expected);
    settle_window(fixture, slot, start, end);
    expected[slot].window = (start, end);
    expected[slot].loop_region = (start, Some(end));
    assert_eq!(states(fixture), *expected);
    materialize_current_readers(fixture, expected);
}

#[test]
fn two_complete_copies_keep_independent_loop_and_complete_reader_custody_after_collection() {
    isolated(
        "copied_loop_custody_tests::two_complete_copies_keep_independent_loop_and_complete_reader_custody_after_collection",
        || {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(4)
                .unwrap();
            let parent =
                workspace.join("test-artifacts/rearrange-human-fixes-20261021/copied-loop-native");
            fs::create_dir_all(&parent).unwrap();
            fs::write(
                parent.join("OWNER.md"),
                b"Owned synthetic cold-source/complete-Copy/loop-custody regression. No Human audio or settings. Root owns execution, evidence review and later cleanup.\n",
            )
            .unwrap();
            let mut fixture =
                Fixture::with_pair_registration_at(false, true, false, "tap", Some(parent));
            settle_window(&mut fixture, 0, START, END);
            copy_to(&mut fixture, 1);
            copy_to(&mut fixture, 2);
            let mut expected = states(&fixture);
            for first in 0..SLOTS.len() {
                for second in first + 1..SLOTS.len() {
                    assert_ne!(expected[first].assignment, expected[second].assignment);
                    assert_ne!(expected[first].lifetime, expected[second].lifetime);
                }
            }
            let original_finite = fixture.with_engine(|engine| {
                let cache = engine.sample_cache.lock().unwrap();
                let source = cache[0].as_ref().unwrap();
                for target in [1, 2] {
                    let copied = cache[target].as_ref().unwrap();
                    assert!(Arc::ptr_eq(&source.samples, &copied.samples));
                    assert!(!Arc::ptr_eq(
                        source.residency.as_ref().unwrap(),
                        copied.residency.as_ref().unwrap()
                    ));
                }
                Arc::downgrade(&source.samples)
            });
            // Human's immediate first copied-pad edit precedes every helper
            // full-reader materialization, so that proof cannot repair/mask it.
            edit_independently(&mut fixture, &mut expected, 1, START + 64, END - 64);
            edit_independently(&mut fixture, &mut expected, 2, START + 128, END - 128);
            edit_independently(&mut fixture, &mut expected, 0, START + 192, END - 192);
            wait_for_with_diagnostics(
                || {
                    fixture.callback.drain();
                    fixture.callback.render(64);
                    collect(&fixture);
                    original_finite.strong_count() == 0
                },
                "actual replaced finite PCM remained owned",
                || {
                    format!(
                        "original finite strong readers={}",
                        original_finite.strong_count()
                    )
                },
            );
            collect(&fixture);
            materialize_current_readers(&fixture, &expected);
            edit_independently(&mut fixture, &mut expected, 1, START + 256, END - 256);
            edit_independently(&mut fixture, &mut expected, 2, START + 320, END - 320);
            edit_independently(&mut fixture, &mut expected, 0, START + 384, END - 384);
            assert_eq!(states(&fixture), expected);
        },
    );
}
