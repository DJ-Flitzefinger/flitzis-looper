"""Explicit acquisition, provenance rejection and atomic installation regressions."""

import hashlib
import io
import json
import platform
import shutil
import subprocess
import sys
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, replace
from threading import Event
from typing import TYPE_CHECKING, BinaryIO

import pytest

from flitzis_looper.analysis import acquisition, artifacts, setup
from flitzis_looper.analysis.contracts import AnalysisIdentity, BeatWorkerRequest, MonoPcmInput
from flitzis_looper.analysis.worker import BeatWorkerAdapter, WorkerConfiguration

if TYPE_CHECKING:
    from pathlib import Path


class _Response(io.BytesIO):
    url = acquisition.CHECKPOINT_URL


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
    result = acquisition.acquire_to_quarantine(destination)

    assert destination.read_bytes() == content
    assert result.sha256 == hashlib.sha256(content).hexdigest()
    assert result.size_bytes == len(content)
    assert requested == [acquisition.CHECKPOINT_URL]
    assert list(tmp_path.iterdir()) == [destination]


@pytest.mark.parametrize("content", [b"", b"x" * 17])
def test_empty_and_oversize_downloads_remove_only_their_partial_output(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, content: bytes
) -> None:
    monkeypatch.setattr(urllib.request, "urlopen", lambda *args, **kwargs: _Response(content))
    previous = tmp_path / "previous.ckpt"
    previous.write_bytes(b"previous installed artifact")
    with pytest.raises(ValueError, match=r"empty artifact|exceeded"):
        acquisition.acquire_to_quarantine(tmp_path / "incomplete.ckpt", max_bytes=16)
    assert previous.read_bytes() == b"previous installed artifact"
    assert not (tmp_path / "incomplete.ckpt").exists()


def test_local_quarantine_never_overwrites_a_previous_artifact(tmp_path: Path) -> None:
    source = tmp_path / "source.ckpt"
    destination = tmp_path / "previous.ckpt"
    source.write_bytes(b"new bytes")
    destination.write_bytes(b"previous bytes")
    with pytest.raises(FileExistsError):
        acquisition.copy_to_quarantine(source, destination)
    assert destination.read_bytes() == b"previous bytes"


def test_failed_stream_removes_partial_quarantine(tmp_path: Path) -> None:
    class BrokenStream(io.BytesIO):
        def read(self, size: int | None = -1) -> bytes:
            if self.tell():
                msg = "fixture connection interrupted"
                raise OSError(msg)
            return super().read(2)

    with pytest.raises(OSError, match="interrupted"):
        acquisition._copy_quarantined(BrokenStream(b"1234"), tmp_path / "partial.ckpt", 10)
    assert not (tmp_path / "partial.ckpt").exists()


@dataclass(frozen=True)
class _SetupFixture:
    install: Path
    scratch: Path
    checkpoint: Path
    source: Path


@pytest.fixture
def prepared(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> _SetupFixture:
    source = tmp_path / "worker-source"
    source.mkdir()
    for filename in artifacts.WORKER_FILES:
        shutil.copyfile(artifacts.worker_source_directory() / filename, source / filename)
    checkpoint = tmp_path / "fixture.ckpt"
    checkpoint.write_bytes(b"deterministic fixture, not neural weights")
    monkeypatch.setattr(artifacts, "worker_source_directory", lambda: source)
    monkeypatch.setattr(
        artifacts,
        "ACCEPTED_ARTIFACT",
        replace(
            artifacts.ACCEPTED_ARTIFACT,
            sha256=hashlib.sha256(checkpoint.read_bytes()).hexdigest(),
            size_bytes=checkpoint.stat().st_size,
        ),
    )
    monkeypatch.setattr(sys, "platform", "win32")
    monkeypatch.setattr(platform, "machine", lambda: "AMD64")
    monkeypatch.setattr(setup, "_provision_environment", _fake_provision)
    return _SetupFixture(tmp_path / "installed", tmp_path / "scratch", checkpoint, source)


def _fake_provision(staging: Path, scratch: Path, *, offline: bool) -> None:
    interpreter = staging / ".venv" / "Scripts" / "python.exe"
    interpreter.parent.mkdir(parents=True)
    interpreter.write_bytes(b"test interpreter identity")
    (staging / ".venv" / "pyvenv.cfg").write_text("version = 3.12.13\n", encoding="utf-8")
    packages = artifacts.expected_packages(staging)
    for name, version in packages.items():
        metadata = staging / ".venv" / "Lib" / "site-packages" / f"{name}.dist-info"
        metadata.mkdir(parents=True)
        (metadata / "METADATA").write_text(f"Name: {name}\nVersion: {version}\n", encoding="utf-8")
        (metadata / "RECORD").write_text("fixture record\n", encoding="utf-8")
    package = staging / ".venv" / "Lib" / "site-packages" / "beat_this"
    package.mkdir()
    (package / "inference.py").write_text("# fixture inference source\n", encoding="utf-8")
    lock_digest = artifacts.file_digest(staging / "uv.lock")
    report = artifacts.EnvironmentManifest(
        1,
        artifacts.FRONTEND_ID,
        f"uv-lock-sha256:{lock_digest}",
        "3.12.13",
        packages,
        lock_digest,
        artifacts.file_digest(staging / "worker.py"),
    )
    (staging / "environment.json").write_bytes(artifacts.ENVIRONMENT_ADAPTER.dump_json(report))


def _install(prepared: _SetupFixture) -> WorkerConfiguration:
    return setup.install_worker(
        prepared.install,
        prepared.scratch,
        checkpoint_file=prepared.checkpoint,
        offline=True,
    )


def test_explicit_offline_install_publishes_only_complete_verified_configuration(
    prepared: _SetupFixture, monkeypatch: pytest.MonkeyPatch
) -> None:
    def no_network(*args: object, **kwargs: object) -> None:
        pytest.fail("offline installation attempted networking")

    monkeypatch.setattr(urllib.request, "urlopen", no_network)
    configuration = _install(prepared)

    assert configuration.manifest is not None
    assert configuration.checkpoint.read_bytes() == prepared.checkpoint.read_bytes()
    assert configuration.model.sha256 == artifacts.ACCEPTED_ARTIFACT.sha256
    assert configuration.model.frontend_id == artifacts.FRONTEND_ID
    assert setup.load_worker_configuration(prepared.install, prepared.scratch) == configuration
    assert list(prepared.install.glob(".staging-*")) == []
    assert list(prepared.install.glob(".current-*")) == []
    assert len(list(prepared.install.glob("installation-*"))) == 1


@pytest.mark.parametrize("failure", ["hash", "missing", "environment", "provision"])
def test_failed_setup_preserves_previous_atomic_install(
    prepared: _SetupFixture, monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    previous = _install(prepared)
    pointer_before = (prepared.install / "current.json").read_bytes()
    if failure == "hash":
        prepared.checkpoint.write_bytes(b"corrupt downloaded bytes")
    elif failure == "missing":
        prepared.checkpoint.unlink()
    else:

        def fail_provision(staging: Path, scratch: Path, *, offline: bool) -> None:
            if failure == "provision":
                raise subprocess.CalledProcessError(1, "fixture uv")
            _fake_provision(staging, scratch, offline=offline)
            environment_path = staging / "environment.json"
            report = json.loads(environment_path.read_bytes())
            report["packages"]["torch"] = "incompatible"
            environment_path.write_text(json.dumps(report), encoding="utf-8")

        monkeypatch.setattr(setup, "_provision_environment", fail_provision)
    with pytest.raises((ValueError, OSError, subprocess.CalledProcessError)):
        _install(prepared)

    assert (prepared.install / "current.json").read_bytes() == pointer_before
    assert setup.load_worker_configuration(prepared.install, prepared.scratch) == previous
    assert list(prepared.install.glob(".staging-*")) == []
    assert len(list(prepared.install.glob("installation-*"))) == 1


def test_offline_without_checkpoint_fails_before_acquisition(prepared: _SetupFixture) -> None:
    with pytest.raises(ValueError, match="offline setup requires"):
        setup.install_worker(prepared.install, prepared.scratch, offline=True)
    assert not prepared.install.exists()


@pytest.mark.parametrize(
    "corruption", ["script", "license", "package", "metadata", "manifest", "package_source"]
)
def test_installed_provenance_changes_cannot_start_inference(
    prepared: _SetupFixture, monkeypatch: pytest.MonkeyPatch, corruption: str
) -> None:
    configuration = _install(prepared)
    manifest_path = configuration.manifest
    assert manifest_path is not None
    if corruption == "script":
        configuration.script.write_text("raise SystemExit(0)\n", encoding="utf-8")
    elif corruption == "metadata":
        metadata = next(configuration.script.parent.glob(".venv/Lib/site-packages/*/METADATA"))
        metadata.unlink()
    elif corruption == "manifest":
        configuration = replace(configuration, manifest=None)
    elif corruption == "package_source":
        source = configuration.script.parent / ".venv/Lib/site-packages/beat_this/inference.py"
        source.write_text("# changed inference code\n", encoding="utf-8")
    else:
        receipt = json.loads(manifest_path.read_bytes())
        if corruption == "license":
            receipt["artifact"]["license"] = "invented"
        else:
            receipt["environment"]["packages"]["torch"] = "incompatible"
        manifest_path.write_text(json.dumps(receipt), encoding="utf-8")
    pcm = prepared.scratch / "mono.f32"
    pcm.write_bytes(bytes(400))
    request = BeatWorkerRequest(
        AnalysisIdentity(1, 1, "fixture", 1), MonoPcmInput(pcm, 22050, 100), configuration.model
    )

    def no_process(*args: object, **kwargs: object) -> None:
        pytest.fail("invalid installed provenance started a worker")

    monkeypatch.setattr(subprocess, "Popen", no_process)
    with ThreadPoolExecutor(max_workers=1) as executor:
        result = executor.submit(BeatWorkerAdapter(configuration).run, request, Event()).result(5)
    assert result.status == "unavailable"
    assert result.reason in {"installation_provenance_mismatch", "missing_installation_manifest"}


def test_install_pointer_cannot_select_arbitrary_paths(prepared: _SetupFixture) -> None:
    _install(prepared)
    pointer_path = prepared.install / "current.json"
    pointer = json.loads(pointer_path.read_bytes())
    pointer["directory"] = "../other-worker"
    pointer_path.write_text(json.dumps(pointer), encoding="utf-8")
    with pytest.raises(ValueError, match="installation_pointer_invalid"):
        setup.load_worker_configuration(prepared.install, prepared.scratch)


def test_provision_uses_locked_isolated_environment_and_explicit_offline_flag(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls: list[tuple[list[str], dict[str, object]]] = []

    def run(command: list[str], **kwargs: object) -> None:
        calls.append((command, kwargs))

    def inspect(staging: Path, log: BinaryIO) -> None:
        assert staging == tmp_path

    monkeypatch.setattr(shutil, "which", lambda _: sys.executable)
    monkeypatch.setattr(subprocess, "run", run)
    monkeypatch.setattr(setup, "_inspect_environment", inspect)
    setup._provision_environment(tmp_path, tmp_path, offline=True)
    command, kwargs = calls[0]
    assert command[1] == "sync"
    assert "--locked" in command
    assert "--offline" in command
    assert "--no-dev" in command
    assert "3.12.13" in command
    assert kwargs["check"] is True
    assert kwargs["timeout"] == 900
    environment = kwargs["env"]
    assert isinstance(environment, dict)
    assert environment["UV_PROJECT_ENVIRONMENT"] == str(tmp_path / ".venv")
