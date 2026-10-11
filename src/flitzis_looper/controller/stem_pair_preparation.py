"""Bounded ordinary stem workers; control polling never decodes or aligns PCM."""

import json
from dataclasses import dataclass, field
from pathlib import Path
from queue import Empty, SimpleQueue
from threading import Event
from typing import TYPE_CHECKING

from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.controller.stem_identity import request_fingerprint
from flitzis_looper.controller.stem_provenance import capture_processing_files
from flitzis_looper.models import StemCacheEntry
from flitzis_looper.stem_pair_selection import (
    STEM_FINGERPRINT_SCHEMA,
    StemGenerationProvenance,
    StemPairSelection,
)
from flitzis_looper_audio import MigrationArtifactLease

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        ProjectAssetLifecycle,
    )
    from flitzis_looper.controller.stem_generation import StemGenerationRequest
    from flitzis_looper.controller.stem_intent import StemGenerationIntent
    from flitzis_looper_audio import (
        AudioEngine,
        PreparedSourceTicket,
        PreparedStemPair,
        ProjectAssetLease,
        SealedProcessingFiles,
    )


@dataclass(frozen=True, slots=True)
class StemPairIntentBinding:
    """Captured controller intent and independent source/artifact authority."""

    generation_intent: StemGenerationIntent | None = None
    own_source_version: str | None = None
    artifact_source_version: str | None = None
    source_path: str | None = None


@dataclass(slots=True)
class PairPreparation:
    """One independent current subscriber, with reserved rollback and exact readers."""

    sample_id: int
    entry: StemCacheEntry
    ticket: PreparedSourceTicket
    content_id: str | None
    previous_entry: StemCacheEntry | None
    retirement: AssetRetirementReservation
    lease: ProjectAssetLease
    cancelled: Event
    processing_request: StemGenerationRequest | None = None
    regenerate_on_incompatible: bool = False
    compatibility_rejected: bool = False
    prepared: PreparedStemPair | None = None
    error: str | None = None
    residency_only: bool = False
    generation_intent: StemGenerationIntent | None = None
    own_source_version: str | None = None
    artifact_source_version: str | None = None
    source_path: str | None = None
    _artifact_version_explicit: bool = field(init=False, repr=False)

    def __post_init__(self) -> None:
        """Capture own authority separately from an explicitly named artifact origin."""
        self._artifact_version_explicit = self.artifact_source_version is not None
        if self.own_source_version is None:
            self.own_source_version = self.entry.source_version
        if self.artifact_source_version is None:
            self.artifact_source_version = self.entry.source_version

    def close(self) -> None:
        """Release this job after actual worker return, preserving any saved assignment."""
        self.lease.release()
        self.retirement.close()


class StemPairPreparationQueue:
    """Use the existing 2-worker/32-queue admission, retaining readers through return."""

    def __init__(
        self,
        audio: AudioEngine,
        assets: ProjectAssetLifecycle,
        runner: Callable[[Callable[[], None]], None],
    ) -> None:
        self._audio = audio
        self._assets = assets
        self._runner = runner
        self._results: SimpleQueue[PairPreparation] = SimpleQueue()
        self.pending: dict[int, PairPreparation] = {}
        self._discard_retries: dict[int, tuple[int, PreparedStemPair]] = {}

    def submit(
        self,
        sample_id: int,
        entry: StemCacheEntry,
        ticket: PreparedSourceTicket,
        content_id: str | None,
        previous_entry: StemCacheEntry | None,
        *,
        components: bool,
        processing_request: StemGenerationRequest | None = None,
        regenerate_on_incompatible: bool = False,
        residency_only: bool = False,
        binding: StemPairIntentBinding | None = None,
    ) -> None:
        """Reserve capacity and exact generation before the performer model changes."""
        captured = StemPairIntentBinding() if binding is None else binding
        if (
            captured.own_source_version is not None
            and captured.own_source_version != entry.source_version
            and captured.artifact_source_version is None
        ):
            message = "Cross-root stem reuse requires an explicit artifact source version"
            raise ValueError(message)
        if sample_id in self.pending:
            message = "Stem pair preparation already pending"
            raise RuntimeError(message)
        if any(owner == sample_id for owner, _ in self._discard_retries.values()):
            message = "Stem pair cleanup admission is still pending"
            raise RuntimeError(message)
        retirement = self._assets.reserve_pending(16)
        try:
            lease = self._assets.acquire(Path(entry.cache_dir))
        except OSError, RuntimeError, ValueError:
            retirement.close()
            raise
        request = PairPreparation(
            sample_id,
            entry,
            ticket,
            content_id,
            previous_entry,
            retirement,
            lease,
            Event(),
            processing_request,
            regenerate_on_incompatible,
            residency_only=residency_only,
            generation_intent=captured.generation_intent,
            own_source_version=captured.own_source_version,
            artifact_source_version=captured.artifact_source_version,
            source_path=captured.source_path,
        )
        self.pending[sample_id] = request
        try:
            self._runner(lambda: self._run(request, components=components))
        except OSError, RuntimeError:
            self.pending.pop(sample_id)
            request.close()
            raise

    def _run(self, request: PairPreparation, *, components: bool) -> None:
        processing = None
        artifact = None
        try:
            if not request.cancelled.is_set():
                if request.processing_request is not None:
                    request.compatibility_rejected = True
                    processing = capture_processing_files(request.processing_request)
                    self._validate_saved_producer(request, processing)
                    artifact = MigrationArtifactLease.capture(
                        str(Path.cwd() / "samples"), request.entry.cache_dir
                    )
                    self._validate_saved_marker(request.entry)
                    request.compatibility_rejected = False
                pair = request.entry.pair
                request.prepared = self._audio.prepare_stem_pair(
                    request.sample_id,
                    request.own_source_version
                    if request.own_source_version is not None
                    else request.entry.source_version,
                    request.entry.cache_dir,
                    request.ticket,
                    components,
                    pair.descriptor_reference if pair else None,
                    request.artifact_source_version if request._artifact_version_explicit else None,
                )
                if processing is not None:
                    processing[0].verify()
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            request.error = str(error)
        finally:
            if artifact is not None:
                artifact.release()
            processing = None
            # Cancellation never closes readers while the worker still uses them.
            if request.cancelled.is_set():
                if request.prepared is not None:
                    # Only the control collector mutates retry state. The completed
                    # result remains reachable until cleanup admission succeeds.
                    request.error = "Stem pair preparation cancelled"
                request.close()
            self._results.put(request)

    @staticmethod
    def _validate_saved_producer(
        request: PairPreparation, processing: tuple[SealedProcessingFiles, str | None] | None
    ) -> None:
        message = "saved stem producer differs from actual current model/config bytes"
        if processing is None or processing[1] is None or request.processing_request is None:
            raise ValueError(message)
        if (
            request.entry.fingerprint_schema != STEM_FINGERPRINT_SCHEMA
            or processing[1] != request.entry.producer_identity
            or request_fingerprint(request.processing_request, processing[1])
            != request.entry.generation_fingerprint
        ):
            raise ValueError(message)

    @staticmethod
    def _validate_saved_marker(entry: StemCacheEntry) -> None:
        with (Path(entry.cache_dir) / ".complete.json").open("rb") as marker:
            encoded = marker.read(65537)
        if len(encoded) > 65536:
            message = "stem provenance marker exceeds its bounded size"
            raise ValueError(message)
        provenance = json.loads(encoded)
        if (
            not isinstance(provenance, dict)
            or provenance.get("fingerprint_schema") != STEM_FINGERPRINT_SCHEMA
            or provenance.get("fingerprint_schema") != entry.fingerprint_schema
            or provenance.get("generation_fingerprint") != entry.generation_fingerprint
            or provenance.get("producer_identity") != entry.producer_identity
        ):
            message = "saved stem provenance differs from its complete verified marker"
            raise ValueError(message)

    def collect(self) -> tuple[PairPreparation, ...]:
        """Collect only returned workers; each result still owns its rollback reservation."""
        ready: list[PairPreparation] = []
        while True:
            try:
                request = self._results.get_nowait()
            except Empty:
                return tuple(ready)
            if self.pending.get(request.sample_id) is request:
                self.pending.pop(request.sample_id)
            ready.append(request)

    def cancel(self, sample_id: int) -> None:
        """Withdraw interest without releasing a queued/running reader prematurely."""
        request = self.pending.get(sample_id)
        if request is not None:
            request.cancelled.set()

    def discard(self, sample_id: int, prepared: PreparedStemPair) -> str | None:
        """Keep failed native queue admission reachable without leaking reservations."""
        self._discard_retries.pop(id(prepared), None)
        try:
            prepared.discard()
        except (OSError, RuntimeError, ValueError) as error:
            self._discard_retries[id(prepared)] = sample_id, prepared
            return str(error)
        self._discard_retries.pop(id(prepared), None)
        return None

    def retry_discards(self) -> tuple[tuple[int, str], ...]:
        """Retry at most eight held results per poll; newer admission stays bounded."""
        errors: list[tuple[int, str]] = []
        for sample_id, prepared in tuple(self._discard_retries.values())[:8]:
            error = self.discard(sample_id, prepared)
            if error is not None:
                errors.append((sample_id, error))
        return tuple(errors)

    @staticmethod
    def selected_entry(request: PairPreparation) -> StemCacheEntry:
        """Validate the actual native disk selection before installing references."""
        if request.prepared is None:
            message = "Complete stem pair worker returned no result"
            raise RuntimeError(message)
        selection = StemPairSelection.model_validate_json(request.prepared.selection_json())
        expected_provenance = None
        if request.entry.fingerprint_schema is not None:
            fingerprint = request.entry.generation_fingerprint
            producer = request.entry.producer_identity
            if fingerprint is None or producer is None:
                message = "Captured stem provenance lacks complete generation metadata"
                raise RuntimeError(message)
            expected_provenance = StemGenerationProvenance(
                fingerprint_schema=request.entry.fingerprint_schema,
                generation_fingerprint=fingerprint,
                producer_identity=producer,
            )
        if selection.provenance != expected_provenance:
            message = "Prepared pair provenance differs from captured generation metadata"
            raise RuntimeError(message)
        if (
            request.entry.pair is not None
            and selection != request.entry.pair
            and (
                not request._artifact_version_explicit
                or request.own_source_version == request.artifact_source_version
                or selection.provenance != request.entry.pair.provenance
            )
        ):
            message = "Prepared pair selection changed during reuse"
            raise RuntimeError(message)
        # Native verifies the complete origin pair/source and the copied
        # target before issuing a new root-bound descriptor/identity. Its
        # provenance remains equal; its marker is never the own source ticket.
        return StemCacheEntry(
            source_version=(
                request.own_source_version
                if request.own_source_version is not None
                else request.entry.source_version
            ),
            cache_dir=selection.wav_generation,
            stems=expected_stem_files(selection.wav_generation),
            available=False,
            pair=selection,
            fingerprint_schema=request.entry.fingerprint_schema,
            generation_fingerprint=request.entry.generation_fingerprint,
            producer_identity=request.entry.producer_identity,
        )
