//! Actual callback publication and original-source correlation, without a device.

use super::*;
use crate::audio_engine::global_rate_status::{GlobalRateStatus, RateState};
use crate::audio_engine::master_provenance::{self, MasterProvenance};

fn status(fixture: &Fixture) -> GlobalRateStatus {
    fixture
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap()
}

fn expected(binding: &InputRuntimePadBinding) -> MasterProvenance {
    MasterProvenance::qualified(
        master_provenance::source_identity(binding).unwrap(),
        binding.binding,
    )
}

fn accepted_master(fixture: &mut Fixture) -> MasterProvenance {
    let binding = fixture.binding();
    fixture
        .engine
        .set_bpm_lock_with_binding(true, Some(&binding))
        .unwrap();
    drain_bpm_fixture(fixture);
    let provenance = expected(&binding);
    assert_eq!(status(fixture).master_provenance, provenance);
    assert!(provenance.accepted_revision().is_some());
    provenance
}

#[test]
fn actual_manual_tap_and_legacy_master_replace_accepted_provenance_even_at_the_same_period() {
    for intent in ["manual", "tap", "legacy"] {
        for bpm in [119.999, 80.0] {
            let period = 60.0 / bpm;
            let mut fixture = Fixture::new();
            let previous = accepted_master(&mut fixture);
            fixture.engine.set_pad_timing_intent(0, intent).unwrap();
            drain_bpm_fixture(&mut fixture);
            fixture.mixer.set_pad_bpm(0, Some(bpm));
            let binding = fixture.binding();
            assert!(binding.binding.accepted.is_none());
            let own = expected(&binding);
            assert_eq!(own.source_identity(), previous.source_identity());
            qualified_rate(&mut fixture, None, period);
            assert_eq!(status(&fixture).master_provenance, previous);
            drain_bpm_fixture(&mut fixture);
            let applied = status(&fixture);
            assert_eq!(applied.effect.state, RateState::Applied);
            assert_eq!(applied.effect.effective.period, Some(period));
            assert_eq!(fixture.transport.master_period_seconds(), Some(period));
            assert_eq!(applied.master_provenance, own);
            assert_eq!(applied.master_provenance.accepted_revision(), None);
        }
    }
}

#[test]
fn queued_and_rejected_foreign_rate_cannot_borrow_or_clear_the_previous_master_proof() {
    let mut fixture = Fixture::new();
    let original = accepted_master(&mut fixture);
    let old_period = fixture.transport.master_period_seconds();
    qualified_rate(&mut fixture, None, PERIOD);
    fixture.engine.input_runtime_ownership.revoke(
        0,
        fixture
            .engine
            .input_runtime_ownership
            .next_authority(0)
            .unwrap(),
    );
    assert_eq!(status(&fixture).master_provenance, original);
    drain_bpm_fixture(&mut fixture);
    assert_eq!(status(&fixture).effect.state, RateState::Rejected);
    assert_eq!(status(&fixture).master_provenance, original);
    assert_eq!(fixture.transport.master_period_seconds(), old_period);
    fixture.global_parameter(ControlParameterMessage::SetSpeed(1.16));
    assert_eq!(status(&fixture).master_provenance, original);
    drain_bpm_fixture(&mut fixture);
    assert_eq!(status(&fixture).effect.state, RateState::Applied);
    assert_eq!(status(&fixture).master_provenance, original);
    assert_eq!(fixture.transport.master_period_seconds(), old_period);
}

#[test]
fn actual_unqualified_master_is_unknown_even_with_an_equal_tuple_then_speed_preserves_it() {
    let mut fixture = Fixture::new();
    let original = accepted_master(&mut fixture);
    let before = status(&fixture).effect.effective;
    fixture.global_parameter(ControlParameterMessage::SetMasterPeriod(PERIOD));
    assert_eq!(status(&fixture).master_provenance, original);
    drain_bpm_fixture(&mut fixture);
    assert_eq!(status(&fixture).effect.effective, before);
    assert_eq!(
        status(&fixture).master_provenance,
        MasterProvenance::Unknown
    );
    fixture.global_parameter(ControlParameterMessage::SetSpeed(1.16));
    drain_bpm_fixture(&mut fixture);
    assert_eq!(status(&fixture).effect.effective.speed, 1.16);
    assert_eq!(
        status(&fixture).master_provenance,
        MasterProvenance::Unknown
    );
}

#[test]
fn qualified_same_tuple_on_and_accepted_refresh_replace_proof_without_a_new_rate_receipt() {
    let mut fixture = Fixture::new();
    fixture.mixer.set_master_period(PERIOD);
    assert_eq!(
        status(&fixture).master_provenance,
        MasterProvenance::Unknown
    );
    let rate_revision = status(&fixture).request_revision;
    let before = fixture.transport;
    let original = accepted_master(&mut fixture);
    assert_eq!(status(&fixture).request_revision, rate_revision);
    assert!(fixture.transport.matches_clock_epoch(&before));
    fixture.mixer.set_master_period(PERIOD);
    assert_eq!(
        status(&fixture).master_provenance,
        MasterProvenance::Unknown
    );
    let ticket = fixture.refresh(Some(PERIOD));
    drain_bpm_fixture(&mut fixture);
    fixture.render(2);
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(status(&fixture).request_revision, rate_revision);
    assert_eq!(status(&fixture).master_provenance, original);
    let loop_only = fixture.refresh(None);
    drain_bpm_fixture(&mut fixture);
    fixture.render(2);
    assert_eq!(loop_only.publication_status(), "accepted");
    assert_eq!(status(&fixture).master_provenance, original);
    let transport = fixture.transport;
    fixture
        .engine
        .set_bpm_lock_with_binding(false, None)
        .unwrap();
    drain_bpm_fixture(&mut fixture);
    assert_eq!(status(&fixture).master_provenance, MasterProvenance::Absent);
    assert!(fixture.transport.matches_clock_epoch(&transport));
}

#[test]
fn qualified_speed_and_master_coalescing_carries_only_the_actual_original_master_proof() {
    for master_first in [false, true] {
        let mut fixture = Fixture::new();
        let original = accepted_master(&mut fixture);
        if !master_first {
            fixture.global_parameter(ControlParameterMessage::SetSpeed(1.16));
        }
        qualified_rate(&mut fixture, Some(1.16), PERIOD / 1.16);
        if master_first {
            fixture.global_parameter(ControlParameterMessage::SetSpeed(1.16));
        }
        assert_eq!(status(&fixture).master_provenance, original);
        drain_bpm_fixture(&mut fixture);
        let applied = status(&fixture);
        assert_eq!(applied.effect.state, RateState::Applied);
        assert_eq!(applied.effect.effective.speed, 1.16);
        assert_eq!(applied.effect.effective.period, Some(PERIOD / 1.16));
        assert_eq!(applied.master_provenance, original);
    }
}

#[test]
fn real_same_original_resident_window_and_context_adoption_preserves_master_proof_without_replay() {
    use crate::messages::{ResidentContext, ResidentControlIntent, ResidentTransaction};
    for context in [
        ResidentContext::FiniteLoop,
        ResidentContext::KeyLockFiniteLoop,
        ResidentContext::FullTrack,
        ResidentContext::KeyLockFullTrack,
    ] {
        let mut fixture = Fixture::with_source_identity(true);
        fixture.mixer.set_pad_loop_region(0, 0.125, Some(1.75));
        let original = accepted_master(&mut fixture);
        let before = status(&fixture);
        let old_binding = fixture.binding();
        let full = fixture.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap();
        let (start, end) = if context.permits_finite_range() {
            (1_000, 14_000)
        } else {
            (0, full.frame_count())
        };
        let window = full.window(start, end, 2, context).unwrap();
        let permit = PreparedSourcePermit::unrestricted();
        permit.mark_pending().unwrap();
        let transport = fixture.transport;
        fixture.apply(
            ControlMessage::RelocateResident(Box::new(ResidentTransaction {
                id: 0,
                sample: window.clone(),
                stems: None,
                binding: old_binding.binding,
                publication: permit.clone(),
                expected_window_revision: 1,
                intent: ResidentControlIntent {
                    key_lock: Some(true),
                    ..Default::default()
                },
                seek_pin: None,
            })),
            &mut ImmediateAudioBufferRetirement,
        );
        assert_eq!(permit.status(), "accepted");
        fixture.engine.sample_cache.lock().unwrap()[0] = Some(window);
        let current = fixture.binding();
        assert!(current.current() && current.available());
        assert_ne!(current.binding.resident, old_binding.binding.resident);
        assert_eq!(
            master_provenance::source_identity(&current),
            original.source_identity()
        );
        let after = status(&fixture);
        assert_eq!(
            (after.request_revision, after.effect.revision),
            (before.request_revision, before.effect.revision)
        );
        assert_eq!(after.effect.effective, before.effect.effective);
        assert_eq!(after.master_provenance, original);
        assert!(fixture.transport.matches_clock_epoch(&transport));
        assert!(fixture.consumer.is_empty() && fixture.parameter_consumer.is_empty());
    }
}

#[test]
fn current_actual_refresh_is_the_next_speed_only_master_base_without_borrowing_receipt_identity() {
    let mut fixture = Fixture::new();
    accepted_master(&mut fixture);
    fixture.global_parameter(ControlParameterMessage::SetSpeed(1.16));
    drain_bpm_fixture(&mut fixture);
    // The qualified refresh uses the actual speed, but keeps the rate receipt
    // identity. A later speed-only request must retain this newer actual period.
    let period = PERIOD / 1.16;
    let ticket = fixture.refresh(Some(period));
    drain_bpm_fixture(&mut fixture);
    fixture.render(2);
    assert_eq!(ticket.publication_status(), "accepted");
    let before = status(&fixture);
    fixture.global_parameter(ControlParameterMessage::SetSpeed(1.25));
    drain_bpm_fixture(&mut fixture);
    let after = status(&fixture);
    assert_eq!(after.effect.state, RateState::Applied);
    assert_eq!(after.effect.effective.period, Some(period));
    assert_eq!(fixture.transport.master_period_seconds(), Some(period));
    assert_eq!(after.master_provenance, before.master_provenance);
}

fn unqualified_master_waiting_for_claim(fixture: &mut Fixture) {
    fixture.global_parameter(ControlParameterMessage::SetMasterPeriod(0.75));
    let ownership = fixture.engine.input_runtime_ownership.clone();
    ownership
        .global_timing_publishing
        .store(true, Ordering::Release);
    fixture.parameters();
    ownership
        .global_timing_publishing
        .store(false, Ordering::Release);
    assert_eq!(status(fixture).effect.state, RateState::Preparing);
    assert!(fixture.mixer.has_pending_global_rate());
}

#[test]
fn terminal_unqualified_refusal_does_not_revive_its_master_during_a_later_speed_only_request() {
    for terminal_lease_busy in [false, true] {
        let mut fixture = Fixture::new();
        let proof = accepted_master(&mut fixture);
        unqualified_master_waiting_for_claim(&mut fixture);
        let ownership = fixture.engine.input_runtime_ownership.clone();
        let before = status(&fixture).effect.effective;
        let transport = fixture.transport;
        ownership
            .global_timing_publishing
            .store(terminal_lease_busy, Ordering::Release);
        fixture.mixer.set_speed(1.25);
        assert_eq!(status(&fixture).effect.effective, before);
        assert_eq!(status(&fixture).master_provenance, proof);
        assert!(fixture.transport.matches_clock_epoch(&transport));
        if terminal_lease_busy {
            assert!(fixture.mixer.has_pending_global_rate());
            fixture.render(0);
            assert_eq!(status(&fixture).effect.effective, before);
            ownership
                .global_timing_publishing
                .store(false, Ordering::Release);
            fixture.render(0);
        }
        let rejected = status(&fixture);
        assert_eq!(rejected.effect.state, RateState::Rejected);
        assert_eq!(
            rejected.effect.error,
            crate::audio_engine::global_rate_status::RateError::OtherRatePending
        );
        assert_eq!(rejected.effect.requested.period, Some(0.75));
        assert!(!fixture.mixer.has_pending_global_rate());
        fixture.global_parameter(ControlParameterMessage::SetSpeed(1.16));
        assert_eq!(status(&fixture).requested.period, before.period);
        drain_bpm_fixture(&mut fixture);
        let applied = status(&fixture);
        assert_eq!(applied.effect.state, RateState::Applied);
        assert_eq!(applied.effect.effective.speed, 1.16);
        assert_eq!(applied.effect.effective.period, before.period);
        assert_eq!(applied.master_provenance, proof);
        assert!(fixture.transport.matches_clock_epoch(&transport));
    }
}

#[test]
fn speed_admitted_while_master_is_pending_keeps_its_original_prefix_until_real_parameter_drain() {
    let mut fixture = Fixture::new();
    let proof = accepted_master(&mut fixture);
    unqualified_master_waiting_for_claim(&mut fixture);
    let before = status(&fixture).effect.effective;
    fixture.global_parameter(ControlParameterMessage::SetSpeed(1.16));
    let queued = status(&fixture);
    assert_eq!(queued.requested.period, Some(0.75));
    assert!(queued.request_revision > queued.effect.revision);
    // Render sees the newer admission first. Only its original parameter message
    // may replace the coalesced master owner, never the observation values.
    fixture.render(0);
    assert!(fixture.mixer.has_pending_global_rate());
    assert_eq!(status(&fixture).effect.effective, before);
    assert_eq!(status(&fixture).master_provenance, proof);
    drain_bpm_fixture(&mut fixture);
    let applied = status(&fixture);
    assert_eq!(applied.effect.state, RateState::Applied);
    assert_eq!(applied.effect.effective.speed, 1.16);
    assert_eq!(applied.effect.effective.period, Some(0.75));
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.75));
    assert_eq!(applied.master_provenance, MasterProvenance::Unknown);
}

#[test]
fn native_metadata_and_status_expose_the_same_canonical_proof_without_an_engine_instance() {
    assert_eq!(AudioEngine::GLOBAL_RATE_PROVENANCE_API_VERSION, 1);
    let mut fixture = Fixture::new();
    let original = accepted_master(&mut fixture);
    Python::attach(|py| {
        let binding = fixture.binding();
        let metadata = binding.metadata(py).unwrap();
        let metadata = metadata.bind(py).cast::<PyDict>().unwrap();
        let dto = fixture.engine.global_rate_status(py).unwrap().unwrap();
        let dto = dto.bind(py);
        let proof = dto.get_item("master_provenance").unwrap().unwrap();
        let proof = proof.cast::<PyDict>().unwrap();
        assert_eq!(
            proof
                .get_item("kind")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "qualified"
        );
        let source = proof
            .get_item("source_identity")
            .unwrap()
            .unwrap()
            .extract::<String>()
            .unwrap();
        assert_eq!(source.len(), 64);
        assert_eq!(
            metadata
                .get_item("source_identity")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            source
        );
        assert_eq!(
            proof
                .get_item("accepted_revision")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            master_provenance::accepted_revision(original.accepted_revision().unwrap())
        );
        let native = py.get_type::<AudioEngine>();
        assert_eq!(
            native
                .getattr("GLOBAL_RATE_PROVENANCE_API_VERSION")
                .unwrap()
                .extract::<u32>()
                .unwrap(),
            1
        );
    });
}
