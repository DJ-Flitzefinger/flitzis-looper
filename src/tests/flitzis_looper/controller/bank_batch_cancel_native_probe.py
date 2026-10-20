"""Final owned-batch cancellation under an actual pending Native Bank parent.

The existing SharedJobProbe owns the productive worker, source leases and Bank
pipeline. Only its declared nonneural backend is held. This observer supplies no
ACK, callback authority or substitute publication guard; Rust advances the real
callback. Dynamic model inference, device and hearing acceptance remain outside
this bounded regression.
"""

import json
from typing import TYPE_CHECKING, cast

from flitzis_looper.controller.stem_job import DIRECT_STEM_INTEREST, StemGenerationInterest
from tests.flitzis_looper.controller.bank_pending_shared_job_native_probe import (
    PendingSharedJobProbe,
)

if TYPE_CHECKING:
    from flitzis_looper.controller.stems import StemGenerationCancellation
    from tests.flitzis_looper.controller.bank_shared_job_native_probe import SharedJobProbe


class BatchBankCancelProbe:
    """Observe one final batch token without borrowing the foreign reader's lifetime."""

    def __init__(self, supplied: object) -> None:
        self.probe = cast("SharedJobProbe", supplied)
        probe, stems = self.probe, self.probe.app.stems
        assert probe._job is not None
        self.job = probe._job
        self.subscribers = self.job.subscribers()
        assert tuple(row.sample_id for row in self.subscribers) == (36, 215)
        self.target, self.foreign = self.subscribers
        self.token = StemGenerationInterest("actual-native-bank-confirmed-batch")
        owner = stems.request_generation_interest(36, self.token)
        assert owner is not None
        assert owner is self.target.interest_owner
        assert owner.current == frozenset({DIRECT_STEM_INTEREST, self.token})
        # Joining through the real admission path preserves the exact captured row.
        # Remove only the direct caller before the Bank captures its participant image.
        assert owner.detach(DIRECT_STEM_INTEREST) == "remaining"
        assert owner.current == frozenset({self.token})
        assert self.job.subscribers()[0] is self.target
        assert self.foreign.interest_owner.current == frozenset({DIRECT_STEM_INTEREST})
        self.owner = owner
        self.cancellation: StemGenerationCancellation | None = None
        self.pending: PendingSharedJobProbe | None = None
        self.cancel_event = self.job.cancel_event
        self.source = self.job.source
        self.processing_files = self.job.processing_files
        self.request = probe.backend.requests[0]
        self.private_file = probe.backend.private_file
        assert self.private_file is not None
        stat = self.private_file.stat()
        self.file_identity = (stat.st_dev, stat.st_ino, stat.st_size)
        self._assert_physical_read()

    def _assert_physical_read(self) -> None:
        probe, job = self.probe, self.job
        assert job.running
        assert job.phase == "running"
        assert not probe.backend.returned.is_set()
        assert probe.backend.requests == [self.request]
        assert job.source is self.source
        assert job.processing_files is self.processing_files
        assert self.processing_files is not None
        assert not job._source_lease.released
        assert not job._generation_lease.released
        assert self.private_file is not None
        stat = self.private_file.stat()
        assert (stat.st_dev, stat.st_ino, stat.st_size) == self.file_identity
        assert self.private_file.read_bytes() == probe.backend.wavs[0]
        assert self.request.source_path.read_bytes() == probe.backend.original
        assert job.cancel_event is self.cancel_event
        assert not self.cancel_event.is_set()

    def cancel_while_pending(self) -> None:
        """Cancel only the owned batch after real Bank publication, before its ACK."""
        probe, stems = self.probe, self.probe.app.stems
        assert self.pending is None
        assert self.cancellation is None
        self.pending = PendingSharedJobProbe(probe)
        self.rows = self.job._subscribers
        self.sources = stems._generation_sources
        self.intents = stems._generation_intents
        self.progress = probe.session.stem_generation_progress
        self.target_progress = self.progress.get(36)
        self.cancellation = stems.cancel_generation_interest(self.owner, self.token)
        assert self.cancellation.disposition == "detached"
        self._assert_retained()

    def _assert_retained(self) -> None:
        probe, stems = self.probe, self.probe.app.stems
        assert self.cancellation is not None
        assert not self.owner.current
        assert not self.owner.committed
        assert stems.generation_cancellation_is_pending(self.cancellation)
        assert self.job._subscribers is self.rows
        assert self.job.subscribers() == self.subscribers
        assert all(
            actual is original
            for actual, original in zip(self.job.subscribers(), self.subscribers, strict=True)
        )
        assert stems._generation_sources is self.sources
        assert stems._generation_intents is self.intents
        assert probe.session.stem_generation_progress is self.progress
        assert self.sources[36] is self.target.source
        assert self.intents[36] is self.target.intent
        assert self.progress.get(36) == self.target_progress
        assert 36 in probe.session.stem_generating_sample_ids
        assert self.target.source_lease is not None
        assert not self.target.source_lease.released
        assert self.foreign.source_lease is not None
        assert not self.foreign.source_lease.released
        assert self.foreign.interest_owner.current == frozenset({DIRECT_STEM_INTEREST})
        assert probe.controller.error is None, self.diagnostics()
        assert not probe.session.stem_generation_errors, self.diagnostics()
        self._assert_physical_read()

    def cancellation_retained_before_ack(self) -> bool:
        """Normal observers must retain the original Bank image and cancelled reader row."""
        probe = self.probe
        assert self.pending is not None
        for _ in range(3):
            probe.controller.poll()
            probe.app._sync_arrangement_observers()
            probe.app.stems.on_frame_render()
            probe.assets.retry_retirements()
            self.pending._assert_pending()
            self._assert_retained()
        return True

    def acknowledged_before_mirror(self) -> None:
        """A real callback ACK alone cannot settle a still-captured control participant."""
        assert self.pending is not None
        assert self.pending.ticket.publication_status() == "accepted"
        assert self.pending.ticket.acknowledgement() is not None
        assert self.pending.bank.mirror is None
        assert self.probe.controller.active
        self._assert_retained()

    def settle_cancelled_bank(self) -> None:
        """Use actual durable Bank settlement while the independent shared read continues."""
        assert self.pending is not None
        assert self.cancellation is not None
        self.probe.settle_shared_bank("copy")
        stems = self.probe.app.stems
        assert self.pending.bank.mirror is not None
        assert self.pending.bank.mirror.applied
        assert self.pending.bank.mirror.settled
        assert not stems.generation_cancellation_is_pending(self.cancellation)
        assert not self.owner.current
        assert not self.owner.committed
        assert self.job.subscribers() == (self.foreign,)
        assert self.job.subscribers()[0] is self.foreign
        assert stems._generation_sources[215] is self.foreign.source
        assert stems._generation_intents[215] is self.foreign.intent
        assert self.foreign.source_lease is not None
        assert not self.foreign.source_lease.released
        assert self.target.source_lease is not None
        assert self.target.source_lease.released
        assert 36 not in stems._generation_sources
        assert 36 not in stems._generation_intents
        self._assert_physical_read()

    def completion_ready(self) -> bool:
        """Finish the same foreign follower's verified files, disk commit and own Native ACK."""
        assert self.pending is not None
        assert self.cancellation is not None
        if not self.pending.completion_ready():
            return False
        assert not self.probe.app.stems.generation_cancellation_is_pending(self.cancellation)
        assert not self.owner.current
        assert not self.owner.committed
        return True

    def diagnostics(self) -> str:
        """Expose actual cancellation, fence and physical-reader state on bounded timeout."""
        stems = self.probe.app.stems
        return json.dumps({
            "shared": json.loads(self.probe.diagnostics()),
            "batch_tokens": len(self.owner.current),
            "cancel_pending": (
                stems.generation_cancellation_is_pending(self.cancellation)
                if self.cancellation is not None
                else None
            ),
            "fenced": sorted(stems._pair_deferred_sample_ids),
            "source_lease_released": self.job._source_lease.released,
            "generation_lease_released": self.job._generation_lease.released,
            "private_file_present": self.private_file is not None and self.private_file.exists(),
        })
