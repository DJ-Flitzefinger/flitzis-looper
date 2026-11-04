"""Actual App/Loader output restoration, driven by one private native ASIO owner."""

import hashlib
import json
import math
import struct
import wave
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch

from flitzis_looper.audio_output_options import AudioOutputOptions
from flitzis_looper.controller import app as app_module
from flitzis_looper.controller.audio_output_restore import require_frontend_idle
from flitzis_looper.controller.stem_cache import cache_dir_for_sample_id, expected_stem_files
from flitzis_looper.models import ProjectState, StemCacheEntry
from flitzis_looper.stem_intent import PadStemIntent
from flitzis_looper_audio import AudioEngine

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController

PADS = 6
SOURCE_RATE = 48_000


def _sha(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def _wave(path: Path, pad: int, gain: float = 1.0) -> None:
    pcm = bytearray()
    for frame in range(3 * SOURCE_RATE):
        for channel in range(2):
            frequency = 137 + 43 * pad + 29 * channel
            sample = int(2400 * math.sin(2 * math.pi * frequency * frame / SOURCE_RATE))
            pcm.extend(struct.pack("<h", round(gain * sample)))
    with wave.open(str(path), "wb") as output:
        output.setnchannels(2)
        output.setsampwidth(2)
        output.setframerate(SOURCE_RATE)
        output.writeframes(pcm)


def write_sources(root: str) -> list[str]:
    """Write complete external inputs for the original native material importer."""
    inputs = Path(root) / "inputs"
    inputs.mkdir()
    paths = []
    for pad in range(PADS):
        path = inputs / f"output-{pad}.wav"
        _wave(path, pad)
        paths.append(str(path))
    return paths


class OutputProbe:
    """Only the Engine constructor binding is replaced; all operations remain real."""

    def __init__(self, engine: AudioEngine, root: str, source_paths: list[str]) -> None:
        self.root = Path(root)
        project = ProjectState(
            audio_output=AudioOutputOptions(
                backend="asio",
                device_id="private-asio-fixture",
                sample_rate_hz=48_000,
                buffer_frames=96,
                left_channel=2,
                right_channel=5,
            ),
            input_mapping_enabled=False,
            key_lock=True,
            speed=1.1,
            multi_loop=True,
            trigger_quantization_enabled=False,
        )
        assert len(source_paths) == PADS
        for pad, reference in enumerate(source_paths):
            path = Path(reference)
            project.sample_paths[pad] = path.relative_to(self.root).as_posix()
            project.manual_bpm[pad] = 120.0
            project.pad_loop_start_s[pad] = 0.25
            project.pad_loop_end_s[pad] = 2.75
            project.pad_loop_auto[pad] = False
            project.pad_grid_offset_samples[pad] = 7 + pad
        self.original_hashes = {
            Path(reference).relative_to(self.root).as_posix(): _sha(Path(reference))
            for reference in source_paths
        }
        self.config = self.root / "project.json"
        self.config.write_text(project.model_dump_json(), encoding="utf-8")
        with patch.object(app_module, "AudioEngine", return_value=engine):
            self.app: AppController = app_module.AppController(project_config_path=self.config)
        assert self.app.audio is engine
        assert type(engine) is AudioEngine
        self.owner_identity = id(engine)
        self.legacy_hashes: dict[str, str] = {}
        self.legacy_selection: dict[str, object] | None = None

    def poll(self) -> None:
        """Use the ordinary UI/control event paths, with no fabricated source receipt."""
        self.app.on_frame_render()
        self.app.poll_runtime_events()
        assert not self.app.stems._jobs
        assert not self.app.stems._inference_jobs
        assert id(self.app.audio) == self.owner_identity

    def tick_allowed(self) -> bool:
        """Restoring phases still need callbacks; exclusive workers retain their borrow."""
        control = self.app.audio_output
        return (
            control._operation is None
            and control.available
            and not self.app.audio.audio_stream_failed()
        )

    def ready(self) -> bool:
        """Read actual controller completion, rather than deriving it from Tick success."""
        return self.app.audio_output.can_perform

    def status(self) -> str:
        """Keep a finite original failure report for an unsuccessful profile."""
        control = self.app.audio_output
        group = self.app.transport.playback._global_playback
        return repr({
            "phase": control.phase,
            "error": control.error,
            "available": control.available,
            "busy": control.busy,
            "epoch": control._epoch,
            "operation": control._operation.kind if control._operation is not None else None,
            "active": sorted(self.app.session.active_sample_ids),
            "paused": sorted(self.app.session.paused_sample_ids),
            "global_stop_engaged": self.app.session.global_stop_engaged,
            "global_restore": sorted(self.app.session.global_stop_restore_sample_ids),
            "global_pending": (
                group._pending.ticket.publication_status() if group._pending is not None else None
            ),
            "global_deferred": (
                group._deferred.capture.state() if group._deferred is not None else None
            ),
            "global_deferred_reason": (
                group._deferred.capture.reason() if group._deferred is not None else None
            ),
            "global_fenced": len(group._fenced_batches),
            "residency_pending": sorted(self.app.transport.residency._pending),
            "captured_starts": sorted(self.app.transport.residency._captured_starts),
            "load_errors": self.app.session.sample_load_errors,
            "stem_errors": self.app.session.stem_generation_errors,
            "loading": sorted(self.app.session.loading_sample_ids),
            "paths": self.app.project.sample_paths[:PADS],
            "requests": dict(self.app.loader._load_request_ids),
            "load_retirements": sorted(self.app.loader._load_retirements),
            "creation_pending": sorted(self.app.loader._material_creation_pending),
            "migration_status": self.app.material_migration.status,
            "migration_preparation": self.app.material_migration._preparation is not None,
            "migration_artifact": self.app.material_migration._artifact_future is not None,
        })

    def install_legacy_intent(self) -> None:
        """Supply a historical five-WAV input, then use actual Apply to install it."""
        assert self.ready()
        version = self.app.stems.source_version_for_pad(0)
        assert version is not None
        cache = cache_dir_for_sample_id(0, self.app.project.sample_paths[0])
        path = self.root / cache
        path.mkdir(parents=True, exist_ok=True)
        assert not any(path.iterdir()), "legacy fixture must not overwrite existing cache files"
        for name in ("vocals", "melody", "bass", "drums", "instrumental"):
            _wave(path / f"{name}.wav", 0, 0.75 if name == "instrumental" else 0.25)
        self.legacy_hashes = {
            str(file.relative_to(self.root)): _sha(file) for file in path.iterdir()
        }
        entry = StemCacheEntry(
            source_version=version, cache_dir=cache, stems=expected_stem_files(cache)
        )
        self.app.project.stem_cache[0] = entry
        self.app.project.pad_stem_mix_mode[0] = "all_stems"
        self.app.project.pad_stem_intent[0] = PadStemIntent(
            enabled_mask=5,
            last_custom_mask=5,
            display_mode="custom",
        )
        self.legacy_selection = entry.model_dump(exclude={"available"})
        self.app._assets.sync_assignments()
        self.app.persistence.mark_dirty()
        self.app.persistence.flush()
        assert self.app.audio_output.apply(self.app.project.audio_output)

    def apply_rate(self, rate: int) -> None:
        """Request one real stopped transaction through the product controller."""
        assert self.ready()
        options = self.app.project.audio_output.model_copy(update={"sample_rate_hz": rate})
        assert self.app.audio_output.apply(options), self.status()

    def make_grid_offset_odd(self) -> None:
        """Distinguish exact original96k rollback from rounded48k roundtripping."""
        self.app.transport.loop.set_grid_offset_samples(0, 15)
        self.app.persistence.flush()

    def apply_auto(self) -> None:
        """Auto must observe the same virtual device's surviving committed96k rate."""
        options = self.app.project.audio_output.model_copy(
            update={"sample_rate_hz": None, "buffer_frames": None}
        )
        assert self.app.audio_output.apply(options), self.status()

    def configuration_idle(self) -> bool:
        """Wait for the original edit's actual frontend and Native collectors."""
        try:
            require_frontend_idle(self.app)
            pool = self.app.stems._stem_worker_pool
            assert pool is not None
            pool.with_idle(self.app.audio.ensure_audio_output_startup_sources_settled)
        except RuntimeError:
            return False
        return True

    def verify(self, rate: int, offsets: list[int]) -> None:
        """Validate current persisted intent, exact old files, and stopped output state."""
        assert self.ready(), self.status()
        actual_rate = self.app.audio.output_sample_rate()
        assert actual_rate == rate, f"Expected {rate} Hz, actual {actual_rate} Hz; {self.status()}"
        assert self.app.project.pad_grid_offset_samples[:PADS] == offsets
        assert self.app.project.pad_loop_start_s[:PADS] == [0.25] * PADS
        assert self.app.project.pad_loop_end_s[:PADS] == [2.75] * PADS
        assert not self.app.session.active_sample_ids
        assert not self.app.session.paused_sample_ids
        for name, digest in self.original_hashes.items():
            with (self.root / name).open("rb") as source:
                assert hashlib.file_digest(source, "sha256").hexdigest() == digest
        for name, digest in self.legacy_hashes.items():
            with (self.root / name).open("rb") as source:
                assert hashlib.file_digest(source, "sha256").hexdigest() == digest
        if self.legacy_selection is not None:
            entry = self.app.project.stem_cache[0]
            assert entry is not None
            assert entry.pair is None
            assert entry.available
            assert entry.model_dump(exclude={"available"}) == self.legacy_selection
            assert len(list((self.root / entry.cache_dir).iterdir())) == 5
        saved = json.loads(self.config.read_text(encoding="utf-8"))
        assert saved["audio_output"] == self.app.project.audio_output.model_dump(mode="json")

    def start(self, cohort: str) -> None:
        """Start all six through the same public, source-bound PAD launch path."""
        assert self.ready()
        if cohort not in {"wet", "off"}:
            message = "Unknown software profile cohort"
            raise ValueError(message)
        self.app.transport.global_params.set_key_lock(enabled=cohort == "wet")
        for pad in range(PADS):
            self.app.transport.playback.trigger_pad_keep_others(pad)

    def stop(self) -> None:
        """Observe the public group's real stop receipt before another operation."""
        self.app.transport.playback.stop_global_start_stop()

    def stopped(self) -> bool:
        """The group and original admitted starts must both be settled."""
        return (
            not self.app.session.active_sample_ids
            and not self.app.transport.residency._captured_starts
        )

    def close(self) -> bool:
        """Use normal App shutdown and its actual original-owner joins."""
        if not self.app.prepare_shutdown():
            return False
        self.app.shut_down()
        pool = self.app.stems._stem_worker_pool
        if pool is not None:
            pool._executor.shutdown(wait=True, cancel_futures=True)
        return True
