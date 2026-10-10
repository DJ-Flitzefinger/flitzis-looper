"""Bounded ordinary stem workers; control polling never decodes or aligns PCM."""

from dataclasses import dataclass
from pathlib import Path
from queue import Empty, SimpleQueue
from threading import Event
from typing import TYPE_CHECKING

from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.models import StemCacheEntry
from flitzis_looper.stem_pair_selection import StemPairSelection

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        ProjectAssetLifecycle,
    )
    from flitzis_looper_audio import (
        AudioEngine,
        PreparedSourceTicket,
        PreparedStemPair,
        ProjectAssetLease,
    )


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
    prepared: PreparedStemPair | None = None
    error: str | None = None

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
    ) -> None:
        """Reserve capacity and exact generation before the performer model changes."""
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
            sample_id, entry, ticket, content_id, previous_entry, retirement, lease, Event()
        )
        self.pending[sample_id] = request
        try:
            self._runner(lambda: self._run(request, components=components))
        except OSError, RuntimeError:
            self.pending.pop(sample_id)
            request.close()
            raise

    def _run(self, request: PairPreparation, *, components: bool) -> None:
        try:
            if not request.cancelled.is_set():
                pair = request.entry.pair
                request.prepared = self._audio.prepare_stem_pair(
                    request.sample_id,
                    request.entry.source_version,
                    request.entry.cache_dir,
                    request.ticket,
                    components,
                    pair.descriptor_reference if pair else None,
                )
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            request.error = str(error)
        finally:
            # Cancellation never closes readers while the worker still uses them.
            if request.cancelled.is_set():
                if request.prepared is not None:
                    # Only the control collector mutates retry state. The completed
                    # result remains reachable until cleanup admission succeeds.
                    request.error = "Stem pair preparation cancelled"
                request.close()
            self._results.put(request)

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
        return StemCacheEntry(
            source_version=request.entry.source_version,
            cache_dir=selection.wav_generation,
            stems=expected_stem_files(selection.wav_generation),
            available=False,
            pair=selection,
        )
