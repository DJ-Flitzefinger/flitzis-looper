"""Explicit acquisition, provenance rejection and atomic installation regressions."""

import hashlib
import io
import json
import platform
import shutil
import subprocess
import sys
import tomllib
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, replace
from pathlib import Path
from threading import Event
from typing import BinaryIO

import pytest

from flitzis_looper.analysis import acquisition, artifacts, cuda_artifacts, setup
from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatModelIdentity,
    BeatWorkerRequest,
    MonoPcmInput,
)
from flitzis_looper.analysis.execution import (
    CUDA_LOCK_SHA256,
    CudaBeatModelIdentity,
    require_cuda_native_capability,
)
from flitzis_looper.analysis.worker import BeatWorkerAdapter, WorkerConfiguration
from flitzis_looper.analysis.worker_profile_markers import expected_packages


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
        acquisition.acquire_to_quarantine(tmp_path / "incomplete.ckpt", max_bytes=16)
    assert previous.read_bytes() == b"previous installed artifact"
    assert not (tmp_path / "incomplete.ckpt").exists()
    assert (tmp_path / "incomplete.ckpt.partial").read_bytes() == content


def test_local_quarantine_never_overwrites_a_previous_artifact(tmp_path: Path) -> None:
    source = tmp_path / "source.ckpt"
    destination = tmp_path / "previous.ckpt"
    source.write_bytes(b"new bytes")
    destination.write_bytes(b"previous bytes")
    with pytest.raises(FileExistsError):
        acquisition.copy_to_quarantine(source, destination)
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
    if artifacts.file_digest(staging / "uv.lock") == CUDA_LOCK_SHA256:
        with (staging / "uv.lock").open("rb") as stream:
            packages = expected_packages(tomllib.load(stream), "flitzis-beat-this-cuda-worker")
    else:
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


def _prepare_cuda_source(prepared: _SetupFixture, monkeypatch: pytest.MonkeyPatch) -> None:
    shipped = Path(artifacts.__file__).resolve().parents[3] / "workers" / "beat_this_cuda"
    source = prepared.source.with_name("beat_this_cuda")
    source.mkdir()
    for filename in cuda_artifacts.CUDA_WORKER_FILES:
        shutil.copyfile(shipped / filename, source / filename)
    monkeypatch.setattr(setup, "cuda_source_directory", lambda: source)
    monkeypatch.setattr(cuda_artifacts, "cuda_source_directory", lambda: source)


def _stage_cuda(prepared: _SetupFixture) -> WorkerConfiguration:
    return setup.install_worker(
        prepared.install,
        prepared.scratch,
        checkpoint_file=prepared.checkpoint,
        offline=True,
        profile="cuda-auto-v2",
        stage_only=True,
    )


@pytest.mark.parametrize("existing_pointer", [False, True])
def test_stage_returns_new_cuda_configuration_without_reading_or_writing_current_pointer(
    prepared: _SetupFixture, monkeypatch: pytest.MonkeyPatch, *, existing_pointer: bool
) -> None:
    previous = _install(prepared) if existing_pointer else None
    pointer = prepared.install / "current.json"
    old_bytes = pointer.read_bytes() if existing_pointer else None
    _prepare_cuda_source(prepared, monkeypatch)
    monkeypatch.setattr(
        setup,
        "require_cuda_native_capability",
        lambda: pytest.fail("inactive staging inspected loaded Native"),
    )
    monkeypatch.setattr(
        setup,
        "load_worker_configuration",
        lambda *_: pytest.fail("staging read current pointer for its return value"),
    )
    staged = _stage_cuda(prepared)
    assert isinstance(staged.model, CudaBeatModelIdentity)
    assert staged.manifest is not None
    assert staged.manifest.parent.name.startswith("installation-")
    assert previous is None or staged.manifest != previous.manifest
    assert (
        setup.load_staged_worker_configuration(
            prepared.install, staged.manifest.parent, prepared.scratch
        )
        == staged
    )
    assert pointer.read_bytes() == old_bytes if existing_pointer else not pointer.exists()


def test_failed_cuda_staging_keeps_the_previous_pointer_and_cpu_configuration(
    prepared: _SetupFixture, monkeypatch: pytest.MonkeyPatch
) -> None:
    previous = _install(prepared)
    old_bytes = (prepared.install / "current.json").read_bytes()
    _prepare_cuda_source(prepared, monkeypatch)

    def corrupt_environment(staging: Path, scratch: Path, *, offline: bool) -> None:
        _fake_provision(staging, scratch, offline=offline)
        path = staging / "environment.json"
        report = json.loads(path.read_bytes())
        report["packages"]["torch"] = "2.8.0+cpu"
        path.write_text(json.dumps(report), encoding="utf-8")

    monkeypatch.setattr(setup, "_provision_environment", corrupt_environment)
    with pytest.raises(ValueError, match="worker_environment_mismatch"):
        _stage_cuda(prepared)
    assert (prepared.install / "current.json").read_bytes() == old_bytes
    assert setup.load_worker_configuration(prepared.install, prepared.scratch) == previous
    assert list(prepared.install.glob(".staging-*")) == []
    assert len(list(prepared.install.glob("installation-*"))) == 1


@pytest.mark.parametrize("failure", ["old_native", "profile", "script", "checkpoint", "outside"])
def test_activation_rejects_unqualified_retained_installation_without_pointer_effect(
    prepared: _SetupFixture, monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    _install(prepared)
    old_bytes = (prepared.install / "current.json").read_bytes()
    _prepare_cuda_source(prepared, monkeypatch)
    staged = _stage_cuda(prepared)
    assert staged.manifest is not None
    installed = staged.manifest.parent
    if failure == "profile":
        report = json.loads(staged.manifest.read_bytes())
        report["profile"] = "unknown-runtime"
        staged.manifest.write_text(json.dumps(report), encoding="utf-8")
    elif failure == "script":
        staged.script.write_text("# mutated after staging\n", encoding="utf-8")
    elif failure == "checkpoint":
        staged.checkpoint.write_bytes(b"mutated after staging")
    elif failure == "outside":
        installed = prepared.install.parent / installed.name
    monkeypatch.setattr(
        setup, "require_cuda_native_capability", lambda: require_cuda_native_capability(object())
    )
    with pytest.raises(
        ValueError,
        match=r"EXECUTION_API_VERSION|profile|installation_project_corrupt|checkpoint_hash_mismatch|installation_child_invalid",
    ):
        setup.activate_worker_installation(prepared.install, installed, prepared.scratch)
    assert (prepared.install / "current.json").read_bytes() == old_bytes
    assert list(prepared.install.glob(".current-*")) == []


def test_activation_selects_the_exact_verified_cuda_child_and_cpu_rollback_ignores_new_capability(
    prepared: _SetupFixture, monkeypatch: pytest.MonkeyPatch
) -> None:
    previous = _install(prepared)
    old_bytes = (prepared.install / "current.json").read_bytes()
    _prepare_cuda_source(prepared, monkeypatch)
    staged = _stage_cuda(prepared)
    assert staged.manifest is not None
    calls = []

    def current_native() -> None:
        assert (prepared.install / "current.json").read_bytes() == old_bytes
        calls.append("capability-before-publication")

    monkeypatch.setattr(setup, "require_cuda_native_capability", current_native)
    activated = setup.activate_worker_installation(
        prepared.install, staged.manifest.parent, prepared.scratch
    )
    assert activated == staged
    assert calls == ["capability-before-publication"]
    assert setup.load_worker_configuration(prepared.install, prepared.scratch) == staged
    monkeypatch.setattr(
        setup,
        "require_cuda_native_capability",
        lambda: pytest.fail("CPU rollback required CUDA Native"),
    )
    assert previous.manifest is not None
    restored = setup.activate_worker_installation(
        prepared.install, previous.manifest.parent, prepared.scratch
    )
    assert restored == previous
    assert (prepared.install / "current.json").read_bytes() == old_bytes


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


def test_failed_environment_setup_retains_download_for_retry_outside_staging(
    prepared: _SetupFixture, monkeypatch: pytest.MonkeyPatch
) -> None:
    payload = prepared.checkpoint.read_bytes()
    monkeypatch.setattr(urllib.request, "urlopen", lambda *_args, **_kwargs: _Response(payload))

    def fail_provision(staging: Path, scratch: Path, *, offline: bool) -> None:
        raise subprocess.CalledProcessError(1, "uv")

    monkeypatch.setattr(setup, "_provision_environment", fail_provision)
    with pytest.raises(subprocess.CalledProcessError):
        setup.install_worker(prepared.install, prepared.scratch)
    retained = (
        prepared.scratch / "downloads" / f"beat-this-{artifacts.ACCEPTED_ARTIFACT.sha256}.ckpt"
    )
    assert retained.read_bytes() == payload
    assert list(prepared.install.glob(".staging-*")) == []
    assert not (prepared.install / "current.json").exists()
    monkeypatch.setattr(
        urllib.request,
        "urlopen",
        lambda *_args, **_kwargs: pytest.fail("redownloaded retained model"),
    )
    monkeypatch.setattr(setup, "_provision_environment", _fake_provision)
    installed = setup.install_worker(prepared.install, prepared.scratch)
    assert installed.checkpoint.read_bytes() == payload
    assert prepared.checkpoint.read_bytes() == payload


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
    assert isinstance(configuration.model, BeatModelIdentity)
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
    assert "timeout" not in kwargs
    environment = kwargs["env"]
    assert isinstance(environment, dict)
    assert environment["UV_PROJECT_ENVIRONMENT"] == str(tmp_path / ".venv")


@pytest.mark.parametrize("profile", ["cuda", "cpu", "unknown", "missing"])
def test_no_package_profile_gets_a_provisioning_wall_clock_deadline(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, profile: str
) -> None:
    if profile in {"cuda", "cpu"}:
        project = (
            cuda_artifacts.cuda_source_directory()
            if profile == "cuda"
            else artifacts.worker_source_directory()
        )
        shutil.copyfile(project / "uv.lock", tmp_path / "uv.lock")
    elif profile == "unknown":
        (tmp_path / "uv.lock").write_bytes(b"arbitrary supplied lock")
        (tmp_path / "profile_markers.py").write_bytes(b"untrusted marker presence")
    calls: list[dict[str, object]] = []

    def run(_command: list[str], **kwargs: object) -> None:
        calls.append(kwargs)

    monkeypatch.setattr(shutil, "which", lambda _: sys.executable)
    monkeypatch.setattr(subprocess, "run", run)
    monkeypatch.setattr(setup, "_inspect_environment", lambda *_: None)
    setup._provision_environment(tmp_path, tmp_path, offline=False)
    assert len(calls) == 1
    assert "timeout" not in calls[0]
