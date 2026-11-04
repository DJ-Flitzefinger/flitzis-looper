"""Output failures retain the original transaction and actual worker custody."""

from threading import Event
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, patch

import pytest

from flitzis_looper.audio_output_options import AudioOutputOptions
from flitzis_looper.controller import audio_output as output_control
from flitzis_looper.controller.audio_output import AudioOutputController
from flitzis_looper.controller.audio_output_restore import (
    capture_output_rollback,
    rescale_grid_offset,
    resolved_output,
)
from flitzis_looper.models import ProjectState, SessionState, StemCacheEntry
from flitzis_looper.stem_pair_selection import StemPairSelection

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController


def control() -> tuple[AudioOutputController, Mock]:
    app = Mock()
    app.project = ProjectState()
    app.session = SessionState()
    app.persistence.audio_output_recovery = None
    app.recording.status.return_value.retired = True
    app.audio.audio_stream_failed.return_value = False
    app._runtime_activated = False
    owner = AudioOutputController(cast("AppController", app))
    return owner, app


def test_control_worker_is_retained_until_actual_return_and_join() -> None:
    owner, _app = control()
    entered, release = Event(), Event()

    def inventory() -> dict[str, object]:
        entered.set()
        assert release.wait(2)
        return {"inventory": [{"id": "asio", "name": "ASIO", "devices": []}]}

    owner._submit("inventory", inventory)
    operation = owner._operation
    assert operation is not None
    assert operation.thread is not None
    try:
        assert entered.wait(2)
        owner.poll()
        assert owner._operation is operation
        assert not owner.inventory
    finally:
        release.set()
        operation.thread.join(2)
    assert not operation.thread.is_alive()
    owner.poll()
    assert owner._operation is None
    assert owner.inventory[0]["id"] == "asio"


def test_cancelled_preflight_reconciles_the_original_device_failure() -> None:
    owner, app = control()
    owner._epoch = 7
    app.audio.cancel_audio_output_change_before_close.return_value = True
    app.audio.audio_stream_failed.return_value = True
    owner._complete(output_control._Operation("apply", error="original file preflight failed"))
    assert owner._epoch is None
    assert not owner.available
    assert owner.phase == "device_failed"
    app.audio.run_with_output_options.assert_not_called()


def test_unknown_preclose_cancel_never_grants_physical_rollback() -> None:
    owner, app = control()
    owner._epoch = 7
    app.audio.cancel_audio_output_change_before_close.side_effect = RuntimeError("unknown effect")
    app.audio.audio_output_change_epoch.return_value = 7
    owner._complete(output_control._Operation("apply", error="preflight failed"))
    assert owner._epoch == 7
    assert owner.phase == "unavailable"
    assert owner._operation is None
    app.audio.retire_audio_output_for_change.assert_not_called()


def test_completed_preclose_cancel_is_reconciled_after_missing_receipt() -> None:
    owner, app = control()
    owner._epoch = 7
    app.audio.cancel_audio_output_change_before_close.side_effect = RuntimeError("receipt lost")
    app.audio.audio_output_change_epoch.return_value = 8
    owner._complete(output_control._Operation("apply", error="preflight failed"))
    assert owner._epoch is None
    assert owner.can_perform
    app.audio.retire_audio_output_for_change.assert_not_called()


def test_device_commit_keeps_startup_input_disabled_until_actual_source_readiness() -> None:
    owner, app = control()
    owner._commit_kind = "startup"
    owner._complete(output_control._Operation("commit", result={"sample_rate_hz": 96_000}))
    assert owner.phase == "startup_restoring"
    assert not owner.can_perform
    app.audio_output_runtime.activate.assert_called_once()
    app.audio_output_runtime.finish_startup.assert_not_called()
    app.audio_output_runtime.startup_restored.return_value = False
    owner.poll()
    app.audio_output_runtime.finish_startup.assert_not_called()
    app.audio_output_runtime.startup_restored.return_value = True
    owner.poll()
    assert owner.can_perform
    owner.poll()
    app.audio_output_runtime.finish_startup.assert_called_once()


def test_cancel_retains_epoch_and_waits_for_original_reader_end() -> None:
    owner, app = control()
    owner._epoch = 7
    owner.phase = "restoring"
    app.audio_output_runtime.require_change_idle.side_effect = RuntimeError("reader still running")
    owner.cancel()
    owner.poll()
    assert owner.phase == "cancelling"
    assert owner.busy
    assert owner._epoch == 7
    assert owner._operation is None
    app.audio.retire_audio_output_for_change.assert_not_called()


def test_newly_installed_rollback_rearms_its_own_failed_close_obligation() -> None:
    owner, app = control()
    owner._epoch = 7
    owner._failed_close_verified = True
    owner._failed_close_attempted = True
    app._runtime_activated = True
    owner._complete(output_control._Operation("rollback", result={"sample_rate_hz": 96_000}))
    assert not owner._failed_close_verified
    assert not owner._failed_close_attempted
    assert owner.phase == "rolling_back"


def test_failed_restore_drains_original_events_without_requesting_an_unhealthy_scope(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    owner, app = control()
    owner._epoch, owner.available = 7, True
    app.audio.audio_stream_failed.return_value = True

    def close_original() -> bool:
        owner.available = False
        return False

    monkeypatch.setattr(owner, "request_failed_close", close_original)
    ordinary = Mock()
    owner.poll_restore_events(ordinary)
    app.audio_output_runtime.cancel_admissions.assert_called_once()
    app._poll_audio_output_restore_events.assert_called_once()
    app.audio.with_audio_output_restore.assert_not_called()
    ordinary.assert_not_called()


def test_blocked_failed_close_keeps_original_event_polling_until_actual_worker_join() -> None:
    owner, app = control()
    owner._epoch, owner.available = 7, False
    entered, release = Event(), Event()

    def close_original() -> dict[str, object]:
        entered.set()
        assert release.wait(2)
        return {"closed": False}

    owner._submit("failure", close_original)
    operation = owner._operation
    assert operation is not None
    assert operation.thread is not None
    ordinary = Mock()
    try:
        assert entered.wait(2)
        owner.poll()
        assert owner._operation is operation
        assert not owner.exclusive_native_access
        owner.poll_restore_events(ordinary)
        app._poll_audio_output_restore_events.assert_called_once()
        app.audio.with_audio_output_restore.assert_not_called()
        ordinary.assert_not_called()
        assert not owner._failed_close_verified
    finally:
        release.set()
        operation.thread.join(2)
    assert not operation.thread.is_alive()
    owner.poll()
    assert owner._operation is None
    assert not owner._failed_close_verified


def _assigned_pair(app: Mock) -> None:
    root = f"samples/materials/M{'a' * 32}"
    pair = StemPairSelection(
        descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{'b' * 32}.json",
        stem_set_identity="b" * 64,
        wav_generation=f"{root}/stems/.ready-{'b' * 32}",
        pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{'b' * 32}",
    )
    app.project.sample_paths[0] = f"{root}/original/source.wav"
    app.project.stem_cache[0] = StemCacheEntry(
        source_version=f"{app.project.sample_paths[0]}|sha256-v1:{'c' * 64}",
        cache_dir=pair.wav_generation,
        pair=pair,
        available=False,
    )
    app.project.pad_stem_mix_mode[0] = "full_mix"


@pytest.mark.parametrize("requested_rate", [None, 96_000])
@pytest.mark.parametrize("phase", ["ready", "device_failed"])
def test_assigned_pair_crossrate_and_auto_enter_original_stopped_transaction(
    requested_rate: int | None, phase: str
) -> None:
    owner, app = control()
    _assigned_pair(app)
    owner._source_startup_completed = True
    owner.observed = {"sample_rate_hz": 48_000}
    owner.phase, owner.available = phase, phase == "ready"
    before = app.project.model_dump_json()
    with patch.object(owner, "_submit") as submit:
        assert owner.apply(AudioOutputOptions(sample_rate_hz=requested_rate))
    submit.assert_called_once()
    assert app.project.model_dump_json() == before
    assert owner.phase == "applying"
    assert owner._epoch is not None
    assert owner._operation is None
    assert owner.error is None
    app.audio_output_runtime.prepare_change.assert_called_once()
    app.audio.prepare_audio_output_restore_plan.assert_not_called()
    app.audio.retire_audio_output_for_change.assert_not_called()
    app.audio.reset_after_audio_output_retirement.assert_not_called()
    app.audio.run_with_output_options.assert_not_called()
    app.persistence.commit_audio_output.assert_not_called()


def test_pair_crossrate_partial_startup_uses_same_original_admission() -> None:
    owner, app = control()
    _assigned_pair(app)
    owner.observed = {"sample_rate_hz": 96_000}
    # Activated controllers do not mean the original Source startup completed.
    app._runtime_activated = True
    owner.phase = "startup_failed"
    with patch.object(owner, "_submit") as submit:
        assert owner.apply(AudioOutputOptions(sample_rate_hz=48_000))
    submit.assert_called_once()
    app.audio_output_runtime.prepare_change.assert_called_once()


def test_successful_partial_startup_repair_marks_pair_guard_completion() -> None:
    owner, app = control()
    app._runtime_activated = True
    owner.available = True
    owner._commit_kind = "apply"
    owner._epoch = 7
    assert not owner._source_startup_completed
    owner._finish_commit()
    assert owner._source_startup_completed


@pytest.mark.parametrize(
    ("offset", "old_rate", "new_rate", "expected"),
    [
        (7, 48_000, 96_000, 14),
        (-7, 48_000, 96_000, -14),
        (1, 96_000, 48_000, 1),
        (-1, 96_000, 48_000, -1),
        (0, 48_000, 44_100, 0),
    ],
)
def test_grid_conversion_uses_original_integer_time_and_symmetric_rounding(
    offset: int,
    old_rate: int,
    new_rate: int,
    expected: int,
) -> None:
    assert rescale_grid_offset(offset, old_rate, new_rate) == expected


def test_actual_auto_mono_geometry_remains_a_valid_rollback_request() -> None:
    request = resolved_output({
        "host": "WASAPI",
        "device_id": "original",
        "sample_rate_hz": 48_000,
        "initialized_engine_period_frames": 480,
        "physical_output_channels": 1,
        "left_channel": 0,
        "right_channel": 0,
    })
    assert request.device_id == "original"
    assert request.buffer_frames == 480
    assert request.left_channel is None
    assert request.right_channel is None


@pytest.mark.parametrize("match_format", [False, True])
def test_wasapi_rollback_retains_client_policy_and_accepted_period(
    *,
    match_format: bool,
) -> None:
    owner, app = control()
    info = {
        "host": "WASAPI",
        "device_id": "original",
        "sample_rate_hz": 48_000,
        "accepted_initialization_period_frames": 480,
        "match_engine_format": match_format,
        "left_channel": 0,
        "right_channel": 1,
        "shared_engine_initialization_snapshot": {"period_frames": 128, "sample_rate_hz": 44_100},
    }
    plan = capture_output_rollback(info)
    assert plan.options.sample_rate_hz == 48000
    assert plan.options.buffer_frames == 480
    assert plan.match_engine_format is match_format
    app.audio.output_device_descriptor.return_value = info
    owner._open(plan.options, match_engine_format=plan.match_engine_format)
    app.audio.run_with_output_options.assert_called_once_with(
        plan.options.model_dump_json(),
        match_engine_format=match_format,
    )
    assert app.project.audio_output.sample_rate_hz is None
    assert "match_engine_format" not in app.project.audio_output.model_dump()


@pytest.mark.parametrize("policy", [None, 0, 1, "false"])
def test_unknown_wasapi_rollback_policy_is_refused_before_touching_old_device(
    policy: object,
) -> None:
    with pytest.raises(ValueError, match="client format policy"):
        capture_output_rollback({
            "host": "WASAPI",
            "device_id": "original",
            "sample_rate_hz": 48_000,
            "initialized_engine_period_frames": 480,
            "match_engine_format": policy,
        })


def test_lost_final_commit_receipt_reconciles_completed_native_epoch() -> None:
    owner, app = control()
    owner._epoch = 7
    owner._commit_kind = "apply"
    app.audio.audio_output_change_epoch.return_value = 8
    with patch.object(owner, "_rollback") as rollback:
        owner._complete(output_control._Operation("commit", error="final receipt lost"))
    rollback.assert_not_called()
    assert owner._epoch is None
    assert owner.can_perform
    app.audio_output_runtime.release_completed_restore.assert_called_once()
    app.audio.retire_audio_output_for_change.assert_not_called()


def test_unknown_final_commit_epoch_retains_original_custody() -> None:
    owner, app = control()
    owner._epoch = 7
    owner._commit_kind = "apply"
    app.audio.audio_output_change_epoch.side_effect = RuntimeError("query unavailable")
    with patch.object(owner, "_rollback") as rollback:
        owner._complete(output_control._Operation("commit", error="final receipt lost"))
    rollback.assert_not_called()
    assert owner._epoch == 7
    assert owner.phase == "unavailable"
    assert not owner.can_perform
    app.audio_output_runtime.release_completed_restore.assert_not_called()
    app.audio.retire_audio_output_for_change.assert_not_called()
