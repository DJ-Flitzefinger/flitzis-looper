"""Exercise retained download bytes with in-memory HTTP responses, never the network."""

import hashlib
import io
import time
import urllib.error
import urllib.request
from email.message import Message
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import acquisition
from flitzis_looper.controller import bs_roformer_assets

if TYPE_CHECKING:
    from pathlib import Path


class Response(io.BytesIO):
    url = acquisition.CHECKPOINT_URL
    status = 200
    headers: dict[str, str] = {}


class Interrupted(Response):
    def read(self, size: int | None = -1) -> bytes:
        if self.tell():
            message = "connection interrupted"
            raise OSError(message)
        return super().read(3)


def test_transient_download_resumes_owned_prefix_and_has_no_total_deadline(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    content = b"abcdef"
    resumed = Response(b"def")
    resumed.status = 206
    resumed.headers = {"Content-Range": "bytes 3-5/6"}
    responses = iter((Interrupted(content), resumed))
    ranges: list[str | None] = []
    waits: list[int] = []

    def opened(request: urllib.request.Request, timeout: int) -> Response:
        assert timeout == 30
        ranges.append(request.get_header("Range"))
        return next(responses)

    monkeypatch.setattr(urllib.request, "urlopen", opened)
    monkeypatch.setattr(time, "sleep", waits.append)
    monkeypatch.setattr(time, "monotonic", lambda: pytest.fail("total deadline"))
    target = tmp_path / "checkpoint"
    result = acquisition.acquire_pinned_asset(
        target,
        source_url=Response.url,
        sha256=hashlib.sha256(content).hexdigest(),
        size_bytes=len(content),
    )
    assert result.path == target
    assert target.read_bytes() == content
    assert ranges == [None, "bytes=3-"]
    assert waits == [20]


def test_permanent_unavailable_retains_partial_and_later_retry_verifies_ignored_range(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    content = b"abcdef"
    requests: list[str | None] = []
    waits: list[int] = []

    def unavailable(request: urllib.request.Request, timeout: int) -> Response:
        requests.append(request.get_header("Range"))
        if len(requests) == 1:
            return Interrupted(content)
        raise urllib.error.HTTPError(request.full_url, 404, "missing fixture", Message(), None)

    monkeypatch.setattr(urllib.request, "urlopen", unavailable)
    monkeypatch.setattr(time, "sleep", waits.append)
    target = tmp_path / "checkpoint"
    checksum = hashlib.sha256(content).hexdigest()
    with pytest.raises(urllib.error.HTTPError, match="404"):
        acquisition.acquire_pinned_asset(
            target, source_url=Response.url, sha256=checksum, size_bytes=len(content)
        )
    partial = tmp_path / "checkpoint.download.partial"
    assert partial.read_bytes() == b"abc"
    assert waits == [20]
    monkeypatch.setattr(urllib.request, "urlopen", lambda *_args, **_kwargs: Response(content))
    acquisition.acquire_pinned_asset(
        target, source_url=Response.url, sha256=checksum, size_bytes=len(content)
    )
    assert target.read_bytes() == content
    assert not partial.exists()


def test_changed_download_identity_and_wrong_pins_preserve_all_prior_bytes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    target = tmp_path / "checkpoint"
    target.write_bytes(b"old installed bytes")
    monkeypatch.setattr(urllib.request, "urlopen", lambda *_args, **_kwargs: Response(b"bad"))
    with pytest.raises(ValueError, match="integrity check"):
        acquisition.acquire_pinned_asset(
            target, source_url=Response.url, sha256="0" * 64, size_bytes=3
        )
    assert target.read_bytes() == b"old installed bytes"
    assert (tmp_path / "checkpoint.download.partial").read_bytes() == b"bad"
    with pytest.raises(ValueError, match="different URL or pin"):
        acquisition.acquire_pinned_asset(
            target, source_url=Response.url + "?different", sha256="0" * 64, size_bytes=3
        )
    assert target.read_bytes() == b"old installed bytes"


def test_bs_installer_uses_resumable_full_pins_and_verifies_installed_bytes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    payload = b"tiny fixture"
    filename = "model.ckpt"
    checksum = hashlib.sha256(payload).hexdigest()
    monkeypatch.setattr(bs_roformer_assets, "MODEL_ASSETS", ((filename, len(payload), checksum),))
    monkeypatch.setattr(urllib.request, "urlopen", lambda *_args, **_kwargs: Response(payload))
    bs_roformer_assets.install_model_assets(tmp_path)
    bs_roformer_assets.verify_model_assets(tmp_path)
    assert (tmp_path / filename).read_bytes() == payload


@pytest.mark.parametrize("suffix", [".partial", ".source.json", ".lock"])
def test_unknown_download_directories_are_preserved_and_fail_before_network(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, suffix: str
) -> None:
    target = tmp_path / "checkpoint"
    unknown = tmp_path / (target.name + suffix)
    unknown.mkdir()
    sentinel = unknown / "human.bin"
    sentinel.write_bytes(b"unknown original")
    monkeypatch.setattr(
        urllib.request, "urlopen", lambda *_args, **_kwargs: pytest.fail("attempted network")
    )
    monkeypatch.setattr(time, "sleep", lambda _: pytest.fail("retried local directory"))
    with pytest.raises(ValueError, match="must be regular files"):
        acquisition.acquire_to_quarantine(target)
    assert sentinel.read_bytes() == b"unknown original"
    assert not target.exists()
