//! Control publication boundaries for arrangement-owned runtime rows.

use super::*;
use crate::audio_engine::content_placement::SlotSnapshot;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::timing::{OutputClockSnapshot, SharedOutputClock};
use rtrb::{Consumer, RingBuffer};

const OWNER: u64 = 41;

struct Rows {
    loaded: Vec<bool>,
    starts: Vec<f64>,
    bindings: Vec<Option<InputRuntimePadBinding>>,
    pitch: Vec<PitchPadProjection>,
}

impl Rows {
    fn new(runtime: &InputRuntime) -> Self {
        let mut rows = Self {
            loaded: vec![false; NUM_SAMPLES],
            starts: vec![0.0; NUM_SAMPLES],
            bindings: vec![None; NUM_SAMPLES],
            pitch: vec![PitchPadProjection::default(); NUM_SAMPLES],
        };
        for id in [0, 6] {
            rows.loaded[id] = true;
            let mut binding = InputRuntimePadBinding::for_test(id, runtime.ownership.clone());
            binding.binding.sample_count = 48_000;
            rows.bindings[id] = Some(binding);
            rows.pitch[id].source_root = Some(4);
            rows.pitch[id].source_mode =
                Some(crate::audio_engine::pitch_actions::PitchKeyMode::Minor);
        }
        rows
    }

    fn publish(&self, runtime: &InputRuntime, multi_loop: bool) -> Result<(), String> {
        runtime.set_runtime_state_with_pitch(
            multi_loop,
            self.loaded.clone(),
            self.starts.clone(),
            vec![None; NUM_SAMPLES],
            self.bindings.iter().map(Option::as_ref).collect(),
            self.pitch.clone(),
        )
    }
}

fn fixture() -> (InputRuntime, Rows, Consumer<ControlMessage>) {
    let (producer, consumer) = RingBuffer::<ControlMessage>::new(16);
    let runtime = InputRuntime::new(Arc::new(Mutex::new(producer)), InputClock::new());
    let rows = Rows::new(&runtime);
    rows.publish(&runtime, true).unwrap();
    (runtime, rows, consumer)
}

fn participants(runtime: &InputRuntime) -> [SlotSnapshot; 2] {
    [0, 1].map(|id| runtime.ownership.slot_snapshot(id).unwrap())
}

fn reserve(runtime: &InputRuntime, slots: &[SlotSnapshot; 2], copy: bool, owner: u64) {
    if copy {
        runtime
            .ownership
            .reserve_copy_set(slots, owner, || true)
            .unwrap();
    } else {
        runtime.ownership.reserve_pair(slots, owner).unwrap();
    }
}

fn release(runtime: &InputRuntime, slots: &[SlotSnapshot; 2], copy: bool, owner: u64) {
    if copy {
        runtime.ownership.release_copy_set(slots, owner).unwrap();
    } else {
        runtime.ownership.release_pair(slots, owner).unwrap();
    }
}

fn revisions(runtime: &InputRuntime) -> Vec<u64> {
    (0..NUM_SAMPLES)
        .map(|id| runtime.ownership.runtime_revision(id))
        .collect()
}

#[test]
fn reserved_runtime_refresh_retains_loaded_empty_and_ack_rows_and_updates_outside() {
    for copy in [false, true] {
        let (runtime, mut rows, _consumer) = fixture();
        let previous = runtime.runtime_state.lock().unwrap().pads.clone();
        let before = revisions(&runtime);
        reserve(&runtime, &participants(&runtime), copy, OWNER);
        assert!(!rows.bindings[0].as_ref().unwrap().current());
        // Neither unavailable input rows nor a stale projected pitch can replace
        // Native's original committed row. Empty targets are equally protected.
        rows.loaded[0] = false;
        rows.bindings[0] = None;
        rows.loaded[1] = true;
        rows.pitch[0].intent = PitchIntent::new(5, 2).unwrap();
        rows.starts[6] = 0.25;

        rows.publish(&runtime, true).unwrap();

        let state = runtime.runtime_state.lock().unwrap();
        assert_eq!(state.pads[0], previous[0]);
        assert_eq!(state.pads[1], previous[1]);
        assert_eq!(state.pads[6].loop_start_s, 0.25);
        assert_eq!(runtime.ownership.runtime_revision(0), before[0]);
        assert_eq!(runtime.ownership.runtime_revision(1), before[1]);
        assert_eq!(runtime.ownership.runtime_revision(6), before[6] + 1);
        drop(state);
        assert!(!runtime.trigger_pad(0, 42));
        assert!(runtime.trigger_pad(6, 42));
    }
}

#[test]
fn explicit_multi_loop_change_supersedes_reserved_originals_and_all_runtime_epochs() {
    let (runtime, rows, _consumer) = fixture();
    let original = runtime.runtime_state.lock().unwrap().pads[0]
        .binding
        .unwrap();
    let before = revisions(&runtime);
    reserve(&runtime, &participants(&runtime), false, OWNER);

    rows.publish(&runtime, false).unwrap();

    assert!(!runtime.ownership.current(0, original));
    assert!(!runtime.runtime_state.lock().unwrap().multi_loop);
    for (id, previous) in before.into_iter().enumerate() {
        assert_eq!(runtime.ownership.runtime_revision(id), previous + 1);
    }
}

#[test]
fn runtime_refresh_rejects_reservation_acquire_release_and_replacement_without_partial_publish() {
    for copy in [false, true] {
        for change in 0..3 {
            let (runtime, mut rows, _consumer) = fixture();
            let slots = participants(&runtime);
            if change != 0 {
                reserve(&runtime, &slots, copy, OWNER);
            }
            let before = revisions(&runtime);
            let previous = runtime.runtime_state.lock().unwrap().pads.clone();
            rows.starts[6] = 0.5;

            let result = runtime.set_runtime_state_with_pitch_before_commit(
                true,
                rows.loaded.clone(),
                rows.starts.clone(),
                vec![None; NUM_SAMPLES],
                rows.bindings.iter().map(Option::as_ref).collect(),
                Some(rows.pitch.clone()),
                || match change {
                    0 => reserve(&runtime, &slots, copy, OWNER),
                    1 => release(&runtime, &slots, copy, OWNER),
                    _ => {
                        release(&runtime, &slots, copy, OWNER);
                        reserve(&runtime, &slots, copy, OWNER + 1);
                    }
                },
            );

            assert!(result.is_err(), "copy={copy}, change={change}");
            assert_eq!(runtime.runtime_state.lock().unwrap().pads, previous);
            assert_eq!(revisions(&runtime), before);
        }
    }
}

#[test]
fn runtime_refresh_rejects_reserved_lifetime_replacement_before_claim() {
    let (runtime, rows, _consumer) = fixture();
    reserve(&runtime, &participants(&runtime), false, OWNER);
    let previous = runtime.runtime_state.lock().unwrap().pads.clone();
    let before = revisions(&runtime);

    let result = runtime.set_runtime_state_before_commit(
        true,
        rows.loaded.clone(),
        rows.starts.clone(),
        vec![None; NUM_SAMPLES],
        rows.bindings.iter().map(Option::as_ref).collect(),
        || {
            runtime.ownership.publish_content_lifetime(
                0,
                crate::audio_engine::content_lifetime::reserve().unwrap(),
            );
        },
    );

    assert!(result.is_err());
    assert_eq!(runtime.runtime_state.lock().unwrap().pads, previous);
    assert_eq!(revisions(&runtime), before);
}

#[test]
fn reserved_rejected_pitch_shadow_survives_clear_boundary_then_normal_refresh_and_refill() {
    let (runtime, mut rows, _consumer) = fixture();
    let clock = Arc::new(SharedOutputClock::new());
    clock.publish(OutputClockSnapshot {
        valid: true,
        observed_at_ns: 10_000_000,
        audible_at_ns: 10_000_000,
        output_frame: 24_000,
        sample_rate_hz: 48_000,
        master_period_seconds: Some(0.5),
        downbeat_frame: 0,
        master_beat: 1.0,
        freshness_ns: 100_000_000,
    });
    runtime.attach_pitch_clock(clock).unwrap();
    runtime.set_selected_pad(0).unwrap();
    let action = pitch::capture_selected(
        PitchActionSpec::parse("pitch.extra:2:set").unwrap(),
        10_000_000,
        10_000_000,
        &runtime.runtime_state,
        &runtime.ownership,
        None,
    )
    .unwrap();
    let held = runtime.runtime_state.lock().unwrap().pads[0].clone();
    let slots = participants(&runtime);
    reserve(&runtime, &slots, false, OWNER);

    // Clear's Native ACK may retire the key before the original MIDI row is
    // mirrored. A rejected action must wait for that same observer fence.
    runtime.ownership.publish_content_lifetime(0, 0);
    assert!(
        runtime
            .cancel_pitch_capture(&action, PitchActionReject::Capacity)
            .unwrap()
    );
    rows.loaded[0] = false;
    rows.bindings[0] = None;
    rows.starts[6] = 0.25;
    rows.publish(&runtime, true).unwrap();
    assert_eq!(runtime.runtime_state.lock().unwrap().pads[0], held);
    assert_eq!(
        runtime.pitch_request_sequence(0).unwrap(),
        action.sequence()
    );

    release(&runtime, &slots, false, OWNER);
    rows.publish(&runtime, true).unwrap();
    let cleared = runtime.runtime_state.lock().unwrap().pads[0].clone();
    assert!(!cleared.loaded);
    assert!(cleared.binding.is_none());
    assert_eq!(cleared.latest_request_sequence, 0);
    let refill = InputRuntimePadBinding::for_test(0, runtime.ownership.clone());
    assert_ne!(
        refill.binding.placement.unwrap().content_key(),
        Some(action.content_key())
    );
    rows.loaded[0] = true;
    rows.bindings[0] = Some(refill);
    rows.publish(&runtime, true).unwrap();
    assert!(runtime.trigger_pad(0, 42));
    assert_eq!(runtime.pitch_request_sequence(0).unwrap(), 0);
}
