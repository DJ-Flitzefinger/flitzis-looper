"""Actual stem worker/controller probe with a native producer replacing device startup."""
# Compiled into an isolated Rust test, never imported as a Python package.
# ruff: noqa: INP001

import struct
import wave
from pathlib import Path
from typing import TYPE_CHECKING, Protocol, cast

from flitzis_looper.controller.stem_cache import verified_stem_cache_available
from flitzis_looper.controller.stem_generation import StemGenerationResult
from flitzis_looper.controller.stems import StemController
from flitzis_looper.models import ProjectState, SessionState

if TYPE_CHECKING:
    from flitzis_looper.controller.stem_generation import (
        StemGenerationRequest,
        StemProgressCallback,
    )
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


class OfflineFixtureBackend:
    """Separation is synthetic; the production worker and publication are unchanged."""

    def __init__(self) -> None:
        self.private_path: Path | None = None

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        self.private_path = request.cache_dir
        request.cache_dir.mkdir(parents=True)
        progress(0.9, "Writing fixture stems")
        shape = request.target_shape
        for name, integer in (
            ("vocals", 2048),
            ("melody", 3072),
            ("bass", 4096),
            ("drums", 5120),
            ("instrumental", 12288),
        ):
            with wave.open(str(request.cache_dir / f"{name}.wav"), "wb") as artifact:
                artifact.setnchannels(shape.channels)
                artifact.setsampwidth(2)
                artifact.setframerate(shape.sample_rate_hz)
                artifact.writeframes(
                    struct.pack("<h", integer) * shape.frame_count * shape.channels
                )
        return StemGenerationResult(
            backend_name="synthetic-fixture",
            model_name="no-model",
            device="cpu",
            cpu_fallback=False,
            artifact_count=5,
        )


class Probe:
    def __init__(
        self, bridge: NativeProducerBridge, source_path: str, frames: int, rate: int
    ) -> None:
        self.audio = cast("AudioEngine", Audio(bridge))
        self.project = ProjectState()
        self.project.sample_paths[0] = source_path
        self.project.sample_durations[0] = frames / rate
        self.project.pad_timing_intent[0] = "legacy"
        self.session = SessionState()
        self.backend = OfflineFixtureBackend()
        self.controller = StemController(
            self.project,
            self.session,
            self.audio,
            stem_backend=self.backend,
            stem_task_runner=lambda work: work(),
        )

    def begin(self) -> str:
        assert self.controller.generate_stems_async(0)
        self.controller.on_frame_render()
        assert not self.session.stem_generation_errors, self.session.stem_generation_errors
        self.original_ticket = self.controller._pending_stem_publications[0].source_ticket
        assert self.original_ticket.publication_status() == "pending"
        entry = self.project.stem_cache[0]
        assert entry is not None
        assert not entry.available
        assert not self.controller.stems_available(0)
        assert self.backend.private_path is not None
        assert not self.backend.private_path.exists()
        assert Path(entry.cache_dir).parent == Path("samples/stems/#1")
        assert Path(entry.cache_dir).name.startswith(".ready-")
        assert verified_stem_cache_available(entry)
        self.saved_path = entry.cache_dir
        return entry.source_version

    def accepted(self) -> None:
        assert self.original_ticket.publication_status() == "accepted"
        self.controller.on_frame_render()
        assert not self.session.stem_generation_errors, self.session.stem_generation_errors
        assert self.controller.stems_available(0)
        assert self.controller.stem_grid_indicator_state(0) == "available"
        entry = self.project.stem_cache[0]
        assert entry is not None
        assert entry.available
        assert entry.cache_dir == self.saved_path
        assert verified_stem_cache_available(entry)

    def restore(self) -> None:
        self.restored_project = ProjectState.model_validate_json(self.project.model_dump_json())
        self.restored_session = SessionState()
        self.restored = StemController(self.restored_project, self.restored_session, self.audio)
        self.restored.restore_stem_cache_from_project_state()
        assert not self.restored.stems_available(0)
        assert self.restored.publish_restored_stem_cache_if_available(0)
        self.restored_ticket = self.restored._pending_stem_publications[0].source_ticket
        assert self.restored_ticket is not self.original_ticket
        assert self.restored_ticket.publication_status() == "pending"
        assert self.original_ticket.publication_status() == "accepted"

    def restored_accepted(self) -> None:
        assert self.restored_ticket.publication_status() == "accepted"
        self.restored.on_frame_render()
        assert self.restored.stems_available(0)
        entry = self.restored_project.stem_cache[0]
        assert entry is not None
        assert entry.cache_dir == self.saved_path
        assert entry.available
        assert verified_stem_cache_available(entry)

    def reject_tampered_restore(self) -> None:
        artifact = Path(self.saved_path) / "vocals.wav"
        original = artifact.read_bytes()
        corrupted = bytearray(original)
        corrupted[-1] ^= 1
        artifact.write_bytes(corrupted)
        try:
            self.restored.restore_stem_cache_from_project_state()
            assert not self.restored.stems_available(0)
            assert self.restored.publish_restored_stem_cache_if_available(0)
            assert not self.restored._pending_stem_publications
            assert self.restored_ticket.publication_status() == "accepted"
            assert artifact.is_file()
        finally:
            artifact.write_bytes(original)
        entry = self.restored_project.stem_cache[0]
        assert entry is not None
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
            in self.restored_session.stem_generation_errors[0]
        )
        assert Path(self.saved_path).is_dir()
        self.controller.shut_down()
        self.restored.shut_down()
        self.controller._assets.release_saved_assignments()
        self.restored._assets.release_saved_assignments()
