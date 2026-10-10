import threading
from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        ProjectAssetLifecycle,
    )
    from flitzis_looper.controller.stem_generation import StemGenerationRequest
    from flitzis_looper_audio import PreparedSourceTicket


@dataclass(frozen=True, slots=True)
class StemSubscriber:
    """One independently admitted content, never a copied native permit."""

    sample_id: int
    ticket: PreparedSourceTicket
    content_id: str | None
    retirement: AssetRetirementReservation


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
    ) -> None:
        self.request = request
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
        self._subscribers: dict[int, StemSubscriber] = {}
        try:
            self.add_subscriber(request.sample_id, ticket, content_id)
        except (OSError, RuntimeError, ValueError):
            self.cancel()
            raise

    def matches(self, request: StemGenerationRequest) -> bool:
        """Share only one verified source, output shape and separator configuration."""
        own = self.request
        return (
            own.source_path == request.source_path
            and own.source_version == request.source_version
            and own.target_shape == request.target_shape
            and own.model_cache_dir == request.model_cache_dir
            and own.separator == request.separator
            and own.device_policy == request.device_policy
            and own.demucs_shifts == request.demucs_shifts
            and own.demucs_overlap == request.demucs_overlap
        )

    def add_subscriber(
        self, sample_id: int, ticket: PreparedSourceTicket, content_id: str | None
    ) -> None:
        """Reserve independent publication cleanup before joining existing work."""
        retirement = self._assets.reserve(16)
        with self._lock:
            if self._cancelled or self._disposed or sample_id in self._subscribers:
                retirement.close()
                message = "Stem subscriber is cancelled or already registered"
                raise RuntimeError(message)
            self._subscribers[sample_id] = StemSubscriber(sample_id, ticket, content_id, retirement)

    def subscribers(self) -> tuple[StemSubscriber, ...]:
        """Snapshot registered interests without borrowing mutable worker state."""
        with self._lock:
            return tuple(self._subscribers.values())

    def remove_subscriber(self, sample_id: int) -> bool:
        """Detach one interest; only the final interest cancels physical work."""
        with self._lock:
            subscriber = self._subscribers.pop(sample_id, None)
            if subscriber is not None:
                subscriber.retirement.close()
            if not self._subscribers:
                self._cancelled = True
                if not self._running:
                    self._dispose_locked()
            return bool(self._subscribers)

    def begin(self) -> PreparedSourceTicket | None:
        """Claim the job once, or ignore a cancelled queued task."""
        with self._lock:
            if self._cancelled or self._disposed or self._begun:
                return None
            self._begun = True
            self._running = True
            return next(iter(self._subscribers.values())).ticket

    def finish_read(self) -> bool:
        """Release completed backend reads and report whether completion is current."""
        with self._lock:
            self._running = False
            self._source_lease.release()
            if self._cancelled:
                self._dispose_locked()
            return not self._cancelled

    def cancel(self) -> None:
        """Retire idle work now and running work after its last backend read."""
        with self._lock:
            self._cancelled = True
            for subscriber in self._subscribers.values():
                subscriber.retirement.close()
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
        for subscriber in self._subscribers.values():
            subscriber.retirement.close()
        self._subscribers.clear()
        self._source_lease.release()
        with self.retirement.activate():
            self._assets.retire(
                self.request.cache_dir, recursive=True, lease=self._generation_lease
            )
        self.retirement.close()
