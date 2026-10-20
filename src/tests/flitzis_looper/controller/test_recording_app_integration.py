"""App routing and failure fences, without simulated positive recording authority.

App fixtures use the explicit Native control-flow mock. Terminal worker metadata
below exercises only the extra failed-stream retirement fence; it is no proof of
Native closure, PCM, codec success, actual Thread join or device behavior.
"""

from dataclasses import replace
from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, call

import pytest
from imgui_bundle import imgui

from flitzis_looper.controller import app as app_module
from flitzis_looper.controller import recording as recording_module
from flitzis_looper.controller.recording import RecordingController, RecordingStatus
from flitzis_looper.models import ProjectState
from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.recording_paths import hold_live_recording_spool
from flitzis_looper.recording_worker import (
    RecordingNativeState,
    RecordingWorkerOutcome,
    RecordingWorkSnapshot,
)
from flitzis_looper.ui import run
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.ui.test_waveform_render_stability import _plot_context

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import AudioEngine


def test_app_owns_same_project_audio_and_cwd_record_root(controller: AppController) -> None:
    owner = controller.recording
    assert owner._project is controller.project
    assert owner._audio is controller._audio
    assert owner._root == Path.cwd().absolute() / "record"
    assert owner._live_spool_witness is hold_live_recording_spool
    assert owner._owner is None
    assert not owner._root.exists()
    assert id(owner) not in {id(member) for member in controller._controllers}


def test_runtime_reports_recording_errors_in_console_without_rendering(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    audio_engine_mock.poll_loader_events.return_value = None
    status = Mock(return_value=RecordingStatus(phase="Failed", error="Writer failed"))
    poll = Mock()
    monkeypatch.setattr(controller.recording, "status", status)
    monkeypatch.setattr(controller.recording, "poll", poll)
    capsys.readouterr()

    for expected in (True, False):
        controller.poll_runtime_events()
        output = capsys.readouterr().err
        assert ("operation=recording reason=Writer failed" in output) is expected
    status.return_value = RecordingStatus()
    controller.poll_runtime_events()
    assert "operation=recording" not in capsys.readouterr().err
    status.return_value = RecordingStatus(phase="Failed", error="Writer failed")
    controller.poll_runtime_events()
    assert "operation=recording reason=Writer failed" in capsys.readouterr().err
    assert poll.call_count == 4
    assert status.return_value.error == "Writer failed"


def test_custom_config_does_not_redirect_record_root(
    audio_engine_mock: Mock, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    config_parent = tmp_path / "custom-settings"
    config_parent.mkdir()
    controller = app_module.AppController(project_config_path=config_parent / "Config.json")
    assert controller.recording._root == tmp_path / "record"
    assert controller.recording._audio is audio_engine_mock
    assert not (config_parent / "record").exists()
    assert not (tmp_path / "record").exists()


@pytest.mark.parametrize("entry", ["absent", "directory", "file"])
def test_startup_discovers_only_existing_entries_without_creating_root(
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    entry: str,
) -> None:
    monkeypatch.chdir(tmp_path)
    root = tmp_path / "record"
    if entry == "directory":
        root.mkdir()
    elif entry == "file":
        root.write_bytes(b"unknown recorder-root collision must be preserved")
    owner_class = Mock()
    monkeypatch.setattr(app_module, "RecordingController", owner_class)
    controller = app_module.AppController()
    owner = owner_class.return_value
    if entry == "absent":
        owner.discover_recovery.assert_not_called()
        assert not root.exists()
    else:
        owner.discover_recovery.assert_called_once_with()
        assert root.exists()
    assert controller.recording is owner
    assert owner_class.call_args.args == (controller.project, audio_engine_mock, root)


def test_startup_lstat_error_is_visible(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = controller.recording._root
    original = Path.lstat

    def lstat(path: Path) -> object:
        if path == root:
            message = "explicit record root permission failure"
            raise PermissionError(message)
        return original(path)

    monkeypatch.setattr(Path, "lstat", lstat)
    with pytest.raises(PermissionError, match="record root permission failure"):
        controller._discover_recordings()
    assert controller.recording.status().phase == "Idle"


def test_app_callbacks_are_deferred_to_original_audio_lifecycle(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.audio_stream_failed.assert_not_called()
    audio_engine_mock.quiesce_failed_audio_stream.assert_not_called()
    failed = controller.recording._stream_failed
    quiesce = controller.recording._quiesce_failed_stream
    assert failed is not None
    assert quiesce is not None
    assert failed() is False
    assert quiesce() is False
    audio_engine_mock.audio_stream_failed.assert_called_once_with()
    audio_engine_mock.quiesce_failed_audio_stream.assert_called_once_with()


def test_recording_facade_retains_original_left_timestamp_without_playback_restart(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    context = UiContext(controller)
    gate = Mock()
    original = Mock(return_value=gate)
    ordinary_start = Mock()
    ordinary_stop = Mock()
    monkeypatch.setattr(
        controller.transport.playback._global_playback, "capture_recording_start", original
    )
    monkeypatch.setattr(
        controller.transport.playback, "start_or_restart_global_start_stop", ordinary_start
    )
    monkeypatch.setattr(controller.transport.playback, "stop_global_start_stop", ordinary_stop)
    received: list[object] = []
    route = Mock(side_effect=lambda factory: received.append(factory()))
    monkeypatch.setattr(controller.recording, "record_group", route)
    context.recording.record_group(received_at_ns=123456789)
    route.assert_called_once()
    original.assert_called_once_with(received_at_ns=123456789)
    assert received == [gate]
    ordinary_start.assert_not_called()
    ordinary_stop.assert_not_called()
    audio_engine_mock.start_mix_capture.assert_not_called()


def test_recording_facade_mix_status_and_ack_are_separate_from_transport(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    context = UiContext(controller)
    mix = Mock()
    ack = Mock(return_value=False)
    state = RecordingStatus(phase="Failed", error="retained take", busy=True, retired=False)
    monkeypatch.setattr(controller.recording, "record_mix", mix)
    monkeypatch.setattr(controller.recording, "status", Mock(return_value=state))
    monkeypatch.setattr(controller.recording, "acknowledge_failure", ack)
    playback = Mock()
    monkeypatch.setattr(
        controller.transport.playback, "start_or_restart_global_start_stop", playback
    )
    context.recording.record_mix()
    assert context.recording.status is state
    assert context.recording.acknowledge_failure() is False
    mix.assert_called_once_with()
    ack.assert_called_once_with()
    playback.assert_not_called()


@pytest.mark.parametrize("settings_open", [False, True])
def test_recording_has_one_runtime_poll_and_no_render_poll(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    settings_open: bool,
) -> None:
    # This routing case has no loader delivery; an unconstrained Mock is not one.
    audio_engine_mock.poll_loader_events.return_value = None
    controller.session.settings_open = settings_open
    poll = Mock()
    monkeypatch.setattr(controller.recording, "poll", poll)
    controller.on_frame_render()
    poll.assert_not_called()
    controller.poll_runtime_events()
    poll.assert_called_once_with()


@pytest.mark.parametrize("bank_ready", [False, True])
def test_shutdown_keeps_fixed_deadline_and_services_recording_without_starving_old_chain(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    bank_ready: bool,
) -> None:
    clock = Mock(side_effect=[100.0, 900.0])
    monkeypatch.setattr(app_module, "monotonic", clock)
    order: list[str] = []

    def prepare_recording(**_kwargs: object) -> bool:
        order.append("recording")
        return False

    def prepare_banks() -> bool:
        order.append("banks")
        return bank_ready

    recording = Mock(side_effect=prepare_recording)
    banks = Mock(side_effect=prepare_banks)
    pairs = Mock(return_value=True)
    arrangement = Mock(return_value=True)
    hold = Mock(side_effect=lambda _reason: order.append("hold"))
    monkeypatch.setattr(controller.recording, "prepare_shutdown", recording)
    monkeypatch.setattr(controller.banks, "prepare_shutdown", banks)
    monkeypatch.setattr(controller.pairs, "prepare_shutdown", pairs)
    monkeypatch.setattr(controller.arrangement, "prepare_shutdown", arrangement)
    monkeypatch.setattr(controller.transport.waveform, "request_hold_release", hold)
    assert controller.prepare_shutdown() is False
    assert controller.prepare_shutdown() is False
    assert order == ["recording", "hold", "banks", "recording", "hold", "banks"]
    assert recording.call_args_list == [call(deadline=130.0), call(deadline=130.0)]
    assert banks.call_count == 2
    assert pairs.call_count == (2 if bank_ready else 0)
    assert arrangement.call_count == (2 if bank_ready else 0)
    clock.assert_called_once_with()
    audio_engine_mock.shut_down.assert_not_called()


def test_shutdown_releases_recording_before_sources_and_audio(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    order: list[str] = []
    monkeypatch.setattr(controller.recording, "prepare_shutdown", Mock(return_value=True))
    for owner in (
        controller.recording,
        controller.material_migration,
        controller.transport.pitch,
        controller.transport.residency,
        controller.accepted_timing,
        controller.loader,
        controller.stems,
    ):
        label = "recording" if owner is controller.recording else type(owner).__name__
        monkeypatch.setattr(
            owner, "shut_down", Mock(side_effect=lambda name=label: order.append(name))
        )
    monkeypatch.setattr(
        controller._audio, "stop_all", Mock(side_effect=lambda: order.append("stop_all"))
    )
    monkeypatch.setattr(
        controller._assets,
        "release_saved_assignments",
        Mock(side_effect=lambda: order.append("assignments")),
    )
    monkeypatch.setattr(
        controller._audio, "shut_down", Mock(side_effect=lambda: order.append("audio"))
    )
    controller.shut_down()
    assert order[0] == "recording"
    assert order[-3:] == ["stop_all", "assignments", "audio"]
    controller.shut_down()
    assert order.count("recording") == 1


def test_pending_shutdown_error_keeps_recording_and_existing_pad_error_visible(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(
        controller.recording,
        "status",
        Mock(return_value=RecordingStatus(error="recording incomplete")),
    )
    controller.arrangement.error = "arrangement pending"
    assert controller.pending_shutdown_error == "recording incomplete\narrangement pending"


def test_close_popup_keeps_recording_details_out_of_the_real_modal(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with _plot_context():
        text = Mock(wraps=imgui.text_wrapped)
        end = Mock(wraps=imgui.end_popup)
        monkeypatch.setattr(imgui, "text_wrapped", text)
        monkeypatch.setattr(imgui, "end_popup", end)
        imgui.new_frame()
        run._show_close_pending("recording deadline expired; incomplete evidence retained")
        imgui.render()
        text.assert_called_once_with("Please wait while pending work finishes.")
        end.assert_called_once_with()


def _terminal_owner(
    root: Path,
    *,
    stream_failed: Mock | None,
    quiesce: Mock | None,
) -> tuple[RecordingController, recording_module._TakeOwner, Mock]:
    """Synthetic terminal observations test only control retirement fencing."""
    audio = Mock()
    controller = RecordingController(
        ProjectState(),
        cast("AudioEngine", audio),
        root,
        live_spool_witness=hold_live_recording_spool,
        stream_failed=stream_failed,
        quiesce_failed_stream=quiesce,
    )
    native = RecordingNativeState(
        session_id=1,
        rate=48000,
        channels=2,
        writer_ready=False,
        started=True,
        rt_sealed=True,
        rt_released=True,
        worker_closed=True,
        worker_joined=True,
        first_frame=1,
        end_frame=2,
        submitted_frames=1,
        written_frames=1,
        empty=False,
        failure="cancelled",
    )
    work = RecordingWorkSnapshot(
        stage="finished",
        ready=None,
        paths=None,
        native=native,
        outcome=RecordingWorkerOutcome(error="cancelled"),
        cleanup_complete=True,
        thread_done=True,
    )
    # A bounded metadata-only interface, without introspecting worker annotations
    # or pretending to construct/join a real recording Thread.
    worker = Mock(spec_set=["snapshot", "join_if_done", "cancel"])
    worker.snapshot.return_value = work
    worker.join_if_done.return_value = True
    owner = recording_module._TakeOwner(worker, RecordingOptions(), None, start_requested=True)
    controller._owner = owner
    return controller, owner, worker


def test_prior_capture_error_does_not_hide_failed_stream_or_false_close(tmp_path: Path) -> None:
    failed = Mock(return_value=True)
    quiesce = Mock(side_effect=[False, True])
    controller, owner, worker = _terminal_owner(tmp_path, stream_failed=failed, quiesce=quiesce)
    gate = Mock()
    owner.gate = gate
    controller.poll()
    assert controller.status().busy
    assert not controller.status().retired
    assert controller.status().retirement_uncertain
    assert controller._owner is owner
    assert owner.gate is gate
    assert worker.snapshot().native.failure == "cancelled"
    controller.poll()
    assert controller.status().retired
    assert not controller.status().retirement_uncertain
    assert owner.gate is None
    assert quiesce.call_count == 2
    assert controller.acknowledge_failure()


def test_checked_quiescence_error_is_visible_and_retried(tmp_path: Path) -> None:
    quiesce = Mock(side_effect=[RuntimeError("checked retirement worker join failed"), True])
    controller, owner, _worker = _terminal_owner(
        tmp_path, stream_failed=Mock(return_value=True), quiesce=quiesce
    )
    controller.poll()
    assert controller._owner is owner
    assert not controller.status().retired
    assert "checked retirement worker join failed" in (controller.status().error or "")
    controller.poll()
    assert controller.status().retired
    assert quiesce.call_count == 2


def test_known_stream_death_overrides_stale_active_snapshot_before_and_after_close(
    tmp_path: Path,
) -> None:
    quiesce = Mock(side_effect=[False, True])
    controller, owner, worker = _terminal_owner(
        tmp_path, stream_failed=Mock(return_value=True), quiesce=quiesce
    )
    work = worker.snapshot.return_value
    worker.snapshot.return_value = replace(
        work,
        stage="waiting",
        native=replace(
            work.native,
            rt_sealed=False,
            rt_released=False,
            worker_closed=False,
            worker_joined=False,
            failure=None,
        ),
        outcome=None,
        cleanup_complete=False,
        thread_done=False,
    )
    worker.join_if_done.return_value = False
    for _ in range(3):
        controller.poll()
        status = controller.status()
        assert status.phase == "Failed"
        assert not status.effective_recording
        assert status.busy
        assert not status.retired
        assert controller._owner is owner
    assert quiesce.call_count == 2
    assert not controller.acknowledge_failure()


@pytest.mark.parametrize("callback", ["failure", "quiescence"])
def test_nonboolean_native_lifecycle_results_cannot_grant_retirement(
    tmp_path: Path, callback: str
) -> None:
    failed = Mock(return_value=Mock() if callback == "failure" else True)
    quiesce = Mock(return_value=Mock() if callback == "quiescence" else True)
    controller, owner, _worker = _terminal_owner(tmp_path, stream_failed=failed, quiesce=quiesce)
    controller.poll()
    assert controller._owner is owner
    assert controller.status().retirement_uncertain
    assert not controller.status().retired
    assert not controller.acknowledge_failure()


def test_failure_getter_error_fences_owner_until_a_truthful_retry(tmp_path: Path) -> None:
    failed = Mock(side_effect=[RuntimeError("stream observation unavailable"), False])
    quiesce = Mock(return_value=True)
    controller, owner, _worker = _terminal_owner(tmp_path, stream_failed=failed, quiesce=quiesce)
    controller.poll()
    assert controller._owner is owner
    assert not controller.status().retired
    controller.poll()
    assert controller.status().retired
    quiesce.assert_not_called()


def test_standalone_stream_failure_has_no_implicit_quiescence_receipt(tmp_path: Path) -> None:
    controller, _owner, worker = _terminal_owner(tmp_path, stream_failed=None, quiesce=None)
    work = worker.snapshot.return_value
    worker.snapshot.return_value = replace(work, native=replace(work.native, failure="stream"))
    controller.poll()
    assert controller.status().busy
    assert not controller.status().retired
    assert not controller.acknowledge_failure()


def test_idle_does_not_request_stream_quiescence(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.recording.poll()
    audio_engine_mock.audio_stream_failed.assert_not_called()
    audio_engine_mock.quiesce_failed_audio_stream.assert_not_called()
