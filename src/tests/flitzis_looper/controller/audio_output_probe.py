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
from flitzis_looper.controller.stem_cache import (
    cache_dir_for_sample_id,
    expected_stem_files,
    source_version_for_sample_path,
)
from flitzis_looper.material_migration_model import MigrationArtifactEvidence
from flitzis_looper.models import BpmLockReference, PadContentIdentity, ProjectState, StemCacheEntry
from flitzis_looper.project_materials import original_asset
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

    def __init__(
        self,
        engine: AudioEngine,
        root: str,
        source_paths: list[str] | None = None,
        profile: str = "standard",
    ) -> None:
        self.root = Path(root)
        self._flat_startup = source_paths is None
        restore_bpm = profile == "saved-bpm"
        project = ProjectState(
            audio_output=AudioOutputOptions(
                backend="asio",
                device_id="private-asio-fixture",
                sample_rate_hz=48_000,
                buffer_frames=192 if restore_bpm else 96,
                left_channel=2,
                right_channel=5,
            ),
            input_mapping_enabled=False,
            key_lock=True,
            speed=1.0 if restore_bpm else 1.1,
            multi_loop=True,
            trigger_quantization_enabled=False,
        )
        if source_paths is None:
            # The original saved-project startup input: six ordinary flat files,
            # without a prior material import or pre-existing PCM cache.
            samples = self.root / "samples"
            samples.mkdir()
            source_paths = []
            for pad in range(PADS):
                path = samples / f"output-{pad}.wav"
                _wave(path, pad)
                source_paths.append(str(path))
        assert len(source_paths) == PADS
        for pad, reference in enumerate(source_paths):
            path = Path(reference)
            project.sample_paths[pad] = path.relative_to(self.root).as_posix()
            project.manual_bpm[pad] = 112.0 if restore_bpm and pad else 120.0
            project.pad_loop_start_s[pad] = 0.25
            project.pad_loop_end_s[pad] = 2.75
            project.pad_loop_auto[pad] = False
            project.pad_grid_offset_samples[pad] = 7 + pad
        if restore_bpm:
            self._save_bpm_profile(project)
        self.original_hashes = {
            Path(reference).relative_to(self.root).as_posix(): _sha(Path(reference))
            for reference in source_paths
        }
        self.config = self.root / "project.json"
        self.config.write_text(project.model_dump_json(), encoding="utf-8")
        self.initial_project_json = self.config.read_text(encoding="utf-8")
        with patch.object(app_module, "AudioEngine", return_value=engine):
            self.app: AppController = app_module.AppController(project_config_path=self.config)
        assert self.app.audio is engine
        assert type(engine) is AudioEngine
        self.owner_identity = id(engine)
        self.legacy_hashes: dict[str, str] = {}
        self.legacy_selection: dict[str, object] | None = None

    def _save_bpm_profile(self, project: ProjectState) -> None:
        """Start from saved settings, existing samples, and an unrelated selected PAD."""
        original = "1" * 32
        project.bpm_lock = True
        project.preload_stem_loops_on_startup = True
        project.bpm_lock_reference = BpmLockReference(pad_id=0, content_instance_id=original)
        project.selected_pad = PADS
        source = project.sample_paths[0]
        assert source is not None
        project.pad_content[0] = PadContentIdentity(
            instance_id=original, material_id=original_asset(source).material_id
        )
        version = source_version_for_sample_path(source)
        assert version is not None
        cache = cache_dir_for_sample_id(0, source)
        path = self.root / cache
        path.mkdir(parents=True, exist_ok=True)
        assert not any(path.iterdir())
        for name in ("vocals", "melody", "bass", "drums", "instrumental"):
            _wave(path / f"{name}.wav", 0, 0.75 if name == "instrumental" else 0.25)
        (path / ".complete.json").write_text(
            json.dumps({
                "schema": "stem-set-sha256-v1",
                "source_version": version,
                "stems": {file.stem: _sha(file) for file in path.glob("*.wav")},
            }),
            encoding="utf-8",
        )
        project.stem_cache[0] = StemCacheEntry(
            source_version=version, cache_dir=cache, stems=expected_stem_files(cache)
        )
        project.pad_stem_mix_mode[0] = "all_stems"
        project.pad_stem_intent[0] = PadStemIntent(
            enabled_mask=5, last_custom_mask=5, display_mode="custom"
        )

    def _bpm_restored(self) -> bool:
        status = self.app.transport.global_params._bpm_lock_application_status()
        return (
            status.effective is True
            and not status.pending
            and self.app.session.bpm_lock_anchor_pad_id == 0
        )

    def _change_saved_bpm_speed(self) -> None:
        assert self.app.project.selected_pad == PADS
        self.app.select_pad(1)
        parameters = self.app.transport.global_params
        parameters.set_multi_loop(enabled=False)
        parameters.set_multi_loop(enabled=True)
        parameters.set_speed(1.05)

    def _saved_bpm_speed_applied(self) -> bool:
        status = self.app.transport.global_params.global_rate_status()
        return (
            self._bpm_restored()
            and status.state == "applied"
            and status.effective_speed == 1.05
            and status.effective_master_period == 0.5 / 1.05
        )

    def _restart_group(self) -> None:
        assert self.app.session.global_stop_restore_sample_ids == set(range(PADS))
        self.app.transport.playback.start_or_restart_global_start_stop()

    def _toggle_bpm_lock(self) -> None:
        self.app.transport.global_params.set_bpm_lock(enabled=not self.app.project.bpm_lock)

    def _bpm_off(self) -> bool:
        status = self.app.transport.global_params._bpm_lock_application_status()
        return status.effective is False and not status.pending

    def _new_bpm_reference(self) -> bool:
        status = self.app.transport.global_params._bpm_lock_application_status()
        return (
            status.effective is True
            and not status.pending
            and self.app.session.bpm_lock_anchor_pad_id == 1
        )

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

    def output_change_succeeded(self) -> bool:
        """Distinguish a successful transaction from a ready rollback or abort."""
        return self.ready() and self.app.audio_output.error is None

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
            "bpm": self.app.transport.global_params._bpm_lock_application_status(),
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

    def install_assigned_pair(self) -> None:
        """Convert the existing five WAVs through the actual controller worker."""
        entry = self.app.project.stem_cache[0]
        assert entry is not None
        assert entry.pair is None
        path = self.root / entry.cache_dir
        marker = path / ".complete.json"
        assert not marker.exists()
        marker.write_text(
            json.dumps({
                "schema": "stem-set-sha256-v1",
                "source_version": entry.source_version,
                "stems": {
                    name: _sha(path / f"{name}.wav")
                    for name in ("vocals", "melody", "bass", "drums", "instrumental")
                },
            }),
            encoding="utf-8",
        )
        ticket = self.app.audio.capture_prepared_source(0, entry.source_version)
        assert self.app.stems._prepare_pair(0, entry, ticket)
        self.legacy_selection = None

    def assigned_pair_ready(self) -> bool:
        """Require the durable Pair and its own current component admission."""
        entry = self.app.project.stem_cache[0]
        return (
            entry is not None
            and entry.pair is not None
            and self.app.stems.stems_available(0)
            and self.app.stems.recovered_copy_selection_ready(0, entry)
            and self.configuration_idle()
        )

    def _save_assigned_pair_baseline(self) -> None:
        """Save the newly created initial Pair before exercising a saved selection."""
        assert self.assigned_pair_ready(), self.status()
        self.app.persistence.flush()
        self.verify_assigned_pair()

    def _arm_restore_scope_capture(self) -> None:
        """Retain a genuine token from the original scoped restoration callback."""
        original = self.app.audio_output_runtime.sources_restored
        patcher = patch.object(self.app.audio_output_runtime, "sources_restored")
        mocked = patcher.start()

        def capture_scoped() -> bool:
            self.restore_job_scope = self.app.audio.capture_audio_output_restore_job()
            assert self.restore_job_scope is not None
            patcher.stop()
            return original()

        mocked.side_effect = capture_scoped

    def _verify_stale_restore_scope(self) -> None:
        """An ended transaction must refuse its genuine token before the callback."""
        scope = self.restore_job_scope
        assert scope is not None
        called = []
        try:
            scope.run(lambda: called.append(True))
        except RuntimeError:
            pass
        else:
            message = "An ended output transaction admitted its stale worker scope"
            raise AssertionError(message)
        assert not called
        self.restore_job_scope = None
        assert self.app.audio.capture_audio_output_restore_job() is None

    def arm_pair_commit_failure(self) -> None:
        """Fail exactly after real new Pair disk/current admission, then permit rollback."""
        previous = self.verify_assigned_pair()
        original = self.app.audio_output_runtime.sources_restored
        patcher = patch.object(self.app.audio_output_runtime, "sources_restored")
        mocked = patcher.start()

        def after_actual_pair() -> bool:
            ready = original()
            entry = self.app.project.stem_cache[0]
            if ready and entry is not None and entry.pair is not None:
                assert entry.pair.stem_set_identity != previous
                saved = ProjectState.model_validate_json(self.config.read_bytes())
                assert saved.stem_cache[0] is not None
                assert saved.stem_cache[0].pair == entry.pair
                patcher.stop()
                self.late_pair_failure_seen = True
                message = "Injected failure after actual Pair selection commit"
                raise RuntimeError(message)
            return ready

        mocked.side_effect = after_actual_pair

    def verify_assigned_pair(self) -> str:
        """Bind current five-output target geometry to actual source and disk state."""
        assert self.assigned_pair_ready(), self.status()
        entry = self.app.project.stem_cache[0]
        assert entry is not None
        assert entry.pair is not None
        descriptor = json.loads((self.root / entry.pair.descriptor_reference).read_text())
        lease = self.app.audio.capture_migration_artifact(entry.pair.descriptor_reference)
        evidence = MigrationArtifactEvidence.model_validate_json(lease.receipt_json())
        encoded = evidence.model_dump_json()
        assert ("original_clock" in json.loads(encoded)) == (
            descriptor["content"]["conversion"]["schema_version"] == 2
        )
        lease.release()
        reopened = self.app.audio.reopen_migration_artifact(encoded)
        reopened.release()
        assert (
            descriptor["content"]["source"]["sample_rate_hz"] == self.app.audio.output_sample_rate()
        )
        saved = ProjectState.model_validate_json(self.config.read_bytes())
        assert saved.stem_cache[0] is not None
        assert saved.stem_cache[0].pair == entry.pair, {
            "saved": saved.stem_cache[0].pair,
            "current": entry.pair,
            "status": self.status(),
        }
        for artifact in descriptor["content"]["artifacts"]:
            path = self.root / entry.pair.pcm_generation / (artifact["name"] + ".f32le")
            assert path.stat().st_size == artifact["pcm_bytes"]
            assert _sha(path) == artifact["pcm_sha256"]
        return entry.pair.stem_set_identity

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

    def verify(self, rate: int, offsets: list[int]) -> dict[str, object] | None:
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
        if self._flat_startup:
            return _flat_startup_snapshot(self)
        return None

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


def _flat_startup_snapshot(probe: OutputProbe) -> dict[str, object]:
    """Validate only the original flat-startup layout after ordinary verification."""
    initial = json.loads(probe.initial_project_json)
    expected = [f"samples/output-{pad}.wav" for pad in range(PADS)]
    assert initial["sample_paths"][:PADS] == expected
    assert sorted(probe.original_hashes) == expected
    assert not probe.app.session.sample_load_errors, probe.status()
    assert not probe.app.session.stem_generation_errors, probe.status()
    assert not probe.app.session.loading_sample_ids, probe.status()
    assert probe.app.project.manual_bpm[:PADS] == [120.0] * PADS
    assert probe.app.project.pad_loop_auto[:PADS] == [False] * PADS
    assert probe.app.project.key_lock
    assert probe.app.project.multi_loop
    assert probe.app.project.speed == 1.1
    assert not probe.app.project.trigger_quantization_enabled
    assert probe.app.material_migration.status == "config_committed", probe.status()
    assert not probe.app.material_migration._startup_scheduled, probe.status()
    references = probe.app.project.sample_paths[:PADS]
    for pad, reference in enumerate(references):
        assert reference is not None
        assert reference != expected[pad]
        assert _sha(probe.root / reference) == probe.original_hashes[expected[pad]]
    saved = json.loads(probe.config.read_text(encoding="utf-8"))
    assert saved["sample_paths"][:PADS] == references
    return {
        "initial_project": initial,
        "flat_sha256": probe.original_hashes,
        "restored_references": references,
        "status": probe.status(),
    }
