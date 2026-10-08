"""Embedded productive controller probe; only device setup is replaced by a native producer."""
# This is compiled into a Rust test, not imported as a Python package.
# ruff: noqa: INP001

import hashlib
import importlib
import sys
from pathlib import Path
from typing import Any

from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import ProjectState, SessionState


def runtime_identity(repository_root: str) -> dict[str, object]:
    repository = Path(repository_root).resolve()
    modules = {}
    for name in (
        "flitzis_looper.controller.loader",
        "flitzis_looper.controller.persistence",
        "flitzis_looper.models",
        "flitzis_looper_audio",
        "flitzis_looper_audio.flitzis_looper_audio",
    ):
        module = importlib.import_module(name)
        path = Path(module.__file__).resolve()
        assert path.is_relative_to(repository), (name, path, repository)
        modules[name] = {
            "path": str(path),
            "bytes": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }
    return {
        "repository": str(repository),
        "python_version": sys.version,
        "python_executable": sys.executable,
        "python_base_prefix": sys.base_prefix,
        "modules": modules,
    }


class Audio:
    def __init__(self, bridge: Any) -> None:
        self.bridge = bridge

    def __getattr__(self, name: str) -> Any:
        return getattr(self.bridge.engine, name)

    def load_sample_async(self, *args: Any, **kwargs: Any) -> int:
        return self.bridge.load_sample_async(*args, **kwargs)

    def output_sample_rate(self) -> int:
        return 48000

    def loaded_sample_shape(self, sample_id: int) -> tuple[int, int, int]:
        return self.bridge.loaded_sample_shape(sample_id)


class Probe:
    def __init__(self, bridge: Any, paths: list[str], *, warm: bool = False) -> None:
        self.audio = Audio(bridge)
        self.project = ProjectPersistence.from_config_path().project if warm else ProjectState()
        self.loaded_saved_project = warm
        self.session = SessionState()
        if warm:
            assert self.project.sample_paths[:200] == paths
            assert sum(path is not None for path in self.project.sample_paths) == 200
            assert self.project.sample_durations[:200] == [120.0] * 200
            assert not any(self.project.pad_loop_auto[:200])
            assert self.project.pad_loop_start_s[:200] == [42.0] * 200
            assert self.project.pad_loop_end_s[:200] == [42.5] * 200
        for pad, path in enumerate(paths):
            if not warm:
                self.project.sample_paths[pad] = path
                self.project.pad_loop_auto[pad] = False
                self.project.pad_loop_start_s[pad] = 42.0
                self.project.pad_loop_end_s[pad] = 42.5
        self.loader = LoaderController(self.project, self.session, self.audio, lambda pad: None)
        self.loader.restore_samples_from_project_state()
        self.initial_deferred = len(self.loader._deferred_restores)
        self.max_retry_admissions = 0

    def poll(self) -> bool:
        before = len(self.loader._deferred_restores)
        self.loader.poll_loader_events()
        self.max_retry_admissions = max(
            self.max_retry_admissions, before - len(self.loader._deferred_restores)
        )
        assert self.max_retry_admissions <= 8
        assert not self.session.sample_load_errors, self.session.sample_load_errors
        return not self.session.loading_sample_ids and not self.loader._deferred_restores

    def save(self) -> None:
        persistence = ProjectPersistence(self.project)
        persistence.bind_audio(self.audio)
        persistence.mark_dirty()
        assert persistence.flush_if_dirty()
        assert not persistence._dirty

    def summary(self) -> dict[str, object]:
        assert sum(path is not None for path in self.project.sample_paths) == 200
        assert all(value == 120.0 for value in self.project.sample_durations[:200])
        return {
            "project_assignments": 200,
            "controller_terminal": True,
            "loaded_actual_saved_project": self.loaded_saved_project,
            "initial_deferred": self.initial_deferred,
            "max_retry_admissions_per_poll": self.max_retry_admissions,
            "historical_acceptance_promoted": False,
            "timing_intent": "legacy",
        }

    def close(self) -> None:
        self.loader.shut_down()
        self.loader._assets.release_saved_assignments()
