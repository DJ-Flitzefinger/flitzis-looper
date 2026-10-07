import time
from typing import TYPE_CHECKING

import pytest

from flitzis_looper_audio import AudioEngine

if TYPE_CHECKING:
    from pathlib import Path


def _wait_for_deletion(path: Path, audio: AudioEngine) -> None:
    deadline = time.monotonic() + 5
    while path.exists() and time.monotonic() < deadline:
        time.sleep(0.01)
    assert not path.exists(), audio.project_asset_cleanup_status()


def test_uninitialized_native_registry_waits_for_last_assignment_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    source = tmp_path / "samples" / "owned.wav"
    source.parent.mkdir()
    source.write_bytes(b"owned complete original")
    audio = AudioEngine()
    first = audio.acquire_project_asset_lease(str(source))
    second = audio.acquire_project_asset_lease(str(source))
    audio.retire_project_asset(str(source))

    first.release()
    assert first.released
    assert not second.released
    assert source.exists()
    second.release()
    _wait_for_deletion(source, audio)
    assert not any(str(source) in error for error in audio.project_asset_cleanup_status()[3])
    audio.shut_down()


def test_native_shutdown_keeps_late_python_reader_and_cleans_after_release(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    source = tmp_path / "samples" / "job-original.wav"
    source.parent.mkdir()
    source.write_bytes(b"backend still reading")
    audio = AudioEngine()
    job = audio.acquire_project_asset_lease(str(source))
    audio.retire_project_asset(str(source))

    audio.shut_down()
    assert source.read_bytes() == b"backend still reading"
    job.release()
    _wait_for_deletion(source, audio)


def test_native_retirement_cannot_delete_external_original_or_unknown_pad_container(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    pad = samples / "stems" / "#1"
    pad.mkdir(parents=True)
    private = pad / "private.txt"
    private.write_bytes(b"private")
    external = tmp_path / "external.wav"
    external.write_bytes(b"external source")
    audio = AudioEngine()

    with pytest.raises((RuntimeError, ValueError)):
        audio.retire_project_asset(str(external))
    with pytest.raises((RuntimeError, ValueError)):
        audio.retire_project_asset(str(pad), recursive=True)
    assert external.read_bytes() == b"external source"
    assert private.read_bytes() == b"private"
    audio.shut_down()
