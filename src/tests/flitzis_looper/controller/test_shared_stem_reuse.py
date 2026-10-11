"""Actual model/file/marker witnesses with controller-only source/pair ACK substitutes.

The owned model fixtures are explicit producer evidence, not neural inference.
Native complete pair/callback custody is covered separately by the Native suite.
"""

import hashlib
import json
import shutil
from importlib.util import find_spec
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import stem_generation, stem_provenance
from flitzis_looper.controller import stems as stems_module
from flitzis_looper.controller.bs_roformer_assets import CHECKPOINT_FILENAME, CONFIG_FILENAME
from flitzis_looper.controller.stem_cache import expected_stem_files, promote_generation_artifacts
from flitzis_looper.controller.stem_generation import AudioShape, StemGenerationRequest
from flitzis_looper.controller.stem_identity import request_fingerprint
from flitzis_looper.models import STEM_KINDS, PadContentIdentity, StemCacheEntry
from flitzis_looper.stem_pair_selection import StemPairSelection
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import FakePreparedSourceTicket, write_test_stem_marker

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller import AppController
    from tests.flitzis_looper.conftest import FakeStemGenerationBackend


def _actual_bs_processing_request(
    tmp: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[StemGenerationRequest, Path]:
    """Copy executed package code, with explicit non-neural model/config inputs."""
    module = find_spec("flitzis_bs_roformer")
    assert module is not None
    assert module.submodule_search_locations is not None
    installed = Path(next(iter(module.submodule_search_locations)))
    package = tmp / "owned-bs-implementation"
    for name in stem_provenance.BS_PRODUCER_FILES:
        destination = package / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(installed / name, destination)
    model = tmp / "owned-bs-model"
    model.mkdir()
    for name in (CHECKPOINT_FILENAME, CONFIG_FILENAME):
        (model / name).write_bytes(b"declared non-neural owned producer input")
    source = tmp / "source.wav"
    write_mono_pcm16_wav(source, 44_100)
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    encoder = tmp / "owned-unproved-ffmpeg.exe"
    encoder.write_bytes(b"declared non-runnable owned tool; no actual module proof")
    request = StemGenerationRequest(
        sample_id=0,
        source_path=source,
        source_version=f"source.wav|sha256-v1:{digest}",
        cache_dir=tmp / "stems",
        target_shape=AudioShape(44_100, 1, 128),
        model_cache_dir=model,
        separator="bs-roformer:musdb18hq",
        encoder_executable=encoder,
    )
    # Only the package location changes. Actual library policy and Native full
    # file witnesses remain production code, with no inference or fake identity.
    monkeypatch.setattr(
        stem_provenance,
        "find_spec",
        lambda _name: SimpleNamespace(submodule_search_locations=[package]),
    )
    return request, package


def _bs_implementation_witnesses(
    request: StemGenerationRequest, package: Path
) -> dict[str, tuple[str, int]]:
    processing = stem_provenance.capture_processing_files(request)
    assert processing is not None
    assert processing[1] is None
    witnesses = processing[0].witnesses()
    expected = stem_provenance.processing_paths(request)
    assert len(witnesses) == 20
    assert len(expected) == 20
    for (name, digest, size), path in zip(witnesses, expected, strict=True):
        assert Path(name).samefile(path)
        assert digest == hashlib.sha256(path.read_bytes()).hexdigest()
        assert size == path.stat().st_size
        if path.is_relative_to(package) or path.is_relative_to(request.model_cache_dir):
            with pytest.raises(PermissionError):
                path.write_bytes(b"rewrite during actual BS model/config/code custody")
    processing[0].verify()
    return {
        str(path): (digest, size)
        for path, (_, digest, size) in zip(expected, witnesses, strict=True)
    }


@pytest.mark.parametrize(
    "changed_file",
    [
        "audio.py",
        "network/attend.py",
        "network/bs_roformer.py",
        "__init__.py",
        "network/__init__.py",
    ],
)
def test_actual_bs_implementation_changes_are_sealed_but_unproved_tools_keep_producer_unknown(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, changed_file: str
) -> None:
    request, package = _actual_bs_processing_request(tmp_path, monkeypatch)
    assert stem_provenance.processing_library_versions(request) is None
    fingerprint = request_fingerprint(request)
    assert fingerprint is not None
    before = _bs_implementation_witnesses(request, package)
    changed = package / changed_file
    assert str(changed) in before
    # All actual Native readers returned before changing this owned copy. No
    # installed package, real checkpoint, or neural implementation is executed.
    with changed.open("ab") as output:
        output.write(b"\n# owned implementation changed after reader settlement\n")
    after = _bs_implementation_witnesses(request, package)
    assert after[str(changed)] != before[str(changed)]
    assert request_fingerprint(request) == fingerprint
    assert stem_provenance.processing_library_versions(request) is None


class _SelectedPair:
    def __init__(self, root: str, cache: str) -> None:
        self.selection = StemPairSelection(
            descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{'b' * 32}.json",
            stem_set_identity="c" * 64,
            wav_generation=cache,
            pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{'d' * 32}",
        )
        self.selected = 0
        self.discarded = 0

    def selection_json(self) -> str:
        return self.selection.model_dump_json()

    def has_components(self) -> bool:
        return False

    def select(self) -> None:
        self.selected += 1

    def discard(self) -> None:
        self.discarded += 1


def _known_set(
    controller: AppController, audio: Mock, tmp: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[StemCacheEntry, _SelectedPair, list[Path]]:
    root = "samples/First name.wav"
    source = tmp / root / "First name.wav"
    source.parent.mkdir(parents=True)
    write_mono_pcm16_wav(source, 44_100)
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    (source.parent / ".material.json").write_text(
        json.dumps({
            "schema_version": 1,
            "sha256": digest,
            "bytes": source.stat().st_size,
            "original_leaf": source.name,
        }),
        encoding="utf-8",
    )
    version = f"{root}/First name.wav|sha256-v1:{digest}"
    model = tmp / "owned-model"
    model.mkdir()
    files = [model / "checkpoint.bin", model / "config.yaml", model / "definition.py"]
    for index, path in enumerate(files):
        path.write_bytes(f"isolated producer input {index}".encode())
    controller.project.stem_separator = "bs-roformer:musdb18hq"
    monkeypatch.setattr(stems_module, "separator_model_cache_dir", lambda _separator: model)
    monkeypatch.setattr(stem_provenance, "processing_paths", lambda _request: tuple(files))
    monkeypatch.setattr(
        stem_provenance, "processing_library_versions", lambda _request: (("owned-fixture", "1"),)
    )
    # This controller-only fixture declares execution lineage separately from
    # its actual Native file guard; composed backend tests use the real verifier.
    monkeypatch.setattr(
        stem_provenance, "loaded_code_identity", lambda _paths: "declared-controller-fixture"
    )
    monkeypatch.setattr(stem_provenance, "processing_source_roots", lambda _request, _guard: (tmp,))
    request = StemGenerationRequest(
        sample_id=0,
        source_path=source,
        source_version=version,
        cache_dir=source.parent / "stems",
        target_shape=AudioShape(44_100, 1, 128),
        model_cache_dir=model,
        separator=controller.project.stem_separator,
    )
    processing = stem_provenance.capture_processing_files(request)
    assert processing is not None
    producer = processing[1]
    processing[0].verify()
    processing = None
    cache = f"{root}/stems/.ready-{'a' * 32}"
    directory = tmp / cache
    directory.mkdir(parents=True)
    for kind in STEM_KINDS:
        write_mono_pcm16_wav(directory / f"{kind}.wav", 44_100)
    write_test_stem_marker(directory, version)
    fingerprint = request_fingerprint(request)
    marker = json.loads((directory / ".complete.json").read_bytes())
    marker |= {"generation_fingerprint": fingerprint, "producer_identity": producer}
    (directory / ".complete.json").write_text(json.dumps(marker), encoding="utf-8")
    entry = StemCacheEntry(
        source_version=version,
        cache_dir=cache,
        stems=expected_stem_files(cache),
        available=True,
        generation_fingerprint=fingerprint,
        producer_identity=producer,
    )
    controller.project.stem_cache[0] = entry
    for slot in (0, 215):
        controller.project.sample_paths[slot] = source.relative_to(tmp).as_posix()
        controller.project.pad_content[slot] = PadContentIdentity(
            instance_id=f"{slot + 1:032x}", material_id=digest[:32]
        )
        controller.project.sample_durations[slot] = 128 / 44_100
    selected = _SelectedPair(root, cache)
    audio.prepare_stem_pair = Mock(return_value=selected)
    audio.capture_prepared_source.side_effect = lambda *_args: FakePreparedSourceTicket()
    controller._assets.sync_assignments()
    return entry, selected, files


@pytest.mark.parametrize("on_load", [False, True])
def test_compatible_completed_set_uses_real_witness_worker_and_no_backend_inference(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    on_load: bool,
) -> None:
    entry, selected, files = _known_set(controller, audio_engine_mock, tmp_path, monkeypatch)
    native_prepare = audio_engine_mock.prepare_stem_pair.side_effect

    def prepare(*_args: object) -> _SelectedPair:
        # Actual FILE_SHARE_READ sealing protects all owned producer images.
        with pytest.raises(PermissionError):
            files[0].write_bytes(b"mutated during compatible pair reopening")
        return selected

    audio_engine_mock.prepare_stem_pair.side_effect = prepare
    if on_load:
        assert controller.stems.reuse_compatible_stems_on_load(215)
    else:
        assert controller.stems.generate_stems_async(215)
    assert not stem_backend.requests
    assert controller.project.stem_cache[215] is None
    controller.stems.on_frame_render()
    current = controller.project.stem_cache[215]
    assert current is not None
    assert current.available
    assert current is not entry
    assert current.cache_dir == entry.cache_dir
    assert current.generation_fingerprint == entry.generation_fingerprint
    assert current.producer_identity == entry.producer_identity
    assert selected.selected == 1
    assert not stem_backend.requests
    audio_engine_mock.prepare_stem_pair.assert_called_once()
    controller.loader.unload_sample(0)
    assert controller.project.stem_cache[215] is current
    assert (tmp_path / current.cache_dir).is_dir()
    audio_engine_mock.prepare_stem_pair.side_effect = native_prepare


@pytest.mark.parametrize("corruption", ["model", "marker"])
def test_unknown_or_changed_actual_producer_never_becomes_compatible_by_saved_metadata(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    corruption: str,
) -> None:
    entry, _selected, files = _known_set(controller, audio_engine_mock, tmp_path, monkeypatch)
    if corruption == "model":
        files[0].write_bytes(b"new actual model bytes")
    else:
        marker_path = tmp_path / entry.cache_dir / ".complete.json"
        marker = json.loads(marker_path.read_bytes())
        marker.pop("generation_fingerprint")
        marker.pop("producer_identity")
        marker_path.write_text(json.dumps(marker), encoding="utf-8")
    assert controller.stems.reuse_compatible_stems_on_load(215)
    controller.stems.on_frame_render()
    assert controller.project.stem_cache[215] is None
    assert controller.project.stem_cache[0] is entry
    error = controller.stems.stem_generation_error(215)
    assert error is not None
    assert "producer" in error or "provenance" in error
    assert not stem_backend.requests
    audio_engine_mock.prepare_stem_pair.assert_not_called()


def test_unknown_historical_set_is_not_stamped_with_current_model_config(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    entry, _selected, _files = _known_set(controller, audio_engine_mock, tmp_path, monkeypatch)
    historical = entry.model_copy(
        update={"generation_fingerprint": None, "producer_identity": None}
    )
    controller.project.stem_cache[0] = historical
    assert not controller.stems.reuse_compatible_stems_on_load(215)
    assert controller.project.stem_cache[0] is historical
    assert controller.project.stem_cache[215] is None
    audio_engine_mock.prepare_stem_pair.assert_not_called()


@pytest.mark.parametrize("corruption", ["model", "marker"])
def test_explicit_generate_incompatible_set_runs_one_fresh_backend_and_retains_old_set(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    corruption: str,
) -> None:
    entry, _selected, files = _known_set(controller, audio_engine_mock, tmp_path, monkeypatch)
    if corruption == "model":
        files[0].write_bytes(b"new model under the same selected name")
    else:
        marker_path = tmp_path / entry.cache_dir / ".complete.json"
        marker = json.loads(marker_path.read_bytes())
        marker.pop("generation_fingerprint")
        marker.pop("producer_identity")
        marker_path.write_text(json.dumps(marker), encoding="utf-8")
    tasks: list[Callable[[], None]] = []
    controller.stems._pair_preparations._runner = tasks.append
    controller.stems._stem_task_runner = tasks.append
    assert controller.stems.generate_stems_async(215)
    assert len(tasks) == 1
    tasks.pop(0)()  # actual Native model + marker verification, without inference
    assert not stem_backend.requests
    controller.stems.on_frame_render()
    assert len(tasks) == 1  # exactly one newly admitted ordinary generation
    assert controller.project.stem_cache[0] is entry
    assert (tmp_path / entry.cache_dir).is_dir()
    audio_engine_mock.prepare_stem_pair.assert_not_called()
    tasks.pop(0)()
    assert len(stem_backend.requests) == 1
    request = stem_backend.requests[0]
    assert request.source_version == entry.source_version
    assert request.model_cache_dir == files[0].parent
    assert ".generation-" in request.cache_dir.name
    assert controller.project.stem_cache[0] is entry
    assert (tmp_path / entry.cache_dir).is_dir()
    controller.loader.unload_sample(215)


def test_explicit_generate_compatibility_rejection_does_not_follow_replaced_content(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    entry, _selected, files = _known_set(controller, audio_engine_mock, tmp_path, monkeypatch)
    files[0].write_bytes(b"changed model")
    tasks: list[Callable[[], None]] = []
    controller.stems._pair_preparations._runner = tasks.append
    controller.stems._stem_task_runner = tasks.append
    assert controller.stems.generate_stems_async(215)
    tasks.pop(0)()
    controller.project.pad_content[215] = PadContentIdentity(instance_id="9" * 32)
    controller.stems.on_frame_render()
    assert not tasks
    assert not stem_backend.requests
    assert controller.project.stem_cache[0] is entry


@pytest.mark.parametrize("settlement", ["promote", "error", "cancel"])
def test_processing_file_guard_outlives_backend_return_through_cache_publication_or_cancel(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    settlement: str,
) -> None:
    _entry, _selected, files = _known_set(controller, audio_engine_mock, tmp_path, monkeypatch)
    controller.project.stem_cache[0] = None
    tasks: list[Callable[[], None]] = []
    controller.stems._stem_task_runner = tasks.append
    if settlement == "error":
        stem_backend.error = RuntimeError("owned producer failed")
    assert controller.stems.generate_stems_async(215)
    admitted_entry = controller.project.stem_cache[215]
    assert admitted_entry is not None
    job = next(iter(controller.stems._jobs.values()))
    tasks.pop(0)()
    assert not job._running
    assert job.processing_files is not None
    with pytest.raises(PermissionError):
        files[0].write_bytes(b"model rewrite before queued result publication")
    if settlement == "cancel":
        controller.loader.unload_sample(215)
    else:
        original = promote_generation_artifacts

        def promote(entry: StemCacheEntry, directory: Path) -> StemCacheEntry:
            with pytest.raises(PermissionError):
                files[0].write_bytes(b"model rewrite during durable marker/WAV promotion")
            return original(entry, directory)

        monkeypatch.setattr(stems_module, "promote_generation_artifacts", promote)
        controller.stems.on_frame_render()
    assert job._disposed
    assert job.processing_files is None
    files[0].write_bytes(b"new model after physical producer settlement")
    assert len(stem_backend.requests) == 1
    if settlement == "cancel":
        assert controller.project.stem_cache[215] is None
    elif settlement == "error":
        # Ordinary errors preserve the admitted unavailable assignment; no
        # successful marker, pair or producer identity is inferred from it.
        assert controller.project.stem_cache[215] is admitted_entry
        assert not admitted_entry.available
        assert admitted_entry.pair is None
        assert admitted_entry.generation_fingerprint is None
        assert admitted_entry.producer_identity is None
        assert controller.stems.stem_generation_error(215) == "owned producer failed"
        assert not (tmp_path / admitted_entry.cache_dir / ".complete.json").exists()


def test_owned_encoder_witness_covers_common_pcm_and_keeps_unproved_demucs_producer_unknown(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = tmp_path / "owned-model"
    model.mkdir()
    (model / stem_generation.DEMUCS_REQUIRED_CHECKPOINTS["htdemucs"][0]).write_bytes(b"weights")
    package = tmp_path / "owned-demucs"
    (package / "remote").mkdir(parents=True)
    for name in stem_provenance.DEMUCS_PRODUCER_FILES:
        (package / name).write_bytes(f"isolated producer {name}".encode())
    tools = tmp_path / "owned-tools"
    tools.mkdir()
    encoder = tools / "ffmpeg.exe"
    encoder.write_bytes(b"owned encoder bytes, not a runnable inference fixture")
    monkeypatch.setattr(
        stem_generation, "demucs_cache_environment", lambda _model: {"PATH": str(tools)}
    )
    monkeypatch.setattr(
        stem_provenance,
        "find_spec",
        lambda _name: SimpleNamespace(submodule_search_locations=[package]),
    )
    resolved = stem_generation.resolved_demucs_encoder(model)
    assert resolved == encoder.resolve()
    request = StemGenerationRequest(
        sample_id=0,
        source_path=tmp_path / "source.wav",
        source_version="source|sha256-v1:" + "1" * 64,
        cache_dir=tmp_path / "cache",
        target_shape=AudioShape(44100, 1, 128),
        model_cache_dir=model,
        encoder_executable=resolved,
    )
    paths = stem_provenance.processing_paths(request)
    assert len(paths) == 24
    assert any(path.name == "pcm16_wav.py" for path in paths)
    assert paths[-1] == resolved
    assert stem_provenance.processing_library_versions(request) is None
    processing = stem_provenance.capture_processing_files(request)
    assert processing is not None
    assert len(processing[0].witnesses()) == 24
    assert processing[1] is None  # no shared encoder DLL execution identity is inferred
    with pytest.raises(PermissionError):
        encoder.write_bytes(b"encoder changed before publication")
    processing[0].verify()
    processing = None
    # The following identity describes only this explicitly attested owned
    # fixture, never the actual shared FFmpeg installation or neural inference.
    monkeypatch.setattr(
        stem_provenance, "processing_library_versions", lambda _request: (("owned-fixture", "1"),)
    )
    monkeypatch.setattr(
        stem_provenance, "loaded_code_identity", lambda _paths: "declared-encoder-file-fixture"
    )
    monkeypatch.setattr(
        stem_provenance, "processing_source_roots", lambda _request, _guard: (tmp_path,)
    )
    processing = stem_provenance.capture_processing_files(request)
    assert processing is not None
    previous_identity = processing[1]
    assert previous_identity is not None
    processing = None
    encoder.write_bytes(b"changed actual encoder bytes")
    processing = stem_provenance.capture_processing_files(request)
    assert processing is not None
    assert processing[1] != previous_identity
