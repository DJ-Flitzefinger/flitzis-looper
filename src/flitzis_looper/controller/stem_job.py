import os
import threading
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, Literal

from flitzis_looper.controller.stem_identity import request_fingerprint, request_signature

if TYPE_CHECKING:
    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        ProjectAssetLifecycle,
    )
    from flitzis_looper.controller.stem_generation import StemGenerationRequest
    from flitzis_looper.controller.stem_intent import StemGenerationIntent
    from flitzis_looper_audio import PreparedSourceTicket, ProjectAssetLease, SealedProcessingFiles

type StemJobPhase = Literal["queued", "resolving", "resolved", "running", "settled", "disposed"]


@dataclass(frozen=True, slots=True)
class StemSubscriber:
    """One independently admitted content, never a copied native permit."""

    sample_id: int
    ticket: PreparedSourceTicket
    content_id: str | None
    retirement: AssetRetirementReservation
    request: StemGenerationRequest | None = None
    intent: StemGenerationIntent | None = None
    source_lease: ProjectAssetLease | None = None

    def close(self) -> None:
        """Release only this interest's source custody and reserved publication capacity."""
        if self.source_lease is not None:
            self.source_lease.release()
        self.retirement.close()


class StemGenerationJob:
    """Keep original and private-generation owners through actual backend reads.

    Cancellation releases an unstarted job immediately. A running backend retains
    its leases until it returns; cancellation never removes files under that read.
    """

    def __init__(
        self,
        request: StemGenerationRequest,
        ticket: PreparedSourceTicket,
        assets: ProjectAssetLifecycle,
        content_id: str | None = None,
        *,
        intent: StemGenerationIntent | None = None,
    ) -> None:
        self.request = request
        self.source_ticket = ticket
        self.intent = intent
        self.resolved_request: StemGenerationRequest | None = None
        self._assets = assets
        self.retirement = assets.reserve(18)
        try:
            self._source_lease = assets.acquire(request.source_path)
        except RuntimeError, ValueError, OSError:
            self.retirement.close()
            raise
        try:
            self._generation_lease = assets.acquire(request.cache_dir)
        except RuntimeError, ValueError, OSError:
            self._source_lease.release()
            self.retirement.close()
            raise
        self._lock = threading.Lock()
        self._cancelled = False
        self._running = False
        self._disposed = False
        self._begun = False
        self._phase: StemJobPhase = "queued"
        self.generation_fingerprint: str | None = None
        self.producer_identity: str | None = None
        self.processing_files: SealedProcessingFiles | None = None
        self._subscribers: dict[int, StemSubscriber] = {}
        try:
            self.add_subscriber(
                request.sample_id, ticket, content_id, request=request, intent=intent
            )
        except OSError, RuntimeError, ValueError:
            self.cancel()
            raise

    def matches(self, request: StemGenerationRequest, producer_identity: str | None = None) -> bool:
        """Share resolved full content fingerprints, never model or source locators."""
        fingerprint = request_fingerprint(request, producer_identity)
        with self._lock:
            return (
                not self._cancelled
                and not self._disposed
                and self.resolved_request is not None
                and fingerprint is not None
                and fingerprint == self.generation_fingerprint
            )

    def cohorts(self, request: StemGenerationRequest) -> bool:
        """Group identical queued locators only before byte capture; this is not identity."""
        own = self.request
        with self._lock:
            return (
                not self._cancelled
                and not self._disposed
                and self._phase == "queued"
                and own.source_version == request.source_version
                and os.path.normcase(str(own.source_path.absolute()))
                == os.path.normcase(str(request.source_path.absolute()))
                and os.path.normcase(str(own.model_cache_dir.absolute()))
                == os.path.normcase(str(request.model_cache_dir.absolute()))
                and request_signature(own) == request_signature(request)
            )

    def add_subscriber(
        self,
        sample_id: int,
        ticket: PreparedSourceTicket,
        content_id: str | None,
        *,
        request: StemGenerationRequest | None = None,
        intent: StemGenerationIntent | None = None,
    ) -> None:
        """Reserve independent publication/source custody before joining existing work."""
        own_request = request or replace(self.request, sample_id=sample_id)
        if own_request.sample_id != sample_id:
            message = "Stem subscriber request must belong to its own pad"
            raise ValueError(message)
        retirement = self._assets.reserve(16)
        try:
            source_lease = self._assets.acquire(own_request.source_path)
        except OSError, RuntimeError, ValueError:
            retirement.close()
            raise
        with self._lock:
            if self._cancelled or self._disposed or sample_id in self._subscribers:
                source_lease.release()
                retirement.close()
                message = "Stem subscriber is cancelled or already registered"
                raise RuntimeError(message)
            self._subscribers[sample_id] = StemSubscriber(
                sample_id, ticket, content_id, retirement, own_request, intent, source_lease
            )

    def update_intent(self, sample_id: int, intent: StemGenerationIntent) -> None:
        """Install one subscriber's resolved revision without changing captured job context."""
        with self._lock:
            subscriber = self._subscribers.get(sample_id)
            if subscriber is None:
                message = "Stem subscriber is no longer registered"
                raise RuntimeError(message)
            if subscriber.request is not None and request_signature(subscriber.request) != (
                intent.signature
            ):
                message = "Resolved stem intent differs from its own captured request"
                raise ValueError(message)
            self._subscribers[sample_id] = replace(subscriber, intent=intent)

    def transfer_subscribers_to(self, target: StemGenerationJob) -> tuple[StemSubscriber, ...]:
        """Move exact interests after resolution without releasing their independent custody."""
        if target is self:
            return ()
        first, second = (self, target) if id(self) < id(target) else (target, self)
        with first._lock, second._lock:
            if self._assets is not target._assets:
                message = "Stem subscribers cannot transfer across project ownership"
                raise ValueError(message)
            if self._running or self._cancelled or self._disposed:
                message = "Stem candidate has unsettled reads or no current interests"
                raise RuntimeError(message)
            if target._cancelled or target._disposed:
                message = "Stem target job is no longer available"
                raise RuntimeError(message)
            if (
                self.resolved_request is None
                or target.resolved_request is None
                or self.generation_fingerprint is None
                or self.generation_fingerprint != target.generation_fingerprint
            ):
                message = "Stem subscriber transfer requires equal verified full fingerprints"
                raise ValueError(message)
            if self._subscribers.keys() & target._subscribers.keys():
                message = "Stem target already contains a subscriber for this pad"
                raise RuntimeError(message)
            transferred = tuple(self._subscribers.values())
            target._subscribers.update(self._subscribers)
            self._subscribers.clear()
            self._cancelled = True
            self._dispose_locked()
            return transferred

    @property
    def running(self) -> bool:
        """Whether resolution or inference currently holds an actual backend read."""
        with self._lock:
            return self._running

    @property
    def phase(self) -> StemJobPhase:
        """Current bounded job phase for control-owned inference serialization."""
        with self._lock:
            return self._phase

    def subscribers(self) -> tuple[StemSubscriber, ...]:
        """Snapshot registered interests without borrowing mutable worker state."""
        with self._lock:
            return tuple(self._subscribers.values())

    def remove_subscriber(self, sample_id: int) -> bool:
        """Detach one interest; only the final interest cancels physical work."""
        with self._lock:
            subscriber = self._subscribers.pop(sample_id, None)
            if subscriber is not None:
                subscriber.close()
            if not self._subscribers:
                self._cancelled = True
                if not self._running:
                    self._dispose_locked()
            return bool(self._subscribers)

    def begin_resolution(self) -> PreparedSourceTicket | None:
        """Claim byte resolution once while preserving the job source through the phase gap."""
        with self._lock:
            if self._cancelled or self._disposed or self._phase != "queued":
                return None
            self._running = True
            self._phase = "resolving"
            return self.source_ticket

    def finish_resolution(self) -> bool:
        """Settle resolution reads without releasing source/model custody before inference."""
        with self._lock:
            self._running = False
            if self._cancelled:
                self._dispose_locked()
            else:
                self._phase = "resolved"
            return not self._cancelled

    def begin(self) -> PreparedSourceTicket | None:
        """Claim inference once, or ignore a cancelled queued task."""
        with self._lock:
            if self._cancelled or self._disposed or self._begun or self._phase == "resolving":
                return None
            self._begun = True
            self._running = True
            self._phase = "running"
            return self.source_ticket

    def finish_read(self) -> bool:
        """Release completed backend reads and report whether completion is current."""
        with self._lock:
            self._running = False
            self._phase = "settled"
            self._source_lease.release()
            if self._cancelled:
                self._dispose_locked()
            return not self._cancelled

    def cancel(self) -> None:
        """Retire idle work now and running work after its last backend read."""
        with self._lock:
            self._cancelled = True
            for subscriber in self._subscribers.values():
                subscriber.close()
            self._subscribers.clear()
            if not self._running:
                self._dispose_locked()

    def dispose(self) -> None:
        """Release a settled event's ownership and retire only its private directory."""
        with self._lock:
            if not self._running:
                self._dispose_locked()

    def _dispose_locked(self) -> None:
        if self._disposed:
            return
        self._disposed = True
        self._phase = "disposed"
        # The guard outlives backend return and the queued event. Successful
        # settlement promotes the immutable WAVs/marker before disposing this job.
        self.processing_files = None
        for subscriber in self._subscribers.values():
            subscriber.close()
        self._subscribers.clear()
        self._source_lease.release()
        with self.retirement.activate():
            self._assets.retire(
                self.request.cache_dir, recursive=True, lease=self._generation_lease
            )
        self.retirement.close()
