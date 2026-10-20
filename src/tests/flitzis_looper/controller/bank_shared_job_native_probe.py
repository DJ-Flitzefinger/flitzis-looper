"""Actual shared inference interests across a real Bank's acknowledged projection.

Only the nonneural inference-output boundary and headless source-shape routing
are substituted. Public Generate, the existing worker pool, registry, Native
leases, Bank ACK, five-WAV verification, pair preparation and publication remain
productive. Rust alone consumes the real callback commands.
"""

import hashlib
import json
import threading
from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import patch

from flitzis_looper.controller import stems as stems_module
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_generation import StemGenerationResult
from flitzis_looper_audio import SealedProcessingFiles
from tests.flitzis_looper.controller.bank_native_probe import BankControllerProbe

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.stem_generation import StemGenerationRequest
    from flitzis_looper.controller.stem_job import StemGenerationJob, StemSubscriber
    from flitzis_looper_audio import AudioEngine, BankMutationTicket


class _SourceAudio:
    """Delegate all authority to the same Native; shape is read from its real cache."""

    def __init__(self, source: object, audio: AudioEngine) -> None:
        self._source = source
        self._audio = audio

    def __getattr__(self, name: str) -> object:
        if name in {
            "loaded_sample_shape",
            "loaded_source_digest",
            "capture_offline_stem_source",
            "offline_stem_source_is_current",
            "capture_prepared_stem_source",
        }:
            return getattr(self._source, name)
        return getattr(self._audio, name)


class _DeclaredBackend:
    """Hold a real pool task's source/model read before returning complete known WAVs."""

    def __init__(self, wavs: tuple[bytes, ...]) -> None:
        assert len(wavs) == 5
        self.wavs = wavs
        self.entered = threading.Event()
        self.finish = threading.Event()
        self.returned = threading.Event()
        self.requests: list[StemGenerationRequest] = []
        self.original = b""
        self.private_file: Path | None = None

    def generate(
        self, request: StemGenerationRequest, progress: Callable[[float, str], None]
    ) -> StemGenerationResult:
        """Substitute declared output bytes, preserving the actual backend read lifetime."""
        assert threading.current_thread().name.startswith("stems_")
        assert request.separator == "demucs:htdemucs"
        assert request.target_shape.sample_rate_hz == 48_000
        assert request.target_shape.channels == 2
        assert request.target_shape.frame_count == 4096
        self.requests.append(request)
        assert len(self.requests) == 1, "equal interests must share one physical inference"
        self.original = request.source_path.read_bytes()
        request.cache_dir.mkdir(parents=True)
        self.private_file = request.cache_dir / "vocals.wav"
        self.private_file.write_bytes(self.wavs[0])
        progress(0.5, "Declared shared inference held across Bank settlement")
        self.entered.set()
        assert self.finish.wait(45), "shared inference was never released"
        assert request.source_path.read_bytes() == self.original
        assert self.private_file.read_bytes() == self.wavs[0]
        names = ("vocals", "melody", "bass", "drums", "instrumental")
        for name, encoded in zip(names, self.wavs, strict=True):
            (request.cache_dir / f"{name}.wav").write_bytes(encoded)
        assert not (request.cache_dir / ".complete.json").exists()
        self.returned.set()
        # Declared inference/encoder receipt, as in public_generate_adoption_tests.
        # Production five-WAV/PCM/provenance verification and promotion are unchanged.
        return StemGenerationResult(
            backend_name="demucs",
            model_name="htdemucs",
            device="cpu",
            cpu_fallback=False,
            artifact_count=5,
            encoder_verified=True,
        )


class SharedJobProbe(BankControllerProbe):
    """Add two bounded shared-job stages to the existing complete Bank test owner."""

    def __init__(self, seed: object, source_audio: object, wavs: tuple[bytes, ...]) -> None:
        super().__init__(seed, actual_workers=True)
        self.backend = _DeclaredBackend(wavs)
        self.app.stems._stem_backend = self.backend
        self.app.stems._on_project_changed = self.persistence.mark_dirty
        self.app.stems.bind_disk_selection_commit(
            self.persistence.commit_stem_selection,
            self.persistence.flush,
            writer_blocked=self.persistence.stem_selection_write_blocked,
        )
        self.source_audio = cast("AudioEngine", _SourceAudio(source_audio, self.audio))
        self.app._audio = self.source_audio
        self.app.stems._audio = self.source_audio
        pool = self.app.stems._stem_worker_pool
        assert pool is not None
        assert pool._executor._max_workers == 2
        self._patches: list[Callable[[], None]] = []
        self._job: StemGenerationJob | None = None
        self._original_subscribers: tuple[StemSubscriber, ...] = ()
        self._subscriber_ids: tuple[int, int] = (36, 215)
        self._old_target_id: str | None = None
        self._follower_id: str | None = None
        self._baseline_pair: object | None = None
        self._bank_after: dict[str, object] | None = None

    def _install_producer(self) -> None:
        directory = Path.cwd() / "declared-bank-shared-producer"
        directory.mkdir()
        files = tuple(
            directory / name for name in ("checkpoint.bin", "definition.txt", "encoder.bin")
        )
        for index, path in enumerate(files):
            path.write_bytes(f"Declared nonneural Bank/SharedStemJob producer {index}".encode())

        def capture(request: StemGenerationRequest) -> tuple[SealedProcessingFiles, str]:
            assert request.model_cache_dir == directory
            guard = SealedProcessingFiles([str(path) for path in files])
            producer = hashlib.sha256(json.dumps(guard.witnesses()).encode()).hexdigest()
            return guard, producer

        replacements = {
            "separator_model_cache_dir": lambda _separator: directory,
            "resolved_demucs_encoder": lambda _model: files[-1],
            "encoder_contract": lambda _encoder: "declared-bank-shared-output-receipt",
            "capture_processing_files": capture,
            "processing_source_roots": lambda _request, _guard: (directory,),
        }
        for name, replacement in replacements.items():
            owner = patch.object(stems_module, name, replacement)
            owner.start()
            self._patches.append(owner.stop)

    def start_shared_job(self, case: str) -> None:
        """Admit two independently current contents through ordinary public Generate."""
        assert case in {"copy", "clear"}
        self._subscriber_ids = (36, 71) if case == "clear" else (36, 215)
        assert self.project.stem_separator == "demucs:htdemucs"
        self._install_producer()
        first, follower = self._subscriber_ids
        a, b = self.project.pad_content[first], self.project.pad_content[follower]
        assert a is not None
        assert b is not None
        assert a.instance_id != b.instance_id
        assert self.project.sample_paths[first] == self.project.sample_paths[follower]
        self._old_target_id, self._follower_id = a.instance_id, b.instance_id
        entry = self.project.stem_cache[0]
        assert entry is not None
        assert entry.pair is not None
        self._baseline_pair = entry.pair.model_dump(mode="json")
        for slot in self._subscriber_ids:
            assert self.app.stems.generate_stems_async(slot), self.diagnostics()

    def shared_job_running(self) -> bool:
        """Observe actual identity coalescing and the existing backend executor."""
        self.app.stems.on_frame_render()
        assert not self.session.stem_generation_errors, self.diagnostics()
        jobs = tuple(self.app.stems._jobs.values())
        if not self.backend.entered.is_set() or len(jobs) != 1:
            return False
        job = jobs[0]
        if {row.sample_id for row in job.subscribers()} != set(self._subscriber_ids):
            return False
        assert job.running
        assert job.phase == "running"
        assert len(self.backend.requests) == 1
        assert job.generation_fingerprint is not None
        assert job.producer_identity is not None
        assert not job._source_lease.released
        assert not job._generation_lease.released
        assert job.processing_files is not None
        self._job = job
        self._original_subscribers = job.subscribers()
        return True

    def begin_shared_bank(self, case: str) -> None:
        """Capture the real bank after both independently admitted interests exist."""
        assert self._job is not None
        assert self._job.running
        assert case in {"copy", "clear"}
        self.capture(None if case == "clear" else 0, 1)
        self.begin()

    def shared_bank_pending(self) -> bool:
        """Poll the actual preparation pool without synthesizing a worker result."""
        assert self.bank is not None
        self.controller.poll()
        assert self.controller.error is None, self.diagnostics()
        if self.bank.ticket is None or not self.bank.published:
            return False
        self.ticket = self.bank.ticket
        assert self.ticket.publication_status() == "pending"
        self.assert_waiting()
        return True

    def shared_bank_ticket(self) -> BankMutationTicket:
        """Expose the same opaque parent only after ordinary publication."""
        assert self.ticket is not None
        return self.ticket

    def settle_shared_bank(self, case: str) -> None:
        """Use normal durable settlement, then check exact detached interests and births."""
        self.settle()
        assert self._job is not None
        job = self._job
        assert job.running
        assert not self.backend.returned.is_set()
        assert not job._source_lease.released
        assert not job._generation_lease.released
        assert job.processing_files is not None
        assert self.backend.private_file is not None
        assert self.backend.private_file.is_file()
        request = self.backend.requests[0]
        assert request.source_path.read_bytes() == self.backend.original
        if case == "clear":
            assert not job.subscribers()
            assert job.cancel_event.is_set()
            assert all(self.project.pad_content[slot] is None for slot in range(36, 72))
        else:
            assert tuple(row.sample_id for row in job.subscribers()) == (215,)
            survivor = job.subscribers()[0]
            assert any(survivor is original for original in self._original_subscribers)
            assert survivor.content_id == self._follower_id
            assert not job.cancel_event.is_set()
            fresh = self.project.pad_content[36]
            assert fresh is not None
            assert fresh.instance_id != self._old_target_id
            source = self.project.pad_content[0]
            assert source is not None
            assert fresh.copied_from == source.instance_id
            for slot in (0, 36, 215):
                entry = self.project.stem_cache[slot]
                assert entry is not None
                assert entry.pair is not None
                assert entry.pair.model_dump(mode="json") == self._baseline_pair
            assert 36 not in self.app.stems._generation_sources
            assert 36 not in self.session.stem_generating_sample_ids
        self._bank_after = self.project.model_dump(mode="json", exclude_computed_fields=True)
        self.forget_settled()

    def request_follower_all(self) -> None:
        """Select ALL through the existing stopped follower's independent control path."""
        self.app.stems.set_stem_enabled_mask(215, 1)
        assert self.app.stems.set_stem_mix_mode(215, "all_stems"), self.diagnostics()

    def follower_all_ready(self) -> bool:
        """Require the baseline's own registration and callback component acknowledgement."""
        self.app.stems.on_frame_render()
        assert not self.session.stem_generation_errors, self.diagnostics()
        status = self.app.stems.stem_residency_status(215)
        return (
            status is not None
            and status.registration == "accepted"
            and status.residency == "accepted"
        )

    def release_backend(self) -> None:
        """Release only the declared inference boundary, never its Native acceptance."""
        assert self._job is not None
        assert self._job.running
        self.backend.finish.set()

    def completion_ready(self, case: str) -> bool:
        """Drive ordinary observers and require the correct original recipient's result."""
        self.app.stems.on_frame_render()
        self.assets.retry_retirements()
        assert not self.session.stem_generation_errors, self.diagnostics()
        assert len(self.backend.requests) == 1
        if not self.backend.returned.is_set() or self.app.stems._jobs:
            return False
        assert self._job is not None
        return self._clear_completion_ready() if case == "clear" else self._copy_completion_ready()

    def _clear_completion_ready(self) -> bool:
        assert self._job is not None
        if self._job.phase != "disposed" or self.backend.requests[0].cache_dir.exists():
            return False
        assert self._job.source is None
        assert self._job.processing_files is None
        assert self._job._source_lease.released
        assert self._job._generation_lease.released
        assert self._bank_after == self.project.model_dump(
            mode="json", exclude_computed_fields=True
        )
        assert not self.session.stem_generating_sample_ids
        assert not self.app.stems._pending_stem_publications
        assert not self.app.stems._pair_preparations.pending
        assert not self.app.stems._disk_selection_settlements
        return True

    def _copy_completion_ready(self) -> bool:
        entry = self.project.stem_cache[215]
        if (
            entry is None
            or entry.pair is None
            or entry.pair.model_dump(mode="json") == self._baseline_pair
        ):
            return False
        status = self.app.stems.stem_residency_status(215)
        if status is None or status.registration != "accepted" or status.residency != "accepted":
            return False
        if status.control_stem_set_identity != entry.pair.stem_set_identity:
            return False
        residency = self.app.stems._committed_residency
        live = residency._live_selections.get(215)
        owner = live.control if live is not None else residency._selections.get(215)
        assert owner is not None
        assert owner.entry.pair == entry.pair
        assert owner.content_id == self._follower_id
        assert owner.source_path == self.project.sample_paths[215]
        assert owner.ticket.is_current()
        assert owner.ticket.publication_status() == "accepted"
        assert owner.receipt is not None
        assert owner.receipt.is_current()
        assert owner.receipt.residency_status() == "accepted"
        assert owner.receipt_intent == ("all_stems", 1)
        follower = self.project.pad_content[215]
        assert follower is not None
        assert follower.instance_id == self._follower_id
        for slot in (0, 36):
            unchanged = self.project.stem_cache[slot]
            assert unchanged is not None
            assert unchanged.pair is not None
            assert unchanged.pair.model_dump(mode="json") == self._baseline_pair
        assert 215 not in self.app.stems._pair_preparations.pending
        assert 215 not in self.app.stems._disk_selection_settlements
        assert 215 not in self.app.stems._pending_stem_publications
        self._assert_assignments()
        self.persistence.flush()
        saved = ProjectPersistence.from_config_path(self.persistence.config_path)
        assert saved.load_error is None
        assert saved.project.pad_content[215] == self.project.pad_content[215]
        saved_entry = saved.project.stem_cache[215]
        assert saved_entry is not None
        assert saved_entry.pair == entry.pair
        return True

    def diagnostics(self) -> str:
        """Keep actual statuses available for a bounded native-test timeout."""
        status = self.app.stems.stem_residency_status(215)
        return json.dumps({
            "bank_status": self.controller.status,
            "bank_error": self.controller.error,
            "job_count": len(self.app.stems._jobs),
            "job_phase": self._job.phase if self._job else None,
            "subscribers": [row.sample_id for row in self._job.subscribers()] if self._job else [],
            "backend_entered": self.backend.entered.is_set(),
            "backend_returned": self.backend.returned.is_set(),
            "errors": self.session.stem_generation_errors,
            "follower": repr(status),
        })

    def release_test_backend(self) -> None:
        """Make failure unwinding release a blocked worker without forging completion."""
        self.backend.finish.set()

    def close(self) -> None:
        """Restore declared producer hooks after the terminal productive owners close."""
        assert not self.app.stems._jobs, self.diagnostics()
        assert self.backend.returned.is_set()
        for stop in reversed(self._patches):
            stop()
        self._patches.clear()
        super().close()
