// Original Native map workers and artifact custody qualify the runtime projection.

mod mapped_launch_custody_tests {
    include!("mapped_launch_custody_tests.rs");
}

use super::*;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_mapping::InputRuntime;
use crate::audio_engine::input_runtime_binding::{self, InputRuntimePadBinding};
use crate::audio_engine::loader_events::LoaderEventDelivery;
use crate::audio_engine::pad_start::CapturedPadStart;
use crate::audio_engine::pitch_actions::PitchActionReject;
use crate::audio_engine::source_map_admission::{SourceMapProjection, SourceMapRow};
use crate::audio_engine::timing::InputClock;

fn runtime_fixture() -> ColdCustody {
    let mut fixture = layout_fixture("legacy");
    fixture.engine.input_runtime = Some(InputRuntime::new_with_ownership(
        fixture.producer.clone(),
        InputClock::new(),
        fixture.engine.input_runtime_ownership.clone(),
    ));
    fixture
}

fn original_delivery(fixture: &ColdCustody) -> (OfflineAnalysisJob, LoaderEventDelivery) {
    let (job, event) = stored_event(fixture, &record(fixture, 0));
    let delivery = Python::attach(|py| {
        let event = event.bind(py).cast::<PyDict>().unwrap();
        let value = event.get_item("_native_event_owner").unwrap().unwrap();
        let delivery = value.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
        assert!(
            job.resolve_source_beatmap_artifact_completion(&delivery)
                .unwrap()
                .is_some()
        );
        delivery.clone()
    });
    (job, delivery)
}

fn binding(fixture: &ColdCustody, id: usize) -> InputRuntimePadBinding {
    input_runtime_binding::capture(&fixture.engine, id)
        .unwrap()
        .unwrap()
}

fn publish(fixture: &ColdCustody, row: SourceMapRow) -> Result<(), String> {
    let binding = binding(fixture, 0);
    let mut bindings = vec![None; NUM_SAMPLES];
    bindings[0] = Some(binding);
    let mut rows = vec![SourceMapRow::default(); NUM_SAMPLES];
    rows[0] = row;
    fixture
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id == 0).collect(),
            vec![32.0 / 48_000.0; NUM_SAMPLES],
            vec![Some(64.0 / 48_000.0); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            None,
            Some(rows),
        )
}

#[test]
fn selected_real_map_is_data_only_until_sync_and_keeps_immutable_pin_after_clear() {
    let fixture = runtime_fixture();
    let (_job, delivery) = original_delivery(&fixture);
    publish(&fixture, SourceMapRow::default()).unwrap();
    let ordinary = binding(&fixture, 0);
    let ownership = &fixture.engine.input_runtime_ownership;
    let before = ownership.launch_revision(0);
    let mut row = SourceMapRow {
        sync_requested: false,
        intent_revision: 1,
        delivery: Some(delivery),
        exact_quarters: None,
    };
    publish(&fixture, row.clone()).unwrap();
    assert_eq!(ownership.launch_revision(0), before);
    assert!(
        ordinary
            .source_map_launch_snapshot()
            .admit(ownership, ordinary.binding)
            .unwrap()
            .is_none()
    );
    row.sync_requested = true;
    publish(&fixture, row).unwrap();
    let mapped = binding(&fixture, 0);
    let held = mapped
        .source_map_launch_snapshot()
        .admit(ownership, mapped.binding)
        .unwrap()
        .unwrap();
    assert_eq!(held.loaded_rate(), 48_000);
    assert_eq!(held.exact_quarters(), None);
    assert!(held.current_for(ownership, mapped.binding));
    let source = held.map().canonical_source_at_beat(0.5);
    assert!(source.is_some());

    fixture.engine.cancel_pad_launches(0).unwrap();
    assert!(matches!(
        mapped
            .source_map_launch_snapshot()
            .admit(ownership, mapped.binding),
        Err(PitchActionReject::LaunchCancelled)
    ));
    let mapped = binding(&fixture, 0);
    assert!(
        mapped
            .source_map_launch_snapshot()
            .admit(ownership, mapped.binding)
            .unwrap()
            .is_some(),
        "a new action snapshots the current revision without republishing the map"
    );

    publish(
        &fixture,
        SourceMapRow {
            sync_requested: true,
            intent_revision: 2,
            delivery: None,
            exact_quarters: None,
        },
    )
    .unwrap();
    assert!(matches!(
        mapped
            .source_map_launch_snapshot()
            .admit(ownership, mapped.binding),
        Err(PitchActionReject::LaunchCancelled)
    ));
    let cleared = binding(&fixture, 0);
    assert!(matches!(
        cleared
            .source_map_launch_snapshot()
            .admit(ownership, cleared.binding),
        Err(PitchActionReject::TimingUnavailable)
    ));
    assert!(matches!(
        CapturedPadStart::capture_from_binding(
            &mapped,
            32.0 / 48_000.0,
            Some(64.0 / 48_000.0),
            1.0,
            false,
            0,
            (48_000, 2),
            None,
        ),
        Err(PitchActionReject::LaunchCancelled)
    ));
    assert_eq!(
        held.map().canonical_source_at_beat(0.5),
        source,
        "an old effective reader keeps its immutable pin"
    );
}

#[test]
fn original_delivery_refuses_other_content_engine_generation_and_loaded_rate() {
    let fixture = runtime_fixture();
    let (_job, delivery) = original_delivery(&fixture);
    let current = binding(&fixture, 0);
    assert!(delivery.qualify_source_map(&current, None).is_ok());
    assert!(
        delivery
            .qualify_source_map(&binding(&fixture, 1), None)
            .is_err()
    );
    let other_engine = runtime_fixture();
    assert!(
        delivery
            .qualify_source_map(&binding(&other_engine, 0), None)
            .is_err()
    );
    let mut changed = current.clone();
    changed.source_generation += 1;
    assert!(delivery.qualify_source_map(&changed, None).is_err());
    let mut changed = current;
    changed.binding.sample_rate_hz = 44_100;
    assert!(delivery.qualify_source_map(&changed, None).is_err());
}

#[test]
fn original_job_cancel_refuses_pending_claim_without_destroying_admitted_geometry() {
    let fixture = runtime_fixture();
    let (job, delivery) = original_delivery(&fixture);
    let current = binding(&fixture, 0);
    let row = SourceMapRow {
        sync_requested: true,
        intent_revision: 1,
        delivery: Some(delivery),
        exact_quarters: None,
    };
    let projection = SourceMapProjection::qualify(&row, Some(&current)).unwrap();
    let ownership = &fixture.engine.input_runtime_ownership;
    let held = projection
        .snapshot(ownership, current.binding)
        .admit(ownership, current.binding)
        .unwrap()
        .unwrap();
    let coordinate = held.map().canonical_source_at_beat(0.5);
    job.cancel();
    assert!(!projection.publication_current(ownership, Some(current.binding)));
    assert!(!held.current_for(ownership, current.binding));
    assert_eq!(held.map().canonical_source_at_beat(0.5), coordinate);
    assert!(publish(&fixture, row).is_err());
}

#[test]
fn real_original_capture_prepares_manual_map_geometry_and_rejects_later_token_cancel() {
    let fixture = runtime_fixture();
    let (job, delivery) = original_delivery(&fixture);
    publish(
        &fixture,
        SourceMapRow {
            sync_requested: true,
            intent_revision: 1,
            delivery: Some(delivery),
            exact_quarters: None,
        },
    )
    .unwrap();
    let original = binding(&fixture, 0);
    let captured = CapturedPadStart::capture_from_binding(
        &original,
        32.0 / 48_000.0,
        Some(64.0 / 48_000.0),
        1.0,
        false,
        0,
        (48_000, 2),
        None,
    )
    .unwrap();
    let geometry = captured.mapped_geometry().unwrap();
    assert_eq!(geometry.exact_quarters(), 0.5);
    assert_eq!(geometry.domain().region.start, 32);
    assert_eq!(geometry.domain().region.end, 64);
    assert!(captured.control_guard().is_ok());
    let end = geometry.unrounded_source_end();
    job.cancel();
    assert!(matches!(
        captured.control_guard(),
        Err(PitchActionReject::SourceChanged)
    ));
    assert_eq!(
        captured.mapped_geometry().unwrap().unrounded_source_end(),
        end
    );
}

fn refresh_without_map_rows(fixture: &ColdCustody, loaded_zero: bool, other_start: usize) {
    let mut bindings = vec![None; NUM_SAMPLES];
    if loaded_zero {
        bindings[0] = Some(binding(fixture, 0));
    }
    bindings[1] = Some(binding(fixture, 1));
    let mut starts = vec![32.0 / 48_000.0; NUM_SAMPLES];
    starts[1] = other_start as f64 / 48_000.0;
    fixture
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES)
                .map(|id| id == 1 || (id == 0 && loaded_zero))
                .collect(),
            starts,
            vec![Some(64.0 / 48_000.0); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            None,
            None,
        )
        .unwrap();
}

fn assert_other_pad_uses_refreshed_region(fixture: &mut ColdCustody, start: usize) {
    assert!(
        fixture
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .trigger_pad(1, 71)
    );
    let crate::messages::ControlMessage::CapturePadStartTarget(target) =
        fixture.consumer.pop().unwrap()
    else {
        panic!("ordinary runtime trigger must retain its original target");
    };
    assert_eq!(
        target.loop_region(),
        (start as f64 / 48_000.0, Some(64.0 / 48_000.0))
    );
    assert!(matches!(
        fixture.consumer.pop().unwrap(),
        crate::messages::ControlMessage::PreparedStart(_)
    ));
    assert!(fixture.consumer.is_empty());
}

#[test]
fn omitted_map_rows_drop_real_old_selection_after_unload_or_replacement_and_refresh_other_pad() {
    for sync_requested in [false, true] {
        for publish_unloaded in [false, true] {
            let mut fixture = runtime_fixture();
            fixture
                .engine
                .input_runtime
                .as_ref()
                .unwrap()
                .set_render_format(48_000, 2)
                .unwrap();
            let (_job, delivery) = original_delivery(&fixture);
            let row = SourceMapRow {
                sync_requested,
                intent_revision: 37,
                delivery: Some(delivery),
                exact_quarters: None,
            };
            publish(&fixture, row.clone()).unwrap();
            let original = binding(&fixture, 0);
            refresh_without_map_rows(&fixture, true, 36);
            if sync_requested {
                let refreshed = binding(&fixture, 0);
                assert!(
                    refreshed
                        .source_map_launch_snapshot()
                        .admit(&fixture.engine.input_runtime_ownership, refreshed.binding,)
                        .unwrap()
                        .is_some(),
                    "benign runtime refresh must preserve the genuine selection"
                );
            }
            let samples = samples_root(&fixture);
            let source_path = original_path(&fixture, 0);
            crate::audio_engine::cold_load::unload_for_producer(
                &fixture.engine,
                0,
                &fixture.producer,
            )
            .unwrap();
            fixture.dispatch_one();
            if publish_unloaded {
                refresh_without_map_rows(&fixture, false, 40);
                assert_other_pad_uses_refreshed_region(&mut fixture, 40);
            }
            load_actual(&mut fixture, &samples, &source_path, 0);
            let replacement = binding(&fixture, 0);
            assert_ne!(replacement.source_generation, original.source_generation);
            refresh_without_map_rows(&fixture, true, 48);
            assert_other_pad_uses_refreshed_region(&mut fixture, 48);
            let replacement = binding(&fixture, 0);
            let admitted = replacement
                .source_map_launch_snapshot()
                .admit(&fixture.engine.input_runtime_ownership, replacement.binding);
            if sync_requested {
                assert!(matches!(
                    admitted,
                    Err(PitchActionReject::TimingUnavailable)
                ));
                assert!(matches!(
                    CapturedPadStart::capture_from_binding(
                        &replacement,
                        32.0 / 48_000.0,
                        Some(64.0 / 48_000.0),
                        1.0,
                        false,
                        0,
                        (48_000, 2),
                        None,
                    ),
                    Err(PitchActionReject::TimingUnavailable)
                ));
                assert!(
                    !fixture
                        .engine
                        .input_runtime
                        .as_ref()
                        .unwrap()
                        .trigger_pad(0, 72)
                );
                assert!(
                    fixture.consumer.is_empty(),
                    "a lost selected source must not fall back to scalar audio"
                );
            } else {
                assert!(admitted.unwrap().is_none());
                let scalar = CapturedPadStart::capture_from_binding(
                    &replacement,
                    32.0 / 48_000.0,
                    Some(64.0 / 48_000.0),
                    1.0,
                    false,
                    0,
                    (48_000, 2),
                    None,
                )
                .unwrap();
                assert!(scalar.mapped_geometry().is_none());
            }
            assert!(
                publish(&fixture, row).is_err(),
                "explicit old Native delivery remains stale"
            );
            assert_other_pad_uses_refreshed_region(&mut fixture, 48);
        }
    }
}

#[test]
fn omitted_map_rows_drop_cancelled_original_job_without_blocking_runtime_refresh() {
    let mut fixture = runtime_fixture();
    fixture
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_render_format(48_000, 2)
        .unwrap();
    let (job, delivery) = original_delivery(&fixture);
    let row = SourceMapRow {
        sync_requested: true,
        intent_revision: 38,
        delivery: Some(delivery),
        exact_quarters: None,
    };
    publish(&fixture, row.clone()).unwrap();
    let original = binding(&fixture, 0);
    let held = original
        .source_map_launch_snapshot()
        .admit(&fixture.engine.input_runtime_ownership, original.binding)
        .unwrap()
        .unwrap();
    let coordinate = held.map().canonical_source_at_beat(0.5);
    job.cancel();
    refresh_without_map_rows(&fixture, true, 48);
    let current = binding(&fixture, 0);
    assert!(matches!(
        current
            .source_map_launch_snapshot()
            .admit(&fixture.engine.input_runtime_ownership, current.binding,),
        Err(PitchActionReject::TimingUnavailable)
    ));
    assert_other_pad_uses_refreshed_region(&mut fixture, 48);
    assert!(publish(&fixture, row).is_err());
    assert_eq!(held.map().canonical_source_at_beat(0.5), coordinate);
}
