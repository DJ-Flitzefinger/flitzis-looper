import threading
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
    from flitzis_looper.controller.stem_generation import StemGenerationRequest
    from flitzis_looper_audio import PreparedSourceTicket


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
    ) -> None:
        self.request = request
        self._ticket: PreparedSourceTicket | None = ticket
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

    def begin(self) -> PreparedSourceTicket | None:
        """Claim the job once, or ignore a cancelled queued task."""
        with self._lock:
            if self._cancelled or self._disposed:
                return None
            self._running = True
            return self._ticket

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
        self._ticket = None
        self._source_lease.release()
        with self.retirement.activate():
            self._assets.retire(
                self.request.cache_dir, recursive=True, lease=self._generation_lease
            )
        self.retirement.close()
