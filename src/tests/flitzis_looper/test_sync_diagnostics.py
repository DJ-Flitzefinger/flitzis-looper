"""Trace failures/limits must not change audio or saved performer intent."""

import json
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

from flitzis_looper import sync_diagnostics as module
from flitzis_looper.models import BeatGrid, ProjectState, SampleAnalysis, SessionState
from flitzis_looper.sync_diagnostics import TimingTrace

if TYPE_CHECKING:
    from pathlib import Path

    import pytest

    from flitzis_looper_audio import AudioEngine


def fixture(root: Path) -> tuple[TimingTrace, Mock, ProjectState]:
    audio = Mock()
    audio.SYNC_DIAGNOSTIC_API_VERSION = 1
    audio.capture_input_timestamp_ns.return_value = 1234
    audio.request_sync_diagnostic_snapshot.side_effect = range(1, 100)
    audio.sync_diagnostic_snapshot.return_value = {
        "schema": "sync-diagnostic-cohort-v1",
        "voices": [],
    }
    audio.pad_source_sync_status.return_value = None
    audio.pad_key_lock_status.return_value = None
    project = ProjectState()
    project.sample_paths[1] = "D:/private/SECRET-SONG.wav"
    project.sample_analysis[1] = SampleAnalysis(
        bpm=94, key="C", beat_grid=BeatGrid(beats=[0, 0.64], downbeats=[0], bars=[0])
    )
    return TimingTrace(cast("AudioEngine", audio), project, SessionState(), root), audio, project


def records(trace: TimingTrace) -> list[dict[str, object]]:
    assert trace.path is not None
    return [json.loads(line) for line in trace.path.read_text().splitlines()]


def test_opt_in_bounded_poll_marker_flush_and_private_source_exclusion(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    now = [100.0]
    monkeypatch.setattr(module, "monotonic", lambda: now[0])
    trace, audio, project = fixture(tmp_path / "diagnostics")
    before = project.model_dump_json()
    trace.poll()
    assert not tmp_path.joinpath("diagnostics").exists()
    audio.assert_not_called()
    trace.start()
    trace.poll()
    for _ in range(20):
        trace.poll()
    assert audio.request_sync_diagnostic_snapshot.call_count == 1
    now[0] = 100.21
    trace.poll()
    trace.mark_drift()
    assert audio.request_sync_diagnostic_snapshot.call_count == 2
    now[0] = 110
    trace.poll()  # No catch-up burst.
    assert audio.request_sync_diagnostic_snapshot.call_count == 3
    trace.close()
    events = records(trace)
    assert events[0]["event"] == "start"
    assert events[-1]["event"] == "stop"
    assert sum(event["event"] == "control_state" for event in events) == 1
    assert any(event["event"] == "cohort" for event in events)
    assert any(event["event"] == "heard_drift" for event in events)
    assert "SECRET-SONG" not in trace.path.read_text()
    assert project.model_dump_json() == before
    forbidden = {"stop_all", "set_speed", "set_master_bpm", "set_pad_loop_region", "set_key_lock"}
    assert not forbidden.intersection(call[0] for call in audio.method_calls)


def test_io_failure_and_old_native_disable_only_trace(tmp_path: Path) -> None:
    blocked = tmp_path / "file"
    blocked.write_text("keep")
    trace, audio, _ = fixture(blocked / "diagnostics")
    trace.start()
    trace.close()
    assert trace.error is not None
    assert not trace.active
    audio.stop_all.assert_not_called()
    audio.SYNC_DIAGNOSTIC_API_VERSION = 0
    trace.start()
    assert "updated Native" in (trace.error or "")


def test_limits_pending_gaps_and_nonfinite_values_are_explicit(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    now = [1.0]
    monkeypatch.setattr(module, "monotonic", lambda: now[0])
    trace, audio, _ = fixture(tmp_path)
    audio.sync_diagnostic_snapshot.return_value = None
    trace.start()
    trace.poll()
    now[0] = 2.1
    trace.poll()
    now[0] = 602
    trace.poll()
    trace.close()
    events = records(trace)
    assert any(event.get("reason") == "callback_snapshot_timeout" for event in events)
    assert events[-1]["reason"] == "duration_limit"
    assert module._finite({"bad": float("nan")}) == {"bad": None}


def test_byte_and_queue_limits_retain_explicit_footer(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    trace, _, _ = fixture(tmp_path)
    monkeypatch.setattr(module, "MAX_BYTES", 4096)
    trace.start()
    trace._emit("oversized", value="a" * 5000)
    trace.close()
    assert records(trace)[-1]["reason"] == "byte_limit"
    assert trace.path is not None
    assert trace.path.stat().st_size <= 4096
    other, _, _ = fixture(tmp_path / "unused")
    for _ in range(65):
        other._emit("fill")
    assert other._dropped == 1
    assert other._stop.is_set()


def test_pending_auxiliary_control_never_blocks_active_cohort(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    now = [1.0]
    monkeypatch.setattr(module, "monotonic", lambda: now[0])
    trace, audio, _ = fixture(tmp_path)
    monkeypatch.setattr(trace, "_control_state", Mock(side_effect=RuntimeError("PRIVATE PATH")))
    audio.pad_source_sync_status.side_effect = RuntimeError("pending foreign pad")
    trace.start()
    trace.poll()
    now[0] = 1.21
    trace.poll()
    trace.close()
    assert audio.request_sync_diagnostic_snapshot.call_count == 2
    audio.pad_source_sync_status.assert_not_called()
    assert any(event["event"] == "cohort" for event in records(trace))
    assert "PRIVATE PATH" not in trace.path.read_text()


def test_writer_thread_start_failure_is_isolated_and_close_safe(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    trace, audio, _ = fixture(tmp_path)
    monkeypatch.setattr(
        module.Thread, "start", Mock(side_effect=RuntimeError("resource unavailable"))
    )
    trace.start()
    trace.close()
    assert not trace.active
    assert trace._thread is None
    assert trace.path is None
    assert trace.error == "Timing log could not start its writer. Playback is unchanged."
    assert audio.method_calls == []
