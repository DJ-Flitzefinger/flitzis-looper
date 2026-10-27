"""Bounded Unicode IPC and actual exact-child cancellation, never a real picker."""

import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import Mock

import pytest

from flitzis_looper import file_picker, file_picker_worker


def picker_with_payload(
    monkeypatch: pytest.MonkeyPatch, payload: bytes, code: int = 0
) -> file_picker.AudioFilePicker:
    def spawn(args: list[str], **_kwargs: object) -> Mock:
        Path(args[-1]).write_bytes(payload)
        return Mock(returncode=code, poll=Mock(return_value=code))

    monkeypatch.setattr(file_picker.subprocess, "Popen", spawn)
    return file_picker.AudioFilePicker()


@pytest.mark.parametrize(
    "path", ["C:/SCRATCHBEATS/Veteran\u2019s Memorial.flac", "C:/東京/🥁 e\u0301.FLAC"]
)
def test_unicode_ipc_preserves_exact_selected_path(
    monkeypatch: pytest.MonkeyPatch, path: str
) -> None:
    picker = picker_with_payload(
        monkeypatch,
        json.dumps({"status": "selected", "path": path}, ensure_ascii=False).encode("utf-8"),
    )
    assert picker.ready()
    assert picker.result() == path
    assert not picker._result_path.exists()


@pytest.mark.parametrize(
    "payload",
    [
        b"[]",
        b"{",
        b"\x92",
        b'{"status":"selected","path":1}',
        b'{"status":"selected","path":"relative.wav"}',
        b'{"status":"selected","path":"C:/bad\\u0000.wav"}',
        b'{"status":"selected","path":"C:/ok.wav","extra":true}',
        b'{"status":"cancelled","status":"selected","path":"C:/ok.wav"}',
        b"x" * (128 * 1024 + 1),
    ],
    ids=[
        "nonobject",
        "truncated",
        "bad-utf8",
        "path-type",
        "relative",
        "nul",
        "extra",
        "duplicate",
        "oversized",
    ],
)
def test_invalid_or_oversized_ipc_never_yields_a_path(
    monkeypatch: pytest.MonkeyPatch, payload: bytes
) -> None:
    picker = picker_with_payload(monkeypatch, payload)
    with pytest.raises((ValueError, TypeError), match=r"result|Expecting|utf-8|fields|byte limit"):
        picker.result()
    assert not picker._result_path.exists()


def test_cancellation_wins_over_terminal_exit_zero_and_a_valid_late_result(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    payload = b'{"status":"selected","path":"C:/late.wav"}'
    picker = picker_with_payload(monkeypatch, payload)
    assert picker.cancel()
    assert picker.result() is None
    picker = picker_with_payload(monkeypatch, payload, code=1)
    with pytest.raises(RuntimeError, match="process failed"):
        picker.result()


def test_actual_owned_child_is_terminated_before_its_result_can_be_consumed(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    actual_spawn = subprocess.Popen

    def spawn(_args: list[str], **kwargs: object) -> subprocess.Popen[bytes]:
        return actual_spawn([sys.executable, "-c", "import time; time.sleep(120)"], **kwargs)  # type: ignore[arg-type]

    monkeypatch.setattr(file_picker.subprocess, "Popen", spawn)
    picker = file_picker.AudioFilePicker()
    try:
        assert not picker.ready()
        assert picker.cancel(wait=True)
        assert picker.ready()
        assert picker.cancelled
        assert picker.result() is None
        assert not picker._result_path.exists()
    finally:
        picker.cancel(wait=True)


def test_terminal_cleanup_failure_is_reported_without_pretending_a_child_is_live(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    picker = picker_with_payload(monkeypatch, b'{"status":"selected","path":"C:/ok.wav"}')
    owned = picker._result_path
    actual_unlink = Path.unlink

    def denied(path: Path, *, missing_ok: bool = False) -> None:
        if path == owned:
            message = "owned IPC cleanup unavailable"
            raise PermissionError(message)
        actual_unlink(path, missing_ok=missing_ok)

    with monkeypatch.context() as patch:
        patch.setattr(Path, "unlink", denied)
        assert picker.cancel()
        assert picker.ready()
        assert picker.cleanup_error is not None
        assert "cleanup unavailable" in picker.cleanup_error
        assert owned.exists()
    owned.unlink()


def test_failed_termination_keeps_ownership_and_cancellation_fence(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def spawn(_args: list[str], **_kwargs: object) -> Mock:
        return Mock(
            returncode=None,
            poll=Mock(return_value=None),
            terminate=Mock(side_effect=PermissionError("owned terminate denied")),
        )

    monkeypatch.setattr(file_picker.subprocess, "Popen", spawn)
    picker = file_picker.AudioFilePicker()
    try:
        with pytest.raises(PermissionError, match="terminate denied"):
            picker.cancel()
        assert picker.cancelled
        assert not picker.ready()
        assert picker._result_path.exists()
    finally:
        # The fake represents no actual child; retire its exact owned IPC file.
        picker._process.poll.return_value = 0  # type: ignore[attr-defined]
        picker.cancel()


@pytest.mark.parametrize(
    "selected", [[], ["C:/SCRATCHBEATS/Veteran\u2019s Memorial.flac"], ["C:/東京/🥁.FLAC"]]
)
def test_worker_writes_exact_utf8_selection_or_cancel(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    selected: list[str],
) -> None:
    output = tmp_path / "result.json"
    monkeypatch.setattr(sys, "argv", ["file_picker_worker", str(output)])
    factory = Mock(return_value=Mock(result=Mock(return_value=selected)))
    monkeypatch.setattr(file_picker_worker.pfd, "open_file", factory)
    file_picker_worker.main()
    value = json.loads(output.read_text(encoding="utf-8"))
    assert value == (
        {"status": "selected", "path": selected[0]} if selected else {"status": "cancelled"}
    )
    assert factory.call_args.kwargs["options"] == file_picker_worker.pfd.opt.none


def test_actual_worker_import_does_not_import_app_controller_or_audio_modules() -> None:
    check = (
        "import sys; import flitzis_looper.file_picker_worker; "
        "assert not any(n.startswith(('flitzis_looper.controller', 'flitzis_looper.ui', "
        "'flitzis_looper_audio')) for n in sys.modules)"
    )
    result = subprocess.run(
        [sys.executable, "-c", check],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        check=False,
        timeout=15,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )
    assert result.returncode == 0, result.stderr.decode("utf-8", errors="replace")
