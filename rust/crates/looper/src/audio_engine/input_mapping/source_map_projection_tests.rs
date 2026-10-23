//! Real runtime publication and original launch witnesses, without device claims.

use super::*;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::input_runtime_binding;

fn fixture() -> LaunchFixture {
    let mut fixture = LaunchFixture::new(64);
    fixture.engine.input_runtime = Some(InputRuntime::new_with_ownership(
        fixture.producer.clone(),
        InputClock::new(),
        fixture.engine.input_runtime_ownership.clone(),
    ));
    publish(&fixture, vec![SourceMapRow::default(); NUM_SAMPLES]).unwrap();
    fixture
}

fn bindings(fixture: &LaunchFixture) -> Vec<Option<InputRuntimePadBinding>> {
    (0..NUM_SAMPLES)
        .map(|id| {
            if id < 2 {
                input_runtime_binding::capture(&fixture.engine, id).unwrap()
            } else {
                None
            }
        })
        .collect()
}

fn publish(fixture: &LaunchFixture, maps: Vec<SourceMapRow>) -> Result<(), String> {
    let bindings = bindings(fixture);
    fixture
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id < 2).collect(),
            vec![16.0 / 48_000.0; NUM_SAMPLES],
            vec![Some(96.0 / 48_000.0); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                crate::audio_engine::input_mapping::PitchPadProjection {
                    sync: true,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(maps),
        )
}

#[test]
fn data_only_selection_revision_keeps_original_scalar_launch_and_runtime_incarnation() {
    let fixture = fixture();
    let original = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    let ownership = &fixture.engine.input_runtime_ownership;
    let before = (ownership.launch_revision(0), ownership.runtime_revision(0));
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0].intent_revision = 97;
    publish(&fixture, maps).unwrap();
    assert_eq!(
        (ownership.launch_revision(0), ownership.runtime_revision(0)),
        before
    );
    assert!(
        original
            .source_map_launch_snapshot()
            .admit(ownership, original.binding)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .runtime_state
            .lock()
            .unwrap()
            .pads[0]
            .source_map
            .intent_revision_for_test(),
        97
    );
}

#[test]
fn sync_without_qualified_delivery_is_unavailable_and_fences_the_old_capture() {
    let fixture = fixture();
    let original = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    let ownership = &fixture.engine.input_runtime_ownership;
    let revision = ownership.launch_revision(0);
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0].sync_requested = true;
    maps[0].intent_revision = 1;
    publish(&fixture, maps).unwrap();
    assert_eq!(ownership.launch_revision(0), revision + 1);
    assert!(matches!(
        original
            .source_map_launch_snapshot()
            .admit(ownership, original.binding),
        Err(PitchActionReject::LaunchCancelled)
    ));
    let current = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    assert!(
        current.current(),
        "physical source preparation stays available"
    );
    assert!(matches!(
        current
            .source_map_launch_snapshot()
            .admit(ownership, current.binding),
        Err(PitchActionReject::TimingUnavailable)
    ));
    assert!(
        !fixture
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .trigger_pad(0, 0)
    );
    assert!(
        fixture.consumer.is_empty(),
        "unavailable mapped intent must not send scalar fallback"
    );
}

#[test]
fn sync_clear_cannot_reuse_a_cached_binding_with_the_new_launch_revision() {
    let fixture = fixture();
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0].sync_requested = true;
    publish(&fixture, maps).unwrap();
    let old = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    publish(&fixture, vec![SourceMapRow::default(); NUM_SAMPLES]).unwrap();
    let ownership = &fixture.engine.input_runtime_ownership;
    assert!(matches!(
        old.source_map_launch_snapshot()
            .admit(ownership, old.binding),
        Err(PitchActionReject::LaunchCancelled)
    ));
    let current = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    assert!(
        current
            .source_map_launch_snapshot()
            .admit(ownership, current.binding)
            .unwrap()
            .is_none()
    );
}

#[test]
fn new_midi_action_gets_fresh_launch_witness_after_stop() {
    let fixture = fixture();
    let original = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    fixture.engine.cancel_pad_launches(0).unwrap();
    let ownership = &fixture.engine.input_runtime_ownership;
    assert!(matches!(
        original
            .source_map_launch_snapshot()
            .admit(ownership, original.binding),
        Err(PitchActionReject::LaunchCancelled)
    ));
    assert!(
        fixture
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .trigger_pad(0, 0)
    );
    assert_eq!(fixture.consumer.slots(), 2);
}

#[test]
fn invalid_map_projection_keeps_all_existing_rows_and_launch_revisions() {
    let fixture = fixture();
    let runtime = fixture.engine.input_runtime.as_ref().unwrap();
    let before = runtime.runtime_state.lock().unwrap().pads.clone();
    let ownership = &fixture.engine.input_runtime_ownership;
    let revision = ownership.launch_revision(0);
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0].sync_requested = true;
    maps[1].exact_quarters = Some(0.0);
    assert!(publish(&fixture, maps).is_err());
    assert_eq!(runtime.runtime_state.lock().unwrap().pads, before);
    assert_eq!(ownership.launch_revision(0), revision);
}

#[test]
fn source_replacement_before_original_marker_refuses_the_whole_map_publication() {
    let fixture = fixture();
    let runtime = fixture.engine.input_runtime.as_ref().unwrap();
    let before = runtime.runtime_state.lock().unwrap().pads.clone();
    let bindings = bindings(&fixture);
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0].sync_requested = true;
    let result = runtime.set_runtime_state_with_source_maps_before_commit(
        true,
        (0..NUM_SAMPLES).map(|id| id < 2).collect(),
        vec![16.0 / 48_000.0; NUM_SAMPLES],
        vec![Some(96.0 / 48_000.0); NUM_SAMPLES],
        bindings.iter().map(Option::as_ref).collect(),
        None,
        Some(maps),
        || fixture.publish(0, 0.75, 19),
    );
    assert!(result.is_err());
    assert_eq!(runtime.runtime_state.lock().unwrap().pads, before);
}

#[test]
fn omitted_rows_keep_sync_intent_and_revision_when_the_original_source_is_replaced() {
    let fixture = fixture();
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0].sync_requested = true;
    maps[0].intent_revision = 91;
    publish(&fixture, maps).unwrap();
    fixture.publish(0, 0.75, 19);
    let runtime = fixture.engine.input_runtime.as_ref().unwrap();
    let bindings = bindings(&fixture);
    let mut starts = vec![16.0 / 48_000.0; NUM_SAMPLES];
    starts[1] = 32.0 / 48_000.0;
    runtime
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id < 2).collect(),
            starts,
            vec![Some(96.0 / 48_000.0); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                PitchPadProjection {
                    sync: true,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            None,
        )
        .unwrap();
    let guard = runtime.runtime_state.lock().unwrap();
    assert_eq!(guard.pads[0].source_map.intent_revision_for_test(), 91);
    assert_eq!(guard.pads[1].loop_start_s, 32.0 / 48_000.0);
    drop(guard);
    let replacement = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    assert!(matches!(
        replacement
            .source_map_launch_snapshot()
            .admit(&fixture.engine.input_runtime_ownership, replacement.binding,),
        Err(PitchActionReject::TimingUnavailable)
    ));
    assert!(!runtime.trigger_pad(0, 0));
    assert!(fixture.consumer.is_empty());
}

#[test]
fn arrangement_reserved_rows_keep_their_map_projection_and_launch_revision() {
    let fixture = fixture();
    let runtime = fixture.engine.input_runtime.as_ref().unwrap();
    let ownership = &fixture.engine.input_runtime_ownership;
    let bindings = bindings(&fixture);
    let participants = [
        ownership.slot_snapshot(0).unwrap(),
        ownership.slot_snapshot(1).unwrap(),
    ];
    let before = runtime.runtime_state.lock().unwrap().pads[..2].to_vec();
    let revision = ownership.launch_revision(0);
    ownership.reserve_pair(&participants, 41).unwrap();
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0].sync_requested = true;
    maps[1].exact_quarters = Some(-1.0);
    runtime
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id < 2).collect(),
            vec![0.0; NUM_SAMPLES],
            vec![None; NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                crate::audio_engine::input_mapping::PitchPadProjection {
                    sync: true,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(maps),
        )
        .unwrap();
    assert_eq!(runtime.runtime_state.lock().unwrap().pads[..2], before);
    assert_eq!(ownership.launch_revision(0), revision);
    ownership.release_pair(&participants, 41).unwrap();
}

#[test]
fn cold_and_parent_off_full_rows_retain_raw_sync_without_acquiring_map_custody() {
    let fixture = fixture();
    let runtime = fixture.engine.input_runtime.as_ref().unwrap();
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0].sync_requested = true;
    maps[0].intent_revision = 71;
    for (multi_loop, bpm_requested) in [(false, true), (true, false), (false, false)] {
        let bindings = bindings(&fixture);
        runtime
            .set_runtime_state_with_policy(
                multi_loop,
                (0..NUM_SAMPLES).map(|id| id < 2).collect(),
                vec![0.0; NUM_SAMPLES],
                vec![None; NUM_SAMPLES],
                bindings.iter().map(Option::as_ref).collect(),
                Some(vec![
                    PitchPadProjection {
                        sync: bpm_requested,
                        ..Default::default()
                    };
                    NUM_SAMPLES
                ]),
                Some(maps.clone()),
                super::super::source_sync_policy::GlobalTimingPolicy::parse(Some(
                    r#"{"schema_version":1,"sync":true,"realtime_bpm":true,"revision":71}"#,
                ))
                .unwrap(),
            )
            .unwrap();
        let guard = runtime.runtime_state.lock().unwrap();
        assert_eq!(guard.pads[0].source_map.intent_revision_for_test(), 71);
        assert_eq!(
            guard.pads[0].source_map.request().mode,
            super::super::source_sync_policy::SourceSyncMode::Legacy
        );
        assert_eq!(
            guard.pads[0].source_map.without_selection().request().mode,
            super::super::source_sync_policy::SourceSyncMode::Legacy
        );
    }
    let bindings = bindings(&fixture);
    runtime
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id < 2).collect(),
            vec![0.0; NUM_SAMPLES],
            vec![None; NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                PitchPadProjection {
                    sync: true,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            None,
        )
        .unwrap();
    let current = input_runtime_binding::capture(&fixture.engine, 0)
        .unwrap()
        .unwrap();
    assert_eq!(
        current.source_map_launch_snapshot().request().mode,
        super::super::source_sync_policy::SourceSyncMode::Mapped
    );
    assert!(matches!(
        current
            .source_map_launch_snapshot()
            .admit(&fixture.engine.input_runtime_ownership, current.binding),
        Err(PitchActionReject::TimingUnavailable)
    ));
    assert!(!PitchPadProjection::default().sync);
}

#[test]
fn hierarchy_exhaustion_and_busy_global_claim_have_no_row_or_counter_prefix() {
    for exhausted in [false, true] {
        let fixture = fixture();
        let runtime = fixture.engine.input_runtime.as_ref().unwrap();
        let ownership = &fixture.engine.input_runtime_ownership;
        let rows = runtime.runtime_state.lock().unwrap().pads.clone();
        let stamp = ownership.hierarchy_stamp.load(Ordering::Acquire);
        let launch = ownership.launch_revision(0);
        let bindings = bindings(&fixture);
        if exhausted {
            ownership
                .global_timing_revision
                .store(u64::MAX, Ordering::Release);
        } else {
            ownership
                .global_timing_publishing
                .store(true, Ordering::Release);
        }
        let revision = ownership.global_timing_revision.load(Ordering::Acquire);
        assert!(
            runtime
                .set_runtime_state_with_pitch(
                    false,
                    (0..NUM_SAMPLES).map(|id| id < 2).collect(),
                    vec![0.0; NUM_SAMPLES],
                    vec![None; NUM_SAMPLES],
                    bindings.iter().map(Option::as_ref).collect(),
                    vec![PitchPadProjection::default(); NUM_SAMPLES]
                )
                .is_err()
        );
        assert_eq!(runtime.runtime_state.lock().unwrap().pads, rows);
        assert_eq!(ownership.hierarchy_stamp.load(Ordering::Acquire), stamp);
        assert_eq!(
            ownership.global_timing_revision.load(Ordering::Acquire),
            revision
        );
        assert_eq!(ownership.launch_revision(0), launch);
        ownership
            .global_timing_publishing
            .store(false, Ordering::Release);
    }
}
