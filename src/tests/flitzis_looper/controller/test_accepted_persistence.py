"""Productive save/load wiring; real source/evidence/adoption proof lives in native tests."""

import json
from concurrent.futures import Future
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.accepted_restore import AcceptedTimingRestore
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.timing_persistence import TimingPersistenceError
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.models import ProjectState, SampleAnalysis, SessionState, TimingIntent
from tests.conftest import write_mono_pcm16_wav

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import ConstantTimingTicket


def _analysis() -> SampleAnalysis:
    return SampleAnalysis.model_validate({
        "bpm": 120.00128936767578,
        "key": "C",
        "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0], "bars": [0.0]},
        "accepted_timing": {
            "schema_version": 1,
            "encoding": "accepted-constant-timing-qm-raw-v1",
            "record": {"fixture": "native codec verifies the full record"},
        },
    })


def _project() -> ProjectState:
    project = ProjectState()
    project.sample_paths[0] = "samples/source.wav"
    project.sample_analysis[0] = _analysis()
    project.pad_timing_intent[0] = "automatic"
    return project


def test_actual_atomic_save_requests_verified_current_export_and_roundtrips(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    project = _project()
    audio = Mock()
    audio.pad_timing_intent.return_value = "automatic"
    accepted = _analysis().accepted_timing
    assert accepted is not None
    exported = accepted.model_dump_json()
    audio.export_current_constant_timing.return_value = exported
    persistence = ProjectPersistence(project)
    persistence.bind_audio(audio)
    persistence.flush()

    restored = ProjectPersistence.from_config_path().project
    restored_analysis = restored.sample_analysis[0]
    assert restored_analysis is not None
    assert restored_analysis.accepted_timing is not None
    assert restored_analysis.accepted_timing.model_dump_json() == exported
    assert restored_analysis.bpm == 120.00128936767578
    assert restored.pad_timing_intent[0] == "automatic"
    audio.export_current_constant_timing.assert_called_once_with(0, "samples/source.wav")
    assert "ticket" not in json.loads(persistence.config_path.read_text())["sample_analysis"][0]


@pytest.mark.parametrize("native_owner", [False, True])
def test_save_cannot_reexport_historical_evidence_without_current_ack(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, native_owner: bool
) -> None:
    monkeypatch.chdir(tmp_path)
    project = _project()
    persistence = ProjectPersistence(project)
    if native_owner:
        audio = Mock()
        audio.pad_timing_intent.return_value = "automatic"
        audio.export_current_constant_timing.return_value = None
        persistence.bind_audio(audio)
    persistence.flush()
    restored = ProjectPersistence.from_config_path().project
    restored_analysis = restored.sample_analysis[0]
    original_analysis = project.sample_analysis[0]
    assert restored_analysis is not None
    assert original_analysis is not None
    assert restored_analysis.accepted_timing is None
    assert restored.pad_timing_intent[0] == "automatic"
    assert original_analysis.accepted_timing is not None


def test_failed_source_verification_preserves_previous_atomic_config(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    persistence = ProjectPersistence(_project())
    persistence.flush()
    old = persistence.config_path.read_bytes()
    audio = Mock()
    audio.pad_timing_intent.return_value = "automatic"
    audio.export_current_constant_timing.side_effect = ValueError("source bytes changed")
    persistence.bind_audio(audio)
    persistence.mark_dirty()
    with pytest.raises(TimingPersistenceError, match="source bytes changed"):
        persistence.flush()
    assert persistence.config_path.read_bytes() == old
    assert persistence._dirty


@pytest.mark.parametrize("immediate", [False, True])
def test_productive_save_rejection_reports_error_and_retains_dirty_atomic_state(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, immediate: bool
) -> None:
    monkeypatch.chdir(tmp_path)
    persistence = ProjectPersistence(_project())
    persistence.flush(now=0.0)
    previous = persistence.config_path.read_bytes()
    persistence.project.volume = 0.5
    audio = Mock()
    audio.pad_timing_intent.return_value = "automatic"
    audio.export_current_constant_timing.side_effect = ValueError("source bytes changed")
    report = Mock()
    persistence.bind_audio(audio, report)
    persistence.mark_dirty()

    saved = (
        persistence.flush_if_dirty(now=100.0) if immediate else persistence.maybe_flush(now=100.0)
    )

    assert saved is False
    report.assert_called_once_with(0, "Timing save rejected: source bytes changed")
    assert persistence._dirty
    assert persistence._last_write_monotonic == 0.0
    assert persistence.config_path.read_bytes() == previous


def test_rejected_debounced_save_throttles_heavy_export_and_retries_after_ten_seconds(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    persistence = ProjectPersistence(_project())
    persistence.flush(now=0.0)
    previous = persistence.config_path.read_bytes()
    audio = Mock()
    audio.pad_timing_intent.return_value = "automatic"
    audio.export_current_constant_timing.side_effect = ValueError("source bytes changed")
    report = Mock()
    persistence.bind_audio(audio, report)
    persistence.mark_dirty()

    assert persistence.maybe_flush(now=20.0) is False
    for now in [20.001, 25.0, 29.999]:
        assert persistence.maybe_flush(now=now) is False
    audio.export_current_constant_timing.assert_called_once_with(0, "samples/source.wav")
    report.assert_called_once_with(0, "Timing save rejected: source bytes changed")
    assert persistence._dirty
    assert persistence.config_path.read_bytes() == previous

    assert persistence.maybe_flush(now=30.0) is False
    assert audio.export_current_constant_timing.call_count == 2
    assert report.call_count == 2
    assert persistence._dirty
    assert persistence.config_path.read_bytes() == previous

    audio.export_current_constant_timing.side_effect = None
    audio.export_current_constant_timing.return_value = None
    assert persistence.maybe_flush(now=40.0) is True
    assert not persistence._dirty
    assert persistence._last_timing_rejection_monotonic is None


def test_controller_shutdown_preserves_audio_teardown_after_real_timing_save_rejection(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/source.wav"
    controller.project.sample_analysis[0] = _analysis()
    controller.project.pad_timing_intent[0] = "automatic"
    audio_engine_mock.pad_timing_intent.return_value = "automatic"
    exporter = Mock(return_value=None)
    audio_engine_mock.export_current_constant_timing = exporter
    controller.persistence.flush(now=0.0)
    previous = controller.persistence.config_path.read_bytes()
    controller.project.volume = 0.5
    controller.persistence.mark_dirty()
    exporter.side_effect = ValueError("source bytes changed")

    controller.shut_down()

    assert (
        controller.session.sample_analysis_errors[0] == "Timing save rejected: source bytes changed"
    )
    assert controller.persistence._dirty
    assert controller.persistence.config_path.read_bytes() == previous
    audio_engine_mock.stop_all.assert_called_once()
    audio_engine_mock.shut_down.assert_called_once()


@pytest.mark.parametrize("intent", ["manual", "tap", "legacy"])
def test_nonaccepted_intent_saves_without_exporting_old_acceptance(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, intent: str
) -> None:
    monkeypatch.chdir(tmp_path)
    project = _project()
    project.manual_bpm[0] = 94.0 if intent != "legacy" else None
    audio = Mock()
    audio.pad_timing_intent.return_value = intent
    persistence = ProjectPersistence(project)
    persistence.bind_audio(audio)
    persistence.flush()
    restored = ProjectPersistence.from_config_path().project
    assert restored.pad_timing_intent[0] == intent
    assert restored.manual_bpm[0] == project.manual_bpm[0]
    restored_analysis = restored.sample_analysis[0]
    assert restored_analysis is not None
    assert restored_analysis.accepted_timing is None
    audio.export_current_constant_timing.assert_not_called()


def test_startup_captures_automatic_without_mutating_authority_before_cold_success(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    (tmp_path / "samples").mkdir()
    write_mono_pcm16_wav(tmp_path / "samples/source.wav", 48_000)
    project = _project()
    audio = Mock()
    audio.output_sample_rate.return_value = 48_000
    audio.load_sample_async.return_value = 7
    changed = Mock()
    loader = LoaderController(project, SessionState(), audio, changed)
    begin = Mock()
    monkeypatch.setattr(loader._accepted_restore, "begin", begin)
    loader.restore_samples_from_project_state()
    audio.load_sample_async.assert_called_once_with(
        0,
        "samples/source.wav",
        run_analysis=False,
        restore_automatic=True,
        replace_assignment=True,
    )
    audio.set_pad_timing_intent.assert_not_called()
    audio.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 7,
            "cached_path": "samples/source.wav",
            "duration_s": 600.0,
        },
        None,
    ]
    loader.poll_loader_events()
    begin.assert_called_once_with(0)
    audio.set_pad_bpm.assert_not_called()
    changed.assert_not_called()


def test_actual_startup_audio_projection_waits_for_fresh_automatic_adoption(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    (tmp_path / "samples").mkdir()
    write_mono_pcm16_wav(tmp_path / "samples/source.wav", 48_000)
    project = _project()
    project.bpm_lock = True
    project.pad_loop_start_s[0] = 5.0
    project.pad_loop_end_s[0] = 10.0
    project.pad_grid_anchor_s[0] = 1.25
    project.pad_grid_offset_samples[0] = -128
    audio = Mock()
    audio.output_sample_rate.return_value = 48_000
    audio.load_sample_async.return_value = 7
    audio.current_constant_timing.return_value = None
    native_intent: list[TimingIntent] = ["legacy"]

    def declare_intent(_sample_id: int, intent: TimingIntent) -> None:
        native_intent[0] = intent

    audio.set_pad_timing_intent.side_effect = declare_intent
    audio.pad_timing_intent.side_effect = lambda _sample_id: native_intent[0]
    session = SessionState()
    transport = TransportController(project, session, audio)
    loader = LoaderController(project, session, audio, transport.bpm.on_pad_bpm_changed)

    loader.restore_samples_from_project_state()
    transport.apply_project_state_to_audio()

    assert native_intent[0] == "legacy"
    assert transport.bpm.current_timing(0) is None
    assert transport.bpm.effective_bpm(0) is None
    audio.set_pad_timing_intent.assert_not_called()
    audio.capture_saved_constant_timing.assert_not_called()
    audio.load_sample_async.assert_called_once_with(
        0,
        "samples/source.wav",
        run_analysis=False,
        restore_automatic=True,
        replace_assignment=True,
    )
    audio.set_pad_bpm.assert_not_called()
    audio.set_pad_timing_metadata.assert_not_called()
    audio.set_pad_loop_region.assert_not_called()
    audio.set_master_bpm.assert_not_called()
    audio.set_master_period.assert_not_called()
    assert session.master_period_seconds is None


@pytest.mark.parametrize("failure_stage", ["admission", "preparation"])
def test_failed_initial_automatic_restore_keeps_saved_timing_unresolved(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failure_stage: str
) -> None:
    monkeypatch.chdir(tmp_path)
    (tmp_path / "samples").mkdir()
    write_mono_pcm16_wav(tmp_path / "samples/source.wav", 48_000)
    project = _project()
    project.bpm_lock = True
    project.pad_loop_start_s[0] = 5.0
    project.pad_loop_end_s[0] = 10.0
    project.pad_grid_anchor_s[0] = 1.25
    project.pad_grid_offset_samples[0] = -128
    previous_project = project.model_dump()
    audio = Mock()
    audio.output_sample_rate.return_value = 48_000
    audio.current_constant_timing.return_value = None
    audio.pad_timing_intent.return_value = "legacy"
    audio.load_sample_async.return_value = 7
    audio.poll_loader_events.return_value = None
    if failure_stage == "admission":
        audio.load_sample_async.side_effect = RuntimeError("cold worker stopped")
    session = SessionState()
    transport = TransportController(project, session, audio)
    loader = LoaderController(project, session, audio, transport.bpm.on_pad_bpm_changed)

    loader.restore_samples_from_project_state()
    if failure_stage == "preparation":
        audio.poll_loader_events.side_effect = [
            {"type": "error", "id": 0, "request_id": 7, "msg": "snapshot failed"},
            None,
        ]
        loader.poll_loader_events()
    transport.apply_project_state_to_audio()
    transport.bpm.on_pad_bpm_changed(0)
    transport.bpm.recompute_master_bpm()

    assert 0 not in session.loading_sample_ids
    assert session.sample_load_errors[0]
    assert project.model_dump() == previous_project
    assert transport.bpm.current_timing(0) is None
    assert transport.bpm.effective_bpm(0) is None
    assert session.master_period_seconds is None
    audio.set_pad_timing_intent.assert_not_called()
    audio.capture_saved_constant_timing.assert_not_called()
    audio.set_pad_bpm.assert_not_called()
    audio.set_pad_timing_metadata.assert_not_called()
    audio.set_pad_loop_region.assert_not_called()
    audio.set_master_bpm.assert_not_called()
    audio.set_master_period.assert_not_called()


@pytest.mark.parametrize(
    ("intent", "manual_bpm"),
    [("manual", 94.0), ("tap", 90.0), ("legacy", None), ("automatic", 91.0)],
)
def test_restored_performer_intent_prevents_accepted_capture(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    intent: TimingIntent,
    manual_bpm: float | None,
) -> None:
    monkeypatch.chdir(tmp_path)
    (tmp_path / "samples").mkdir()
    write_mono_pcm16_wav(tmp_path / "samples/source.wav", 48_000)
    project = _project()
    project.pad_timing_intent[0] = intent
    project.manual_bpm[0] = manual_bpm
    audio = Mock()
    audio.pad_timing_intent.return_value = intent
    audio.output_sample_rate.return_value = 48_000
    audio.load_sample_async.return_value = 7
    loader = LoaderController(project, SessionState(), audio, Mock())
    loader.restore_samples_from_project_state()
    audio.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 7,
            "cached_path": "samples/source.wav",
            "duration_s": 600.0,
        },
        None,
    ]

    loader.poll_loader_events()

    audio.capture_saved_constant_timing.assert_not_called()
    assert not loader._accepted_restore._pending
    assert project.manual_bpm[0] == manual_bpm
    assert project.pad_timing_intent[0] == intent


@pytest.mark.parametrize("intent", ["manual", "tap"])
@pytest.mark.parametrize("analysis_present", [False, True])
def test_explicit_intent_without_numeric_override_survives_loader_refresh_and_save(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    intent: TimingIntent,
    *,
    analysis_present: bool,
) -> None:
    monkeypatch.chdir(tmp_path)
    (tmp_path / "samples").mkdir()
    write_mono_pcm16_wav(tmp_path / "samples/source.wav", 48_000)
    project = _project()
    project.pad_timing_intent[0] = intent
    if not analysis_present:
        project.sample_analysis[0] = None
    assert project.manual_bpm[0] is None
    native_intent: list[TimingIntent] = ["legacy"]
    audio = Mock()
    audio.output_sample_rate.return_value = 48_000
    audio.load_sample_async.return_value = 7
    audio.current_constant_timing.return_value = None

    def declare_intent(_sample_id: int, declared: TimingIntent) -> None:
        native_intent[0] = declared

    def publish_legacy_value(_sample_id: int, _value: float | None) -> None:
        native_intent[0] = "legacy"

    audio.set_pad_bpm.side_effect = publish_legacy_value
    audio.set_pad_timing_metadata.side_effect = publish_legacy_value
    audio.set_pad_timing_intent.side_effect = declare_intent
    audio.pad_timing_intent.side_effect = lambda _sample_id: native_intent[0]
    session = SessionState()
    transport = TransportController(project, session, audio)
    loader = LoaderController(project, session, audio, transport.bpm.on_pad_bpm_changed)
    loader.restore_samples_from_project_state()
    transport.apply_project_state_to_audio()
    assert native_intent[0] == intent
    audio.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 7,
            "cached_path": "samples/source.wav",
            "duration_s": 600.0,
        },
        None,
    ]

    loader.poll_loader_events()
    assert native_intent[0] == intent
    audio.set_pad_timing_intent.assert_called_with(0, intent)
    audio.capture_saved_constant_timing.assert_not_called()

    persistence = ProjectPersistence(project)
    persistence.bind_audio(audio)
    persistence.flush()
    restored = ProjectPersistence.from_config_path().project
    assert restored.pad_timing_intent[0] == intent
    assert restored.manual_bpm[0] is None
    analysis = restored.sample_analysis[0]
    if analysis_present:
        assert analysis is not None
        assert analysis.accepted_timing is None
    else:
        assert analysis is None
    audio.export_current_constant_timing.assert_not_called()


@pytest.mark.parametrize("stale_request", [False, True])
def test_stale_loader_completion_cannot_capture_saved_timing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, stale_request: bool
) -> None:
    monkeypatch.chdir(tmp_path)
    (tmp_path / "samples").mkdir()
    write_mono_pcm16_wav(tmp_path / "samples/source.wav", 48_000)
    audio = Mock()
    audio.output_sample_rate.return_value = 48_000
    audio.load_sample_async.return_value = 7
    changed = Mock()
    loader = LoaderController(_project(), SessionState(), audio, changed)
    loader.restore_samples_from_project_state()
    audio.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 6 if stale_request else 7,
            "timing_stale": not stale_request,
            "cached_path": "samples/source.wav",
            "duration_s": 600.0,
        },
        None,
    ]

    loader.poll_loader_events()

    audio.capture_saved_constant_timing.assert_not_called()
    assert not loader._accepted_restore._pending
    changed.assert_not_called()


class _Worker:
    def __init__(self, future: Future[ConstantTimingTicket]) -> None:
        self.future = future

    def submit(self, _function: object, _capture: object) -> Future[ConstantTimingTicket]:
        return self.future

    def shutdown(self, *, wait: bool, cancel_futures: bool) -> None:
        assert wait
        assert cancel_futures


def _restoration(
    monkeypatch: pytest.MonkeyPatch,
) -> tuple[AcceptedTimingRestore, Mock, Mock, Future[ConstantTimingTicket]]:
    project = _project()
    audio = Mock()
    audio.pad_timing_intent.return_value = "automatic"
    audio.current_constant_timing.return_value = None
    future: Future[ConstantTimingTicket] = Future()
    monkeypatch.setattr(
        "flitzis_looper.controller.accepted_restore.ThreadPoolExecutor",
        lambda **_kwargs: _Worker(future),
    )
    adopted = Mock()
    restore = AcceptedTimingRestore(project, SessionState(), audio, adopted)
    restore.begin(0)
    analysis = project.sample_analysis[0]
    assert analysis is not None
    assert analysis.accepted_timing is not None
    audio.capture_saved_constant_timing.assert_called_once_with(
        0, analysis.accepted_timing.model_dump_json(), "samples/source.wav"
    )
    return restore, audio, adopted, future


def test_restore_needs_ack_and_matching_fresh_request_before_refresh(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    restore, audio, adopted, future = _restoration(monkeypatch)
    restore.poll()
    adopted.assert_not_called()
    ticket = Mock()
    ticket.publication_status.return_value = "pending"
    ticket.metadata.return_value = {"request_id": 17}
    ticket.accepted_metadata.return_value = {"revision": "complete-revision"}
    future.set_result(ticket)
    restore.poll()
    adopted.assert_not_called()
    ticket.publication_status.return_value = "accepted"
    audio.current_constant_timing.return_value = {
        "revision": "complete-revision",
        "accepted_request_id": 17,
    }
    restore.poll()
    adopted.assert_called_once_with(0)
    assert not restore._pending


@pytest.mark.parametrize("failure", ["worker", "rejected", "historical", "newer_request", "manual"])
def test_failed_stale_or_historical_restore_never_refreshes(
    monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    restore, audio, adopted, future = _restoration(monkeypatch)
    ticket = Mock()
    ticket.publication_status.return_value = "rejected" if failure == "rejected" else "accepted"
    ticket.metadata.return_value = {"request_id": 17}
    ticket.accepted_metadata.return_value = {"revision": "complete-revision"}
    if failure == "worker":
        future.set_exception(ValueError("stale captured source"))
    else:
        future.set_result(ticket)
    if failure == "newer_request":
        audio.current_constant_timing.return_value = {
            "revision": "complete-revision",
            "accepted_request_id": 19,
        }
    if failure == "manual":
        audio.pad_timing_intent.return_value = "manual"
    restore.poll()
    adopted.assert_not_called()
    assert not restore._pending


def test_restore_shutdown_cancels_queued_work_and_drains_owned_worker(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    shutdown = Mock()
    monkeypatch.setattr(_Worker, "shutdown", shutdown)
    restore, _audio, adopted, future = _restoration(monkeypatch)

    restore.shut_down()

    shutdown.assert_called_once_with(wait=True, cancel_futures=True)
    assert future.cancelled()
    assert not restore._pending
    assert restore._worker is None
    adopted.assert_not_called()
