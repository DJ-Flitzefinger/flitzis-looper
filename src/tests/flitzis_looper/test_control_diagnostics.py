"""Console reporting preserves failed control attempts without frame-wise repetition."""

import re
import threading
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.control_diagnostics import ConsoleDiagnostics, poll_control_diagnostics
from flitzis_looper.controller.stem_generation import StemGenerationResult
from flitzis_looper.input_mapping.actions import describe_pitch_rejection
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.controller.test_stem_batch import _ControlledStems
from tests.flitzis_looper.controller.test_stem_delete import _ControlledDelete
from tests.flitzis_looper.controller.test_stem_delete_inventory import _metadata
from tests.flitzis_looper.controller.transport.test_pitch_actions import (
    _CapturedPitch,
    _pitch_fixture,
)
from tests.flitzis_looper.controller.transport.test_residency import resident
from tests.flitzis_looper.input_mapping.test_pitch_actions import _pitch_event
from tests.flitzis_looper.ui.test_context import _record_original_starts

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_batch_inventory import StemBatchInventory


def test_unchanged_poll_is_reported_once_and_recovery_allows_same_failure(
    capsys: pytest.CaptureFixture[str],
) -> None:
    sink = ConsoleDiagnostics()
    for _ in range(12):
        sink.observe("pad_start", "prepared_mismatch", pad_id=2, request=11)
    first = capsys.readouterr().err.splitlines()
    assert len(first) == 1
    assert re.match(r"\[\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d", first[0])
    assert "operation=pad_start PAD=3 request=11 reason=prepared_mismatch" in first[0]

    sink.observe("pad_start", None, pad_id=2)
    sink.observe("pad_start", "prepared_mismatch", pad_id=2, request=11)
    sink.observe("pad_start", "prepared_mismatch", pad_id=2, request=12)

    assert len(capsys.readouterr().err.splitlines()) == 2


def test_private_source_and_unknown_absolute_paths_are_redacted(
    capsys: pytest.CaptureFixture[str],
) -> None:
    source = r"D:\Private Sessions\Unreleased title.wav"
    sink = ConsoleDiagnostics(lambda: (source,))
    sink.observe("load", f"Cold load canceled: '{source}'", pad_id=0)
    sink.observe("analysis", "Decoder refused '/home/person/private demo.wav'", pad_id=0)
    sink.observe("stem_generation", r"Cannot open C:\another private folder\secret.wav", pad_id=0)
    sink.observe("load", "Invalid\nreason\r\n" + "x" * 2000, pad_id=1)

    lines = capsys.readouterr().err.splitlines()
    assert len(lines) == 4
    assert "Cold load canceled: '[private source]'" in lines[0]
    for line in lines:
        assert "Private Sessions" not in line
        assert "Unreleased" not in line
        assert "private demo" not in line
        assert "secret.wav" not in line
    assert len(lines[-1].split("reason=", 1)[1]) == 1000


def test_console_sink_rejects_calls_from_another_thread() -> None:
    sink = ConsoleDiagnostics()
    errors: list[str] = []

    def wrong_thread() -> None:
        try:
            sink.observe("callback", "must not print")
        except RuntimeError as error:
            errors.append(str(error))

    thread = threading.Thread(target=wrong_thread)
    thread.start()
    thread.join()
    assert errors == ["Console diagnostics belong to their Python control thread"]


def test_new_failed_pad_gesture_logs_again_but_held_repetitions_do_not(
    controller: AppController,
    audio_engine_mock: Mock,
    capsys: pytest.CaptureFixture[str],
) -> None:
    ctx = UiContext(controller)
    controller.project.sample_paths[0] = "samples/synthetic-loaded.wav"
    controller.session.active_sample_ids.add(0)
    audio_engine_mock.cancel_pad_launches.side_effect = RuntimeError("prepared_mismatch")
    before_project = controller.project.model_dump()
    before_session = controller.session.model_dump()

    ctx.audio.pads.stop_pad(0)
    for _ in range(10):
        ctx.audio.pads.stop_pad(0, continuous=True)

    first = capsys.readouterr().err.splitlines()
    assert len(first) == 1
    assert "operation=stop_pad PAD=1 reason=prepared_mismatch" in first[0]
    assert controller.project.model_dump() == before_project
    assert controller.session.model_dump() == before_session
    audio_engine_mock.stop_sample.assert_not_called()

    ctx.audio.pads.stop_pad(0)

    assert len(capsys.readouterr().err.splitlines()) == 1
    audio_engine_mock.cancel_pad_launches.side_effect = None
    ctx.audio.pads.stop_pad(0)
    audio_engine_mock.stop_sample.assert_called_once_with(0)
    assert ctx.audio.pads.residency_status(0) == (None, None)


def test_async_load_errors_reach_console_without_selected_pad_rendering(
    controller: AppController, capsys: pytest.CaptureFixture[str]
) -> None:
    controller.session.sample_load_errors[37] = "Cold load canceled"
    controller.session.sample_analysis_errors[4] = "synthetic analysis refusal"
    controller.session.stem_generation_errors[17] = "synthetic separator refusal"
    capsys.readouterr()

    poll_control_diagnostics(controller)
    first = capsys.readouterr().err
    poll_control_diagnostics(controller)

    assert "operation=load PAD=38 reason=Cold load canceled" in first
    assert "operation=analysis PAD=5 reason=synthetic analysis refusal" in first
    assert "operation=stem_generation PAD=18 reason=synthetic separator refusal" in first
    assert not capsys.readouterr().err


def test_global_rejection_logs_each_attempt_without_mutating_intent(
    controller: AppController,
    audio_engine_mock: Mock,
    capsys: pytest.CaptureFixture[str],
) -> None:
    ctx = UiContext(controller)
    audio_engine_mock.cancel_all_launches.side_effect = RuntimeError("content placement pending")
    before_project = controller.project.model_dump()

    ctx.audio.global_.stop_start_stop()
    ctx.audio.global_.stop_start_stop()

    lines = capsys.readouterr().err.splitlines()
    assert len(lines) == 2
    assert all("operation=global_stop_start_stop" in line for line in lines)
    assert all("reason=content placement pending" in line for line in lines)
    assert controller.project.model_dump() == before_project
    audio_engine_mock.stop_all.assert_not_called()


def test_source_key_popup_reports_a_failed_correction(
    controller: AppController, capsys: pytest.CaptureFixture[str]
) -> None:
    ctx = UiContext(controller)
    ctx.ui.source_key.report_error("original source generation changed")
    assert "operation=source_key_correction reason=original source generation changed" in (
        capsys.readouterr().err
    )


def test_diagnostic_tick_does_not_query_pad_native_status(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    def forbidden(*_args: object) -> None:
        message = "diagnostic polling must use cached errors without new pad/native queries"
        raise AssertionError(message)

    for owner, name in (
        (controller.transport.residency, "status"),
        (controller.transport.residency, "key_lock_status"),
        (controller.transport.pitch, "status"),
        (controller.transport.waveform, "readiness"),
        (controller.stems, "stem_residency_status"),
    ):
        monkeypatch.setattr(owner, name, forbidden)

    poll_control_diagnostics(controller)


def _failed_stem_batch() -> tuple[_ControlledStems, StemBatchInventory]:
    generation = _ControlledStems((4,))
    inventory = generation.inventory()
    assert generation.batch.start(inventory)
    generation.batch.poll()
    generation.owners[4].mark_failed(r"Cannot encode 'D:\Private Music\song.wav'")
    generation.batch.poll()
    assert generation.batch.summary is not None
    assert generation.batch.summary.failed == 1
    return generation, inventory


def test_bulk_failures_and_cleanup_are_reported_without_gui_once_per_original_operation(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    tmp_path: Path,
) -> None:
    # Real operation owners/immutable summaries; the established controlled
    # backends produce logical failures, not invented Native/audio acceptance.
    generation, inventory = _failed_stem_batch()
    monkeypatch.setattr(controller, "stem_batch", generation.batch)
    deletion = _ControlledDelete((_metadata(17),))
    delete_inventory = deletion.inventory()
    deletion.failures[17] = OSError(r"Cannot withdraw 'C:\Private Stems\vocal.wav'")
    assert deletion.operation.start(delete_inventory)
    deletion.operation.poll()
    assert deletion.operation.summary is not None
    assert deletion.operation.summary.failed == 1
    monkeypatch.setattr(controller, "stem_delete", deletion.operation)
    controller._assets.retirement_errors[tmp_path / "owned"] = (
        r"Cannot remove 'C:\Private Cache\pending.wav'"
    )
    audio_engine_mock.project_asset_cleanup_status.return_value = (
        2,
        8,
        0,
        [r"Native cleanup failed at 'D:\Private Cache\reader.pcm'"],
    )
    before = controller.project.model_dump(), controller.session.model_dump()
    audio_engine_mock.project_asset_cleanup_status.reset_mock()
    capsys.readouterr()

    for _ in range(8):
        poll_control_diagnostics(controller)

    first = capsys.readouterr().err.splitlines()
    assert len(first) == 3
    assert sum("operation=stem_batch PAD=5" in line for line in first) == 1
    assert sum("operation=stem_delete PAD=18" in line for line in first) == 1
    cleanup = next(line for line in first if "operation=stem_cleanup" in line)
    assert "Native cleanup failed at '[private path]'" in cleanup
    assert "Cannot remove '[private path]'" in cleanup
    assert not any("Private" in line or "song.wav" in line for line in first)
    assert audio_engine_mock.project_asset_cleanup_status.call_count == 8
    audio_engine_mock.retire_project_asset.assert_not_called()
    assert (controller.project.model_dump(), controller.session.model_dump()) == before

    # A new original operation can fail identically before an intervening poll
    # observes an empty result. Its own metadata identity must log again.
    assert generation.batch.start(inventory)
    generation.batch.poll()
    assert deletion.operation.start(delete_inventory)
    deletion.operation.poll()
    poll_control_diagnostics(controller)
    retried = capsys.readouterr().err.splitlines()
    assert len(retried) == 2
    assert any("operation=stem_batch PAD=5" in line for line in retried)
    assert any("operation=stem_delete PAD=18" in line for line in retried)
    generation.batch.dismiss_result()
    deletion.operation.dismiss_result()
    poll_control_diagnostics(controller)
    assert not capsys.readouterr().err
    assert generation.batch.summary is None
    assert deletion.operation.summary is None
    assert "stem_batch" not in {key[0] for key in controller.diagnostics._observed}
    assert "stem_delete" not in {key[0] for key in controller.diagnostics._observed}


def test_cleanup_snapshot_failure_and_recurrence_use_console_without_gui(
    controller: AppController,
    audio_engine_mock: Mock,
    capsys: pytest.CaptureFixture[str],
) -> None:
    audio_engine_mock.project_asset_cleanup_status.side_effect = RuntimeError(
        r"Worker unavailable at 'D:\Private Cache\source.pcm'"
    )
    capsys.readouterr()
    poll_control_diagnostics(controller)
    poll_control_diagnostics(controller)
    failed = capsys.readouterr().err.splitlines()
    assert len(failed) == 1
    assert "operation=stem_cleanup" in failed[0]
    assert "Cleanup status unavailable: Worker unavailable at '[private path]'" in failed[0]
    audio_engine_mock.project_asset_cleanup_status.side_effect = None
    audio_engine_mock.project_asset_cleanup_status.return_value = (0, 0, 0, [])
    poll_control_diagnostics(controller)
    assert not capsys.readouterr().err
    audio_engine_mock.project_asset_cleanup_status.return_value = (
        1,
        2,
        0,
        [r"Cannot remove 'D:\Private Cache\source.pcm'"],
    )
    poll_control_diagnostics(controller)
    poll_control_diagnostics(controller)
    assert len(capsys.readouterr().err.splitlines()) == 1
    audio_engine_mock.project_asset_cleanup_status.return_value = (0, 0, 0, [])
    poll_control_diagnostics(controller)
    audio_engine_mock.project_asset_cleanup_status.return_value = (
        1,
        2,
        0,
        [r"Cannot remove 'D:\Private Cache\source.pcm'"],
    )
    poll_control_diagnostics(controller)
    recurring = capsys.readouterr().err.splitlines()
    assert len(recurring) == 1
    assert "Cannot remove '[private path]'" in recurring[0]
    assert "Private Cache" not in recurring[0]
    audio_engine_mock.retire_project_asset.assert_not_called()


def test_recording_recovery_notice_uses_existing_console_dedup_without_a_save_fence(
    controller: AppController, capsys: pytest.CaptureFixture[str]
) -> None:
    notice = "Recording settings restored defaults for missing WAV precision."
    controller.persistence.recording_settings_warning = notice
    before_project = controller.project.model_dump()
    before_load_error = controller.persistence.load_error
    capsys.readouterr()
    poll_control_diagnostics(controller)
    poll_control_diagnostics(controller)
    lines = capsys.readouterr().err.splitlines()
    assert len(lines) == 1
    assert f"operation=recording_settings reason={notice}" in lines[0]
    assert controller.persistence.load_error == before_load_error
    assert controller.project.model_dump() == before_project


def test_transient_loader_error_is_printed_before_a_later_event_clears_it(
    controller: AppController, capsys: pytest.CaptureFixture[str]
) -> None:
    def failed(sample_id: int, _event: dict[str, object]) -> None:
        controller.session.sample_load_errors[sample_id] = "Cold load canceled"

    def recovered(sample_id: int, _event: dict[str, object]) -> None:
        controller.session.sample_load_errors.pop(sample_id, None)

    handlers: dict[str, Callable[[int, dict[str, object]], None]] = {
        "error": failed,
        "success": recovered,
    }
    controller.loader._dispatch_loader_event({"type": "error", "id": 37}, handlers)
    controller.loader._dispatch_loader_event({"type": "success", "id": 37}, handlers)

    assert 37 not in controller.session.sample_load_errors
    assert "operation=load PAD=38 reason=Cold load canceled" in capsys.readouterr().err


def test_historical_pad_error_does_not_block_a_new_authoritative_start(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    originals = _record_original_starts(audio_engine_mock)
    ctx = UiContext(controller)
    controller.transport.residency.report_launch_error(0, "old prepared_mismatch")
    controller.session.sample_load_errors[0] = "old Cold load canceled"

    ctx.audio.pads.trigger_pad(0, received_at_ns=17)

    (original,) = originals
    assert original.received_at_ns == 17
    audio_engine_mock.capture_pad_start.assert_called_once()
    audio_engine_mock.prepare_captured_pad_start.assert_called_once_with(original)
    audio_engine_mock.publish_pad_start.assert_not_called()
    ticket.status = "accepted"

    controller.transport.residency.poll()

    audio_engine_mock.publish_pad_start.assert_called_once_with(original, ticket)
    assert ctx.audio.pads.residency_status(0) == (None, None)


@pytest.mark.parametrize("failure", ["rejection", "missing_capture", "capture_exception"])
def test_pitch_input_failure_is_reported_before_success_in_the_same_native_event_batch(
    controller: AppController,
    audio_engine_mock: Mock,
    capsys: pytest.CaptureFixture[str],
    failure: str,
) -> None:
    _, publish, cancel = _pitch_fixture(controller, audio_engine_mock)
    controller.input_mapping.set_enabled(enabled=True)
    rejected = _CapturedPitch(1, state="rejected", reason="prepared_mismatch")
    first = _pitch_event(rejected)
    if failure == "rejection":
        first["pitch_rejection"] = "unsupported_pitch"
        expected = describe_pitch_rejection("unsupported_pitch")
    elif failure == "missing_capture":
        first.pop("captured_pitch")
        expected = "original selected content capture is unavailable"
    else:
        expected = "prepared_mismatch"
    successful = _CapturedPitch(2, extra_shift=7, received_at_ns=777)
    audio_engine_mock.poll_input_events.side_effect = [first, _pitch_event(successful), None]

    controller.input_mapping._poll_rust_input_events()

    console = capsys.readouterr().err
    assert "operation=pitch_input reason=Pitch choice not accepted:" in console
    assert expected in console
    assert controller.session.input_mapping_error is None
    assert successful.accepted
    assert controller.project.pad_key_intent[0].extra_shift == 7
    publish.assert_called_once_with(successful)
    cancel.assert_not_called()
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    audio_engine_mock.capture_selected_pitch_action.assert_not_called()


@pytest.mark.parametrize("failure", ["invalid_action", "capture_exception"])
def test_selected_pitch_input_error_is_reported_before_the_next_success_clears_it(
    controller: AppController,
    audio_engine_mock: Mock,
    capsys: pytest.CaptureFixture[str],
    failure: str,
) -> None:
    _, publish, cancel = _pitch_fixture(controller, audio_engine_mock)
    successful = _CapturedPitch(1, extra_shift=7, received_at_ns=777)
    first_key = "pitch.extra:7:set"
    if failure == "capture_exception":
        audio_engine_mock.capture_selected_pitch_action.side_effect = [
            RuntimeError("original selected capture refused"),
            successful,
        ]
        expected = "original selected capture refused"
    else:
        first_key = "pitch.invalid:set"
        audio_engine_mock.capture_selected_pitch_action.return_value = successful
        expected = describe_pitch_rejection("invalid_action")

    assert not controller.input_mapping._execute_pitch_action(first_key, received_at_ns=123)
    assert controller.input_mapping._execute_pitch_action("pitch.extra:7:set", received_at_ns=777)

    console = capsys.readouterr().err
    assert "operation=pitch_input reason=Pitch choice not accepted:" in console
    assert expected in console
    assert controller.session.input_mapping_error is None
    publish.assert_called_once_with(successful)
    cancel.assert_not_called()
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    calls = audio_engine_mock.capture_selected_pitch_action.call_args_list
    assert len(calls) == (2 if failure == "capture_exception" else 1)
    assert calls[-1].kwargs["received_at_ns"] == 777


def test_private_stem_cleanup_diagnostic_is_reported_before_later_state_cleanup(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    cache_dir = tmp_path / "synthetic-private-stem-job"
    job = Mock()
    job.dispose.side_effect = OSError(r"Cannot remove D:\Private Sessions\secret.wav")
    monkeypatch.setattr(controller.stems, "_jobs", {cache_dir: job})

    controller.stems._discard_generation_artifacts(3, cache_dir)
    controller.stems._clear_stem_generation_messages(3)

    assert 3 not in controller.session.stem_generation_diagnostics
    console = capsys.readouterr().err
    assert "operation=stem_generation_diagnostic PAD=4" in console
    assert "Private stem artifact cleanup failed: Cannot remove [private path]" in console
    assert "Private Sessions" not in console
    assert "secret.wav" not in console
    job.dispose.assert_called_once()


def test_backend_diagnostic_remains_in_console_after_its_snapshot_is_cleared(
    controller: AppController, capsys: pytest.CaptureFixture[str]
) -> None:
    result = StemGenerationResult(
        backend_name="synthetic",
        model_name="synthetic",
        device="cpu",
        cpu_fallback=True,
        artifact_count=0,
        diagnostic=r"CUDA failed reading 'C:\Private Models\secret.bin'; completed on CPU",
    )

    controller.stems._record_generation_result(7, result)
    controller.stems._clear_stem_generation_messages(7)

    assert 7 not in controller.session.stem_generation_diagnostics
    console = capsys.readouterr().err
    assert "operation=stem_generation_diagnostic PAD=8" in console
    assert "CUDA failed reading '[private path]'; completed on CPU" in console
    assert "Private Models" not in console
    assert "secret.bin" not in console
