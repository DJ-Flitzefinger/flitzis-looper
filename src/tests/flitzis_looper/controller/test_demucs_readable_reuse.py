"""Composed Demucs software proof with real Native readers and shared FFmpeg.

Only neural output is substituted by an owned child fixture. Its checkpoint
contains declared non-neural bytes; no inference or download is claimed. Source,
pair ACK and retirement ownership are controller substitutes; Native callback,
pair and retirement custody are tested separately. The actual wrapper,
converters, backend, promotion and compatible reuse run here.
"""

import hashlib
import json
import os
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path
from textwrap import dedent
from typing import TYPE_CHECKING, Protocol
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import stem_provenance
from flitzis_looper.controller import stems as stems_module
from flitzis_looper.controller.stem_cache import promote_generation_artifacts
from flitzis_looper.controller.stem_generation import (
    DEMUCS_REQUIRED_CHECKPOINTS,
    DemucsStemGenerationBackend,
    run_command,
)
from flitzis_looper.controller.stem_identity import request_fingerprint
from flitzis_looper.encoder_provenance import (
    ENCODER_CONTRACT_ENV,
    ENCODER_RECEIPT,
    ENCODER_RESULT,
    SHARED_LIBRARIES,
    supported_encoder_images,
    verified_encoder_output,
)
from flitzis_looper.models import STEM_KINDS, PadContentIdentity
from flitzis_looper.pcm16_wav import PcmWavReader
from flitzis_looper.stem_pair_selection import STEM_FINGERPRINT_SCHEMA, StemGenerationProvenance
from flitzis_looper_audio import MigrationArtifactLease
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import FakePreparedSourceTicket
from tests.flitzis_looper.controller.test_shared_stem_reuse import _SelectedPair

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_generation import (
        CommandResult,
        StemGenerationRequest,
        StemGenerationResult,
    )
    from flitzis_looper.controller.stem_job import StemGenerationJob
    from flitzis_looper.models import StemCacheEntry


_CHECKPOINT_BYTES = b"owned non-neural Demucs checkpoint fixture; never loaded as weights"
_CHILD_PID = "FLITZIS_NN_OUTPUT_FIXTURE_PID_V1:"
_PROCESSING_ROLES = (
    "fixture_checkpoint",
    *(f"demucs/{name}" for name in stem_provenance.DEMUCS_PRODUCER_FILES),
    "host/demucs_streaming.py",
    "host/controller/stem_artifacts.py",
    "host/pcm16_wav.py",
    "host/controller/stem_generation.py",
    "host/encoder_provenance.py",
    "host/controller/processing_code.py",
    "host/__init__.py",
    "encoder/ffmpeg.exe",
    *(f"encoder/{name}" for name in SHARED_LIBRARIES),
    "input/ffprobe.exe",
)
_CHILD = dedent("""\
    import argparse
    import json
    import os
    from pathlib import Path

    import torch
    import demucs.audio
    from flitzis_looper import demucs_streaming
    from flitzis_looper.encoder_provenance import ENCODER_CONTRACT_ENV, ENCODER_RECEIPT

    parser = argparse.ArgumentParser()
    parser.add_argument("-n")
    parser.add_argument("-o", type=Path)
    parser.add_argument("--filename")
    parser.add_argument("--clip-mode")
    parser.add_argument("--shifts")
    parser.add_argument("--overlap")
    parser.add_argument("-d")
    parser.add_argument("source", type=Path)
    args = parser.parse_args()
    assert args.n == "htdemucs"
    assert args.filename == "{stem}.{ext}"
    assert args.clip_mode == "clamp"
    checkpoint = Path(os.environ["TORCH_HOME"]) / "hub/checkpoints/955717e8-8726e21a.th"
    assert checkpoint.read_bytes() == (
        b"owned non-neural Demucs checkpoint fixture; never loaded as weights"
    )

    def neural_output_fixture():
        # The actual wrapper has already installed its stock-input proxy and
        # PCM16 save_audio adapter. This fixture replaces only neural output.
        assert isinstance(demucs.audio.sp, demucs_streaming._InputTools)
        source = demucs.audio.AudioFile(args.source)
        assert source.samplerate() == 44100
        assert source.channels() == 1
        decoded = source.read(streams=0, samplerate=44100, channels=1)
        assert tuple(decoded.shape) == (1, 128)
        output = args.o / args.n / args.source.stem
        output.mkdir(parents=True)
        if os.environ.get("FLITZIS_FIXTURE_PREBORN") == "1":
            contract = os.environ[ENCODER_CONTRACT_ENV]
            forged = {
                "contract": contract,
                "verified": True,
                "runs": [
                    {"leaf": name + ".wav", "proof": {
                        "pid": 100 + index, "images": json.loads(contract)
                    }} for index, name in enumerate(("vocals", "drums", "bass", "other"))
                ],
            }
            (output / ENCODER_RECEIPT).write_text(json.dumps(forged), encoding="utf-8")
        for name, value in (("vocals", .10), ("drums", .02), ("bass", .03), ("other", .04)):
            tensor = torch.full((2, 128), value, dtype=torch.float32)
            demucs_streaming._separate.save_audio(
                tensor, output / (name + ".wav"), 44100, clip=args.clip_mode
            )

    demucs_streaming._separate.main = neural_output_fixture
    print("FLITZIS_NN_OUTPUT_FIXTURE_PID_V1:" + str(os.getpid()), flush=True)
    demucs_streaming.main()
""")


def _digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _file_evidence(path: Path) -> dict[str, object]:
    metadata = path.stat()
    return {
        "path": str(path.resolve(strict=True)),
        "file_id": {"st_dev": metadata.st_dev, "st_ino": metadata.st_ino},
        "bytes": metadata.st_size,
        "sha256": _digest(path),
    }


def _complete_evidence(directory: Path) -> dict[str, object]:
    marker = directory / ".complete.json"
    encoded = marker.read_bytes()
    files = []
    for kind in STEM_KINDS:
        path = directory / f"{kind}.wav"
        with PcmWavReader(path) as wav:
            geometry = {
                "sample_rate_hz": wav.getframerate(),
                "channels": wav.getnchannels(),
                "frames": wav.getnframes(),
                "sample_width_bytes": wav.getsampwidth(),
            }
            wav.verify()
        files.append(_file_evidence(path) | {"kind": kind, "geometry": geometry})
    return {
        "directory": str(directory.resolve(strict=True)),
        "marker": _file_evidence(marker) | {"body_utf8": encoded.decode("utf-8")},
        "marker_fields": json.loads(encoded),
        "wavs": files,
    }


def _append_evidence(record: dict[str, object], name: str, value: object) -> None:
    observations = record.setdefault(name, [])
    assert isinstance(observations, list)
    observations.append(value)


def _assert_shape(path: Path, *, channels: int) -> None:
    with PcmWavReader(path) as wav:
        assert (wav.getframerate(), wav.getnchannels(), wav.getnframes(), wav.getsampwidth()) == (
            44_100,
            channels,
            128,
            2,
        )
        wav.verify()


def _owned_encoder(tmp: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    configured = os.environ.get("FLITZIS_MATERIAL_ENCODER")
    if configured is None:
        pytest.skip("Actual shared FFmpeg proof requires explicit FLITZIS_MATERIAL_ENCODER")
    source = Path(configured).resolve(strict=True)
    images = supported_encoder_images(source)
    assert len(images) == 8
    destination = tmp / "owned-encoder"
    destination.mkdir()
    for image in (*images, source.with_name("ffprobe.exe")):
        shutil.copyfile(image, destination / image.name)
    monkeypatch.setenv("FLITZIS_FFMPEG_DIR", str(destination))
    monkeypatch.setenv("PATH", str(destination) + os.pathsep + os.environ.get("PATH", ""))
    return destination / source.name


class _FixtureRunner:
    """Forward all real preflights, substituting only the exact neural CLI command."""

    def __init__(self, script: Path, source: Path, checkpoint: Path) -> None:
        self.script = script
        self.source = source
        self.checkpoint = checkpoint
        self.commands: list[list[str]] = []
        self.neural_commands: list[list[str]] = []
        self.births: list[tuple[str, int, int, str]] = []
        self.converter_pids: set[int] = set()
        self.records: list[dict[str, object]] = []
        self.command_records: list[dict[str, object]] = []
        self.preborn = False

    def __call__(self, args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        self.commands.append(args.copy())
        if args[1:3] != ["-m", "flitzis_looper.demucs_streaming"]:
            result = run_command(args, cwd=cwd, env=env)
            self._record_command(args, args, cwd, result)
            return result
        self._assert_neural_command(args, env)
        self.neural_commands.append(args.copy())
        child_env = env | {"FLITZIS_FIXTURE_PREBORN": "1" if self.preborn else "0"}
        # Every actual requested Demucs argument is forwarded unchanged.
        executed = [args[0], str(self.script), *args[3:]]
        result = run_command(executed, cwd=cwd, env=child_env)
        self._record_command(args, executed, cwd, result)
        assert result.returncode == 0, result.stderr
        self._assert_actual_encoder_receipt(Path(args[6]), env, result.stdout)
        return result

    def _record_command(
        self, requested: list[str], executed: list[str], cwd: Path, result: CommandResult
    ) -> None:
        self.command_records.append({
            "requested_argv": requested.copy(),
            "executed_argv": executed.copy(),
            "cwd": str(cwd),
            "returncode": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
        })

    def _assert_neural_command(self, args: list[str], env: dict[str, str]) -> None:
        assert args[0] == sys.executable
        assert args[3:6] == ["-n", "htdemucs", "-o"]
        assert args[7:16] == [
            "--filename",
            "{stem}.{ext}",
            "--clip-mode",
            "clamp",
            "--shifts",
            "2",
            "--overlap",
            "0.25",
            "-d",
        ]
        assert args[16] in {"cpu", "cuda"}
        assert args[17:] == [str(self.source)]
        assert Path(env["TORCH_HOME"]) / "hub/checkpoints" == self.checkpoint.parent
        assert self.checkpoint.read_bytes() == _CHECKPOINT_BYTES
        has_encoder_contract = ENCODER_CONTRACT_ENV in env
        assert has_encoder_contract

    def _assert_actual_encoder_receipt(
        self, output: Path, env: dict[str, str], stdout: str
    ) -> None:
        for kind in ("vocals", "drums", "bass", "other"):
            _assert_shape(output / "htdemucs" / self.source.stem / f"{kind}.wav", channels=2)
        lines = [line for line in stdout.splitlines() if line.startswith(ENCODER_RESULT)]
        assert len(lines) == 1
        claims = json.loads(lines[0].removeprefix(ENCODER_RESULT))
        valid = verified_encoder_output(output, env[ENCODER_CONTRACT_ENV], stdout)
        record = self._capture_child_receipt(output, env, stdout, claims, valid=valid)
        if self.preborn:
            assert claims == []
            assert not valid
            return
        assert valid
        assert len(claims) == 1
        path, device, identity, digest = claims[0]
        receipt = Path(path)
        assert receipt.name == ENCODER_RECEIPT
        assert receipt.is_relative_to(output)
        metadata = receipt.stat()
        assert (metadata.st_dev, metadata.st_ino) == (device, identity)
        assert _digest(receipt) == digest
        body = json.loads(receipt.read_bytes())
        assert body["verified"] is True
        assert body["contract"] == env[ENCODER_CONTRACT_ENV]
        expected_images = json.loads(env[ENCODER_CONTRACT_ENV])
        assert len(expected_images) == 8
        pids = {run["proof"]["pid"] for run in body["runs"]}
        assert len(pids) == 4
        assert all(run["proof"]["images"] == expected_images for run in body["runs"])
        assert record["child_pid"] not in pids
        record["validated_converter_module_proofs"] = body["runs"]
        self.converter_pids.update(pids)
        self.births.append((path, device, identity, digest))

    def _capture_child_receipt(
        self,
        output: Path,
        env: dict[str, str],
        stdout: str,
        claims: object,
        *,
        valid: bool,
    ) -> dict[str, object]:
        pids = [
            token.removeprefix(_CHILD_PID)
            for token in stdout.split()
            if token.startswith(_CHILD_PID)
        ]
        assert len(pids) == 1
        child_pid = int(pids[0])
        assert child_pid > 0
        receipt = output / "htdemucs" / self.source.stem / ENCODER_RECEIPT
        encoded = receipt.read_bytes()
        assert len(encoded) <= 32_768
        record: dict[str, object] = {
            "child_pid": child_pid,
            "child_pid_origin": "owned child os.getpid() captured by actual run_command stdout",
            "stdout": stdout,
            "requested_argv": self.neural_commands[-1].copy(),
            "executed_argv": self.command_records[-1]["executed_argv"],
            "script": _file_evidence(self.script),
            "torch_home": env["TORCH_HOME"],
            "checkpoint": _file_evidence(self.checkpoint),
            "source": _file_evidence(self.source),
            "receipt": _file_evidence(receipt) | {"body_utf8": encoded.decode("utf-8")},
            "claims_from_child_stdout": claims,
            "verified_encoder_output": valid,
            "preborn_fixture": self.preborn,
            "receipt_scope": (
                "preexisting owned forged body; no validated birth or converter PID claims"
                if self.preborn
                else "actual exclusive child birth checked against stdout FileID/SHA"
            ),
            "validated_converter_module_proofs": [],
        }
        self.records.append(record)
        return record


class _RecordingBackend(DemucsStemGenerationBackend):
    def __init__(self, runner: _FixtureRunner) -> None:
        super().__init__(command_runner=runner)
        self.requests: list[StemGenerationRequest] = []
        self.results: list[StemGenerationResult] = []

    def generate(
        self, request: StemGenerationRequest, progress: Callable[[float, str], None]
    ) -> StemGenerationResult:
        self.requests.append(request)
        assert stem_provenance.processing_library_versions(request) is not None
        result = super().generate(request, progress)
        self.results.append(result)
        return result


class _ControlPair(_SelectedPair):
    """Independent subscriber ACK substitute, never a Native disk/callback proof."""

    def __init__(
        self,
        root: str,
        cache: str,
        *,
        components: bool,
        provenance: StemGenerationProvenance | None,
    ) -> None:
        super().__init__(root, cache, provenance)
        self.components = components

    def has_components(self) -> bool:
        return self.components


class _BackendRecords(Protocol):
    requests: list[StemGenerationRequest]
    results: list[StemGenerationResult]

    def generate(
        self, request: StemGenerationRequest, progress: Callable[[float, str], None]
    ) -> StemGenerationResult: ...


class _RunnerRecords(Protocol):
    commands: list[list[str]]
    neural_commands: list[list[str]]
    records: list[dict[str, object]]


def _marker_provenance(marker: dict[str, object]) -> StemGenerationProvenance | None:
    if marker.get("fingerprint_schema") is None:
        return None
    assert marker["fingerprint_schema"] == STEM_FINGERPRINT_SCHEMA
    return StemGenerationProvenance.model_validate({
        "schema": marker["fingerprint_schema"],
        "generation_fingerprint": marker["generation_fingerprint"],
        "producer_identity": marker["producer_identity"],
    })


@dataclass(slots=True)
class _Rig[Backend: _BackendRecords, Runner: _RunnerRecords]:
    controller: AppController
    audio: Mock
    checkpoint: Path
    encoder: Path
    backend: Backend
    runner: Runner
    tasks: list[Callable[[], None]]
    pairs: list[_ControlPair]

    def assert_processing_writes_denied(self) -> None:
        for path in (self.checkpoint, self.encoder.with_name(SHARED_LIBRARIES[0])):
            with pytest.raises(PermissionError):
                path.write_bytes(b"rewrite during actual producer custody")

    def prepare_pair(
        self,
        sample_id: int,
        version: str,
        cache: str,
        _ticket: FakePreparedSourceTicket,
        components: object,
        _descriptor: str | None,
        _artifact_version: str | None = None,
    ) -> _ControlPair:
        assert sample_id in {0, 215}
        if sample_id == 215:
            # The actual PairQueue has sealed current producer files and its
            # complete Native MigrationArtifactLease before this ACK substitute.
            self.assert_processing_writes_denied()
        lease = MigrationArtifactLease.capture(str(Path.cwd() / "samples"), cache)
        try:
            receipt = json.loads(lease.receipt_json())
            assert receipt["kind"] == "stem_directory"
            assert {proof["name"] for proof in receipt["files"]} == {
                *(f"{kind}.wav" for kind in STEM_KINDS),
                ".complete.json",
            }
            for proof in receipt["files"]:
                path = Path(cache) / proof["name"]
                assert proof["sha256"] == _digest(path)
                assert proof["bytes"] == path.stat().st_size
            marker = Path(cache) / ".complete.json"
            payload = json.loads(marker.read_bytes())
            assert payload["source_version"] == (_artifact_version or version)
            provenance = _marker_provenance(payload)
            with pytest.raises(PermissionError):
                marker.write_bytes(b"rewrite during actual complete-set held reader")
            _append_evidence(
                self.runner.records[-1],
                "native_complete_set_leases",
                {
                    "sample_id": sample_id,
                    "source_version": version,
                    "include_components": components,
                    "native_receipt": receipt,
                    "held_complete_set": _complete_evidence(Path(cache)),
                    "marker_write_denied_during_native_lease": True,
                    "authority_scope": (
                        "real Native artifact custody; subscriber ACK is a substitute"
                    ),
                },
            )
        finally:
            lease.release()
        assert isinstance(components, bool)
        pair = _ControlPair(
            "samples/Fixture take.wav", cache, components=components, provenance=provenance
        )
        self.pairs.append(pair)
        return pair

    def _run_origin_backend(self) -> StemGenerationJob:
        assert self.controller.stems.generate_stems_async(0)
        job = next(iter(self.controller.stems._jobs.values()))
        assert len(self.tasks) == 1
        self.tasks.pop(0)()
        assert str(job.phase) == "resolved"
        assert job.resolved_request is not None
        assert job.resolved_request.original_bytes == job.request.source_path.stat().st_size
        assert not self.backend.results
        self.assert_processing_writes_denied()
        self.controller.stems.on_frame_render()
        assert len(self.tasks) == 1
        self.tasks.pop(0)()
        assert len(self.backend.results) == 1, self.controller.stems.stem_generation_error(0)
        assert len(self.runner.neural_commands) == 1
        return job

    def generate_origin(self, monkeypatch: pytest.MonkeyPatch) -> StemCacheEntry:
        job = self._run_origin_backend()
        assert job.processing_files is not None
        self._assert_actual_processing_witnesses(job)
        self.assert_processing_writes_denied()
        self.runner.records[-1]["processing_guard_held_after_backend"] = True

        def promote(entry: StemCacheEntry, directory: Path) -> StemCacheEntry:
            assert job.processing_files is not None
            self.assert_processing_writes_denied()
            self.runner.records[-1]["processing_guard_held_during_actual_promotion"] = True
            return promote_generation_artifacts(entry, directory)

        monkeypatch.setattr(stems_module, "promote_generation_artifacts", promote)
        self.controller.stems.on_frame_render()
        assert job._disposed
        assert job.processing_files is None
        self.runner.records[-1]["job_processing_reference_cleared_after_promotion"] = True
        assert len(self.tasks) == 1
        self.tasks.pop(0)()
        self.controller.stems.on_frame_render()
        entry = self.controller.project.stem_cache[0]
        assert entry is not None
        assert entry.available
        assert self.pairs[0].selected == 1
        if entry.generation_fingerprint is not None:
            self.assert_processing_writes_denied()
            self.runner.records[-1]["processing_guard_held_by_selected_snapshot"] = True
        return entry

    def resolve_reuse(self, *, prepare: bool) -> None:
        """Resolve bytes before the independent pair worker, without new inference."""
        backend_count = len(self.backend.requests)
        command_count = len(self.runner.commands)
        assert len(self.tasks) == 1
        self.tasks.pop(0)()
        self.controller.stems.on_frame_render()
        if prepare:
            assert len(self.tasks) == 1
            self.tasks.pop(0)()
            self.controller.stems.on_frame_render()
        assert not self.tasks
        assert len(self.backend.requests) == backend_count
        assert len(self.runner.commands) == command_count

    def withdraw_verified_snapshot(self) -> None:
        """Withdraw runtime evidence via effective Settings ABA before owned file mutation."""
        separator = self.controller.project.stem_separator
        self.controller.project.stem_separator = (
            "demucs:htdemucs" if separator == "bs-roformer:musdb18hq" else "bs-roformer:musdb18hq"
        )
        self.controller.project.stem_separator = separator
        self.controller.stems.on_frame_render()
        assert 0 not in self.controller.stems._verified_sets

    def _assert_actual_processing_witnesses(self, job: StemGenerationJob) -> None:
        guard = job.processing_files
        assert guard is not None
        witnesses = guard.witnesses()
        assert len(witnesses) == 32
        expected = stem_provenance.processing_paths(self.backend.requests[0])
        assert len(expected) == 32
        for (name, digest, size), requested in zip(witnesses, expected, strict=True):
            path = Path(name)
            assert path.samefile(requested)
            assert digest == _digest(path)
            assert size == path.stat().st_size
        guard.verify()
        self.runner.records[-1]["held_processing_files"] = [
            _file_evidence(Path(name))
            | {
                "order": index,
                "role": role,
                "native_canonical_path": name,
                "native_sha256": digest,
                "native_bytes": size,
                "requested_path": str(requested),
                "captured_while_native_guard_held": True,
            }
            for index, (role, (name, digest, size), requested) in enumerate(
                zip(_PROCESSING_ROLES, witnesses, expected, strict=True)
            )
        ]
        self.runner.records[-1]["processing_library_versions"] = (
            stem_provenance.processing_library_versions(self.backend.requests[0])
        )


type _DemucsRig = _Rig[_RecordingBackend, _FixtureRunner]


def _source_assignments(controller: AppController, tmp: Path) -> Path:
    source = tmp / "samples/Fixture take.wav/Fixture take.wav"
    source.parent.mkdir(parents=True)
    write_mono_pcm16_wav(source, 44_100)
    digest = _digest(source)
    (source.parent / ".material.json").write_text(
        json.dumps({
            "schema_version": 1,
            "sha256": digest,
            "bytes": source.stat().st_size,
            "original_leaf": source.name,
        }),
        encoding="utf-8",
    )
    for slot in (0, 215):
        controller.project.sample_paths[slot] = source.relative_to(tmp).as_posix()
        controller.project.sample_durations[slot] = 128 / 44_100
        controller.project.pad_content[slot] = PadContentIdentity(
            instance_id=f"{slot + 1:032x}", material_id=digest[:32]
        )
        controller._audio.load_sample_async(slot, source.relative_to(tmp).as_posix())
    controller.project.stem_separator = "demucs:htdemucs"
    controller.project.demucs_shifts = 2
    controller.project.demucs_overlap = 0.25
    controller.project.pad_stem_mix_mode[0] = "full_mix"
    controller.project.pad_stem_mix_mode[215] = "all_stems"
    controller.project.pad_gain_db[0] = -4
    controller.project.pad_gain_db[215] = -11
    controller.project.pad_key_lock[215] = True
    controller.session.pad_stem_enabled_mask[215] = 0b0101
    controller._assets.sync_assignments()
    return source


@pytest.fixture
def demucs_rig(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    request: pytest.FixtureRequest,
) -> Iterator[_DemucsRig]:
    if os.name != "nt":
        pytest.skip("Actual shared encoder module/FileID proof is Windows-specific")
    encoder = _owned_encoder(tmp_path, monkeypatch)
    source = _source_assignments(controller, tmp_path)
    checkpoint = tmp_path / "torch/hub/checkpoints" / DEMUCS_REQUIRED_CHECKPOINTS["htdemucs"][0]
    checkpoint.parent.mkdir(parents=True)
    checkpoint.write_bytes(_CHECKPOINT_BYTES)
    monkeypatch.setattr(
        stems_module, "separator_model_cache_dir", lambda _separator: checkpoint.parent
    )
    script = tmp_path / "owned-neural-output-fixture.py"
    script.write_text(_CHILD, encoding="utf-8")
    runner = _FixtureRunner(script, source, checkpoint)
    backend = _RecordingBackend(runner)
    tasks: list[Callable[[], None]] = []
    rig = _Rig(controller, audio_engine_mock, checkpoint, encoder, backend, runner, tasks, [])
    controller.stems._stem_backend = backend
    controller.stems._stem_task_runner = tasks.append
    controller.stems._pair_preparations._runner = tasks.append
    audio_engine_mock.prepare_stem_pair = Mock(side_effect=rig.prepare_pair)

    def source_ticket(sample_id: int, _version: str) -> FakePreparedSourceTicket:
        ticket = FakePreparedSourceTicket()
        ticket.source_request = sample_id + 1
        return ticket

    def publish(_pair: _ControlPair, ticket: FakePreparedSourceTicket) -> None:
        ticket.status = "pending"

    audio_engine_mock.capture_prepared_source.side_effect = source_ticket
    audio_engine_mock.publish_stem_pair.side_effect = publish
    try:
        yield rig
    finally:
        try:
            _retain_composed_evidence(rig, tmp_path, request.node.nodeid)
        finally:
            controller.stems.shut_down()


def _pad_evidence[Backend: _BackendRecords, Runner: _RunnerRecords](
    rig: _Rig[Backend, Runner], sample_id: int
) -> dict[str, object]:
    project = rig.controller.project
    content = project.pad_content[sample_id]
    entry = project.stem_cache[sample_id]
    pending = rig.controller.stems._pending_stem_publications.get(sample_id)
    publication = None
    if pending is not None:
        assert isinstance(pending.source_ticket, FakePreparedSourceTicket)
        publication = {
            "source_request": pending.source_ticket.source_request,
            "status": pending.source_ticket.publication_status(),
            "scope": "controller source/pair ACK substitute",
        }
    return {
        "sample_id": sample_id,
        "source": project.sample_paths[sample_id],
        "content": None if content is None else content.model_dump(mode="json"),
        "stem_entry": None if entry is None else entry.model_dump(mode="json"),
        "mix_mode": project.pad_stem_mix_mode[sample_id],
        "gain_db": project.pad_gain_db[sample_id],
        "key_lock": project.pad_key_lock[sample_id],
        "key_intent": project.pad_key_intent[sample_id].model_dump(mode="json"),
        "enabled_mask": rig.controller.session.pad_stem_enabled_mask[sample_id],
        "pending_publication": publication,
        "error": rig.controller.stems.stem_generation_error(sample_id),
    }


def _control_evidence[Backend: _BackendRecords, Runner: _RunnerRecords](
    rig: _Rig[Backend, Runner],
) -> dict[str, object]:
    origin = rig.controller.project.stem_cache[0]
    second = rig.controller.project.stem_cache[215]
    return {
        "pads": [_pad_evidence(rig, sample_id) for sample_id in (0, 215)],
        "distinct_entry_objects": None
        if origin is None or second is None
        else origin is not second,
        "pair_selected_counts": [pair.selected for pair in rig.pairs],
        "source_capture_count": rig.audio.capture_prepared_source.call_count,
        "pair_prepare_count": rig.audio.prepare_stem_pair.call_count,
        "pair_publish_count": rig.audio.publish_stem_pair.call_count,
        "backend_request_count": len(rig.backend.requests),
        "command_count": len(rig.runner.commands),
        "neural_fixture_command_count": len(rig.runner.neural_commands),
    }


def _retain_composed_evidence(rig: _DemucsRig, tmp_path: Path, test_id: str) -> None:
    document = {
        "schema": "demucs-composed-software-evidence-v1",
        "test_id": test_id,
        "scope": {
            "neural_output": "owned child supplies four known Torch tensors; no NN inference",
            "checkpoint": "owned declared non-neural bytes raw-read, never executed as weights",
            "control": "source/pair ACK and retirement/callback custody are substitutes",
            "native": "actual SealedProcessingFiles and MigrationArtifactLease file custody",
            "child_stdout": (
                "actual run_command returned bounded stdout; child PID is self-reported"
            ),
            "module_proofs": (
                "only validated actual encoder receipt runs count as converter proofs"
            ),
            "file_ids": "actual Windows stat st_dev/st_ino; custody is stated per observation",
        },
        "commands": rig.runner.command_records,
        "attempts": rig.runner.records,
        "validated_encoder_births": rig.runner.births,
        "validated_converter_pids": sorted(rig.runner.converter_pids),
        "backend_results": [
            {
                "device": result.device,
                "cpu_fallback": result.cpu_fallback,
                "artifact_count": result.artifact_count,
                "encoder_verified": result.encoder_verified,
            }
            for result in rig.backend.results
        ],
        "final_checkpoint": _file_evidence(rig.checkpoint),
        "final_shared_library": _file_evidence(rig.encoder.with_name(SHARED_LIBRARIES[0])),
        "final_complete_sets": {
            entry.cache_dir: _complete_evidence(Path(entry.cache_dir))
            for sample_id in (0, 215)
            if (entry := rig.controller.project.stem_cache[sample_id]) is not None
        },
        "final_control_state": _control_evidence(rig),
    }
    encoded = json.dumps(document, indent=2, sort_keys=True).encode("utf-8")
    assert len(encoded) <= 262_144
    target = tmp_path / "demucs-composed-evidence.json"
    assert not target.is_relative_to(tmp_path / "samples")
    target.write_bytes(encoded)


def _accept_independent_publication(rig: _DemucsRig, origin: StemCacheEntry) -> StemCacheEntry:
    second = rig.controller.project.stem_cache[215]
    assert second is not None
    assert second is not origin
    assert not second.available
    pending = rig.controller.stems._pending_stem_publications[215]
    assert pending.source_ticket.publication_status() == "pending"
    assert rig.pairs[1].selected == 0
    ticket = pending.source_ticket
    assert isinstance(ticket, FakePreparedSourceTicket)
    assert ticket.source_request == 216
    rig.audio.capture_prepared_source.assert_any_call(0, origin.source_version)
    rig.audio.capture_prepared_source.assert_any_call(215, origin.source_version)
    _append_evidence(
        rig.runner.records[-1],
        "subscriber_transitions",
        {
            "phase": "own_pad_215_pending",
            "control_state": _control_evidence(rig),
        },
    )
    ticket.status = "accepted"
    rig.controller.stems.on_frame_render()
    current = rig.controller.project.stem_cache[215]
    assert current is not None
    assert current.available
    assert current.cache_dir == origin.cache_dir
    assert rig.pairs[1].selected == 1
    _append_evidence(
        rig.runner.records[-1],
        "subscriber_transitions",
        {
            "phase": "own_pad_215_accepted",
            "source_request": ticket.source_request,
            "ticket_status": ticket.publication_status(),
            "control_state": _control_evidence(rig),
        },
    )
    return current


def _assert_complete_generation[Backend: _BackendRecords, Runner: _RunnerRecords](
    rig: _Rig[Backend, Runner], entry: StemCacheEntry
) -> dict[str, tuple[int, int, str]]:
    directory = Path(entry.cache_dir)
    marker = json.loads((directory / ".complete.json").read_bytes())
    assert marker["source_version"] == entry.source_version
    assert marker["schema"] == "stem-set-sha256-v1"
    assert marker["stems"] == {kind: _digest(directory / f"{kind}.wav") for kind in STEM_KINDS}
    for kind in STEM_KINDS:
        _assert_shape(directory / f"{kind}.wav", channels=1)
    assert rig.backend.results[0].artifact_count == 5
    _append_evidence(
        rig.runner.records[-1],
        "completed_generations",
        {
            "generation": _complete_evidence(directory),
            "control_state": _control_evidence(rig),
        },
    )
    return {
        path.name: (path.stat().st_dev, path.stat().st_ino, _digest(path))
        for path in directory.iterdir()
    }


def _assert_known_producer(rig: _DemucsRig, entry: StemCacheEntry) -> None:
    assert rig.backend.results[0].encoder_verified
    assert len(rig.runner.births) == 1
    assert len(rig.runner.converter_pids) == 4
    request = rig.backend.requests[0]
    assert request.encoder_executable == rig.encoder
    assert stem_provenance.processing_library_versions(request) is not None
    processing = stem_provenance.capture_processing_files(request)
    assert processing is not None
    assert processing[1] is not None
    assert entry.producer_identity == processing[1]
    assert request.original_bytes == request.source_path.stat().st_size
    assert entry.fingerprint_schema == STEM_FINGERPRINT_SCHEMA
    assert entry.generation_fingerprint == request_fingerprint(request, processing[1])
    processing[0].verify()
    marker = json.loads((Path(entry.cache_dir) / ".complete.json").read_bytes())
    assert marker["producer_identity"] == entry.producer_identity
    assert marker["generation_fingerprint"] == entry.generation_fingerprint
    assert marker["fingerprint_schema"] == STEM_FINGERPRINT_SCHEMA
    assert entry.pair is not None
    assert entry.pair.provenance == _marker_provenance(marker)
    assert set(marker) == {
        "schema",
        "source_version",
        "stems",
        "producer_identity",
        "generation_fingerprint",
        "fingerprint_schema",
    }


@pytest.mark.parametrize("on_load", [False, True])
def test_demucs_actual_wrapper_encoder_publication_and_independent_completed_reuse(
    demucs_rig: _DemucsRig, monkeypatch: pytest.MonkeyPatch, *, on_load: bool
) -> None:
    rig = demucs_rig
    origin = rig.generate_origin(monkeypatch)
    before = _assert_complete_generation(rig, origin)
    _assert_known_producer(rig, origin)
    command_count = len(rig.runner.commands)
    if on_load:
        assert rig.controller.stems.reuse_compatible_stems_on_load(215)
    else:
        assert rig.controller.stems.generate_stems_async(215)
    if on_load:
        assert rig.controller.project.stem_cache[215] is None
    else:
        admitted = rig.controller.project.stem_cache[215]
        assert admitted is not None
        assert not admitted.available
        assert admitted.fingerprint_schema is None
    rig.resolve_reuse(prepare=True)
    current = _accept_independent_publication(rig, origin)
    assert len(rig.runner.commands) == command_count
    assert len(rig.backend.requests) == 1
    assert _assert_complete_generation(rig, current) == before
    assert rig.controller.project.pad_gain_db[215] == -11
    assert rig.controller.project.pad_key_lock[215]
    assert rig.controller.session.pad_stem_enabled_mask[215] == 0b0101
    rig.controller.loader.unload_sample(0)
    assert rig.controller.project.stem_cache[215] is current
    assert _assert_complete_generation(rig, current) == before


@pytest.mark.parametrize("corruption", ["model", "dependency", "historical_marker"])
def test_demucs_passive_reuse_rejects_actual_changed_or_unknown_producer_without_inference(
    demucs_rig: _DemucsRig, monkeypatch: pytest.MonkeyPatch, corruption: str
) -> None:
    rig = demucs_rig
    origin = rig.generate_origin(monkeypatch)
    _assert_known_producer(rig, origin)
    directory = Path(origin.cache_dir)
    wav_hashes = {kind: _digest(directory / f"{kind}.wav") for kind in STEM_KINDS}
    if corruption == "historical_marker":
        marker = directory / ".complete.json"
        payload = json.loads(marker.read_bytes())
        payload.pop("generation_fingerprint")
        payload.pop("producer_identity")
        marker.write_text(json.dumps(payload), encoding="utf-8")
    else:
        rig.withdraw_verified_snapshot()
        path = (
            rig.checkpoint if corruption == "model" else rig.encoder.with_name(SHARED_LIBRARIES[0])
        )
        with path.open("ab") as changed:
            changed.write(b"owned fixture bytes changed after completed producer settlement")
    command_count = len(rig.runner.commands)
    assert rig.controller.stems.reuse_compatible_stems_on_load(215)
    rig.resolve_reuse(prepare=corruption == "historical_marker")
    assert rig.controller.project.stem_cache[215] is None
    assert rig.controller.project.stem_cache[0] is origin
    assert len(rig.pairs) == 1
    assert len(rig.runner.commands) == command_count
    assert len(rig.backend.requests) == 1
    if corruption == "historical_marker":
        error = rig.controller.stems.stem_generation_error(215)
        assert error is not None
        assert "provenance" in error
    else:
        assert rig.controller.stems.stem_generation_error(215) is None
        assert "current producer" in rig.controller.session.stem_generation_diagnostics[215]
    assert wav_hashes == {kind: _digest(directory / f"{kind}.wav") for kind in STEM_KINDS}


def test_demucs_preexisting_forged_receipt_cannot_stamp_completed_set_provenance(
    demucs_rig: _DemucsRig, monkeypatch: pytest.MonkeyPatch
) -> None:
    rig = demucs_rig
    rig.runner.preborn = True
    entry = rig.generate_origin(monkeypatch)
    _assert_complete_generation(rig, entry)
    assert not rig.backend.results[0].encoder_verified
    assert not rig.runner.births
    assert entry.generation_fingerprint is None
    assert entry.producer_identity is None
    assert entry.fingerprint_schema is None
    assert entry.pair is not None
    assert entry.pair.provenance is None
    marker = json.loads((Path(entry.cache_dir) / ".complete.json").read_bytes())
    assert set(marker) == {"schema", "source_version", "stems"}
    command_count = len(rig.runner.commands)
    assert not rig.controller.stems.reuse_compatible_stems_on_load(215)
    assert not rig.tasks
    assert len(rig.runner.commands) == command_count
    assert rig.controller.project.stem_cache[215] is None
