"""Real 216-source batch/pool/Config/Native composition with declared nonneural output.

Rust supplies actual canonical source adoptions and alone consumes callback commands.
Only the headless warning envelope, streamless shape accessor and inference output
are fixtures. This is no neural, child-process, RSS, voice-count or musical proof.
"""

import hashlib
import io
import json
import threading
import wave
from array import array
from contextlib import ExitStack
from functools import partial
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import patch

from flitzis_looper.controller import stem_pair_preparation as pair_module
from flitzis_looper.controller import stems as stems_module
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.committed_stem_residency import resident_stem_admission_pressure
from flitzis_looper.controller.performance_confirmation import (
    PerformanceConfirmationController,
    StemBatchConfirmation,
)
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_batch import StemBatchController
from flitzis_looper.controller.stem_generation import StemGenerationResult
from flitzis_looper.controller.stem_job import DIRECT_STEM_INTEREST
from flitzis_looper.controller.stem_workers import STEM_QUEUED_JOBS, STEM_WORKERS
from flitzis_looper.controller.stems import StemController
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from flitzis_looper_audio import SealedProcessingFiles

if TYPE_CHECKING:
    from collections.abc import Callable
    from concurrent.futures import Future

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_admission import (
        StemAdmissionCandidate,
        StemAdmissionResult,
    )
    from flitzis_looper.controller.stem_generation import StemGenerationRequest
    from flitzis_looper.controller.stem_job import StemGenerationInterests
    from flitzis_looper.controller.stem_workers import StemWorkerReservation
    from flitzis_looper.controller.stems import _StemAdmissionOwner
    from flitzis_looper_audio import AudioEngine


class _ShapeAudio:
    """Keep every authority on one Native; only its streamless shape query differs."""

    def __init__(self, audio: AudioEngine, shape_audio: object) -> None:
        self._audio = audio
        self._shape_audio = shape_audio

    def __getattr__(self, name: str) -> object:
        owner = self._shape_audio if name == "loaded_sample_shape" else self._audio
        return getattr(owner, name)


def _wav(samples: array[int], rate: int) -> bytes:
    output = io.BytesIO()
    with wave.open(output, "wb") as writer:
        writer.setparams((2, 2, rate, len(samples) // 2, "NONE", "not compressed"))
        writer.writeframes(samples.tobytes())
    return output.getvalue()


class _DeclaredBackend:
    """Write independently source-bound complete WAVs on the actual stems workers."""

    def __init__(self, expected: set[str]) -> None:
        self.expected = expected
        self.requests: list[StemGenerationRequest] = []
        self.digests: list[str] = []
        self.attempts: list[dict[str, object]] = []
        self.failures: list[dict[str, object]] = []
        self.trace: Callable[[StemGenerationRequest], dict[str, object]] | None = None
        self._lock = threading.Lock()

    def generate(
        self, request: StemGenerationRequest, progress: Callable[[float, str], None]
    ) -> StemGenerationResult:
        """Preserve the fast successful path; diagnose only an actual failed attempt."""
        try:
            return self._generate(request, progress)
        except BaseException as error:
            detail: dict[str, object] = {"type": type(error).__name__, "error": repr(error)}
            try:
                if self.trace is not None:
                    detail.update(self.trace(request))
            except (
                AttributeError,
                KeyError,
                OSError,
                RuntimeError,
                TypeError,
                ValueError,
            ) as observation_error:
                detail["observation_error"] = repr(observation_error)
            with self._lock:
                self.failures.append({"pad_id": request.sample_id, **detail})
            raise

    def _generate(
        self, request: StemGenerationRequest, progress: Callable[[float, str], None]
    ) -> StemGenerationResult:
        """Substitute inference only; preserve actual source reads and verification."""
        assert threading.current_thread().name.startswith("stems_")
        encoded = request.source_path.read_bytes()
        digest = hashlib.sha256(encoded).hexdigest()
        assert digest in self.expected
        with self._lock:
            self.attempts.append({"pad_id": request.sample_id, "digest": digest})
            assert digest not in self.digests, "one material inferred twice"
            self.requests.append(request)
            self.digests.append(digest)
        with wave.open(io.BytesIO(encoded), "rb") as reader:
            assert (reader.getnchannels(), reader.getsampwidth()) == (2, 2)
            rate, frames = reader.getframerate(), reader.getnframes()
            original = array("h", reader.readframes(frames))
        assert (rate, 2, frames) == (
            request.target_shape.sample_rate_hz,
            request.target_shape.channels,
            request.target_shape.frame_count,
        )
        quarter = array("h", (int(value / 4) for value in original))
        drums = array(
            "h", (value - 3 * part for value, part in zip(original, quarter, strict=True))
        )
        instrumental = array(
            "h", (value - part for value, part in zip(original, quarter, strict=True))
        )
        request.cache_dir.mkdir(parents=True)
        progress(0.5, "Declared nonneural 216-item composition output")
        for name, samples in (
            ("vocals", quarter),
            ("melody", quarter),
            ("bass", quarter),
            ("drums", drums),
            ("instrumental", instrumental),
        ):
            (request.cache_dir / f"{name}.wav").write_bytes(_wav(samples, rate))
        assert request.source_path.read_bytes() == encoded
        return StemGenerationResult(
            backend_name="demucs",
            model_name="htdemucs",
            device="cpu",
            cpu_fallback=False,
            artifact_count=5,
            encoder_verified=True,
        )


class _PoolObservation:
    """Observe the same executor and original arbiter without replacing either."""

    def __init__(self, stems: StemController, stack: ExitStack) -> None:
        pool = stems._stem_worker_pool
        assert pool is not None
        assert pool._executor._max_workers == STEM_WORKERS == 2
        self.stems = stems
        self.running = self.queued = self.submitted = 0
        self.peak_running = self.peak_queued = self.peak_submitted = 0
        self.peak_jobs = self.peak_preparations = self.peak_requested = 0
        self.grants: list[tuple[str, str]] = []
        self._lock = threading.Lock()
        self._submit = pool._executor.submit
        self._admit = stems._admit_stem_candidate
        stack.enter_context(patch.object(pool._executor, "submit", self.submit))
        stack.enter_context(patch.object(stems, "_admit_stem_candidate", self.admit))

    def submit(
        self, target: Callable[[Callable[[], None]], None], work: Callable[[], None]
    ) -> Future[None]:
        """Count actual executor custody from accepted submission through return."""
        with self._lock:
            self.queued += 1
            self.submitted += 1
            self.peak_queued = max(self.peak_queued, self.queued)
            self.peak_submitted = max(self.peak_submitted, self.submitted)
            assert self.submitted <= STEM_WORKERS + STEM_QUEUED_JOBS
        # Preserve the original reservation's _run/release wrapper. Observe
        # actual work inside it so a returned permit cannot temporarily make
        # observation count an already completed target plus its successor.
        return self._submit(target, partial(self._execute, work))

    def _execute(self, target: Callable[[], None]) -> None:
        with self._lock:
            self.queued -= 1
            self.running += 1
            self.peak_running = max(self.peak_running, self.running)
            assert self.running <= STEM_WORKERS
        try:
            target()
        finally:
            with self._lock:
                self.running -= 1
                self.submitted -= 1

    def admit(
        self,
        candidate: StemAdmissionCandidate[_StemAdmissionOwner],
        reservation: StemWorkerReservation,
    ) -> StemAdmissionResult:
        """Observe real successful grants and retained ownership at each admission."""
        result = self._admit(candidate, reservation)
        if result == "submitted":
            self.grants.append(candidate.lane)
        self.observe()
        return result

    def observe(self) -> None:
        """Bound actual controller metadata, including returned preparation custody."""
        self.peak_jobs = max(self.peak_jobs, len(self.stems._jobs))
        self.peak_preparations = max(self.peak_preparations, self.stems._stem_preparation_count())
        self.peak_requested = max(self.peak_requested, len(self.stems._requested_generations))
        assert self.peak_jobs <= 34
        assert self.peak_preparations <= 34
        assert self.peak_requested <= 216


class StemBatch216Probe:
    """Compose original warning/batch/publication owners around genuine 216 sources."""

    def __init__(self, audio: AudioEngine, shape_audio: object, input_json: str) -> None:
        rows = json.loads(input_json)
        assert len(rows) == 216
        self.audio = cast("AudioEngine", _ShapeAudio(audio, shape_audio))
        self.project = ProjectState(
            key_lock=False, resident_pcm_budget_mib=128, selected_pad=215, selected_bank=5
        )
        for slot, row in enumerate(rows):
            self.project.sample_paths[slot] = row["new_reference"]
            self.project.sample_durations[slot] = 4096 / 48_000
            self.project.pad_content[slot] = PadContentIdentity(
                instance_id=f"{slot + 1:032x}",
                material_id=row["material_id"],
                material_creation=row["material_creation"],
            )
            self.project.pad_key_lock[slot] = False
        self._original_contents = tuple(self.project.pad_content)
        self._original_paths = tuple(self.project.sample_paths)
        self._expected = {row["original"]["sha256"] for row in rows}
        self.backend = _DeclaredBackend(self._expected)
        self.session = SessionState(settings_open=False)
        self.persistence = ProjectPersistence(self.project)
        self.persistence.config_path = Path("samples/batch-216.json")
        self.persistence.bind_audio(self.audio)
        self.assets = ProjectAssetLifecycle(self.project, self.audio)
        self.assets.sync_assignments()
        self.persistence.mark_dirty()
        self.persistence.flush()
        self.stems = StemController(
            self.project,
            self.session,
            self.audio,
            on_project_changed=self.persistence.mark_dirty,
            stem_backend=self.backend,
            asset_lifecycle=self.assets,
        )
        self.backend.trace = self._inference_trace
        self.stems.bind_disk_selection_commit(
            self.persistence.commit_stem_selection,
            self.persistence.flush,
            writer_blocked=self.persistence.stem_selection_write_blocked,
        )
        self.batch = StemBatchController(self.stems)
        # No arrangement is active or exercised. This envelope supplies only the
        # warning's App references; no Native acceptance or worker is synthesized.
        idle = SimpleNamespace(active=False)
        envelope = SimpleNamespace(
            project=self.project,
            stems=self.stems,
            stem_batch=self.batch,
            stem_delete=idle,
            _close_requested=False,
            banks=idle,
            pairs=idle,
            arrangement=idle,
        )
        self.warning = PerformanceConfirmationController(cast("AppController", envelope))
        self._patches = ExitStack()
        self._release = threading.Event()
        self._producer_files = self._install_producer()
        self.observation = _PoolObservation(self.stems, self._patches)
        self._direct: list[StemGenerationInterests] = []
        self._all_cursor = 0
        self._before = self.persistence.config_path.read_bytes()

    def _inference_trace(self, request: StemGenerationRequest) -> dict[str, object]:
        """Observe original attempt identity and existing custody without changing admission."""
        job = next(
            (job for job in tuple(self.stems._jobs.values()) if job.resolved_request is request),
            None,
        )
        fingerprint = job.generation_fingerprint if job is not None else None
        return {
            "fingerprint": fingerprint,
            "producer": job.producer_identity if job is not None else None,
            "saved_matching": sum(
                entry is not None and entry.generation_fingerprint == fingerprint
                for entry in tuple(self.project.stem_cache)
            ),
            "pending_pair_matching": sum(
                row.entry.generation_fingerprint == fingerprint
                for row in tuple(self.stems._pair_preparations.pending.values())
            ),
            "settlement_matching": sum(
                row.entry.generation_fingerprint == fingerprint
                for row in tuple(self.stems._disk_selection_settlements.values())
            ),
        }

    def _install_producer(self) -> tuple[Path, ...]:
        directory = Path.cwd() / "declared-batch-216-producer"
        directory.mkdir()
        files = tuple(directory / name for name in ("weights.bin", "definition.txt", "encoder.bin"))
        for index, path in enumerate(files):
            path.write_bytes(f"Explicit nonneural 216-PAD producer {index}".encode())

        def capture(request: StemGenerationRequest) -> tuple[SealedProcessingFiles, str]:
            assert request.model_cache_dir == directory
            guard = SealedProcessingFiles([str(path) for path in files])
            producer = hashlib.sha256(json.dumps(guard.witnesses()).encode("utf-8")).hexdigest()
            assert self._release.wait(60), "Root never released the actual full-pool checkpoint"
            return guard, producer

        replacements = {
            "separator_model_cache_dir": lambda _separator: directory,
            "resolved_demucs_encoder": lambda _model: files[-1],
            "encoder_contract": lambda _encoder: "declared-batch-216-output-receipt",
            "capture_processing_files": capture,
            "processing_source_roots": lambda _request, _guard: (directory,),
        }
        for name, replacement in replacements.items():
            self._patches.enter_context(patch.object(stems_module, name, replacement))
        # Reuse workers freshly validate the same declared producer as resolution.
        # Their imported alias is independent of the controller's resolve alias.
        self._patches.enter_context(patch.object(pair_module, "capture_processing_files", capture))
        return files

    def start(self) -> None:
        """Accept the exact public warning without changing its original PAD targets."""
        self.warning.request_all_loaded_stems()
        warning = self.warning.pending
        assert isinstance(warning, StemBatchConfirmation)
        assert len(warning.inventory.items) == 216
        assert all(
            item is not None and item.unavailable_reason is None for item in warning.inventory.items
        )
        assert not self.stems._jobs
        assert not self.stems._requested_generations
        assert self.assets._reserved == 0
        assert self.persistence.config_path.read_bytes() == self._before
        self.warning.accept(warning)
        self.warning.accept(warning)
        assert self.warning.pending is None

    def poll(self) -> None:
        """Advance real controllers while Rust separately drains actual callback work."""
        before = len(self.observation.grants)
        self.stems.on_frame_render()
        assert len(self.observation.grants) - before <= 6
        self.batch.poll()
        self.assets.retry_retirements()
        self.observation.observe()
        for slot, error in tuple(self.session.stem_generation_errors.items()):
            demand = self.stems._demands.get(slot)
            assert self._all_cursor > 0, self.diagnostics()
            assert resident_stem_admission_pressure(error), self.diagnostics()
            assert demand is not None, self.diagnostics()
            assert demand.explicit, self.diagnostics()
            assert demand.state in {"queued", "deferred", "preparing"}, self.diagnostics()
        assert (self.project.selected_pad, self.project.selected_bank) == (215, 5)
        assert not self.session.settings_open
        assert tuple(self.project.pad_content) == self._original_contents
        assert tuple(self.project.sample_paths) == self._original_paths

    def full_pool_ready(self) -> bool:
        """Keep two real resolve readers blocked with exactly32 queued and182 pending."""
        summary = self.batch.summary
        if summary is None or any(row.owner is None for row in self.batch._rows or ()):
            return False
        if self.observation.running != 2 or self.observation.queued != 32:
            return False
        assert summary.total == summary.queued + summary.running == 216
        assert len(self.stems._jobs) == 34
        assert len(self.stems._requested_generations) == 182
        assert not self.backend.requests
        assert self.persistence.config_path.read_bytes() == self._before
        if not self._direct:
            # Both running originals gain independent interests; whichever equal
            # material resolves first must retain a DIRECT inference path. The
            # final pending original also competes through the direct resolve lane.
            for slot in (0, 1, 215):
                owner = self.stems.request_generation_interest(slot, DIRECT_STEM_INTEREST)
                assert owner is not None
                assert DIRECT_STEM_INTEREST in owner.current
                self._direct.append(owner)
        return True

    def release(self) -> None:
        """Release the declared producer checkpoint without manufacturing outcomes."""
        self._release.set()

    def complete(self) -> bool:
        """Require every original durable result and each own accepted registration."""
        summary = self.batch.summary
        if summary is None or summary.active:
            return False
        assert summary.done == summary.total == 216, self.diagnostics()
        assert summary.skipped == summary.failed == summary.cancelled == 0
        assert [item.pad_id for item in summary.items] == list(range(216))
        assert len(self.backend.requests) == len(self._expected)
        assert len(self._direct) == 3
        assert all(owner.committed for owner in self._direct)
        if self.stems._jobs or self.stems._stem_preparation_count():
            return False
        selections = self.stems._committed_residency._selections
        if len(selections) != 216:
            return False
        for slot in range(216):
            selection = selections[slot]
            if selection.ticket.publication_status() != "accepted":
                return False
            assert selection.ticket.is_current()
            content = self._original_contents[slot]
            assert content is not None
            assert selection.content_id == content.instance_id
            assert selection.source_path == self._original_paths[slot]
            assert selection.entry == self.project.stem_cache[slot]
        return True

    def adopt_all(self) -> None:
        """Request at most eight stopped-PAD ALL residencies per host poll."""
        end = min(self._all_cursor + 8, 216)
        for slot in range(self._all_cursor, end):
            assert self.stems.set_stem_mix_mode(slot, "all_stems"), self.diagnostics()
        self._all_cursor = end

    def all_ready(self) -> bool:
        """Every PAD must retain its own current registration and component ACK."""
        if self._all_cursor != 216:
            return False
        selections = self.stems._committed_residency._selections
        assert len(selections) == 216
        for slot, selection in selections.items():
            assert selection.ticket.is_current()
            assert selection.ticket.publication_status() == "accepted"
            receipt = selection.receipt
            if receipt is None or receipt.residency_status() != "accepted":
                return False
            assert receipt.is_current()
            assert selection.receipt_intent == (
                "all_stems",
                self.project.pad_stem_intent[slot].enabled_mask,
            )
        budget = self.audio.get_resident_pcm_budget_status()
        assert budget["budget_mib"] == 128
        assert budget["over_budget"] is False
        return budget["pending_reservation_count"] == budget["pending_resident_bytes"] == 0

    def finish(self) -> str:
        """Reopen actual Config and report bounded software observations truthfully."""
        assert self.complete()
        assert self.all_ready()
        assert not self.session.stem_generation_errors, self.diagnostics()
        self.persistence.flush()
        saved = ProjectPersistence.from_config_path(self.persistence.config_path)
        assert saved.load_error is None
        for slot in range(216):
            entry, reopened = self.project.stem_cache[slot], saved.project.stem_cache[slot]
            assert entry is not None
            assert entry.pair is not None
            assert reopened is not None
            assert reopened.pair == entry.pair
            assert saved.project.pad_content[slot] == self._original_contents[slot]
            assert saved.project.sample_paths[slot] == self._original_paths[slot]
            assert saved.project.pad_stem_mix_mode[slot] == "all_stems"
        assert (saved.project.selected_pad, saved.project.selected_bank) == (215, 5)
        assert len({entry.cache_dir for entry in self.project.stem_cache if entry}) == len(
            self._expected
        )
        assert all(
            ("direct", phase) in self.observation.grants
            for phase in ("resolve", "infer", "prepare")
        )
        assert self.observation.peak_running == 2
        assert self.observation.peak_submitted == 34
        assert self.observation.peak_queued <= 32
        return self.diagnostics()

    def diagnostics(self) -> str:
        """Return finite actionable diagnostics without a recursive owner dump."""
        summary = self.batch.summary
        observed = self.observation
        return json.dumps({
            "scope": (
                "declared nonneural actual216 software composition; no process/RSS/music claim"
            ),
            "inferences": len(self.backend.requests),
            "attempts": len(self.backend.attempts),
            "first_attempts": self.backend.attempts[:4],
            "failed_attempts": self.backend.failures[:4],
            "jobs": len(self.stems._jobs),
            "preparations": self.stems._stem_preparation_count(),
            "requested": len(self.stems._requested_generations),
            "done": summary.done if summary else None,
            "failed": summary.failed if summary else None,
            "errors": self.session.stem_generation_errors,
            "peaks": {
                "workers": observed.peak_running,
                "queued": observed.peak_queued,
                "admitted": observed.peak_submitted,
                "jobs": observed.peak_jobs,
                "preparations": observed.peak_preparations,
                "requested": observed.peak_requested,
            },
            "grant_lanes": sorted(set(observed.grants)),
        })

    def progress_marker(self) -> tuple[int, int, int, int, int]:
        """Monotone semantic progress for the finite engineering host watchdog."""
        summary = self.batch.summary
        selections = tuple(self.stems._committed_residency._selections.values())
        return (
            len(self.observation.grants),
            len(self.backend.requests),
            summary.done + summary.skipped + summary.failed + summary.cancelled if summary else 0,
            sum(row.ticket.publication_status() == "accepted" for row in selections),
            sum(
                row.receipt is not None and row.receipt.residency_status() == "accepted"
                for row in selections
            ),
        )

    def close(self) -> None:
        """Release real completed control owners before Rust drops generated sources."""
        self.release()
        self.stems.shut_down()
        self._patches.close()
        self.assets.release_saved_assignments()
