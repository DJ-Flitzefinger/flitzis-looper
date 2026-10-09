"""Opt-in actual separator worker, publication and restoration probe without CPAL."""
# Compiled into an isolated ignored Rust test; never imported as a package.
# ruff: noqa: INP001

import hashlib
import json
import sys
import time
from dataclasses import asdict
from pathlib import Path
from typing import TYPE_CHECKING, Protocol, cast
from unittest.mock import patch

from flitzis_looper.controller.bs_roformer_generation import BsRoformerStemGenerationBackend
from flitzis_looper.controller.stem_cache import verified_stem_cache_available
from flitzis_looper.controller.stem_generation import (
    CommandResult,
    DemucsStemGenerationBackend,
    StemGenerationRequest,
    StemGenerationResult,
    run_command,
)
from flitzis_looper.controller.stem_separators import SelectedStemGenerationBackend
from flitzis_looper.controller.stems import StemController
from flitzis_looper.models import STEM_KINDS, STEM_SEPARATORS, ProjectState, SessionState

if TYPE_CHECKING:
    from flitzis_looper.controller.stem_generation import StemProgressCallback
    from flitzis_looper_audio import AudioEngine, PreparedSourceTicket


class NativeProducerBridge(Protocol):
    engine: AudioEngine

    def publish_prepared_stems(
        self, sample_id: int, version: str, cache_dir: str, ticket: PreparedSourceTicket
    ) -> None: ...

    def loaded_sample_shape(self, sample_id: int) -> tuple[int, int, int]: ...


class Audio:
    def __init__(self, bridge: NativeProducerBridge) -> None:
        self.bridge = bridge

    def __getattr__(self, name: str) -> object:
        return getattr(self.bridge.engine, name)

    def publish_prepared_stems(
        self, sample_id: int, version: str, cache_dir: str, ticket: PreparedSourceTicket
    ) -> None:
        self.bridge.publish_prepared_stems(sample_id, version, cache_dir, ticket)

    def loaded_sample_shape(self, sample_id: int) -> tuple[int, int, int]:
        return self.bridge.loaded_sample_shape(sample_id)


class RecordingBackend:
    """Record actual subprocess execution while using both production adapters."""

    def __init__(self, python_executable: Path) -> None:
        self.python_executable = python_executable
        self.commands: list[dict[str, object]] = []
        self.request: StemGenerationRequest | None = None
        self.result: StemGenerationResult | None = None
        self.selected = SelectedStemGenerationBackend({
            "bs-roformer:musdb18hq": BsRoformerStemGenerationBackend(command_runner=self.run),
            "demucs:htdemucs": DemucsStemGenerationBackend(command_runner=self.run),
        })

    def run(self, args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        # Embedded PyO3 reports the native test EXE as sys.executable. The
        # standalone productive app already uses its actual Python executable.
        actual_args = (
            [str(self.python_executable), *args[1:]] if args[0] == sys.executable else args
        )
        started = time.monotonic()
        result = run_command(actual_args, cwd=cwd, env=env)
        self.commands.append({
            "requested_args": args,
            "args": actual_args,
            "cwd": str(cwd),
            "seconds": time.monotonic() - started,
            "result": asdict(result),
        })
        return result

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        self.request = request
        self.result = self.selected.generate(request, progress)
        return self.result


class ForbiddenBackend:
    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        msg = "cache restore attempted separator inference"
        raise AssertionError(msg)


class Probe:
    def __init__(
        self,
        bridge: NativeProducerBridge,
        source_path: str,
        frames: int,
        rate: int,
        config_path: str,
    ) -> None:
        self.config = cast("dict[str, object]", json.loads(Path(config_path).read_text("utf-8")))
        separator = self.config["separator"]
        assert separator in STEM_SEPARATORS
        self.model_cache = Path(str(self.config["model_cache_dir"]))
        assert self.model_cache.is_absolute()
        self.output_path = Path(str(self.config["output_path"]))
        self.audio = cast("AudioEngine", Audio(bridge))
        self.project = ProjectState(stem_separator=separator)
        self.project.sample_paths[0] = source_path
        self.project.sample_durations[0] = frames / rate
        self.project.pad_timing_intent[0] = "legacy"
        self.session = SessionState()
        executable = self.config.get("python_executable")
        assert isinstance(executable, str)
        assert executable.strip()
        python_executable = Path(executable)
        assert python_executable.is_absolute()
        assert python_executable.is_file()
        self.backend = RecordingBackend(python_executable)
        # The productive bounded pool runs the actual subprocess adapters.
        self.controller = StemController(
            self.project, self.session, self.audio, stem_backend=self.backend
        )

    def begin(self) -> str:
        # Inject only the explicit local model-cache locator, never model execution.
        with patch(
            "flitzis_looper.controller.stems.separator_model_cache_dir",
            return_value=self.model_cache,
        ):
            assert self.controller.generate_stems_async(0)
        deadline = time.monotonic() + 600
        while 0 not in self.controller._pending_stem_publications:
            self.controller.on_frame_render()
            assert not self.session.stem_generation_errors, self.session.stem_generation_errors
            assert time.monotonic() < deadline, "actual separator inference timed out"
            time.sleep(0.01)
        self.original_ticket = self.controller._pending_stem_publications[0].source_ticket
        assert self.original_ticket.publication_status() == "pending"
        assert not self.controller.stems_available(0)
        request, result = self.backend.request, self.backend.result
        assert request is not None
        assert result is not None
        assert request.separator == self.config["separator"]
        assert request.model_cache_dir == self.model_cache
        assert result.device == self.config["expected_device"]
        assert not result.cpu_fallback
        assert result.artifact_count == len(STEM_KINDS)
        assert not request.cache_dir.exists()
        entry = self.project.stem_cache[0]
        assert entry is not None
        assert not entry.available
        assert verified_stem_cache_available(entry)
        self.saved_path = entry.cache_dir
        return entry.source_version

    def accepted(self) -> None:
        assert self.original_ticket.publication_status() == "accepted"
        self.controller.on_frame_render()
        assert not self.session.stem_generation_errors, self.session.stem_generation_errors
        assert self.controller.stems_available(0)

    def restore(self) -> None:
        self.restored_project = ProjectState.model_validate_json(self.project.model_dump_json())
        self.restored_project.stem_separator = "bs-roformer:musdb18hq"
        self.restored_session = SessionState()
        self.restored = StemController(
            self.restored_project,
            self.restored_session,
            self.audio,
            stem_backend=ForbiddenBackend(),
        )
        missing = Path("uninstalled-selected-model")
        assert not missing.exists()
        with patch(
            "flitzis_looper.controller.stems.separator_model_cache_dir", return_value=missing
        ) as locator:
            self.restored.restore_stem_cache_from_project_state()
            assert not self.restored.stems_available(0)
            assert self.restored.publish_restored_stem_cache_if_available(0)
            locator.assert_not_called()
        self.restored_ticket = self.restored._pending_stem_publications[0].source_ticket
        assert self.restored_ticket is not self.original_ticket
        assert self.restored_ticket.publication_status() == "pending"

    def restored_accepted(self) -> None:
        assert self.restored_ticket.publication_status() == "accepted"
        self.restored.on_frame_render()
        assert self.restored.stems_available(0)
        entry = self.restored_project.stem_cache[0]
        assert entry is not None
        assert entry.available
        assert entry.cache_dir == self.saved_path
        assert verified_stem_cache_available(entry)

    def begin_stale_restore(self) -> None:
        self.restored.restore_stem_cache_from_project_state()
        assert self.restored.publish_restored_stem_cache_if_available(0)
        self.stale_ticket = self.restored._pending_stem_publications[0].source_ticket
        assert self.stale_ticket.publication_status() == "pending"

    def rejected(self) -> None:
        assert self.stale_ticket.publication_status() == "rejected"
        self.restored.on_frame_render()
        assert not self.restored.stems_available(0)
        assert (
            "native source/request/timing validation"
            in (self.restored_session.stem_generation_errors[0])
        )
        assert Path(self.saved_path).is_dir()

    def finish(self) -> None:
        result = self.backend.result
        assert result is not None
        report: dict[str, object] = {
            "config": self.config,
            "generation_result": asdict(result),
            "commands": self.backend.commands,
            "embedded_sys_executable": sys.executable,
            "subprocess_python_executable": str(self.backend.python_executable),
            "cache_dir": str(Path(self.saved_path).resolve()),
            "artifacts": {
                kind: hashlib.sha256(
                    (Path(self.saved_path) / f"{kind}.wav").read_bytes()
                ).hexdigest()
                for kind in STEM_KINDS
            },
            "source_version": self.backend.request.source_version if self.backend.request else None,
            "statuses": [
                self.original_ticket.publication_status(),
                self.restored_ticket.publication_status(),
                self.stale_ticket.publication_status(),
            ],
            "restored_without_inference": True,
        }
        self.output_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
        self.controller.shut_down()
        self.restored.shut_down()
        self.controller._assets.release_saved_assignments()
        self.restored._assets.release_saved_assignments()
