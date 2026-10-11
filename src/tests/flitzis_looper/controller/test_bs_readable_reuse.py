"""Composed BS IO/publication/reuse with only the neural network substituted.

The fixture assets are explicitly non-release bytes, verified by the actual
full-file asset verifier against their own expected size/hash. It executes the
actual worker, decoder, overlap-add and owned WAV writers. Current source/pair
ACKs are controller substitutes; actual Native artifact readers run separately.
No genuine release-model, music, callback or resource acceptance is claimed.
"""

import json
import os
import shutil
import sys
from importlib.util import find_spec
from pathlib import Path
from textwrap import dedent
from types import ModuleType, SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.bs_input_provenance import (
    BS_INPUT_CONTRACT_ENV,
    BS_INPUT_RECEIPT,
    BS_INPUT_RESULT,
    verified_input_output,
)
from flitzis_looper.controller import bs_roformer_assets, bs_roformer_generation, stem_provenance
from flitzis_looper.controller import stems as stems_module
from flitzis_looper.controller.bs_roformer_generation import BsRoformerStemGenerationBackend
from flitzis_looper.controller.stem_generation import run_command
from flitzis_looper.controller.stem_identity import request_fingerprint
from flitzis_looper.models import STEM_KINDS
from flitzis_looper.pcm16_wav import PcmWavReader
from flitzis_looper.stem_pair_selection import STEM_FINGERPRINT_SCHEMA
from tests.flitzis_looper.conftest import FakePreparedSourceTicket
from tests.flitzis_looper.controller.test_demucs_readable_reuse import (
    _accept_independent_publication,
    _assert_complete_generation,
    _digest,
    _marker_provenance,
    _owned_encoder,
    _Rig,
    _source_assignments,
)

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


_CHECKPOINT = b"owned BS neural-output fixture checkpoint; no release weights"
_CONFIG = b"owned BS neural-output fixture config; no release model definition"
_CHILD = dedent("""\
    import hashlib
    import json
    import os
    import sys
    from pathlib import Path
    import torch
    from torch import nn
    from flitzis_bs_roformer import __main__ as worker
    from flitzis_bs_roformer.model import verify_file
    from flitzis_looper.bs_input_provenance import BS_INPUT_CONTRACT_ENV, BS_INPUT_RECEIPT

    class OwnedNeuralOutput(nn.Module):
        def forward(self, data):
            weights = torch.tensor((.02, .03, .04, .10), device=data.device).view(1, 4, 1, 1)
            return data.unsqueeze(1) * weights

    def owned_model_fixture(checkpoint, config, device):
        expected = (
            (checkpoint, b"owned BS neural-output fixture checkpoint; no release weights"),
            (config, b"owned BS neural-output fixture config; no release model definition"),
        )
        for path, payload in expected:
            verify_file(path, len(payload), hashlib.sha256(payload).hexdigest())
        assert device in ("cpu", "cuda")
        return OwnedNeuralOutput().eval().to(device)

    worker.load_model = owned_model_fixture
    output = Path(sys.argv[sys.argv.index("--output") + 1])
    output.mkdir(parents=True, exist_ok=True)
    if os.environ.get("FLITZIS_FIXTURE_PREBORN") == "1":
        contract = os.environ[BS_INPUT_CONTRACT_ENV]
        images = json.loads(contract)
        forged = {
            "contract": contract,
            "runs": [
                {"kind": "ffprobe", "argv": [images[8][0]],
                 "proof": {"pid": 100, "images": [images[8], *images[1:8]]}},
                {"kind": "ffmpeg", "argv": [images[0][0]],
                 "proof": {"pid": 101, "images": images[:8]}},
            ],
        }
        preborn = output / BS_INPUT_RECEIPT
        preborn.write_text(json.dumps(forged), encoding="utf-8")
        preborn_bytes = preborn.read_bytes()
        before = preborn.stat()
    print("FLITZIS_FIXTURE_CHILD_PID=" + str(os.getpid()), flush=True)
    worker.main()
    if os.environ.get("FLITZIS_FIXTURE_PREBORN") == "1":
        after = preborn.stat()
        assert (before.st_dev, before.st_ino) == (after.st_dev, after.st_ino)
        assert preborn.read_bytes() == preborn_bytes
        preserved = [after.st_dev, after.st_ino, hashlib.sha256(preborn_bytes).hexdigest()]
        sys.stdout.write("FLITZIS_BS_PREBORN_V1:" + json.dumps(preserved) + "\\n")
        sys.stdout.flush()
""")


class _BsFixtureRunner:
    """Forward preflights and substitute only the worker's neural model loader."""

    def __init__(self, script: Path, source: Path, model: Path, package: Path) -> None:
        self.script = script
        self.source = source
        self.model = model
        self.package = package
        self.commands: list[list[str]] = []
        self.neural_commands: list[list[str]] = []
        self.records: list[dict[str, object]] = []
        self.preborn = False
        self.receipt_corruption: str | None = None

    def __call__(self, args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        self.commands.append(args.copy())
        if args[1:3] != ["-m", "flitzis_bs_roformer"]:
            return run_command(args, cwd=cwd, env=env)
        self._assert_worker_arguments(args, env)
        self.neural_commands.append(args.copy())
        child_env = env | {
            "PYTHONPATH": str(self.package.parent) + os.pathsep + env.get("PYTHONPATH", ""),
            "FLITZIS_FIXTURE_PREBORN": "1" if self.preborn else "0",
        }
        # Bare-name CreateProcess searches CWD before PATH on Windows. These
        # actual foreign-looking owned fixtures must never execute or change.
        poison = {
            cwd / name: b"owned non-executable CWD shadow" for name in ("ffmpeg.exe", "ffprobe.exe")
        }
        for path, payload in poison.items():
            path.write_bytes(payload)
        try:
            result = run_command([args[0], str(self.script), *args[3:]], cwd=cwd, env=child_env)
            assert result.returncode == 0, result.stderr
            self._capture_receipt(Path(args[6]), env, result.stdout)
            assert all(path.read_bytes() == payload for path, payload in poison.items())
        finally:
            for path in poison:
                path.unlink()
        return result

    def _assert_worker_arguments(self, args: list[str], env: dict[str, str]) -> None:
        assert args[0] == sys.executable
        assert args[3:6] == ["--source", str(self.source), "--output"]
        assert args[7:13] == [
            "--checkpoint",
            str(self.model / bs_roformer_assets.CHECKPOINT_FILENAME),
            "--config",
            str(self.model / bs_roformer_assets.CONFIG_FILENAME),
            "--device",
            args[12],
        ]
        assert args[12] in {"cpu", "cuda"}
        has_contract = BS_INPUT_CONTRACT_ENV in env
        assert has_contract

    def _capture_receipt(self, output: Path, env: dict[str, str], stdout: str) -> None:
        lines = [line for line in stdout.splitlines() if line.startswith(BS_INPUT_RESULT)]
        assert len(lines) == 1
        claims = json.loads(lines[0].removeprefix(BS_INPUT_RESULT))
        valid = verified_input_output(output, env[BS_INPUT_CONTRACT_ENV], stdout)
        body = (output / BS_INPUT_RECEIPT).read_bytes()
        record: dict[str, object] = {
            "stdout": stdout,
            "body_utf8": body.decode(),
            "body_sha256": _digest(output / BS_INPUT_RECEIPT),
            "valid": valid,
            "claims": claims,
            "preborn": self.preborn,
            "worker_argv": self.neural_commands[-1],
            "actual_fixture_argv": [
                sys.executable,
                str(self.script),
                *self.neural_commands[-1][3:],
            ],
            "fixture_script_sha256": _digest(self.script),
            "worker_process_id": int(
                next(
                    line.removeprefix("FLITZIS_FIXTURE_CHILD_PID=").split()[0]
                    for line in stdout.splitlines()
                    if line.startswith("FLITZIS_FIXTURE_CHILD_PID=")
                )
            ),
        }
        self.records.append(record)
        if self.preborn:
            assert claims == []
            assert not valid
            self._assert_matching_fields(json.loads(body), env[BS_INPUT_CONTRACT_ENV])
            prefix = "FLITZIS_BS_PREBORN_V1:"
            assert stdout.count(prefix) == 1
            preserved, _end = json.JSONDecoder().raw_decode(stdout.partition(prefix)[2])
            actual = (output / BS_INPUT_RECEIPT).stat()
            assert preserved == [actual.st_dev, actual.st_ino, record["body_sha256"]]
            record["matching_preborn_preserved"] = True
            return
        assert valid
        assert len(claims) == 1
        path, device, identity, digest = claims[0]
        actual = Path(path).stat()
        assert (actual.st_dev, actual.st_ino) == (device, identity)
        assert digest == record["body_sha256"]
        receipt = json.loads(body)
        runs = receipt["runs"]
        assert len(runs) == 2
        assert [run["kind"] for run in runs] == ["ffprobe", "ffmpeg"]
        assert len({run["proof"]["pid"] for run in runs}) == 2
        assert all(len(run["proof"]["images"]) == 8 for run in runs)
        self._assert_matching_fields(receipt, env[BS_INPUT_CONTRACT_ENV])
        if self.receipt_corruption is not None:
            self._assert_changed_receipt_rejected(
                output, env[BS_INPUT_CONTRACT_ENV], stdout, record
            )

    @staticmethod
    def _assert_matching_fields(body: dict[str, object], contract: str) -> None:
        expected = json.loads(contract)
        assert body["contract"] == contract
        runs = body["runs"]
        assert isinstance(runs, list)
        assert len(runs) == 2
        assert [run["kind"] for run in runs] == ["ffprobe", "ffmpeg"]
        assert runs[0]["proof"]["images"] == [expected[8], *expected[1:8]]
        assert runs[1]["proof"]["images"] == expected[:8]
        assert runs[0]["argv"][0] == expected[8][0]
        assert runs[1]["argv"][0] == expected[0][0]
        assert runs[0]["proof"]["pid"] != runs[1]["proof"]["pid"]

    def _assert_changed_receipt_rejected(
        self, output: Path, contract: str, stdout: str, record: dict[str, object]
    ) -> None:
        path = output / BS_INPUT_RECEIPT
        original = path.read_bytes()
        before = path.stat()
        before_identity = (before.st_dev, before.st_ino)
        if self.receipt_corruption == "body":
            # Valid JSON with precisely unchanged model/tool/argv/PID facts;
            # only the child's returned full body digest becomes stale.
            path.write_bytes(original + b"\n")
            after = path.stat()
            assert (after.st_dev, after.st_ino) == before_identity
            assert _digest(path) != record["body_sha256"]
            self._assert_matching_fields(json.loads(path.read_bytes()), contract)
        else:
            assert self.receipt_corruption == "file_id"
            replacement = path.with_name(path.name + ".replacement")
            replacement.write_bytes(original)
            replacement.replace(path)
            after = path.stat()
            assert (after.st_dev, after.st_ino) != before_identity
            assert _digest(path) == record["body_sha256"]
            self._assert_matching_fields(json.loads(path.read_bytes()), contract)
        assert not verified_input_output(output, contract, stdout)
        record["late_change"] = {
            "kind": self.receipt_corruption,
            "before_identity": before_identity,
            "after_identity": [after.st_dev, after.st_ino],
            "current_body_sha256": _digest(path),
            "rejected": True,
        }


class _BsRecordingBackend(BsRoformerStemGenerationBackend):
    def __init__(self, runner: _BsFixtureRunner) -> None:
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


class _BsRig(_Rig[_BsRecordingBackend, _BsFixtureRunner]):
    def assert_processing_writes_denied(self) -> None:
        super().assert_processing_writes_denied()
        for path in (
            self.checkpoint.with_name(bs_roformer_assets.CONFIG_FILENAME),
            self.runner.package / "audio.py",
            self.encoder.with_name("ffprobe.exe"),
        ):
            with pytest.raises(PermissionError):
                path.write_bytes(b"rewrite during actual BS input/model/code custody")

    def _assert_actual_processing_witnesses(self, job: StemGenerationJob) -> None:
        guard = job.processing_files
        assert guard is not None
        expected = stem_provenance.processing_paths(self.backend.requests[0])
        assert len(expected) == 33
        assert len(guard.witnesses()) == 33
        proofs = []
        for (name, digest, size), requested in zip(guard.witnesses(), expected, strict=True):
            path = Path(name)
            assert path.samefile(requested)
            assert digest == _digest(path)
            assert size == path.stat().st_size
            proofs.append((name, digest, size, path.stat().st_dev, path.stat().st_ino))
        guard.verify()
        self.runner.records[-1]["full_processing_witnesses"] = proofs


def _owned_model_verifier(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Own declared model bytes and their guarded full-file verifier source."""
    model = tmp_path / "owned-bs-model"
    model.mkdir()
    checkpoint = model / bs_roformer_assets.CHECKPOINT_FILENAME
    checkpoint.write_bytes(_CHECKPOINT)
    (model / bs_roformer_assets.CONFIG_FILENAME).write_bytes(_CONFIG)
    # Actual full-file verifier remains installed; only its declared expected
    # fixture bytes differ from the unchanged productive release constants.
    fixture_assets = (
        (checkpoint.name, len(_CHECKPOINT), _digest(checkpoint)),
        (
            bs_roformer_assets.CONFIG_FILENAME,
            len(_CONFIG),
            _digest(model / bs_roformer_assets.CONFIG_FILENAME),
        ),
    )
    # Bind the fixture substitution to actual guarded, compiled source and its
    # loaded globals, rather than exempting a patched production constant.
    original_asset_source = Path(bs_roformer_assets.__file__).read_text(encoding="utf-8")
    begin = original_asset_source.index("MODEL_ASSETS = (")
    end = original_asset_source.index("\nASSET_BASE_URL", begin)
    fixture_source = (
        original_asset_source[:begin]
        + f"MODEL_ASSETS = {fixture_assets!r}\n"
        + original_asset_source[end:]
    )
    asset_path = tmp_path / "owned-bs-assets.py"
    asset_path.write_text(fixture_source, encoding="utf-8")
    asset_module = ModuleType("flitzis_owned_bs_assets")
    asset_module.__dict__.update({
        "__file__": str(asset_path),
        "__package__": "flitzis_looper.controller",
    })
    monkeypatch.setitem(sys.modules, asset_module.__name__, asset_module)
    exec(compile(fixture_source, str(asset_path), "exec", dont_inherit=True), asset_module.__dict__)
    asset_module.verify_model_assets(model)
    monkeypatch.setattr(
        bs_roformer_generation, "verify_model_assets", asset_module.verify_model_assets
    )
    return model


@pytest.fixture
def bs_rig(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> Iterator[_BsRig]:
    if os.name != "nt":
        pytest.skip("Actual shared input-tool module/FileID proof is Windows-specific")
    encoder = _owned_encoder(tmp_path, monkeypatch)
    source = _source_assignments(controller, tmp_path)
    controller.project.stem_separator = "bs-roformer:musdb18hq"
    model = _owned_model_verifier(tmp_path, monkeypatch)
    monkeypatch.setattr(stems_module, "separator_model_cache_dir", lambda _separator: model)
    vendor = Path(__file__).resolve().parents[4] / "vendor/bs_roformer/src/flitzis_bs_roformer"
    package = tmp_path / "owned-package/flitzis_bs_roformer"
    for name in stem_provenance.BS_PRODUCER_FILES:
        target = package / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(vendor / name, target)
    original_find_spec = find_spec
    monkeypatch.setattr(
        stem_provenance,
        "find_spec",
        lambda name: (
            SimpleNamespace(submodule_search_locations=[str(package)])
            if name == "flitzis_bs_roformer"
            else original_find_spec(name)
        ),
    )
    script = tmp_path / "owned-bs-neural-output.py"
    script.write_text(_CHILD, encoding="utf-8")
    runner = _BsFixtureRunner(script, source, model, package)
    backend = _BsRecordingBackend(runner)
    tasks: list[Callable[[], None]] = []
    rig = _BsRig(
        controller,
        audio_engine_mock,
        model / bs_roformer_assets.CHECKPOINT_FILENAME,
        encoder,
        backend,
        runner,
        tasks,
        [],
    )
    controller.stems._stem_backend = backend
    controller.stems._stem_task_runner = tasks.append
    controller.stems._pair_preparations._runner = tasks.append
    audio_engine_mock.prepare_stem_pair = Mock(side_effect=rig.prepare_pair)

    def source_ticket(sample_id: int, _version: str) -> FakePreparedSourceTicket:
        ticket = FakePreparedSourceTicket()
        ticket.source_request = sample_id + 1
        return ticket

    def publish(_pair: object, ticket: FakePreparedSourceTicket) -> None:
        ticket.status = "pending"

    audio_engine_mock.capture_prepared_source.side_effect = source_ticket
    audio_engine_mock.publish_stem_pair.side_effect = publish
    try:
        yield rig
    finally:
        # The backend attempt is gone after generation. Preserve bounded actual
        # observation bytes outside the product fixture before normal teardown.
        retained = {
            "scope": (
                "actual BS probe/decode/worker/WAV/artifacts/promotion/reuse; "
                "owned NN output; source/pair ACK doubles; actual Native artifact leases"
            ),
            "release_model_neural_music_resources": "REQUIRED OPEN",
            "commands": runner.commands,
            "neural_commands": runner.neural_commands,
            "child_receipts": runner.records,
            "current_sets": [
                {
                    "slot": slot,
                    "cache_dir": entry.cache_dir,
                    "available": entry.available,
                    "source_version": entry.source_version,
                    "fingerprint": entry.generation_fingerprint,
                    "producer": entry.producer_identity,
                    "files": _current_files(Path(entry.cache_dir)),
                }
                for slot in (0, 215)
                if (entry := controller.project.stem_cache[slot]) is not None
            ],
            "selected_counts": [pair.selected for pair in rig.pairs],
            "subscriber215": {
                "gain_db": controller.project.pad_gain_db[215],
                "key_lock": controller.project.pad_key_lock[215],
                "mask": controller.session.pad_stem_enabled_mask[215],
            },
        }
        receipt_path = tmp_path.parent / (tmp_path.name + "-bs-software-receipt.json")
        receipt_path.write_text(json.dumps(retained, indent=2), encoding="utf-8")
        controller.stems.shut_down()


def _current_files(directory: Path) -> list[dict[str, object]]:
    if not directory.is_dir():
        return []
    return [
        {
            "leaf": path.name,
            "sha256": _digest(path),
            "bytes": path.stat().st_size,
            "device": path.stat().st_dev,
            "file_id": path.stat().st_ino,
            **(_wav_geometry(path) if path.suffix == ".wav" else {}),
            **({"marker": json.loads(path.read_bytes())} if path.name == ".complete.json" else {}),
        }
        for path in directory.iterdir()
        if path.is_file()
    ]


def _wav_geometry(path: Path) -> dict[str, object]:
    with PcmWavReader(path) as handle:
        return {
            "geometry": [
                handle.getnframes(),
                handle.getframerate(),
                handle.getnchannels(),
                handle.getsampwidth(),
            ],
            "pcm_verify_sha256": handle.verify(),
        }


def _assert_known_bs(rig: _BsRig, entry: StemCacheEntry) -> None:
    assert rig.backend.results[0].decoder_verified
    assert len(rig.runner.records) == 1
    request = rig.backend.requests[0]
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


@pytest.mark.parametrize("on_load", [False, True])
def test_bs_actual_decode_worker_publication_and_own_subscriber_completed_reuse(
    bs_rig: _BsRig, monkeypatch: pytest.MonkeyPatch, *, on_load: bool
) -> None:
    rig = bs_rig
    origin = rig.generate_origin(monkeypatch)
    before = _assert_complete_generation(rig, origin)
    _assert_known_bs(rig, origin)
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


@pytest.mark.parametrize(
    "corruption", ["model", "implementation", "input_tool", "historical_marker"]
)
def test_bs_passive_reuse_rejects_changed_or_unknown_lineage_without_new_commands(
    bs_rig: _BsRig, monkeypatch: pytest.MonkeyPatch, corruption: str
) -> None:
    rig = bs_rig
    origin = rig.generate_origin(monkeypatch)
    _assert_known_bs(rig, origin)
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
        path = {
            "model": rig.checkpoint,
            "implementation": rig.runner.package / "audio.py",
            "input_tool": rig.encoder.with_name("ffprobe.exe"),
        }[corruption]
        with path.open("ab") as changed:
            changed.write(b"owned post-settlement changed implementation/input/model bytes")
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


def test_bs_preexisting_forged_tool_record_cannot_stamp_producer(
    bs_rig: _BsRig, monkeypatch: pytest.MonkeyPatch
) -> None:
    rig = bs_rig
    rig.runner.preborn = True
    entry = rig.generate_origin(monkeypatch)
    _assert_complete_generation(rig, entry)
    assert not rig.backend.results[0].decoder_verified
    assert entry.generation_fingerprint is None
    assert entry.producer_identity is None
    assert entry.fingerprint_schema is None
    assert entry.pair is not None
    assert entry.pair.provenance is None
    marker = json.loads((Path(entry.cache_dir) / ".complete.json").read_bytes())
    assert set(marker) == {"schema", "source_version", "stems"}
    commands = len(rig.runner.commands)
    assert not rig.controller.stems.reuse_compatible_stems_on_load(215)
    assert not rig.tasks
    assert len(rig.runner.commands) == commands


@pytest.mark.parametrize("receipt_corruption", ["body", "file_id"])
def test_bs_actual_child_receipt_rejects_late_body_change_or_equal_byte_new_file_id(
    bs_rig: _BsRig, monkeypatch: pytest.MonkeyPatch, receipt_corruption: str
) -> None:
    rig = bs_rig
    rig.runner.receipt_corruption = receipt_corruption
    entry = rig.generate_origin(monkeypatch)
    _assert_complete_generation(rig, entry)
    assert len(rig.runner.records) == 1
    actual = rig.runner.records[0]
    assert actual["valid"] is True
    change = actual["late_change"]
    assert isinstance(change, dict)
    assert change["kind"] == receipt_corruption
    assert change["rejected"] is True
    assert not rig.backend.results[0].decoder_verified
    assert entry.generation_fingerprint is None
    assert entry.producer_identity is None
    assert entry.fingerprint_schema is None
    assert entry.pair is not None
    assert entry.pair.provenance is None
    commands = len(rig.runner.commands)
    assert not rig.controller.stems.reuse_compatible_stems_on_load(215)
    assert not rig.tasks
    assert len(rig.runner.commands) == commands
