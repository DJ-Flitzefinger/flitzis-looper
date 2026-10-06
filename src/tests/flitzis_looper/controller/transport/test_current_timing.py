"""Current native authority through productive Python control consumers."""

import dataclasses
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.models import BeatGrid, SampleAnalysis
from tests.flitzis_looper.conftest import current_timing_metadata

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


PRECISE_PERIOD = 0.2245048325556449


@pytest.fixture
def accepted_pad(controller: AppController, audio_engine_mock: Mock) -> dict[str, object]:
    project = controller.project
    project.sample_paths[0] = "samples/legacy-path.wav"
    project.sample_durations[0] = 600.0
    project.sample_analysis[0] = SampleAnalysis(
        bpm=135.0, key="C", beat_grid=BeatGrid(beats=[1.0], downbeats=[2.0], bars=[2.0])
    )
    project.pad_loop_auto[0] = True
    project.pad_loop_bars[0] = 1.0
    metadata = current_timing_metadata(period=PRECISE_PERIOD)
    audio_engine_mock.current_constant_timing.return_value = metadata
    audio_engine_mock.pad_timing_intent.return_value = "automatic"

    def legacy_edit(*_args: object) -> None:
        audio_engine_mock.current_constant_timing.return_value = None
        audio_engine_mock.pad_timing_intent.return_value = "legacy"

    def declare_intent(_sample_id: int, intent: str) -> None:
        audio_engine_mock.pad_timing_intent.return_value = intent

    audio_engine_mock.set_pad_bpm.side_effect = legacy_edit
    audio_engine_mock.set_pad_timing_metadata.side_effect = legacy_edit
    audio_engine_mock.set_pad_timing_intent.side_effect = declare_intent
    return metadata


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
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    controller.project.bpm_lock = True
    controller.project.speed = 1.0
    controller.transport.apply_project_state_to_audio()
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    audio_engine_mock.set_master_bpm.assert_not_called()
    audio_engine_mock.set_master_period.assert_called_with(PRECISE_PERIOD)
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
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    controller.transport.global_params.set_bpm_lock(enabled=True)
    audio_engine_mock.reset_mock()
    audio_engine_mock.current_constant_timing.return_value = None
    with pytest.raises(RuntimeError, match="not acknowledged"):
        controller.transport.global_params.set_speed(1.25)
    assert controller.project.speed == 1.0
    audio_engine_mock.set_speed.assert_not_called()
    audio_engine_mock.set_master_period.assert_not_called()


def test_locked_bpm_nudge_uses_one_source_snapshot_for_target_and_master(
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    controller.transport.global_params.set_bpm_lock(enabled=True)
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
        speed, PRECISE_PERIOD / speed
    )
    assert controller.session.bpm_lock_anchor_revision == accepted_pad["revision"]


def test_bpm_lock_speed_and_bpm_target_use_fresh_anchor_period(
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    controller.transport.global_params.set_bpm_lock(enabled=True)
    audio_engine_mock.set_master_period.assert_called_once_with(PRECISE_PERIOD)
    assert controller.transport.global_params.speed_reference_period_seconds() == PRECISE_PERIOD
    replacement_period = 0.501234567891
    audio_engine_mock.current_constant_timing.return_value = dict(
        accepted_pad, revision="replacement", period_seconds_per_quarter=replacement_period
    )
    controller.transport.global_params.set_speed(1.25)
    audio_engine_mock.set_speed_and_master_period.assert_called_with(
        1.25, replacement_period / 1.25
    )
    assert controller.session.bpm_lock_anchor_revision == "replacement"
    assert controller.transport.global_params.effective_display_bpm() == (
        60.0 / (replacement_period / 1.25)
    )
    assert controller.transport.global_params.set_effective_display_bpm(145.0)
    target_speed = replacement_period * (145.0 / 60.0)
    assert controller.project.speed == target_speed
    audio_engine_mock.set_speed_and_master_period.assert_called_with(
        target_speed, replacement_period / target_speed
    )
    audio_engine_mock.set_master_bpm.assert_not_called()
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_failed_master_admission_retains_session_publication_and_current_revision(
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    controller.transport.global_params.set_bpm_lock(enabled=True)
    before = controller.session.model_dump()
    audio_engine_mock.set_master_period.side_effect = RuntimeError("full parameter ring")
    with pytest.raises(RuntimeError, match="full parameter ring"):
        controller.transport.bpm.recompute_master_bpm()
    assert controller.session.model_dump() == before
    timing = controller.transport.bpm.current_timing(0)
    assert timing is not None
    assert timing.accepted_revision == accepted_pad["revision"]


def test_full_batch_admission_keeps_speed_master_and_current_timing_coupled(
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    controller.transport.global_params.set_bpm_lock(enabled=True)
    before = controller.session.model_dump()
    audio_engine_mock.reset_mock()
    audio_engine_mock.set_speed_and_master_period.side_effect = RuntimeError("full parameter ring")
    with pytest.raises(RuntimeError, match="full parameter ring"):
        controller.transport.global_params.set_speed(1.25)
    assert controller.project.speed == 1.0
    assert controller.session.model_dump() == before
    audio_engine_mock.set_speed.assert_not_called()
    audio_engine_mock.set_master_period.assert_not_called()
    timing = controller.transport.bpm.current_timing(0)
    assert timing is not None
    assert timing.accepted_revision == accepted_pad["revision"]


def test_manual_and_clear_retire_accepted_and_preserve_explicit_authority(
    controller: AppController, audio_engine_mock: Mock, accepted_pad: dict[str, object]
) -> None:
    controller.transport.bpm.set_manual_bpm(0, 123.4567890123)
    timing = controller.transport.bpm.current_timing(0)
    assert timing is not None
    assert timing.accepted_revision is None
    assert timing.period_seconds == 60.0 / 123.4567890123
    audio_engine_mock.set_pad_timing_intent.assert_called_with(0, "manual")
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
