"""Bounded historical-evidence restoration under fresh native ownership."""

from concurrent.futures import Future, ThreadPoolExecutor
from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.accepted_timing import PersistedAcceptedTiming
    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import AudioEngine, ConstantTimingTicket


@dataclass
class _Restore:
    future: Future[ConstantTimingTicket]
    path: str


def _matches_adopted_request(
    current: dict[str, object] | None,
    captured: dict[str, object],
    accepted: dict[str, object] | None,
) -> bool:
    return (
        current is not None
        and accepted is not None
        and current.get("revision") == accepted.get("revision")
        and current.get("accepted_request_id") == captured.get("request_id")
    )


class AcceptedTimingRestore:
    """Verify at most one queued restoration per pad with one heavy worker.

    Native capture runs synchronously at admission. The worker cannot capture a
    later source or authority after an intervening edit. Only the native current
    resolver and matching callback acknowledgement permit completion refresh.
    """

    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_adopted: Callable[[int], None],
    ) -> None:
        self._project = project
        self._session = session
        self._audio = audio
        self._on_adopted = on_adopted
        self._worker: ThreadPoolExecutor | None = None
        self._pending: dict[int, _Restore] = {}
        self._migration_futures: set[Future[ConstantTimingTicket]] = set()

    def prepare_for_migration(
        self, sample_id: int, record: PersistedAcceptedTiming, new_path: str
    ) -> Future[ConstantTimingTicket]:
        """Use the same bounded worker and real capture without changing saved references."""
        self._migration_futures = {
            future for future in self._migration_futures if not future.done()
        }
        if len(self._migration_futures) + len(self._pending) >= 32:
            message = "saved timing preparation queue full (32 jobs)"
            raise RuntimeError(message)
        captured = self._audio.capture_saved_constant_timing(
            sample_id, record.model_dump_json(), new_path
        )
        if self._worker is None:
            self._worker = ThreadPoolExecutor(max_workers=1, thread_name_prefix="timing-restore")
        future = self._worker.submit(self._audio.restore_constant_timing, captured)
        self._migration_futures.add(future)
        return future

    def has_pending(self) -> bool:
        """Report owned restoration work before a new shared native preparation."""
        return bool(self._pending)

    def begin(self, sample_id: int) -> None:
        """Capture current native ownership before scheduling heavy verification."""
        self.cancel(sample_id)
        analysis = self._project.sample_analysis[sample_id]
        path = self._project.sample_paths[sample_id]
        if analysis is None or analysis.accepted_timing is None or path is None:
            self._session.sample_analysis_errors[sample_id] = (
                "Saved Automatic timing has no supported complete evidence"
            )
            return
        try:
            captured = self._audio.capture_saved_constant_timing(
                sample_id, analysis.accepted_timing.model_dump_json(), path
            )
        except (RuntimeError, ValueError) as error:
            self._session.sample_analysis_errors[sample_id] = str(error)
            return
        if self._worker is None:
            self._worker = ThreadPoolExecutor(max_workers=1, thread_name_prefix="timing-restore")
        future = self._worker.submit(self._audio.restore_constant_timing, captured)
        self._pending[sample_id] = _Restore(future, path)

    def poll(self) -> None:
        """Observe genuine adoption; polling never publishes accepted audio truth."""
        for sample_id, restore in list(self._pending.items()):
            if (
                self._project.sample_paths[sample_id] != restore.path
                or self._project.manual_bpm[sample_id] is not None
                or self._audio.pad_timing_intent(sample_id) != "automatic"
            ):
                self.cancel(sample_id)
                continue
            if not restore.future.done():
                continue
            if restore.future.cancelled():
                self._pending.pop(sample_id)
                continue
            try:
                ticket = restore.future.result()
                status = ticket.publication_status()
                if status == "pending":
                    continue
                self._pending.pop(sample_id)
                if status != "accepted":
                    self._session.sample_analysis_errors[sample_id] = (
                        "Fresh saved timing adoption was rejected"
                    )
                    continue
                current = self._audio.current_constant_timing(sample_id)
                captured = ticket.metadata()
                accepted = ticket.accepted_metadata()
                if not _matches_adopted_request(current, captured, accepted):
                    continue
                self._session.sample_analysis_errors.pop(sample_id, None)
                self._on_adopted(sample_id)
            except (RuntimeError, ValueError) as error:
                self._pending.pop(sample_id, None)
                self._session.sample_analysis_errors[sample_id] = str(error)

    def cancel(self, sample_id: int) -> None:
        """Drop polling ownership; native source/request/authority guards retire work."""
        restore = self._pending.pop(sample_id, None)
        if restore is not None:
            restore.future.cancel()

    def shut_down(self) -> None:
        """Drain owned off-thread verification before native stream teardown."""
        for sample_id in list(self._pending):
            self.cancel(sample_id)
        for future in self._migration_futures:
            future.cancel()
        self._migration_futures.clear()
        if self._worker is not None:
            self._worker.shutdown(wait=True, cancel_futures=True)
            self._worker = None
