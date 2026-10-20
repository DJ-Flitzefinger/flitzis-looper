"""Shared fake backend keeps real Native witness checks on owned small inputs."""

import hashlib
import os
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from flitzis_looper_audio import SealedProcessingFiles
from tests.flitzis_looper.controller.test_asset_lifecycle import _source

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller import AppController
    from tests.flitzis_looper.conftest import FakeStemGenerationBackend


def test_default_fake_backend_uses_owned_small_actual_native_processing_witnesses(
    controller: AppController,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
) -> None:
    _source(controller, tmp_path)
    tasks: list[Callable[[], None]] = []
    controller.stems._stem_task_runner = tasks.append
    assert controller.stems.generate_stems_async(0)
    job = next(iter(controller.stems._jobs.values()))
    assert len(tasks) == 1
    tasks.pop(0)()
    guard = job.processing_files
    assert isinstance(guard, SealedProcessingFiles)
    expected = tuple(
        tmp_path / "owned-fake-stem-producer" / name
        for name in ("checkpoint.bin", "recipe.json", "backend.py")
    )
    witnesses = guard.witnesses()
    assert len(witnesses) == len(expected) == 3
    assert sum(size for _name, _digest, size in witnesses) < 1024
    for (name, digest, size), path in zip(witnesses, expected, strict=True):
        assert Path(name).samefile(path)
        content = path.read_bytes()
        assert size == len(content)
        assert digest == hashlib.sha256(content).hexdigest()
    guard.verify()
    assert job.producer_identity is None
    assert job.generation_fingerprint is None

    if os.name == "nt":
        with pytest.raises(PermissionError):
            expected[0].write_bytes(b"overwrite while held")
        with pytest.raises(PermissionError):
            expected[0].rename(expected[0].with_name("replaced.bin"))
    guard.verify()
    controller.stems.on_frame_render()
    assert len(tasks) == 1
    assert not stem_backend.requests
    tasks.pop(0)()
    assert len(stem_backend.requests) == 1
    controller.stems.on_frame_render()
    selected = controller.project.stem_cache[0]
    assert selected is not None
    assert selected.available
    assert not controller.stems._jobs
    assert job.processing_files is None
    assert controller._assets._reserved == 0
    del guard

    # Actual Native sealing hashes the changed complete file after final release.
    changed = b"changed fake checkpoint after actual read settlement"
    expected[0].write_bytes(changed)
    replacement = SealedProcessingFiles([str(path) for path in expected])
    replacement.verify()
    next_witnesses = replacement.witnesses()
    assert next_witnesses[0][1] != witnesses[0][1]
    assert next_witnesses[0][1] == hashlib.sha256(changed).hexdigest()
    assert next_witnesses[0][2] == len(changed)
    assert next_witnesses[1:] == witnesses[1:]
