// Original Native map workers and artifact custody qualify the runtime projection.

mod mapped_launch_custody_tests {
    include!("mapped_launch_custody_tests.rs");
}

use super::*;
use crate::audio_engine::audio_stream::{
    drain_control_messages, drain_parameter_messages, render_scheduled_audio,
};
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::constant_timing::l3_constant_timing_fixture;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_mapping::InputRuntime;
use crate::audio_engine::input_runtime_binding::{
    self, InputRuntimeOwnership, InputRuntimePadBinding,
};
use crate::audio_engine::loader_events::LoaderEventDelivery;
use crate::audio_engine::pad_start::CapturedPadStart;
use crate::audio_engine::pitch_actions::PitchActionReject;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::source_feed_history::{SourceFeedSnapshot, SourceFeedSpan};
use crate::audio_engine::source_map_admission::{SourceMapProjection, SourceMapRow};
use crate::audio_engine::source_reader::{FractionalSourceTaps, StemRenderSelection};
use crate::audio_engine::timing::InputClock;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, ControlParameterMessage, TriggerQuantization};
use std::sync::{Arc, Mutex};

fn live_map_delivery(
    fixture: &ColdCustody,
    middle: Option<(f64, f64)>,
) -> (OfflineAnalysisJob, LoaderEventDelivery) {
    live_map_delivery_with_start_beat(fixture, middle, 0.0)
}

fn live_map_delivery_with_start_beat(
    fixture: &ColdCustody,
    middle: Option<(f64, f64)>,
    start_beat: f64,
) -> (OfflineAnalysisJob, LoaderEventDelivery) {
    let mut expected = record(fixture, 0);
    let anchor = |id: &str, seconds, beat| {
        serde_json::json!({
            "id": id, "source_seconds_bits": bits(seconds), "beat_bits": bits(beat),
            "raw_beat_index": null, "review": "reviewed", "sample_edit": null,
        })
    };
    let mut anchors = vec![anchor("start", 0.0, start_beat)];
    if let Some((seconds, beat)) = middle {
        anchors.push(anchor("middle", seconds, beat));
    }
    anchors.push(anchor("end", 1.0, 4.0));
    expected["record"]["accepted"]["intervals"] = serde_json::json!(vec![
            expected["record"]["accepted"]["intervals"][0].clone();
            anchors.len() - 1
        ]);
    expected["record"]["accepted"]["anchors"] = serde_json::json!(anchors);
    seal(&mut expected);
    let (job, event) = stored_event(fixture, &expected);
    let delivery = Python::attach(|py| {
        let event = event.bind(py).cast::<PyDict>().unwrap();
        let owner = event.get_item("_native_event_owner").unwrap().unwrap();
        let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
        assert!(
            job.resolve_source_beatmap_artifact_completion(&delivery)
                .unwrap()
                .is_some()
        );
        delivery.clone()
    });
    (job, delivery)
}

fn publish_live_map(fixture: &ColdCustody, delivery: &LoaderEventDelivery, revision: u64) {
    let current = binding(fixture, 0);
    let mut bindings = vec![None; NUM_SAMPLES];
    bindings[0] = Some(current);
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0] = SourceMapRow {
        sync_requested: true,
        intent_revision: revision,
        delivery: Some(delivery.clone()),
        ordinary_pending: false,
        exact_quarters: None,
    };
    fixture
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id == 0).collect(),
            vec![0.0; NUM_SAMPLES],
            vec![Some(1.0); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                crate::audio_engine::input_mapping::PitchPadProjection {
                    sync: true,
                    key_lock: false,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(maps),
        )
        .unwrap();
}

fn live_map_snapshot(
    fixture: &mut ColdCustody,
) -> Arc<crate::audio_engine::input_mapping::RuntimePolicySnapshot> {
    let runtime = fixture.engine.input_runtime.as_ref().unwrap();
    runtime.request_runtime_policy_adoption().unwrap();
    let snapshot = runtime.runtime_policy_snapshot().unwrap().unwrap();
    // This custody fixture has no adoption effect. Release queued work on the control thread
    // so cache reuse can be examined independently of actual callback acknowledgement.
    while let Ok(command) = fixture.consumer.pop() {
        if let crate::messages::ControlMessage::AdoptRuntimePolicy(work) = command {
            work.finish(
                Err(crate::audio_engine::input_mapping::RuntimePolicyError::Unsupported),
                None,
            );
        }
    }
    snapshot
}

#[test]
fn runtime_map_snapshot_reuses_original_prepared_owner_without_inventing_a_launch() {
    use crate::audio_engine::pad_start::PadStartCustody;
    use crate::audio_engine::source_map_admission::RuntimeMapClass;
    let mut path = ReaderDomainPath::new(&vec![0_i16; 65_536]);
    let (_job, delivery) = live_map_delivery(&path.source, None);
    publish_live_map(&path.source, &delivery, 1);
    let snapshot = live_map_snapshot(&mut path.source);
    let source = snapshot.prepared_map(0).unwrap();
    let preparation = snapshot.map_preparation(0);
    assert_eq!(preparation.class, RuntimeMapClass::Ready);
    assert_eq!(preparation.witness.unwrap().intent_revision, 1);
    let domain = preparation.domain.unwrap();
    assert_eq!(domain.sample_frames, 65_536);
    assert_eq!(
        domain.region,
        crate::audio_engine::source_reader::FrameRange {
            start: 0,
            end: 48_000
        }
    );
    assert_eq!(
        domain.period.scalar_period().map(f64::to_bits),
        Some(48_000.0_f64.to_bits())
    );
    assert_eq!(snapshot.row(0).unwrap().map_preparation, preparation);
    assert!(snapshot.map_current(0));
    assert!(source.geometry().has_compact_continuation());
    assert!(source.belongs_to(&path.source.engine.input_runtime_ownership));
    assert!(!source.belongs_to(&Arc::new(InputRuntimeOwnership::default())));
    let custody = PadStartCustody::for_runtime_map(source);
    assert!(custody.original_launch().is_none());
    assert!(Arc::ptr_eq(custody.effective_source().unwrap(), source));
    let cached = live_map_snapshot(&mut path.source);
    assert!(Arc::ptr_eq(&snapshot, &cached));
    assert!(Arc::ptr_eq(source, cached.prepared_map(0).unwrap()));

    publish_live_map(&path.source, &delivery, 2);
    let next = live_map_snapshot(&mut path.source);
    let next_source = next.prepared_map(0).unwrap();
    assert!(!Arc::ptr_eq(source, next_source));
    assert!(
        source.same_source(next_source),
        "a raw request revision is not a new map trajectory"
    );
    assert_ne!(preparation.witness, next.map_preparation(0).witness);
    assert!(!snapshot.map_current(0));
    assert!(next.map_current(0));
    assert!(custody.original_launch().is_none());
}

#[test]
fn map_job_cancel_revokes_pending_runtime_admission_but_preserves_effective_original_pins() {
    use crate::audio_engine::pad_start::PadStartCustody;
    let mut path = ReaderDomainPath::new(&vec![0_i16; 65_536]);
    let (job, delivery) = live_map_delivery(&path.source, None);
    publish_live_map(&path.source, &delivery, 1);
    let snapshot = live_map_snapshot(&mut path.source);
    assert!(snapshot.row_current(0) && snapshot.map_current(0));
    let source = snapshot.prepared_map(0).unwrap();
    let custody = PadStartCustody::for_runtime_map(source);
    let coordinate = source.geometry().map().canonical_source_at_beat(2.0);
    job.cancel();
    assert!(
        snapshot.row_current(0),
        "data-only cancellation keeps runtime and source intent"
    );
    assert!(
        !snapshot.map_current(0),
        "the original job token still fences pending admission"
    );
    assert!(source.belongs_to(&path.source.engine.input_runtime_ownership));
    assert!(delivery.source_map_belongs_to(&path.source.engine.input_runtime_ownership));
    assert_eq!(
        source.geometry().map().canonical_source_at_beat(2.0),
        coordinate
    );
    assert!(Arc::ptr_eq(custody.effective_source().unwrap(), source));
    assert!(custody.original_launch().is_none());
}

#[test]
fn general_physical_runtime_derivative_keeps_original_legacy_launch_and_cache_custody() {
    use crate::audio_engine::general_source_numerics::{Integer, MappedReaderPeriod};
    use crate::audio_engine::source_map_admission::RuntimeMapClass;
    let mut path = ReaderDomainPath::new(&vec![0_i16; 65_536]);
    // The original binary64 0.1 * 48000 has a rounded product which differs from its
    // exact dyadic product. The old exact intersection still refuses; the later
    // physical-Q contract now admits this exact formerly unsupported input.
    let (_job, delivery) = live_map_delivery(&path.source, Some((0.1, 0.4)));
    publish_live_map(&path.source, &delivery, 3);
    let snapshot = live_map_snapshot(&mut path.source);
    let preparation = snapshot.map_preparation(0);
    assert_eq!(preparation.class, RuntimeMapClass::Ready);
    let domain = preparation.domain.unwrap();
    assert_eq!(
        domain.period.physical_quarters().unwrap().parts(),
        (
            Integer::from_u128(4).unwrap(),
            Integer::from_u128(1).unwrap()
        )
    );
    assert_eq!(
        domain.period.mapped_period(domain.region),
        MappedReaderPeriod::physical(48_000)
    );
    let source = snapshot.prepared_map(0).unwrap();
    assert!(source.geometry().has_general_continuation());
    assert_eq!(source.geometry().domain().musical_period, domain.period);
    assert!(preparation.witness.is_some());
    assert!(snapshot.map_current(0));
    let launch = CapturedPadStart::capture_from_binding(
        &binding(&path.source, 0),
        0.0,
        Some(1.0),
        1.0,
        false,
        0,
        (48_000, 2),
        None,
    )
    .unwrap();
    let legacy = launch.mapped_geometry().unwrap();
    assert!(!legacy.has_compact_continuation());
    assert!(!legacy.has_general_continuation());
    assert!(legacy.clone().qualify_exact_continuation().is_err());
    assert_eq!(
        launch.sync_request().mode,
        crate::audio_engine::source_sync_policy::SourceSyncMode::Mapped
    );
    // This helper intentionally terminates queued work without a callback. Ready
    // preparation and cache custody are not an actual-adoption acknowledgement.
    let cached = live_map_snapshot(&mut path.source);
    assert!(Arc::ptr_eq(&snapshot, &cached));
    assert_eq!(cached.map_preparation(0), preparation);
    assert!(Arc::ptr_eq(cached.prepared_map(0).unwrap(), source));
}

#[test]
fn unsupported_physical_grid_runtime_derivative_keeps_original_legacy_launch_and_domain_evidence() {
    use crate::audio_engine::source_map_admission::RuntimeMapClass;
    let mut path = ReaderDomainPath::new(&vec![0_i16; 65_536]);
    // Finite increasing legacy geometry stays valid, but the exact beat grid
    // requires -200, outside the separately documented [-126,0] compact class.
    let (_job, delivery) =
        live_map_delivery_with_start_beat(&path.source, Some((0.1, 0.4)), -2.0_f64.powi(-200));
    publish_live_map(&path.source, &delivery, 3);
    let snapshot = live_map_snapshot(&mut path.source);
    let preparation = snapshot.map_preparation(0);
    assert_eq!(preparation.class, RuntimeMapClass::Unsupported);
    assert_eq!(
        preparation
            .domain
            .unwrap()
            .period
            .scalar_period()
            .map(f64::to_bits),
        Some(48_000.0_f64.to_bits())
    );
    assert!(preparation.witness.is_some());
    assert!(snapshot.prepared_map(0).is_none());
    assert!(snapshot.map_current(0));
    let launch = CapturedPadStart::capture_from_binding(
        &binding(&path.source, 0),
        0.0,
        Some(1.0),
        1.0,
        false,
        0,
        (48_000, 2),
        None,
    )
    .unwrap();
    assert!(launch.mapped_geometry().is_some());
    assert!(!launch.mapped_geometry().unwrap().has_compact_continuation());
    assert_eq!(
        launch.sync_request().mode,
        crate::audio_engine::source_sync_policy::SourceSyncMode::Mapped
    );
    let cached = live_map_snapshot(&mut path.source);
    assert!(Arc::ptr_eq(&snapshot, &cached));
    assert_eq!(cached.map_preparation(0), preparation);
}

#[test]
fn effective_map_replacement_moves_old_arc_to_retirement_and_keeps_genuine_original_launch() {
    use crate::audio_engine::pad_start::PadStartCustody;
    let mut path = ReaderDomainPath::new(&vec![0_i16; 65_536]);
    let (_first_job, first) = live_map_delivery(&path.source, None);
    publish_live_map(&path.source, &first, 1);
    let original = CapturedPadStart::capture_from_binding(
        &binding(&path.source, 0),
        0.0,
        Some(1.0),
        1.0,
        false,
        0,
        (48_000, 2),
        None,
    )
    .unwrap();
    let first_snapshot = live_map_snapshot(&mut path.source);
    let first_source = first_snapshot.prepared_map(0).unwrap().clone();
    let weak_first = Arc::downgrade(&first_source);
    // Exercise aggregate storage only; this does not manufacture a voice/adoption permission.
    let mut custody = PadStartCustody::from(original.clone());
    assert!(custody.replace_effective_map(&first_source).is_none());
    assert!(custody.original_launch().unwrap().same_owner(&original));
    let (_second_job, second) = live_map_delivery(&path.source, Some((0.5, 1.0)));
    publish_live_map(&path.source, &second, 2);
    let second_snapshot = live_map_snapshot(&mut path.source);
    let second_source = second_snapshot.prepared_map(0).unwrap();
    assert!(!first_source.same_source(second_source));
    let retired = custody.replace_effective_map(second_source).unwrap();
    assert!(retired.original_launch().is_none());
    assert!(Arc::ptr_eq(
        retired.effective_source().unwrap(),
        &first_source
    ));
    assert!(Arc::ptr_eq(
        custody.effective_source().unwrap(),
        second_source
    ));
    assert!(custody.original_launch().unwrap().same_owner(&original));
    let count = Arc::strong_count(second_source);
    assert!(custody.replace_effective_map(second_source).is_none());
    assert_eq!(Arc::strong_count(second_source), count);
    drop(first_source);
    drop(first_snapshot);
    assert_eq!(Arc::strong_count(retired.effective_source().unwrap()), 1);
    assert!(
        weak_first.upgrade().is_some(),
        "the exact displaced map still belongs to retirement"
    );
    drop(retired); // This fixture releases the last source owner on its control/test thread.
    assert!(weak_first.upgrade().is_none());
}

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
            Some(vec![
                crate::audio_engine::input_mapping::PitchPadProjection {
                    sync: true,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
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
        ordinary_pending: false,
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
            ordinary_pending: false,
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
fn pending_flag_cannot_replace_a_selected_original_delivery_or_its_source_checks() {
    let fixture = runtime_fixture();
    let (_job, delivery) = original_delivery(&fixture);
    let mut row = SourceMapRow {
        sync_requested: true,
        intent_revision: 1,
        delivery: Some(delivery),
        exact_quarters: None,
        ordinary_pending: true,
    };
    let current = binding(&fixture, 0);
    assert!(SourceMapProjection::qualify(&row, Some(&current)).is_err());
    Python::attach(|py| {
        let wire = PyDict::new(py);
        wire.set_item("sync_requested", true).unwrap();
        wire.set_item("intent_revision", 1).unwrap();
        wire.set_item(
            "delivery",
            Py::new(py, row.delivery.clone().unwrap()).unwrap(),
        )
        .unwrap();
        wire.set_item("exact_quarters", py.None()).unwrap();
        wire.set_item("ordinary_pending", true).unwrap();
        assert!(crate::audio_engine::source_map_api::parse_rows(py, vec![wire.unbind()]).is_err());
    });
    row.ordinary_pending = false;
    let mut projection = SourceMapProjection::qualify(&row, Some(&current)).unwrap();
    projection.remask_parents(true, true);
    assert!(!projection.permits_ordinary_pending());
    assert!(
        projection
            .snapshot(&fixture.engine.input_runtime_ownership, current.binding)
            .admit(&fixture.engine.input_runtime_ownership, current.binding)
            .unwrap()
            .is_some()
    );
    assert!(SourceMapProjection::qualify(&row, Some(&binding(&fixture, 1))).is_err());
    assert!(SourceMapProjection::qualify(&row, None).is_err());
    row.delivery = None;
    row.ordinary_pending = true;
    assert!(SourceMapProjection::qualify(&row, None).is_err());
    let mut stale = current.clone();
    stale.source_generation += 1;
    assert!(SourceMapProjection::qualify(&row, Some(&stale)).is_err());
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
        ordinary_pending: false,
        exact_quarters: None,
    };
    let mut projection = SourceMapProjection::qualify(&row, Some(&current)).unwrap();
    projection.remask_parents(true, true);
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
            ordinary_pending: false,
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
            Some(vec![
                crate::audio_engine::input_mapping::PitchPadProjection {
                    sync: true,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
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
                ordinary_pending: false,
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
        ordinary_pending: false,
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

const READER_DOMAIN_RATE: u32 = 48_000;
const READER_DOMAIN_START: usize = 32_768;
const READER_DOMAIN_END: usize = 65_536;
const READER_DOMAIN_OUTPUT_FRAMES: usize = 65_536;

struct ReaderDomainPath {
    source: ColdCustody,
    timeline: TransportTimeline,
    scheduler: FixedCapacityScheduler<16>,
    feedback: Vec<AudioMessage>,
    parameters: rtrb::Consumer<ControlParameterMessage>,
}

impl ReaderDomainPath {
    fn new(pcm: &[i16]) -> Self {
        let mut source = ColdCustody::new_selected_with_signal(None, pcm);
        source.engine.input_runtime = Some(InputRuntime::new_with_ownership(
            source.producer.clone(),
            InputClock::new(),
            source.engine.input_runtime_ownership.clone(),
        ));
        source.engine.test_output_format = Some((READER_DOMAIN_RATE, 2));
        source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_render_format(READER_DOMAIN_RATE, 2)
            .unwrap();
        let (producer, parameters) = rtrb::RingBuffer::new(8);
        source.engine.test_parameter_producer = Some(Arc::new(Mutex::new(producer)));
        Self {
            source,
            timeline: TransportTimeline::new(READER_DOMAIN_RATE),
            scheduler: FixedCapacityScheduler::new(),
            feedback: Vec::new(),
            parameters,
        }
    }

    fn drain(&mut self) {
        drain_control_messages(
            &mut self.source.consumer,
            &mut self.scheduler,
            self.timeline.output_frame(),
            &mut TriggerQuantization::Immediate,
            &mut self.timeline,
            &mut self.source.mixer,
            &mut self.feedback,
            &mut ImmediateAudioBufferRetirement,
        );
        assert!(self.source.consumer.is_empty());
    }

    fn publish_map_request(&self, delivery: &LoaderEventDelivery, mapped: bool) {
        let current = binding(&self.source, 0);
        let mut bindings = vec![None; NUM_SAMPLES];
        bindings[0] = Some(current);
        let mut rows = vec![SourceMapRow::default(); NUM_SAMPLES];
        rows[0] = SourceMapRow {
            sync_requested: mapped,
            intent_revision: u64::from(mapped),
            delivery: Some(delivery.clone()),
            ordinary_pending: false,
            exact_quarters: None,
        };
        self.source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_runtime_state_with_source_maps(
                true,
                (0..NUM_SAMPLES).map(|id| id == 0).collect(),
                vec![READER_DOMAIN_START as f64 / f64::from(READER_DOMAIN_RATE); NUM_SAMPLES],
                vec![Some(READER_DOMAIN_END as f64 / f64::from(READER_DOMAIN_RATE)); NUM_SAMPLES],
                bindings.iter().map(Option::as_ref).collect(),
                Some(vec![
                    crate::audio_engine::input_mapping::PitchPadProjection {
                        sync: true,
                        ..Default::default()
                    };
                    NUM_SAMPLES
                ]),
                Some(rows),
            )
            .unwrap();
    }

    fn start(&mut self) -> CapturedPadStart {
        self.drain();
        let start = self
            .source
            .engine
            .capture_pad_start(
                0,
                READER_DOMAIN_START as f64 / f64::from(READER_DOMAIN_RATE),
                Some(READER_DOMAIN_END as f64 / f64::from(READER_DOMAIN_RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap();
        assert!(self.source.engine.publish_pad_start(&start, None).unwrap());
        self.drain();
        assert_eq!(start.state(), "applied", "{:?}", start.reason());
        assert_eq!(start.target_frame(), Some(self.timeline.output_frame()));
        assert_eq!(start.execution_frame(), start.target_frame());
        start
    }

    fn render(&mut self, frames: usize) {
        let mut output = vec![0.0; frames * 2];
        render_scheduled_audio(
            &mut self.source.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            self.timeline.output_frame(),
            2,
            &mut self.timeline,
            &mut self.feedback,
            &mut ImmediateAudioBufferRetirement,
        );
        self.timeline.advance_by_rendered_frames(frames);
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    fn last_actual_source_read(
        &self,
        expected_active_start: u64,
    ) -> (SourceFeedSpan, FractionalSourceTaps, [u32; 2]) {
        let voice = self
            .source
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == 0)
            .unwrap();
        assert!(!voice.stretch.has_active_native_ownership());
        let sample = voice.sample.as_ref().unwrap();
        let mut history = SourceFeedSnapshot::new();
        assert!(voice.source_feed_history.capture_into(&mut history));
        assert_eq!(
            history.retained_range(),
            Some(expected_active_start..expected_active_start + READER_DOMAIN_OUTPUT_FRAMES as u64)
        );
        // This physical slot's attempted-feed counter remains monotonic across
        // stop/restart. It is separate from elapsed frames in either launch.
        let active_e = expected_active_start + READER_DOMAIN_OUTPUT_FRAMES as u64 - 1;
        let span = history
            .spans()
            .find(|span| span.active_start <= active_e && active_e < span.active_end())
            .unwrap();
        let read = span
            .slice((active_e - span.active_start) as usize, 1)
            .unwrap();
        let position = read.playback.position();
        assert_eq!(read.plan.frame_pos, position.frame);
        assert_eq!(read.plan.seek_mode, position.seek_mode);
        assert_eq!(read.plan.selection, StemRenderSelection::full_mix());
        assert!(!read.plan.transition.is_active());
        let readers = read.context.resolve_readers(read.plan, None).unwrap();
        assert!(readers.current.is_none() && readers.outgoing.is_none());
        let taps = read.plan.fractional_taps(position.fraction);
        let pcm = std::array::from_fn(|channel| {
            assert!(
                read.plan
                    .tap_available_with_readers(sample, readers, taps.left, channel)
            );
            assert!(taps.right.is_none_or(|right| {
                read.plan
                    .tap_available_with_readers(sample, readers, right, channel)
            }));
            read.plan
                .sample_fractional_with_readers(sample, readers, position.fraction, 0.0, channel)
                .to_bits()
        });
        (read, taps, pcm)
    }
}

#[test]
fn accepted_scalar_and_qualified_map_same_pair_have_different_actual_reader_pcm() {
    // Synthetic declared BeatThis labels exercise real acceptance/publication and ACKs.
    // They assert neither model inference, musical/hearing acceptance nor live conversion.
    let mut pcm = vec![0_i16; 131_072];
    pcm[READER_DOMAIN_START] = 16_384;
    let mut path = ReaderDomainPath::new(&pcm);
    path.source
        .engine
        .set_pad_timing_intent(0, "automatic")
        .unwrap();
    path.drain();
    let timing = l3_constant_timing_fixture::publish_synthetic_loaded_timing(
        &path.source.engine,
        &path.source.producer,
        0,
        32_768.25 / f64::from(READER_DOMAIN_RATE),
    );
    assert_eq!(timing.publication_status().unwrap(), "pending");
    path.drain();
    assert_eq!(timing.publication_status().unwrap(), "accepted");
    let original_binding = binding(&path.source, 0);
    let accepted = original_binding.binding.accepted.unwrap();
    let scalar_period = accepted.period_seconds * f64::from(READER_DOMAIN_RATE);
    assert!((scalar_period - 32_768.25).abs() < 1.0e-7);
    assert_eq!(accepted.sample_rate_hz, READER_DOMAIN_RATE);
    assert_eq!(
        path.source
            .engine
            .current_timing_acknowledgements
            .current_for_slot(&path.source.engine.input_runtime_ownership, 0),
        accepted.publication_epoch
    );
    Python::attach(|py| {
        let metadata = path
            .source
            .engine
            .current_constant_timing(py, 0)
            .unwrap()
            .unwrap();
        let metadata = metadata.bind(py).cast::<PyDict>().unwrap();
        let period = metadata
            .get_item("period_seconds_per_quarter")
            .unwrap()
            .unwrap()
            .extract::<f64>()
            .unwrap();
        assert_eq!(period.to_bits(), accepted.period_seconds.to_bits());
        assert_eq!(
            metadata
                .get_item("frame_count")
                .unwrap()
                .unwrap()
                .extract::<usize>()
                .unwrap(),
            pcm.len()
        );
    });

    let original_pcm = path.source.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    assert_eq!(original_pcm.frame_count(), pcm.len());
    assert_eq!(original_pcm.sample_at_absolute(READER_DOMAIN_START, 0), 0.5);
    assert_eq!(
        original_pcm.sample_at_absolute(READER_DOMAIN_END - 1, 0),
        0.0
    );
    let mut map_record = record(&path.source, 0);
    map_record["record"]["accepted"]["anchors"][1]["source_seconds_bits"] = serde_json::json!(
        bits(READER_DOMAIN_END as f64 / f64::from(READER_DOMAIN_RATE))
    );
    map_record["record"]["accepted"]["anchors"][1]["beat_bits"] = serde_json::json!(bits(2.0));
    seal(&mut map_record);
    let (map_job, event) = stored_event(&path.source, &map_record);
    let delivery = Python::attach(|py| {
        let event = event.bind(py).cast::<PyDict>().unwrap();
        let owner = event.get_item("_native_event_owner").unwrap().unwrap();
        let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
        assert!(
            map_job
                .resolve_source_beatmap_artifact_completion(&delivery)
                .unwrap()
                .is_some()
        );
        delivery.clone()
    });

    let master_period = READER_DOMAIN_OUTPUT_FRAMES as f64 / f64::from(READER_DOMAIN_RATE);
    path.source
        .engine
        .set_bpm_lock_with_binding(false, None)
        .unwrap();
    path.drain();
    drain_parameter_messages(
        &mut path.parameters,
        &mut path.source.mixer,
        &mut path.timeline,
    );
    path.render(0);
    let (off_revision, off) = path
        .source
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read_bpm()
        .unwrap();
    assert_eq!(off_revision, off.revision);
    assert!(off.applied && !off.requested && !off.effective);
    assert_eq!(off.execution_frame, Some(0));
    // This separately declared scalar clock follows the completed OFF effect;
    // the later legacy flag-only ON must preserve exactly this clock.
    path.source
        .engine
        .set_speed_and_master_period(0.5, master_period)
        .unwrap();
    path.drain();
    drain_parameter_messages(
        &mut path.parameters,
        &mut path.source.mixer,
        &mut path.timeline,
    );
    assert!(path.parameters.is_empty());
    let master = path.timeline;
    path.publish_map_request(&delivery, false);
    let scalar_start = path.start();
    assert!(scalar_start.mapped_geometry().is_none());
    assert_eq!(scalar_start.target_frame(), Some(0));
    path.render(READER_DOMAIN_OUTPUT_FRAMES);
    let (scalar_read, scalar_taps, scalar_pcm) = path.last_actual_source_read(0);
    assert!(!scalar_read.playback.is_mapped());
    assert_eq!(
        scalar_read.plan.loop_period.scalar_period(),
        Some(scalar_period)
    );
    assert_eq!(scalar_read.context.binding.accepted, Some(accepted));
    let scalar_position = scalar_read.playback.position();
    assert_eq!(scalar_position.frame, READER_DOMAIN_END - 1);
    assert_eq!(scalar_position.fraction.to_bits(), 0.5_f64.to_bits());
    assert_eq!(scalar_taps.left, READER_DOMAIN_END - 1);
    assert_eq!(scalar_taps.right, Some(READER_DOMAIN_START));
    let expected_scalar_gain =
        (0.5 / (scalar_period - (READER_DOMAIN_END - READER_DOMAIN_START - 1) as f64)) as f32;
    assert_eq!(
        scalar_taps.right_gain.to_bits(),
        expected_scalar_gain.to_bits()
    );

    // Keep the same source/master and advance time normally across a genuine stop/restart.
    path.source.engine.stop_sample(0).unwrap();
    path.drain();
    assert!(path.source.mixer.voices.iter().all(|voice| !voice.active));
    path.source
        .engine
        .set_bpm_lock_with_binding(true, None)
        .unwrap();
    path.drain();
    drain_parameter_messages(
        &mut path.parameters,
        &mut path.source.mixer,
        &mut path.timeline,
    );
    path.render(0);
    let (bpm_revision, bpm_effect) = path
        .source
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read_bpm()
        .unwrap();
    assert_eq!(bpm_revision, bpm_effect.revision);
    assert!(bpm_effect.applied && bpm_effect.requested && bpm_effect.effective);
    assert_eq!(
        bpm_effect.execution_frame,
        Some(READER_DOMAIN_OUTPUT_FRAMES as u64)
    );
    assert!(path.timeline.matches_clock_epoch(&master));
    assert_eq!(path.timeline.master_period_seconds(), Some(master_period));
    path.publish_map_request(&delivery, true);
    let mapped_binding = binding(&path.source, 0);
    assert_eq!(mapped_binding.binding.accepted, Some(accepted));
    assert_eq!(
        mapped_binding.source_generation,
        original_binding.source_generation
    );
    // Policy publications advance the global layout counter without changing
    // this original fixed-slot/content/lifetime admission. This is not equality
    // of two committed transaction receipts; current qualification stays separate.
    let original_placement = original_binding.binding.placement.unwrap();
    let mapped_placement = mapped_binding.binding.placement.unwrap();
    assert!(mapped_placement.same_projection(original_placement));
    assert_ne!(
        mapped_placement.layout_revision(),
        original_placement.layout_revision()
    );
    let qualified = delivery.qualify_source_map(&mapped_binding, None).unwrap();
    assert!(qualified.current_for(
        &path.source.engine.input_runtime_ownership,
        mapped_binding.binding
    ));
    let mapped_start = path.start();
    assert_eq!(
        mapped_start.target_frame(),
        Some(READER_DOMAIN_OUTPUT_FRAMES as u64)
    );
    let geometry = mapped_start.mapped_geometry().unwrap();
    assert_eq!(geometry.exact_quarters(), 1.0);
    assert_eq!(geometry.domain().region.start, READER_DOMAIN_START);
    assert_eq!(geometry.domain().region.end, READER_DOMAIN_END);
    assert_eq!(
        geometry.domain().musical_period.scalar_period(),
        Some(32_768.0)
    );
    path.render(READER_DOMAIN_OUTPUT_FRAMES);
    let (mapped_read, mapped_taps, mapped_pcm) =
        path.last_actual_source_read(READER_DOMAIN_OUTPUT_FRAMES as u64);
    assert!(mapped_read.playback.is_mapped());
    let mapped_position = mapped_read.playback.position();
    assert_eq!(mapped_position.frame, scalar_position.frame);
    assert_eq!(
        mapped_position.fraction.to_bits(),
        scalar_position.fraction.to_bits()
    );
    assert_eq!(mapped_position.seek_mode, scalar_position.seek_mode);
    assert_eq!(mapped_read.context.binding.accepted, Some(accepted));
    assert_eq!(
        mapped_read.context.binding.source_address,
        scalar_read.context.binding.source_address
    );
    let voice = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 0)
        .unwrap();
    assert!(original_pcm.same_source(voice.sample.as_ref().unwrap()));
    assert!(Arc::ptr_eq(
        &original_pcm.samples,
        &voice.sample.as_ref().unwrap().samples
    ));
    assert!(
        voice
            .mapped_start
            .as_ref()
            .unwrap()
            .original()
            .same_owner(&mapped_start)
    );
    assert!(master.matches_clock_epoch(&path.timeline));
    assert!(!scalar_read.plan.matches_exact(mapped_read.plan));
    assert!(!scalar_read.plan.matches_source_contract(mapped_read.plan));
    assert_eq!(mapped_taps.left, scalar_taps.left);
    assert_eq!(mapped_taps.right, scalar_taps.right);
    assert_eq!(mapped_taps.right_gain.to_bits(), 0.5_f32.to_bits());
    assert_ne!(
        scalar_taps.right_gain.to_bits(),
        mapped_taps.right_gain.to_bits()
    );
    for channel in 0..2 {
        assert_eq!(
            scalar_pcm[channel],
            (0.5 * scalar_taps.right_gain).to_bits()
        );
        assert_eq!(mapped_pcm[channel], 0.25_f32.to_bits());
        assert_ne!(scalar_pcm[channel], mapped_pcm[channel]);
    }
    println!(
        "ACCEPTED_READER_DOMAIN {}",
        serde_json::json!({
            "scope": "same original cold source, synthetic timing labels, real accepted ACK and qualified map; separate actual starts/raw source reads; no live conversion, model inference, DSP-output or hearing acceptance",
            "sample_rate_hz": READER_DOMAIN_RATE,
            "source_frames": pcm.len(),
            "accepted_period_seconds_bits": bits(accepted.period_seconds),
            "scalar_period_frames_bits": bits(scalar_period),
            "mapped_period_frames_bits": bits(mapped_read.plan.loop_period.scalar_period().unwrap()),
            "local_e": READER_DOMAIN_OUTPUT_FRAMES - 1,
            "scalar_active_e": scalar_read.active_start,
            "mapped_active_e": mapped_read.active_start,
            "scalar_absolute_e": scalar_start.target_frame().unwrap() + READER_DOMAIN_OUTPUT_FRAMES as u64 - 1,
            "mapped_absolute_e": mapped_read.playback.mapped_output_frame().unwrap(),
            "source_frame": mapped_position.frame,
            "fraction_bits": bits(mapped_position.fraction),
            "taps": [mapped_taps.left, mapped_taps.right.unwrap()],
            "scalar_gain_bits": format!("{:08x}", scalar_taps.right_gain.to_bits()),
            "mapped_gain_bits": format!("{:08x}", mapped_taps.right_gain.to_bits()),
            "scalar_pcm_bits": scalar_pcm.map(|value| format!("{value:08x}")),
            "mapped_pcm_bits": mapped_pcm.map(|value| format!("{value:08x}")),
        })
    );
}

#[test]
fn genuine_new_map_delivery_cancels_original_refresh_before_admission_and_after_enqueue() {
    use crate::audio_engine::accepted_timing_refresh;
    for after_enqueue in [false, true] {
        let mut path = ReaderDomainPath::new(&vec![0_i16; 131_072]);
        path.source
            .engine
            .set_pad_timing_intent(0, "automatic")
            .unwrap();
        path.drain();
        let timing = l3_constant_timing_fixture::publish_synthetic_loaded_timing(
            &path.source.engine,
            &path.source.producer,
            0,
            0.5,
        );
        path.drain();
        assert_eq!(timing.publication_status().unwrap(), "accepted");
        let (_first_job, first) = original_delivery(&path.source);
        path.publish_map_request(&first, true);
        let original = binding(&path.source, 0);
        let first_map = first.qualify_source_map(&original, None).unwrap();
        let key = original.binding.placement.unwrap().content_key().unwrap();
        let stamp = path
            .source
            .engine
            .input_runtime_ownership
            .hierarchy_stamp
            .load(std::sync::atomic::Ordering::Acquire);
        let launch = original.source_map_launch_snapshot().launch_revision();
        let before = path.source.mixer.loop_region_frames(0);
        let ticket = after_enqueue.then(|| {
            accepted_timing_refresh::enqueue(
                &path.source.engine,
                &path.source.producer,
                &original,
                0.25,
                Some(1.0),
                None,
            )
            .unwrap()
        });
        if after_enqueue {
            path.drain();
        }

        let mut changed = record(&path.source, 0);
        changed["record"]["accepted"]["anchors"][1]["source_seconds_bits"] =
            serde_json::json!(bits(80.0 / 48_000.0));
        seal(&mut changed);
        let (second_job, event) = stored_event(&path.source, &changed);
        let second = Python::attach(|py| {
            let event = event.bind(py).cast::<PyDict>().unwrap();
            let owner = event.get_item("_native_event_owner").unwrap().unwrap();
            let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
            assert!(
                second_job
                    .resolve_source_beatmap_artifact_completion(&delivery)
                    .unwrap()
                    .is_some()
            );
            delivery.clone()
        });
        let current = binding(&path.source, 0);
        let second_map = second.qualify_source_map(&current, None).unwrap();
        assert_ne!(
            first_map.map().map_revision(),
            second_map.map().map_revision()
        );
        path.publish_map_request(&second, true);
        let current = binding(&path.source, 0);
        assert_eq!(current.binding.placement.unwrap().content_key(), Some(key));
        assert_eq!(
            path.source
                .engine
                .input_runtime_ownership
                .hierarchy_stamp
                .load(std::sync::atomic::Ordering::Acquire),
            stamp
        );
        assert_ne!(
            current.source_map_launch_snapshot().launch_revision(),
            launch
        );
        if let Some(ticket) = ticket {
            path.render(2);
            assert_eq!(ticket.publication_status(), "rejected");
            assert!(!ticket.is_current());
        } else {
            // Rebased scalar authority/runtime fields cannot replace the supplied
            // capture's original map-cancellation witness with a fresh snapshot.
            let mut rebased_original = original.clone();
            rebased_original.binding = current.binding;
            assert!(rebased_original.current());
            assert!(
                accepted_timing_refresh::enqueue(
                    &path.source.engine,
                    &path.source.producer,
                    &rebased_original,
                    0.25,
                    Some(1.0),
                    None
                )
                .is_err()
            );
        }
        assert_eq!(path.source.mixer.loop_region_frames(0), before);
    }
}

#[test]
fn qualified_on_with_real_mapped_voice_noops_equal_period_and_keeps_different_period_pending() {
    use crate::audio_engine::global_rate_status::BpmReason;
    let mut path = ReaderDomainPath::new(&vec![0_i16; 131_072]);
    path.source
        .engine
        .set_pad_timing_intent(0, "automatic")
        .unwrap();
    path.drain();
    let timing = l3_constant_timing_fixture::publish_synthetic_loaded_timing(
        &path.source.engine,
        &path.source.producer,
        0,
        0.5,
    );
    path.drain();
    assert_eq!(timing.publication_status().unwrap(), "accepted");
    let original = binding(&path.source, 0);
    path.source
        .engine
        .set_bpm_lock_with_binding(true, Some(&original))
        .unwrap();
    path.drain();
    drain_parameter_messages(
        &mut path.parameters,
        &mut path.source.mixer,
        &mut path.timeline,
    );
    path.render(0);
    assert!(
        path.source
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read_bpm()
            .unwrap()
            .1
            .applied
    );
    let mut map_record = record(&path.source, 0);
    map_record["record"]["accepted"]["anchors"][1]["source_seconds_bits"] = serde_json::json!(
        bits(READER_DOMAIN_END as f64 / f64::from(READER_DOMAIN_RATE))
    );
    map_record["record"]["accepted"]["anchors"][1]["beat_bits"] = serde_json::json!(bits(2.0));
    seal(&mut map_record);
    let (job, event) = stored_event(&path.source, &map_record);
    let delivery = Python::attach(|py| {
        let event = event.bind(py).cast::<PyDict>().unwrap();
        let owner = event.get_item("_native_event_owner").unwrap().unwrap();
        let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
        assert!(
            job.resolve_source_beatmap_artifact_completion(&delivery)
                .unwrap()
                .is_some()
        );
        delivery.clone()
    });
    path.publish_map_request(&delivery, true);
    let start = path.start();
    assert!(start.mapped_geometry().is_some());
    path.render(3);
    assert!(path.source.mixer.has_mapped_voice());
    let original_owner = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap()
        .mapped_start
        .as_ref()
        .unwrap()
        .original()
        .clone();
    let before = path.timeline;
    let current = binding(&path.source, 0);
    path.source
        .engine
        .set_bpm_lock_with_binding(true, Some(&current))
        .unwrap();
    path.drain();
    drain_parameter_messages(
        &mut path.parameters,
        &mut path.source.mixer,
        &mut path.timeline,
    );
    path.render(0);
    let equal = path
        .source
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read_bpm()
        .unwrap()
        .1;
    assert!(equal.applied && equal.effective);
    assert!(path.timeline.matches_clock_epoch(&before));
    let timing = l3_constant_timing_fixture::publish_synthetic_loaded_timing(
        &path.source.engine,
        &path.source.producer,
        0,
        0.75,
    );
    path.drain();
    assert_eq!(timing.publication_status().unwrap(), "accepted");
    let current = binding(&path.source, 0);
    assert!((current.binding.accepted.unwrap().period_seconds - 0.75).abs() < 1e-12);
    path.source
        .engine
        .set_bpm_lock_with_binding(true, Some(&current))
        .unwrap();
    path.drain();
    drain_parameter_messages(
        &mut path.parameters,
        &mut path.source.mixer,
        &mut path.timeline,
    );
    path.render(0);
    let different = path
        .source
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read_bpm()
        .unwrap()
        .1;
    assert!(!different.applied && different.effective && different.revision > equal.revision);
    assert_eq!(different.reason, BpmReason::MappedActive);
    assert!(path.timeline.matches_clock_epoch(&before));
    let voice = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap();
    assert!(
        voice
            .mapped_start
            .as_ref()
            .unwrap()
            .original()
            .same_owner(&original_owner)
    );
}
