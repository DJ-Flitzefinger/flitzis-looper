"""Pinned acquisition and the selected Beat This ONNX manifest."""

import hashlib
import io
import json
import urllib.request
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import acquisition, setup

if TYPE_CHECKING:
    from pathlib import Path

_URL = "https://models.example.test/fixture.bin"


class _Response(io.BytesIO):
    url = _URL


def test_explicit_download_streams_bytes_and_measures_digest(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    content = b"bounded author-hosted artifact fixture"
    requested: list[str] = []

    def urlopen(request: urllib.request.Request, timeout: int) -> _Response:
        requested.append(request.full_url)
        assert timeout == 30
        return _Response(content)

    monkeypatch.setattr(urllib.request, "urlopen", urlopen)
    destination = tmp_path / "quarantine.ckpt"
    result = acquisition.acquire_to_quarantine(destination, source_url=_URL, max_bytes=len(content))

    assert destination.read_bytes() == content
    assert result.sha256 == hashlib.sha256(content).hexdigest()
    assert result.size_bytes == len(content)
    assert requested == [_URL]
    assert {path.name for path in tmp_path.iterdir()} == {
        destination.name,
        destination.name + ".lock",
        destination.name + ".source.json",
    }


@pytest.mark.parametrize("content", [b"", b"x" * 17])
def test_empty_and_oversize_downloads_retain_their_partial_output(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, content: bytes
) -> None:
    monkeypatch.setattr(urllib.request, "urlopen", lambda *args, **kwargs: _Response(content))
    previous = tmp_path / "previous.ckpt"
    previous.write_bytes(b"previous installed artifact")
    with pytest.raises(ValueError, match=r"empty artifact|exceeded"):
        acquisition.acquire_to_quarantine(
            tmp_path / "incomplete.ckpt", source_url=_URL, max_bytes=16
        )
    assert previous.read_bytes() == b"previous installed artifact"
    assert not (tmp_path / "incomplete.ckpt").exists()
    assert (tmp_path / "incomplete.ckpt.partial").read_bytes() == content


def test_local_quarantine_never_overwrites_a_previous_artifact(tmp_path: Path) -> None:
    source = tmp_path / "source.ckpt"
    destination = tmp_path / "previous.ckpt"
    source.write_bytes(b"new bytes")
    destination.write_bytes(b"previous bytes")
    with source.open("rb") as stream, pytest.raises(FileExistsError):
        acquisition._copy_quarantined(stream, destination, 32)
    assert destination.read_bytes() == b"previous bytes"


def test_failed_stream_retains_partial_quarantine(tmp_path: Path) -> None:
    class BrokenStream(io.BytesIO):
        def read(self, size: int | None = -1) -> bytes:
            if self.tell():
                msg = "fixture connection interrupted"
                raise OSError(msg)
            return super().read(2)

    with pytest.raises(OSError, match="interrupted"):
        acquisition._copy_quarantined(BrokenStream(b"1234"), tmp_path / "partial.ckpt", 10)
    assert (tmp_path / "partial.ckpt").read_bytes() == b"12"


def test_install_pointer_cannot_select_arbitrary_paths(tmp_path: Path) -> None:
    pointer = tmp_path / "current.json"
    pointer.write_text(
        json.dumps({"schema_version": 1, "directory": "../outside", "manifest_sha256": "0" * 64})
    )
    with pytest.raises(ValueError, match="installation_pointer_invalid"):
        setup.load_worker_configuration(tmp_path, tmp_path)


def test_retired_installation_is_not_executed_or_reselected(tmp_path: Path) -> None:
    installed = tmp_path / f"installation-{'1' * 32}"
    installed.mkdir()
    manifest = b'{"schema_version":1,"profile":"legacy-cpu-v1"}'
    (installed / "manifest.json").write_bytes(manifest)
    pointer = tmp_path / "current.json"
    saved = json.dumps({
        "schema_version": 1,
        "directory": installed.name,
        "manifest_sha256": hashlib.sha256(manifest).hexdigest(),
    })
    pointer.write_text(saved)
    with pytest.raises(ValueError, match="BeatThis requires the ONNX worker"):
        setup.load_worker_configuration(tmp_path, tmp_path, native_only=True)
    assert pointer.read_text() == saved
    assert (installed / "manifest.json").read_bytes() == manifest
