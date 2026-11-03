import json
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import audio_workload
from flitzis_looper.controller.audio_workload import AudioWorkloadCapture
from flitzis_looper.models import ProjectState, SessionState

if TYPE_CHECKING:
    from flitzis_looper_audio import AudioEngine

type CaptureFixture = tuple[AudioWorkloadCapture, Mock, list[float]]


@pytest.fixture
def capture(monkeypatch: pytest.MonkeyPatch) -> CaptureFixture:
    clock = [0.0]
    monkeypatch.setattr(audio_workload, "monotonic", lambda: clock[0])
    monkeypatch.setattr(audio_workload, "runtime_artifact_identity", lambda: {"fixture": True})
    audio = Mock()
    audio.output_device_descriptor.return_value = {
        "sample_rate_hz": 48_000,
        "channels": 2,
        "requested_buffer_frames": 512,
        "path": "PRIVATE",
    }
    audio.request_loop_acceptance_snapshot.side_effect = [256, 513, 768]
    owner = AudioWorkloadCapture(
        ProjectState(), SessionState(active_sample_ids={0, 1}), cast("AudioEngine", audio)
    )
    return owner, audio, clock


def result(owner: AudioWorkloadCapture) -> dict[str, object]:
    assert owner.report_text is not None
    return json.loads(owner.report_text)


def test_serial_capture_keeps_geometry_separate_from_unavailable_voice(
    capture: CaptureFixture,
) -> None:
    owner, audio, _ = capture
    audio.loop_acceptance_snapshot.side_effect = [
        None,
        {
            "request_id": 256,
            "status": "unavailable",
            "fresh": True,
            "current_acknowledged": False,
            "reason": "effective voice has no current accepted source",
            "callback_frames": 960,
            "native_pitch_scale": 999,
            "current_binding": {"path": "PRIVATE"},
        },
        {
            "request_id": 513,
            "status": "available",
            "fresh": True,
            "current_acknowledged": True,
            "callback_frames": 480,
            "applied_source_rate": 1.2,
            "native_pitch_scale": 1 / 1.2,
            "current_binding": {"path": "PRIVATE"},
        },
    ]
    owner.start()
    owner.poll()  # Request PAD 0.
    owner.start()  # No overlapping capture/request.
    owner.poll()  # Still pending.
    assert audio.request_loop_acceptance_snapshot.call_count == 1
    owner.poll()  # Receive PAD 0 before requesting PAD 1.
    assert audio.request_loop_acceptance_snapshot.call_count == 1
    owner.poll()
    owner.poll()
    owner.poll()
    report = result(owner)
    assert report["capture_status"] == "complete"
    assert report["output_device"]["requested_buffer_frames"] == 512
    first, second = report["observations"]
    assert first["callback"]["callback_frames"] == 960
    assert "effective_voice" not in first
    assert second["effective_voice"]["applied_source_rate"] == 1.2
    assert "PRIVATE" not in owner.report_text
    audio.stop_all.assert_not_called()
    audio.run.assert_not_called()


@pytest.mark.parametrize("kind", ["stale", "revoked", "multiple"])
def test_unavailable_voice_never_becomes_effective_in_report(
    capture: CaptureFixture, kind: str
) -> None:
    owner, audio, _ = capture
    audio.loop_acceptance_snapshot.return_value = {
        "request_id": 256,
        "status": "unavailable",
        "fresh": kind != "stale",
        "current_acknowledged": False,
        "reason": kind,
        "applied_source_rate": 1.2,
        "callback_frames": 128,
    }
    owner.start()
    owner.poll()
    owner.poll()
    owner.cancel()
    first = result(owner)["observations"][0]
    assert first["callback"]["callback_frames"] == 128
    assert "effective_voice" not in first


def test_pending_timeout_does_not_retry_and_total_deadline_lists_remainder(
    capture: CaptureFixture,
) -> None:
    owner, audio, clock = capture
    audio.loop_acceptance_snapshot.return_value = None
    owner.start()
    owner.poll()
    clock[0] = 0.3
    owner.poll()
    assert audio.request_loop_acceptance_snapshot.call_count == 1
    clock[0] = 5.0
    owner.poll()
    report = result(owner)
    assert report["observations"][0]["status"].startswith("timeout")
    assert report["observations"][1]["status"] == "not attempted: capture deadline reached"
    assert not owner.active
    owner.poll()
    assert audio.request_loop_acceptance_snapshot.call_count == 1


def test_cancel_ignores_late_reply_and_repeat_replaces_report(capture: CaptureFixture) -> None:
    owner, audio, _ = capture
    owner.start()
    owner.poll()
    owner.cancel("application closing")
    before = owner.report_text
    owner.poll()
    assert owner.report_text == before
    audio.loop_acceptance_snapshot.assert_not_called()
    owner.start()
    assert owner.report_text is None
    owner.poll()
    owner.cancel()
    assert len(result(owner)["observations"]) == 2


def test_frozen_control_selection_intent_changes_and_request_identity(
    capture: CaptureFixture,
) -> None:
    owner, audio, _ = capture
    owner.start()
    owner._session.active_sample_ids = {4}
    owner.poll()
    owner._project.pad_gain_db[0] = -6
    audio.loop_acceptance_snapshot.return_value = {
        "request_id": 999,
        "fresh": True,
        "current_acknowledged": True,
        "native_pitch_scale": 2.0,
        "callback_frames": 512,
    }
    owner.poll()
    owner.cancel()
    report = result(owner)
    first = report["observations"][0]
    assert report["pad_ids"] == [0, 1]
    assert report["control_end"]["active_pad_ids"] == [4]
    assert first["control_intent_at_request"]["gain_db"] == 0
    assert first["control_intent_at_read"]["gain_db"] == -6
    assert "callback" not in first
    assert "effective_voice" not in first
    assert first["current_acknowledged"] is False


def test_empty_selection_still_observes_one_existing_callback(capture: CaptureFixture) -> None:
    owner, audio, _ = capture
    owner._session.active_sample_ids.clear()
    owner._project.selected_pad = 5
    owner.start()
    owner.poll()
    audio.request_loop_acceptance_snapshot.assert_called_once_with(5)
    owner.cancel()
