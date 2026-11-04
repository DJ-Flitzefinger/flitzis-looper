"""Restore existing legacy components without creating or selecting a new pair."""

from concurrent.futures import Future
from dataclasses import dataclass
from typing import TYPE_CHECKING

from flitzis_looper.controller.committed_stem_residency import resident_stem_admission_pressure

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.models import StemCacheEntry
    from flitzis_looper_audio import (
        ExistingLegacyStemWork,
        LegacyStemRestoreReceipt,
        PreparedExistingLegacyStems,
        PreparedSourceTicket,
    )


@dataclass(slots=True)
class ExistingLegacyRestore:
    """Original publication and work stay owned through actual worker return."""

    ticket: PreparedSourceTicket
    work: ExistingLegacyStemWork
    future: Future[PreparedExistingLegacyStems]
    published: bool = False
    receipt: LegacyStemRestoreReceipt | None = None

    def cancel(self) -> None:
        """Withdraw unclaimed work; running work still owes its real reader END."""
        self.work.cancel()
        self.future.cancel()
        if self.receipt is not None:
            self.receipt.cancel()


def _admit(app: AppController, epoch: int, sample_id: int, entry: StemCacheEntry) -> bool:
    pool = app.stems._stem_worker_pool
    if pool is None:
        message = "Original stem worker pool is unavailable"
        raise RuntimeError(message)
    reservation = pool.try_reserve()
    if reservation is None:
        return False
    with reservation:
        ticket = app.audio.capture_prepared_source(sample_id, entry.source_version)
        work = app.audio.capture_existing_legacy_stems_for_output(
            epoch, sample_id, entry.cache_dir, ticket
        )
        future: Future[PreparedExistingLegacyStems] = Future()

        def prepare() -> None:
            if not future.set_running_or_notify_cancel():
                return
            try:
                result = work.prepare()
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                future.set_exception(error)
            else:
                future.set_result(result)

        reservation.submit(prepare)
        app._output_legacy_restores[sample_id] = ExistingLegacyRestore(ticket, work, future)
    return True


def legacy_restore_ready(
    app: AppController, epoch: int, sample_id: int, entry: StemCacheEntry
) -> bool:
    """Require the original five-file publication and actual mode/mask callback ACK."""
    mode = app.project.pad_stem_mix_mode[sample_id]
    if mode == "full_mix" and not app.project.preload_stem_loops_on_startup:
        return True
    owner = app._output_legacy_restores.get(sample_id)
    if owner is None:
        try:
            _admit(app, epoch, sample_id, entry)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            if not resident_stem_admission_pressure(str(error)):
                raise
        return False
    if not _legacy_publication_ready(app, epoch, sample_id, owner):
        return False
    if not _legacy_selection_ready(app, epoch, sample_id, owner, mode):
        return False
    current = app.project.stem_cache[sample_id]
    if current is None or current.model_dump(exclude={"available"}) != entry.model_dump(
        exclude={"available"}
    ):
        message = "Original legacy stem selection changed during restoration"
        raise RuntimeError(message)
    app.project.stem_cache[sample_id] = current.model_copy(update={"available": True})
    return True


def _legacy_publication_ready(
    app: AppController, epoch: int, sample_id: int, owner: ExistingLegacyRestore
) -> bool:
    if not owner.future.done():
        return False
    if not owner.published:
        try:
            prepared = owner.future.result()
            app.audio.publish_existing_legacy_stems_for_output(epoch, prepared, owner.ticket)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            if not resident_stem_admission_pressure(str(error)):
                raise
            # Retry only a declared refusal, after the original worker returns.
            pool = app.stems._stem_worker_pool
            if pool is not None and pool.idle:
                del app._output_legacy_restores[sample_id]
            return False
        owner.published = True
    status = owner.ticket.publication_status()
    if status in {"captured", "pending"}:
        return False
    if status != "accepted" or not owner.ticket.is_current():
        message = "Existing legacy stem publication was rejected or superseded"
        raise RuntimeError(message)
    return True


def _legacy_selection_ready(
    app: AppController, epoch: int, sample_id: int, owner: ExistingLegacyRestore, mode: str
) -> bool:
    if owner.receipt is None:
        mask = app.project.pad_stem_intent[sample_id].enabled_mask
        try:
            owner.receipt = app.audio.restore_legacy_stem_output_selection(
                epoch, owner.ticket, mode, mask
            )
        except RuntimeError as error:
            if not resident_stem_admission_pressure(str(error)):
                raise
            return False
    if owner.receipt.status() == "pending":
        return False
    if owner.receipt.status() != "accepted" or not owner.receipt.is_current():
        message = "Existing legacy stem mode or mask has no current callback acknowledgement"
        raise RuntimeError(message)
    return True
