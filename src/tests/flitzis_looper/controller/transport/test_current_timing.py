"""Current native authority through productive Python control consumers."""

import dataclasses
from typing import TYPE_CHECKING, cast
from unittest.mock import ANY, Mock

import pytest

from flitzis_looper.models import BeatGrid, PadContentIdentity, SampleAnalysis
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding, current_timing_metadata
from tests.flitzis_looper.controller.test_timing_hierarchy import (
    BPM_REFERENCE_SOURCE_IDENTITY,
    _master_provenance,
)

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import InputRuntimePadBinding


PRECISE_PERIOD = 0.2245048325556449


@pytest.fixture
def accepted_pad(controller: AppController, audio_engine_mock: Mock) -> dict[str, object]:
    project = controller.project
    project.multi_loop = True
    project.sample_paths[0] = "samples/legacy-path.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    project.pad_timing_intent[0] = "automatic"
    project.sample_durations[0] = 600.0
    project.sample_analysis[0] = SampleAnalysis(
        bpm=135.0, key="C", beat_grid=BeatGrid(beats=[1.0], downbeats=[2.0], bars=[2.0])
    )
    project.pad_loop_auto[0] = True
    project.pad_loop_bars[0] = 1.0
    metadata = current_timing_metadata(period=PRECISE_PERIOD)
    audio_engine_mock.current_constant_timing.return_value = metadata
    audio_engine_mock.pad_timing_intent.return_value = "automatic"

    def current_binding(sample_id: int) -> FakeInputRuntimePadBinding:
        current = audio_engine_mock.current_constant_timing.return_value
        accepted = current if isinstance(current, dict) else None
        binding = FakeInputRuntimePadBinding(
            sample_id,
            accepted_timing=accepted,
            intent=audio_engine_mock.pad_timing_intent.return_value,
            source_identity=BPM_REFERENCE_SOURCE_IDENTITY,
        )
        if accepted is None:
            for key in (
                "source_id",
                "source_generation",
                "source_sha256",
                "sample_rate_hz",
                "frame_count",
            ):
                binding._metadata[key] = metadata[key]
        return binding

    audio_engine_mock.current_input_runtime_pad_binding.side_effect = current_binding

    def legacy_edit(*_args: object, expected_source_generation: int | None = None) -> None:
        audio_engine_mock.current_constant_timing.return_value = None
        audio_engine_mock.pad_timing_intent.return_value = "legacy"

    def declare_intent(
        _sample_id: int, intent: str, *, expected_source_generation: int | None = None
    ) -> None:
        audio_engine_mock.pad_timing_intent.return_value = intent

    audio_engine_mock.set_pad_bpm.side_effect = legacy_edit
    audio_engine_mock.set_pad_timing_metadata.side_effect = legacy_edit
    audio_engine_mock.set_pad_timing_intent.side_effect = declare_intent
    return metadata


@pytest.fixture
def bpm_runtime(audio_engine_mock: Mock, accepted_pad: dict[str, object]) -> None:
    """Model explicit receipts only; actual Native source/audio guards are tested separately."""
    receipt: dict[str, object] = {
        "request_revision": 0,
        "effect_revision": 0,
        "state": "applied",
        "requested_enabled": False,
        "effective_enabled": False,
        "reason": None,
        "execution_frame": None,
        "submission_available": True,
    }
    rate: dict[str, object] = {
        "request_revision": 0,
        "effect_revision": 0,
        "state": "applied",
        "requested_speed": 1.0,
        "requested_master_period": None,
        "effective_speed": 1.0,
        "effective_master_period": None,
        "execution_frame": None,
        "error": None,
        "master_provenance": _master_provenance(None, kind="absent"),
    }

    def admit(*, enabled: bool, anchor_binding: InputRuntimePadBinding | None = None) -> None:
        if enabled:
            assert anchor_binding is not None
            assert anchor_binding.metadata()["accepted_timing"] == accepted_pad
        receipt.update(
            request_revision=cast("int", receipt["request_revision"]) + 1,
            state="queued",
            requested_enabled=enabled,
            execution_frame=None,
        )

    audio_engine_mock.bpm_lock_status = Mock(return_value=receipt)
    audio_engine_mock.set_bpm_lock = Mock(side_effect=admit)
    audio_engine_mock.global_rate_status = Mock(return_value=rate)
    audio_engine_mock._content_placement_pending = Mock(return_value=False)


def _acknowledge_rate(
    controller: AppController, audio: Mock, *, speed: float, period: float
) -> None:
    feedback = audio.global_rate_status.return_value
    assert isinstance(feedback, dict)
    revision = feedback["request_revision"] + 1
    binding = audio.current_input_runtime_pad_binding(0)
    metadata = binding.metadata()
    accepted = metadata["accepted_timing"]
    feedback.update(
        request_revision=revision,
        effect_revision=revision,
        state="applied",
        requested_speed=speed,
        requested_master_period=period,
        effective_speed=speed,
        effective_master_period=period,
        execution_frame=64 * revision,
        error=None,
        master_provenance=_master_provenance(
            cast("str", metadata["source_identity"]),
            cast("str", accepted["revision"]) if isinstance(accepted, dict) else None,
        ),
    )
    controller.transport.global_params._poll_global_rate_status()


def _acknowledge_bpm(controller: AppController, audio: Mock) -> None:
    receipt = audio.bpm_lock_status.return_value
    assert isinstance(receipt, dict)
    receipt.update(
        effect_revision=receipt["request_revision"],
        state="applied",
        effective_enabled=True,
        execution_frame=64,
    )
    # Native initial ON owns the coupled period/bootstrap effect before its flag ACK.
    _acknowledge_rate(controller, audio, speed=1.0, period=PRECISE_PERIOD)
    controller.transport.global_params.poll_bpm_lock_application()


@pytest.fixture
def enabled_bpm(controller: AppController, audio_engine_mock: Mock, bpm_runtime: None) -> None:
    controller.transport.global_params.set_bpm_lock(enabled=True)
    _acknowledge_bpm(controller, audio_engine_mock)


def test_resolver_retains_complete_identity_without_bpm_roundtrip(
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    assert 60.0 / (60.0 / PRECISE_PERIOD) != PRECISE_PERIOD
    timing = controller.transport.bpm.current_timing(0)
    assert timing is not None
    assert timing.period_seconds == PRECISE_PERIOD
    assert timing.origin_seconds == -0.125
    assert timing.sample_rate_hz == 48_000
    assert timing.accepted_revision == accepted_pad["revision"]
    assert timing.origin_provenance == accepted_pad["origin_provenance"]
    identity = timing.accepted_identity
    assert identity is not None
    for field in dataclasses.fields(identity):
        assert getattr(identity, field.name) == accepted_pad[field.name]
    with pytest.raises(dataclasses.FrozenInstanceError):
        timing.period_seconds = 0.5  # type: ignore[misc]
    audio_engine_mock.current_constant_timing.assert_called_once_with(0)
    assert controller.transport.bpm.effective_bpm(0) == 60.0 / PRECISE_PERIOD


def test_resolver_uses_replacement_revision_and_never_caches_historical_authority(
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    first = controller.transport.bpm.current_timing(0)
    assert first is not None
    # Pending or rejected replacement leaves the native current return unchanged.
    assert controller.transport.bpm.current_timing(0) == first
    replacement = dict(accepted_pad, revision="same-period-new-evidence", publication_epoch=12)
    audio_engine_mock.current_constant_timing.return_value = replacement
    second = controller.transport.bpm.current_timing(0)
    assert second is not None
    assert second.period_seconds == first.period_seconds
    assert second.accepted_revision != first.accepted_revision
    assert second.accepted_identity != first.accepted_identity
    audio_engine_mock.current_constant_timing.return_value = None
    assert controller.transport.bpm.current_timing(0) is None
    assert controller.transport.bpm.effective_bpm(0) is None
    audio_engine_mock.pad_timing_intent.return_value = "legacy"
    controller.project.pad_timing_intent[0] = "legacy"
    legacy = controller.transport.bpm.current_timing(0)
    assert legacy is not None
    assert legacy.accepted_revision is None
    assert legacy.period_seconds == 60.0 / 135.0
    assert legacy.origin_seconds == 2.0


@pytest.mark.parametrize(
    ("key", "value"),
    [
        ("pad_id", 1),
        ("period_seconds_per_quarter", 0.0),
        ("revision", ""),
        ("origin_seconds", float("nan")),
        ("sample_rate_hz", 0),
        ("pcm_sha256", None),
    ],
)
def test_invalid_current_contract_cannot_silently_fall_back_to_old_estimate(
    controller: AppController,
    audio_engine_mock: Mock,
    accepted_pad: dict[str, object],
    key: str,
    value: object,
) -> None:
    audio_engine_mock.current_constant_timing.return_value = dict(accepted_pad, **{key: value})
    with pytest.raises(RuntimeError, match="current timing"):
        controller.transport.bpm.current_timing(0)
    audio_engine_mock.set_pad_bpm.assert_not_called()


def test_restore_and_refresh_preserve_acknowledged_exact_current_timing(
    controller: AppController,
    audio_engine_mock: Mock,
    accepted_pad: dict[str, object],
    bpm_runtime: None,
) -> None:
    controller.project.bpm_lock = True
    controller.project.speed = 1.0
    controller.transport.apply_project_state_to_audio()
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    audio_engine_mock.set_master_bpm.assert_not_called()
    audio_engine_mock.set_master_period.assert_not_called()
    audio_engine_mock.set_bpm_lock.assert_called_once_with(enabled=True, anchor_binding=ANY)
    assert controller.session.master_period_seconds is None
    _acknowledge_bpm(controller, audio_engine_mock)
    assert controller.session.master_period_seconds == PRECISE_PERIOD
    assert controller.session.bpm_lock_anchor_revision == accepted_pad["revision"]
    assert controller.transport.loop.grid_anchor_sec(0) == -0.125


def test_unacknowledged_automatic_refresh_cannot_replay_legacy_state(
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    controller.project.bpm_lock = True
    controller.session.bpm_lock_anchor_pad_id = 0
    controller.session.master_period_seconds = PRECISE_PERIOD
    controller.session.master_bpm = 60.0 / PRECISE_PERIOD
    controller.session.bpm_lock_anchor_revision = str(accepted_pad["revision"])
    audio_engine_mock.current_constant_timing.return_value = None
    controller.transport.bpm.on_pad_bpm_changed(0)
    controller.transport.bpm.recompute_master_bpm()
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.set_master_period.assert_not_called()
    audio_engine_mock.set_master_bpm.assert_not_called()
    assert controller.session.master_period_seconds == PRECISE_PERIOD
    assert controller.session.bpm_lock_anchor_revision == accepted_pad["revision"]


def test_restore_unavailable_automatic_timing_retains_native_loop(
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    audio_engine_mock.current_constant_timing.return_value = None
    controller.transport.apply_project_state_to_audio()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_speed_rejects_unacknowledged_anchor_before_saving_or_publishing(
    controller: AppController,
    audio_engine_mock: Mock,
    accepted_pad: dict[str, object],
    enabled_bpm: None,
) -> None:
    before = controller.session.model_dump()
    audio_engine_mock.reset_mock()
    audio_engine_mock.current_constant_timing.return_value = None
    with pytest.raises(RuntimeError, match="not acknowledged"):
        controller.transport.global_params.set_speed(1.25)
    assert controller.project.speed == 1.0
    audio_engine_mock.set_speed.assert_not_called()
    audio_engine_mock.set_master_period.assert_not_called()
    audio_engine_mock.set_speed_and_master_period.assert_not_called()
    assert controller.session.model_dump() == before
    audio_engine_mock.current_constant_timing.assert_called_once_with(0)


def test_pending_bpm_on_rejects_speed_even_before_actual_flag_changes(
    controller: AppController, audio_engine_mock: Mock, bpm_runtime: None
) -> None:
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    audio_engine_mock.reset_mock()
    with pytest.raises(RuntimeError, match="not acknowledged"):
        parameters.set_speed(1.25)
    assert controller.project.speed == 1.0
    assert controller.session.master_period_seconds is None
    audio_engine_mock.current_constant_timing.assert_not_called()
    audio_engine_mock.set_speed.assert_not_called()
    audio_engine_mock.set_speed_and_master_period.assert_not_called()


def test_pending_bpm_off_retains_actual_master_and_refuses_unqualified_speed(
    controller: AppController, audio_engine_mock: Mock, enabled_bpm: None
) -> None:
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=False)
    audio_engine_mock.reset_mock()
    with pytest.raises(RuntimeError, match="not acknowledged"):
        parameters.set_speed(1.25)
    assert controller.project.bpm_lock is False
    assert controller.project.speed == 1.0
    assert controller.session.master_period_seconds == PRECISE_PERIOD
    audio_engine_mock.current_constant_timing.assert_not_called()
    audio_engine_mock.set_speed.assert_not_called()
    audio_engine_mock.set_speed_and_master_period.assert_not_called()


def test_locked_bpm_nudge_uses_one_source_snapshot_for_target_and_master(
    controller: AppController,
    audio_engine_mock: Mock,
    accepted_pad: dict[str, object],
    enabled_bpm: None,
) -> None:
    audio_engine_mock.reset_mock()
    audio_engine_mock.current_constant_timing.side_effect = [
        accepted_pad,
        dict(accepted_pad, period_seconds_per_quarter=0.75, revision="later"),
    ]
    controller.transport.global_params.nudge_speed_by_bpm_steps(1)
    target_bpm = round(60.0 / PRECISE_PERIOD + 0.1, 2)
    speed = PRECISE_PERIOD * (target_bpm / 60.0)
    assert controller.project.speed == speed
    audio_engine_mock.current_constant_timing.assert_called_once_with(0)
    audio_engine_mock.set_speed_and_master_period.assert_called_once_with(
        speed, PRECISE_PERIOD / speed, anchor_binding=ANY
    )
    binding = audio_engine_mock.set_speed_and_master_period.call_args.kwargs["anchor_binding"]
    assert binding.metadata()["accepted_timing"] == accepted_pad
    assert controller.session.master_period_seconds == PRECISE_PERIOD
    assert controller.session.bpm_lock_anchor_revision == accepted_pad["revision"]


def test_changed_binding_refuses_one_snapshot_speed_without_mixed_or_unqualified_effect(
    controller: AppController,
    audio_engine_mock: Mock,
    accepted_pad: dict[str, object],
    enabled_bpm: None,
) -> None:
    before = controller.session.model_dump()
    audio_engine_mock.reset_mock()
    replacement = dict(accepted_pad, period_seconds_per_quarter=0.75, revision="later")
    audio_engine_mock.current_constant_timing.side_effect = [accepted_pad, replacement]
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = FakeInputRuntimePadBinding(
        accepted_timing=replacement, intent="automatic"
    )
    with pytest.raises(RuntimeError, match="reference changed"):
        controller.transport.global_params.nudge_speed_by_bpm_steps(1)
    audio_engine_mock.current_constant_timing.assert_called_once_with(0)
    audio_engine_mock.set_speed_and_master_period.assert_not_called()
    audio_engine_mock.set_speed.assert_not_called()
    audio_engine_mock.set_master_period.assert_not_called()
    assert controller.project.speed == 1.0
    assert controller.session.model_dump() == before


def test_bpm_lock_speed_and_bpm_target_use_fresh_anchor_period(
    controller: AppController,
    audio_engine_mock: Mock,
    accepted_pad: dict[str, object],
    enabled_bpm: None,
) -> None:
    audio_engine_mock.set_master_period.assert_not_called()
    assert controller.session.master_period_seconds == PRECISE_PERIOD
    assert controller.transport.global_params._speed_reference_period_seconds() == PRECISE_PERIOD
    replacement_period = 0.501234567891
    audio_engine_mock.current_constant_timing.return_value = dict(
        accepted_pad, revision="replacement", period_seconds_per_quarter=replacement_period
    )
    controller.transport.global_params.set_speed(1.25)
    audio_engine_mock.set_speed_and_master_period.assert_called_with(
        1.25, replacement_period / 1.25, anchor_binding=ANY
    )
    binding = audio_engine_mock.set_speed_and_master_period.call_args.kwargs["anchor_binding"]
    assert binding.metadata()["accepted_timing"]["revision"] == "replacement"
    assert controller.session.master_period_seconds == PRECISE_PERIOD
    assert controller.session.bpm_lock_anchor_revision == accepted_pad["revision"]
    _acknowledge_rate(controller, audio_engine_mock, speed=1.25, period=replacement_period / 1.25)
    assert controller.session.master_period_seconds == replacement_period / 1.25
    assert controller.transport.global_params.effective_display_bpm() == (
        60.0 / (replacement_period / 1.25)
    )
    assert controller.transport.global_params.set_effective_display_bpm(145.0)
    target_speed = replacement_period * (145.0 / 60.0)
    assert controller.project.speed == target_speed
    audio_engine_mock.set_speed_and_master_period.assert_called_with(
        target_speed, replacement_period / target_speed, anchor_binding=ANY
    )
    assert controller.session.master_period_seconds == replacement_period / 1.25
    _acknowledge_rate(
        controller, audio_engine_mock, speed=target_speed, period=replacement_period / target_speed
    )
    assert controller.session.master_period_seconds == replacement_period / target_speed
    refresh = Mock()
    refresh.publication_status.return_value = "pending"
    refresh.is_current.return_value = True
    audio_engine_mock.refresh_current_constant_timing.return_value = refresh
    controller.accepted_timing.refresh_current(0)
    controller.accepted_timing.poll()
    # The preceding qualified speed/master ACK already published this revision.
    assert controller.session.bpm_lock_anchor_revision == "replacement"
    refresh.publication_status.return_value = "accepted"
    controller.accepted_timing.poll()
    assert controller.session.bpm_lock_anchor_revision == "replacement"
    audio_engine_mock.set_master_bpm.assert_not_called()
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_failed_master_admission_retains_session_publication_and_current_revision(
    controller: AppController,
    audio_engine_mock: Mock,
    accepted_pad: dict[str, object],
    enabled_bpm: None,
) -> None:
    before = controller.session.model_dump(exclude={"sample_analysis_errors"})
    before_offset = controller.project.pad_grid_offset_samples[0]
    owner = controller.accepted_timing
    owner.refresh_current(0)
    request = owner._waiting_refresh[0]
    audio_engine_mock.refresh_current_constant_timing.side_effect = RuntimeError(
        "full parameter ring"
    )
    owner.poll()
    audio_engine_mock.refresh_current_constant_timing.assert_called_once_with(
        ANY, 0.0, round(PRECISE_PERIOD * 4.0 * 48_000) / 48_000, PRECISE_PERIOD
    )
    binding = audio_engine_mock.refresh_current_constant_timing.call_args.args[0]
    assert binding.metadata()["accepted_timing"] == accepted_pad
    assert owner._waiting_refresh[0] is request
    assert request.admission_failures == 1
    assert controller.session.sample_analysis_errors[0] == "full parameter ring"
    assert controller.session.model_dump(exclude={"sample_analysis_errors"}) == before
    assert controller.project.pad_grid_offset_samples[0] == before_offset
    audio_engine_mock.set_master_period.assert_not_called()
    audio_engine_mock.set_master_bpm.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    timing = controller.transport.bpm.current_timing(0)
    assert timing is not None
    assert timing.accepted_revision == accepted_pad["revision"]


def test_full_batch_admission_keeps_speed_master_and_current_timing_coupled(
    controller: AppController,
    audio_engine_mock: Mock,
    accepted_pad: dict[str, object],
    enabled_bpm: None,
) -> None:
    before = controller.session.model_dump()
    audio_engine_mock.reset_mock()
    audio_engine_mock.set_speed_and_master_period.side_effect = RuntimeError("full parameter ring")
    with pytest.raises(RuntimeError, match="full parameter ring"):
        controller.transport.global_params.set_speed(1.25)
    assert controller.project.speed == 1.0
    assert controller.session.model_dump() == before
    audio_engine_mock.set_speed_and_master_period.assert_called_once_with(
        1.25, PRECISE_PERIOD / 1.25, anchor_binding=ANY
    )
    binding = audio_engine_mock.set_speed_and_master_period.call_args.kwargs["anchor_binding"]
    assert binding.metadata()["accepted_timing"] == accepted_pad
    audio_engine_mock.set_speed.assert_not_called()
    audio_engine_mock.set_master_period.assert_not_called()
    timing = controller.transport.bpm.current_timing(0)
    assert timing is not None
    assert timing.accepted_revision == accepted_pad["revision"]


def test_manual_and_clear_retire_accepted_and_preserve_explicit_authority(
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    controller.transport.bpm.set_manual_bpm(0, 123.4567890123)
    audio_engine_mock.set_pad_bpm.assert_called_once_with(
        0, 123.4567890123, expected_source_generation=accepted_pad["source_generation"]
    )
    timing = controller.transport.bpm.current_timing(0)
    assert timing is not None
    assert timing.accepted_revision is None
    assert timing.period_seconds == 60.0 / 123.4567890123
    audio_engine_mock.set_pad_timing_intent.assert_called_with(
        0, "manual", expected_source_generation=accepted_pad["source_generation"]
    )
    assert audio_engine_mock.pad_timing_intent(0) == "manual"
    controller.transport.bpm.clear_manual_bpm(0)
    assert controller.transport.bpm.effective_bpm(0) == 135.0
    assert audio_engine_mock.pad_timing_intent(0) == "legacy"
    audio_engine_mock.pad_timing_intent.return_value = "automatic"
    assert controller.transport.bpm.current_timing(0) is None


def test_tap_authority_follows_all_legacy_numeric_publications(
    controller: AppController,
    audio_engine_mock: Mock,
    accepted_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    taps = iter([0.0, 0.5])
    monkeypatch.setattr("flitzis_looper.controller.transport.bpm.monotonic", lambda: next(taps))
    assert controller.transport.bpm.tap_bpm(0) is None
    assert controller.transport.bpm.tap_bpm(0) == 120.0
    audio_engine_mock.set_pad_bpm.assert_called_once_with(
        0, 120.0, expected_source_generation=accepted_pad["source_generation"]
    )
    audio_engine_mock.set_pad_timing_intent.assert_called_with(
        0, "tap", expected_source_generation=accepted_pad["source_generation"]
    )
    assert audio_engine_mock.pad_timing_intent(0) == "tap"
    timing = controller.transport.bpm.current_timing(0)
    assert timing is not None
    assert timing.accepted_revision is None


def test_failed_manual_admission_retains_both_current_and_saved_intent(
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    before = controller.transport.bpm.current_timing(0)
    audio_engine_mock.set_pad_bpm.side_effect = RuntimeError("full parameter ring")
    with pytest.raises(RuntimeError, match="full parameter ring"):
        controller.transport.bpm.set_manual_bpm(0, 120.0)
    assert controller.project.manual_bpm[0] is None
    assert controller.transport.bpm.current_timing(0) == before
    audio_engine_mock.set_pad_timing_intent.assert_not_called()


def test_state_restore_manual_intent_overrides_native_automatic_record(
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    controller.project.manual_bpm[0] = 90.0
    controller.transport.apply_project_state_to_audio()
    assert controller.transport.bpm.effective_bpm(0) == 90.0
    audio_engine_mock.set_pad_bpm.assert_called_with(0, 90.0)
    assert audio_engine_mock.pad_timing_intent(0) == "manual"
