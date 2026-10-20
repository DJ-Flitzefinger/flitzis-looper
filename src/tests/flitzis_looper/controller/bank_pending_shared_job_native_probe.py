"""Observe one real shared result crossing an unacknowledged Bank Config writer.

The supplied SharedJobProbe owns all productive controllers and workers. This
observer grants no Native permissions, supplies no ACK and does not catch writer
errors as success. Rust alone advances the actual callback after the retained
completion checkpoint.
"""

import hashlib
import json
from pathlib import Path
from typing import TYPE_CHECKING, cast

from flitzis_looper.controller.bank_arrangement_journal import (
    BankJournalOwner,
    validated_bank_history,
    written_bank_image_matches,
)
from flitzis_looper.copy_snapshot import musical_content_data
from flitzis_looper.models import STEM_KINDS

if TYPE_CHECKING:
    from tests.flitzis_looper.controller.bank_shared_job_native_probe import SharedJobProbe


class PendingSharedJobProbe:
    """Check exact result custody while permitting unrelated follower observations."""

    def __init__(self, supplied: object) -> None:
        self.probe = cast("SharedJobProbe", supplied)
        probe = self.probe
        assert probe.bank is not None
        assert probe.ticket is not None
        assert probe._job is not None
        self.bank = probe.bank
        self.ticket = probe.ticket
        self.job = probe._job
        self.subscribers = self.job.subscribers()
        assert tuple(row.sample_id for row in self.subscribers) == (36, 215)
        assert len(probe.backend.requests) == 1
        self.request = probe.backend.requests[0]
        self.fingerprint = self.job.generation_fingerprint
        self.producer = self.job.producer_identity
        self.participants = {
            slot: musical_content_data(probe.project, slot) for slot in self.bank.participant_ids
        }
        self.source_images = {slot: musical_content_data(probe.project, slot) for slot in range(36)}
        self.private = self.request.cache_dir
        self.published = self.private.with_name(
            self.private.name.replace(".generation-", ".ready-")
        )
        self.digests = {
            kind: hashlib.sha256(encoded).hexdigest()
            for kind, encoded in zip(STEM_KINDS, probe.backend.wavs, strict=True)
        }
        self.retained_route: str | None = None
        self.terminal_event_id: int | None = None
        assert probe.project.pad_stem_mix_mode[36] == "full_mix"
        assert probe.project.pad_stem_mix_mode[215] == "full_mix"
        self._assert_pending()

    def _assert_pending(self) -> None:
        probe, bank, ticket = self.probe, self.bank, self.ticket
        assert probe.controller._bank is bank
        assert probe.bank is bank
        assert bank.ticket is ticket
        assert ticket.publication_status() == "pending"
        assert ticket.acknowledgement() is None
        assert bank.acknowledgement is None
        assert bank.receipt is None
        assert bank.mirror is None
        assert bank.publication is None
        assert bank.journal is not None
        assert bank.journal.journal is not None
        assert bank.journal.journal.phase == "publication_pending"
        assert probe.persistence._migration_owner == bank.owner
        assert not bank.writer_released
        assert probe.persistence.config_path.read_bytes() == probe._before_config
        assert len(probe.receipts) == probe._receipt_start
        assert probe.controller.error is None, self.diagnostics()
        assert not probe.session.stem_generation_errors, self.diagnostics()
        assert probe.controller.observer_fenced_sample_ids == bank.participant_ids
        assert 215 not in bank.participant_ids
        for slot, image in self.participants.items():
            assert musical_content_data(probe.project, slot) == image, slot
        assert probe._job is self.job
        assert probe.backend.requests == [self.request]
        assert self.job.generation_fingerprint == self.fingerprint
        assert self.job.producer_identity == self.producer

    def release_while_pending(self) -> None:
        """Return declared WAVs while the actual Bank parent still lacks its ACK."""
        self._assert_pending()
        assert self.job.running
        assert not self.probe.backend.returned.is_set()
        self.probe.release_backend()

    def completion_retained_before_ack(self) -> bool:
        """Require returned-result custody under the writer, never an unreturned task."""
        probe, stems = self.probe, self.probe.app.stems
        probe.controller.poll()
        probe.app._sync_arrangement_observers()
        stems.on_frame_render()
        probe.assets.retry_retirements()
        self._assert_pending()
        if not probe.backend.returned.is_set() or self.job.running:
            return False
        assert self.job.phase == "settled", self.diagnostics()
        assert stems._jobs.get(self.private) is self.job
        assert not self.job._generation_lease.released
        assert self.job.processing_files is not None
        delivery = stems._pair_held_generation_events.get((self.job, "terminal"))
        if delivery is None:
            return False
        assert delivery.job is self.job
        assert delivery.event.event_type == "success", self.diagnostics()
        assert delivery.event.result is not None
        assert delivery.event.cache_dir == self.private
        if self.terminal_event_id is None:
            self.terminal_event_id = id(delivery.event)
        assert id(delivery.event) == self.terminal_event_id
        retained = {row.sample_id: row for row in delivery.remaining}
        assert retained.get(36) is self.subscribers[0]
        if retained.get(215) is self.subscribers[1]:
            self.retained_route = "original terminal delivery retains both interests"
        else:
            preparation = stems._pair_preparations.pending.get(215)
            settlement = stems._disk_selection_settlements.get(215)
            if settlement is not None:
                preparation = settlement.request
                assert not settlement.lease.released
            elif preparation is None or not any(
                own is preparation for own in stems._pair_preparations._held_results
            ):
                # A queued or still-running Pair worker has not exercised writer pressure.
                return False
            assert preparation is not None
            assert preparation.prepared is not None
            assert preparation.error is None, self.diagnostics()
            assert not preparation.cancelled.is_set()
            assert not preparation.lease.released
            assert preparation.content_id == probe._follower_id
            assert preparation.entry.generation_fingerprint == self.fingerprint
            assert preparation.entry.producer_identity == self.producer
            self.retained_route = "original follower complete Pair retained under writer"
        result_directory = self.published if self.published.exists() else self.private
        self._assert_result_bytes(result_directory)
        assert self.job.subscribers() == self.subscribers
        assert all(
            actual is original
            for actual, original in zip(self.job.subscribers(), self.subscribers, strict=True)
        )
        return True

    def _assert_result_bytes(self, directory: Path) -> None:
        for kind, digest in self.digests.items():
            assert hashlib.sha256((directory / f"{kind}.wav").read_bytes()).hexdigest() == digest

    def settle_bank(self) -> bool:
        """Consume the real parent ACK and validate its current-revision durable image."""
        probe, bank = self.probe, self.bank
        assert self.retained_route is not None
        assert self.ticket.publication_status() == "accepted"
        assert self.ticket.acknowledgement() is not None
        probe.controller.poll()
        assert probe.controller.error is None, self.diagnostics()
        if probe.controller.active:
            return False
        assert probe.controller.status == "completed"
        assert bank.acknowledgement is not None
        receipt = bank.receipt
        assert receipt is not None
        assert probe.receipts[probe._receipt_start :] == [receipt]
        assert probe.receipt_owners[probe._receipt_start :] == [bank.owner]
        assert receipt.revision == probe.persistence.revision
        assert receipt.revision >= probe._before_revision
        written = probe.persistence.config_path.read_bytes()
        assert written == receipt.written_text.encode("utf-8")
        assert hashlib.sha256(written).hexdigest() == receipt.sha256
        assert written_bank_image_matches(
            receipt.projected(), bank.request.bank, image_role="after"
        )
        assert bank.journal is not None
        reopened = BankJournalOwner.open(Path.cwd() / "samples", bank.owner)
        assert reopened.encoded == bank.journal.encoded
        records = validated_bank_history(reopened.encoded, bank.owner)
        assert records[-1].phase == "settled"
        assert records[-1].committed_revision == receipt.revision
        assert records[-1].committed_config_sha256 == receipt.sha256
        assert {record.phase for record in records} == {
            "captured",
            "publication_pending",
            "native_ack_observed",
            "config_committed",
            "settled",
        }
        probe._assert_targets(bank)
        probe._assert_assignments()
        probe._assert_released(bank)
        for slot, image in self.source_images.items():
            assert musical_content_data(probe.project, slot) == image, slot
        fresh, source = probe.project.pad_content[36], probe.project.pad_content[0]
        assert fresh is not None
        assert source is not None
        assert fresh.instance_id != probe._old_target_id
        assert fresh.copied_from == source.instance_id
        assert self.job.subscribers() == (self.subscribers[1],)
        assert self.job.subscribers()[0] is self.subscribers[1]
        assert not self.job.cancel_event.is_set()
        self._assert_no_inherited_result()
        probe.forget_settled()
        return True

    def _assert_no_inherited_result(self) -> None:
        probe = self.probe
        for slot in (0, 36):
            entry = probe.project.stem_cache[slot]
            assert entry is not None
            assert entry.pair is not None
            assert entry.pair.model_dump(mode="json") == probe._baseline_pair
        assert 36 not in probe.app.stems._generation_sources
        assert 36 not in probe.session.stem_generating_sample_ids
        assert 36 not in probe.app.stems._pair_preparations.pending
        assert 36 not in probe.app.stems._pending_stem_publications

    def completion_ready(self) -> bool:
        """Require that same result's original follower, disk selection and own ALL ACK."""
        probe = self.probe
        if not probe.completion_ready("copy"):
            return False
        assert probe._job is self.job
        assert probe.backend.requests == [self.request]
        assert self.job.phase == "disposed"
        assert self.job._generation_lease.released
        assert self.job._source_lease.released
        assert self.job.processing_files is None
        entry = probe.project.stem_cache[215]
        assert entry is not None
        assert entry.generation_fingerprint == self.fingerprint
        assert entry.producer_identity == self.producer
        assert (Path.cwd() / entry.cache_dir).absolute() == self.published.absolute()
        assert not self.private.exists()
        self._assert_result_bytes(self.published)
        self._assert_no_inherited_result()
        assert (self.job, "terminal") not in probe.app.stems._pair_held_generation_events
        return True

    def diagnostics(self) -> str:
        """Expose actual custody and errors for Root's bounded Native execution."""
        stems = self.probe.app.stems
        delivery = stems._pair_held_generation_events.get((self.job, "terminal"))
        return json.dumps({
            "shared": json.loads(self.probe.diagnostics()),
            "writer": self.probe.persistence._migration_owner,
            "revision": self.probe.persistence.revision,
            "captured_revision": self.probe._before_revision,
            "retained_route": self.retained_route,
            "terminal_remaining": [row.sample_id for row in delivery.remaining] if delivery else [],
            "pair_pending": sorted(stems._pair_preparations.pending),
            "returned_pairs_held": [
                row.sample_id for row in stems._pair_preparations._held_results
            ],
            "disk_settlements": sorted(stems._disk_selection_settlements),
            "published": self.published.exists(),
        })
