"""Three actual process lifetimes around the productive Bank startup path.

Rust alone creates sources and advances the callback. Sidecar JSON is assertion
evidence; only actual current Native tickets authorize the recovery transaction.
The one injected failure loses a return AFTER the real atomic Config write.
"""

import hashlib
import json
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch

from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.bank_arrangement_journal import (
    BankArrangementJournal,
    BankJournalOwner,
    project_with_journal_bank,
    validated_bank_history,
    written_bank_image_matches,
)
from flitzis_looper.controller.pad_arrangement import PadArrangementController
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.copy_snapshot import CONTENT_FIELDS, musical_content_data
from flitzis_looper.models import ProjectState, SessionState
from tests.flitzis_looper.controller.bank_native_probe import BankControllerProbe

if TYPE_CHECKING:
    from flitzis_looper.controller.bank_arrangement_startup import BankArrangementStartup
    from flitzis_looper.controller.persistence import ConfigCommitReceipt
    from flitzis_looper_audio import AudioEngine, PadCopyTicket, ProjectAssetLease

_PROOF = Path("bank-recovery-proof.json")
_INTERRUPTION = "BANK_ACTUAL_CONFIG_REPLACED_RETURN_LOST"


def _musical_project(project: ProjectState) -> dict[str, object]:
    """Compare every musical cell, keeping availability separate from saved intent."""
    data = project.model_dump(mode="json", exclude_computed_fields=True)
    content_fields = {field for _, field in CONTENT_FIELDS}
    return {
        "globals": {
            name: value
            for name, value in data.items()
            if name not in content_fields and name != "config_revision"
        },
        "slots": [musical_content_data(project, slot) for slot in range(216)],
    }


def export_pending(probe: BankControllerProbe) -> str:
    """Save exact actual publication-pending evidence without closing its owner."""
    bank, ticket = probe.bank, probe.ticket
    assert bank is not None
    assert ticket is not None
    assert bank.journal is not None
    assert bank.journal.journal is not None
    original = bank.journal.journal
    assert original.phase == "publication_pending"
    assert original.resume_of is None
    assert ticket.publication_status() == "pending"
    assert ticket.acknowledgement() is None
    assert probe.persistence._migration_owner == bank.owner
    assert bank.custody.readers
    assert all(not reader.released for reader in bank.custody.readers)
    assert probe.persistence.config_path.read_bytes() == probe._before_config
    after = project_with_journal_bank(probe.project, original)
    data = original.bank.projected()
    assert len(data.slots) == 36
    assert {row.target_id for row in data.slots if row.after.source_identity} == {36, 71}
    assert {row.target_id for row in data.slots if row.before.source_identity} == {36, 37, 71}
    proof = {
        "initial": json.loads(bank.initial.model_dump_json()),
        "parent_history": bank.journal.encoded,
        "before": _musical_project(probe.project),
        "after": _musical_project(after),
        "config_sha256": hashlib.sha256(probe._before_config).hexdigest(),
    }
    _PROOF.write_text(json.dumps(proof), encoding="utf-8")
    return json.dumps({"owner": bank.owner, "phase": original.phase, "slots": 36})


class _RestoredSeed:
    """Load the actual selected Config without calling the fresh-source constructor."""

    def __init__(self, audio: AudioEngine) -> None:
        self.audio = audio
        path = Path("samples/copy-public.json")
        before = path.read_bytes()
        self.persistence = ProjectPersistence.from_config_path(path)
        assert self.persistence.load_error is None, self.persistence.load_error
        self.persistence.config_path = path
        self.persistence.bind_audio(audio)
        self.project = self.persistence.project
        self.session = SessionState()
        self.assets = ProjectAssetLifecycle(self.project, audio)
        self.assets.sync_assignments()
        self.callbacks: list[int] = []
        self.identities: list[str] = []
        self.lifetimes: list[int] = []
        self.before_callbacks: list[int] = []
        self.arrangement = PadArrangementController(
            self.persistence,
            self.session,
            audio,
            self.assets,
            stem_task_runner=lambda work: work(),
            on_copy_committed=self._committed,
        )
        assert path.read_bytes() == before, "restored constructor must never write fresh Config"

    def _committed(self, _target: int, _ticket: PadCopyTicket) -> None:
        message = "Bank recovery must not admit another single-PAD Copy"
        raise AssertionError(message)

    def close(self) -> None:
        """The receiver owns no extra source/pair fixture holders to retire."""
        assert not self.arrangement.active


class RecoveryProbe(BankControllerProbe):
    """Restore real source/control/selection owners before normal Bank polling."""

    def __init__(self, audio: AudioEngine, stage: str) -> None:
        assert stage in {"resume_before", "recover_after"}
        self.stage = stage
        self.proof = json.loads(_PROOF.read_text(encoding="utf-8"))
        self.original = BankArrangementJournal.model_validate_json(
            json.dumps(self.proof["initial"]), strict=True
        )
        self._lost_return = False
        self._release_observed = False
        self._transfers: list[tuple[str, str]] = []
        self._held_readers: list[ProjectAssetLease] = []
        self._restore_loaded: set[int] = set()
        self._participant_restore_evidence: dict[int, dict[str, object] | None] = {}
        self._outside_restore_evidence: dict[int, dict[str, object]] = {}
        self._loaded_bytes = Path("samples/copy-public.json").read_bytes()
        super().__init__(_RestoredSeed(audio), actual_workers=True)
        assert self.persistence.config_path.read_bytes() == self._loaded_bytes
        self.controller.bind_startup()
        startup = self._startup()
        assert startup.active, startup.error
        assert startup.error is None, startup.error
        assert startup.histories
        assert startup.histories[0].initial.transaction_id == self.original.transaction_id
        assert startup.histories[0].encoded == self.proof["parent_history"]
        assert startup._custody.acquired
        assert startup._custody.readers
        self._held_readers = list(startup._custody.readers)
        assert all(not reader.released for reader in startup._custody.readers)
        self._bind_recovery_observers()
        self.app.loader._bind_restored_assignment_guard(self._assignment_held)
        self.app.stems.bind_restored_assignment_guard(self._assignment_held)
        self.app.loader.set_restored_sample_loaded_callback(self._restored)
        self.app._sync_arrangement_observers()
        self._restore_ids = frozenset(
            slot for slot, path in enumerate(self.project.sample_paths) if path is not None
        )
        assert self._restore_ids == (
            {0, 35, 36, 37, 71, 72} if stage == "resume_before" else {0, 35, 36, 71, 72}
        )
        if stage == "resume_before":
            assert startup.status == "restore_original"
            assert self.persistence._migration_owner == self.original.transaction_id
            self._assert_image("before")
        else:
            assert startup.status == "restore_after"
            assert startup._after is not None
            assert not startup._after.terminal
            assert startup._after.owner.journal is not None
            assert startup._after.owner.journal.phase == "native_ack_observed"
            assert startup._after.owner.initial.resume_of == self.original.transaction_id
            assert startup._after.owner.initial.bank == self.original.bank
            assert startup._after.owner.encoded == self.proof["interrupted_child_history"]
            self._assert_image("after")
        self._before_revision = self.persistence.revision

    def _bind_recovery_observers(self) -> None:
        self._participant_ids = (
            frozenset(range(72)) if self.stage == "resume_before" else frozenset(range(36, 72))
        )
        self._transfer = self.persistence.transfer_migration
        self._transfer_patch = patch.object(
            self.persistence, "transfer_migration", self._transfer_writer
        )
        self._transfer_patch.start()
        self._release = self.persistence.release_migration
        self._release_patch = patch.object(
            self.persistence, "release_migration", self._release_writer
        )
        self._release_patch.start()
        startup = self._startup()
        self._after_intents = startup._after_intents
        self._after_intents_patch = None
        if self.stage == "recover_after":
            # This pure intent builder is evaluated after the real participant
            # witness and immediately before the actual capture_recovered_bank
            # call. Observe unreserved runtime here, then forward its exact data.
            self._after_intents_patch = patch.object(
                type(startup), "_after_intents", staticmethod(self._observe_after_intents)
            )
            self._after_intents_patch.start()

    def _startup(self) -> BankArrangementStartup:
        startup = self.controller._startup
        assert startup is not None
        return startup

    def _assignment_held(self, slot: int) -> bool:
        return slot in self.controller.fenced_sample_ids

    def _restored(self, slot: int) -> bool:
        self._restore_loaded.add(slot)
        return self.app.stems.publish_restored_stem_cache_if_available(slot)

    def start_restore(self) -> None:
        """Use the App ordering: all saved controls precede actual Cold admission."""
        assert self.app.stems._stem_worker_pool is not None
        paths = self.app.loader._validate_restored_sample_paths()
        assert frozenset(paths) == self._restore_ids
        self.app.transport.apply_project_state_to_audio(restored_sample_ids=frozenset(paths))
        self.app.loader.restore_samples_from_project_state(validated_paths=paths)
        self.app.stems.restore_stem_cache_from_project_state()
        assert self.persistence.config_path.read_bytes() == self._loaded_bytes

    def _poll_restore_owners(self) -> None:
        """Collect real background work while Rust keeps the sole callback pump live."""
        self.controller.poll()
        self.app._sync_arrangement_observers()
        self.app.loader.poll_loader_events()
        self.app.stems.on_frame_render()
        assert not self.session.sample_load_errors, self.diagnostics()
        assert not self.tasks, "restoration and Bank preparation use the production worker pool"

    def poll_restore(self) -> bool:
        """One ordinary Bank/observer/Loader/Stem pass, preserving actual worker jobs."""
        self._poll_restore_owners()
        if self.stage == "resume_before":
            bank = self.controller._bank
            if bank is None:
                return False
            self.bank, self.request = bank, bank.request
            assert bank.captured is not None
            assert bank.ticket is None
            assert not bank.published
            assert bank.recovery_handoff is not None
            assert bank.recovery_handoff.transferred
            assert bank.recovery_handoff.delivered
            assert self._transfers == [(self.original.transaction_id, bank.owner)]
            assert bank.initial.resume_of == self.original.transaction_id
            assert bank.initial.bank == self.original.bank
            assert bank.initial.captured_config_sha256 == self.original.captured_config_sha256
            assert (
                bank.initial.captured_project_image_json
                == self.original.captured_project_image_json
            )
            self._assert_sources_restored()
            return True
        recovery = self._startup()._after
        assert recovery is not None
        if recovery.acceptance is None:
            return False
        self._assert_sources_restored()
        return True

    def poll_bank_preparation(self) -> bool:
        """Wait for the same admitted worker, collecting its real publication before ACK."""
        assert self.stage == "resume_before"
        assert self.bank is not None
        assert self.controller._bank is self.bank
        self._poll_restore_owners()
        bank = self.bank
        assert self.controller._bank is bank
        if bank.ticket is None:
            return False
        assert bank.worker_done
        assert bank.published
        self.ticket = bank.ticket
        self.assert_waiting()
        return True

    def _transfer_writer(self, previous: str, owner: str) -> tuple[int, ProjectState, str | None]:
        durable = BankJournalOwner.open(self._startup().samples, owner)
        assert durable.journal == durable.initial
        assert durable.initial.phase == "captured"
        assert durable.initial.resume_of == previous == self.original.transaction_id
        assert durable.initial.bank == self.original.bank
        assert self.persistence._migration_owner == previous
        assert self.persistence.config_path.read_bytes() == self._loaded_bytes
        assert self.controller._bank is None
        self._observe_unreserved_participants()
        self._transfers.append((previous, owner))
        return self._transfer(previous, owner)

    def _observe_after_intents(self, record: BankArrangementJournal) -> list[str | None]:
        recovery = self._startup()._after
        assert recovery is not None
        assert recovery.acceptance is None
        assert recovery.owner.journal == record
        assert record.bank == self.original.bank
        self._observe_unreserved_participants()
        return self._after_intents(record)

    def poll_outside_restore(self) -> bool:
        """Restore outside owners independently, without repolling the accepted Bank."""
        self.app._sync_arrangement_observers()
        self.app.loader.poll_loader_events()
        self.app.stems.on_frame_render()
        assert not self.session.sample_load_errors, self.diagnostics()
        outside = self._restore_ids - self._participant_ids
        for slot in outside - self._outside_restore_evidence.keys():
            if slot in self.session.loading_sample_ids or slot not in self._restore_loaded:
                continue
            if not self._selection_ready(slot):
                continue
            self._outside_restore_evidence[slot] = self._unreserved_source_evidence(slot)
        return self._outside_restore_evidence.keys() == outside

    def _release_writer(self, owner: str) -> None:
        startup = self._startup()
        assert self.stage == "recover_after"
        assert startup._after is not None
        terminal = startup._after.owner
        assert terminal.journal is not None
        assert terminal.journal.phase == "settled"
        assert terminal.initial.transaction_id == owner
        assert not self._ancestor_path().exists(), "ancestor must compact before writer release"
        assert not startup._custody.readers
        assert all(reader.released for reader in self._held_readers)
        assert startup._retirement is not None
        assert startup._retirement.remaining == 0
        self._release(owner)
        self._release_observed = True

    def _record_receipt(
        self, owner: str, revision: int, project: ProjectState
    ) -> ConfigCommitReceipt:
        receipt = super()._record_receipt(owner, revision, project)
        assert (
            receipt.sha256 == hashlib.sha256(self.persistence.config_path.read_bytes()).hexdigest()
        )
        if self.stage == "resume_before" and not self._lost_return:
            self._lost_return = True
            assert written_bank_image_matches(
                receipt.projected(), self.original.bank, image_role="after"
            )
            self._assert_image("before")
            raise OSError(_INTERRUPTION)
        return receipt

    def assert_waiting(self) -> None:
        """Keep the resumed transaction pending until the real callback runs."""
        assert self.bank is not None
        assert self.bank.journal is not None
        assert self.ticket is not None
        assert self.ticket.publication_status() == "pending"
        assert self.ticket.acknowledgement() is None
        assert self.bank.journal.journal is not None
        assert self.bank.journal.journal.phase == "publication_pending"
        assert not self.receipts
        assert self.persistence.config_path.read_bytes() == self._loaded_bytes
        self._assert_image("before")

    def interrupt_after_actual_write(self) -> str:
        """Lose exactly one real receipt return, then leave all original owners live."""
        assert self.bank is not None
        assert self.ticket is not None
        assert self.ticket.acknowledgement() is not None
        assert self._outside_restore_evidence.keys() == self._restore_ids - self._participant_ids
        self.controller.poll()
        assert self._lost_return
        assert self.controller.status == "settlement_pending"
        assert self.controller.error == _INTERRUPTION
        assert len(self.receipts) == 1
        receipt = self.receipts[0]
        bank = self.bank
        assert bank.acknowledgement is not None
        assert bank.receipt is None
        assert not bank.native_finished
        assert not bank.writer_released
        assert bank.mirror is not None
        assert not bank.mirror.applied
        assert bank.publication is not None
        assert not bank.publication.applied
        assert bank.journal is not None
        assert bank.journal.journal is not None
        record = bank.journal.journal
        assert record.phase == "native_ack_observed"
        assert record.image_role == "before"
        assert record.committed_revision is None
        assert record.committed_config_sha256 is None
        assert self.persistence._migration_owner == bank.owner
        assert bank.custody.readers
        assert all(not reader.released for reader in bank.custody.readers)
        assert self._ancestor_path().exists()
        self._assert_image("before")
        assert _musical_project(receipt.projected()) == self.proof["after"]
        assert receipt.written_text.encode("utf-8") == self.persistence.config_path.read_bytes()
        assert receipt.revision == self.persistence.revision
        self.proof["interrupted_child_history"] = bank.journal.encoded
        self.proof["interrupted_owner"] = bank.owner
        self.proof["lost_receipt"] = {
            "revision": receipt.revision,
            "sha256": receipt.sha256,
            "restored_before_reservation": self._participant_restore_evidence,
            "outside_restore": self._outside_restore_evidence,
            "written_text": receipt.written_text,
        }
        _PROOF.write_text(json.dumps(self.proof), encoding="utf-8")
        return json.dumps({
            "owner": bank.owner,
            "phase": record.phase,
            "lost_return": _INTERRUPTION,
            "revision": receipt.revision,
            "sha256": receipt.sha256,
        })

    def assert_fresh_probe_pending(self) -> str:
        """Disk after evidence alone cannot finish, write or compact the fixed lineage."""
        startup = self._startup()
        recovery = startup._after
        assert recovery is not None
        assert recovery.acceptance is not None
        assert not self.receipts
        assert recovery.receipt is None
        assert self.persistence._migration_owner == self.proof["interrupted_owner"]
        assert self.persistence.config_path.read_bytes() == self._loaded_bytes
        assert startup.active
        assert not startup._compacted
        assert startup._custody.readers
        assert all(not reader.released for reader in startup._custody.readers)
        assert self._ancestor_path().exists()
        for owner, expected in zip(
            startup.histories,
            [self.proof["parent_history"], self.proof["interrupted_child_history"]],
            strict=True,
        ):
            assert owner.store.history() == owner.encoded == expected
        metadata = json.loads(recovery.acceptance.metadata_json())
        assert metadata["status"] == "pending", self.diagnostics()
        assert not self.audio.validate_recovered_bank(recovery.acceptance)
        self._assert_image("after")
        return json.dumps(metadata)

    def settle_recovered(self) -> str:
        """Require that fresh all36 callback receipt before current-revision settlement."""
        startup = self._startup()
        recovery = startup._after
        assert recovery is not None
        assert recovery.acceptance is not None
        acceptance = recovery.acceptance
        metadata = json.loads(acceptance.metadata_json())
        assert metadata["status"] == "accepted", self.diagnostics()
        assert self.audio.validate_recovered_bank(recovery.acceptance)
        assert self._outside_restore_evidence.keys() == self._restore_ids - self._participant_ids
        self.controller.poll()
        assert not self.controller.active, self.diagnostics()
        assert startup.status == "completed", self.diagnostics()
        assert self.controller.error is None
        assert self._release_observed
        assert self.persistence._migration_owner is None
        assert len(self.receipts) == 1
        receipt = self.receipts[0]
        assert receipt.revision == self.persistence.revision
        assert receipt.written_text.encode("utf-8") == self.persistence.config_path.read_bytes()
        assert (
            receipt.sha256 == hashlib.sha256(self.persistence.config_path.read_bytes()).hexdigest()
        )
        assert recovery.receipt is receipt
        assert json.loads(acceptance.metadata_json())["status"] == "released"
        assert not self.audio.validate_recovered_bank(acceptance)
        assert recovery.acceptance is None
        assert not self._ancestor_path().exists()
        owner = BankJournalOwner.open(startup.samples, self.proof["interrupted_owner"])
        history = validated_bank_history(owner.encoded, owner.initial.transaction_id)
        assert history[-1].phase == "settled"
        assert history[-1].committed_revision == receipt.revision
        assert history[-1].committed_config_sha256 == receipt.sha256
        assert history[-1].bank == self.original.bank
        assert history[-1].resume_of == self.original.transaction_id
        assert not startup._custody.readers
        assert all(reader.released for reader in self._held_readers)
        self.app._sync_arrangement_observers()
        assert not self.controller.fenced_sample_ids
        assert not self.controller.observer_fenced_sample_ids
        self._assert_image("after")
        assert _musical_project(receipt.projected()) == self.proof["after"]
        return json.dumps({
            "owner": owner.initial.transaction_id,
            "phase": history[-1].phase,
            "revision": receipt.revision,
            "sha256": receipt.sha256,
            "fresh_native": metadata,
            "original_compacted": True,
            "fixed_bank": json.loads(self.original.bank.payload_json),
            "restored_before_reservation": self._participant_restore_evidence,
            "outside_restore": self._outside_restore_evidence,
        })

    def _assert_sources_restored(self) -> None:
        """Check retained scalar observations; current Native tickets still own all effects."""
        assert not self._participant_ids.intersection(self.session.loading_sample_ids)
        assert not self._participant_ids.intersection(self.session.analyzing_sample_ids)
        assert self._restore_ids.intersection(self._participant_ids) <= self._restore_loaded
        assert self._participant_restore_evidence.keys() == self._participant_ids
        for slot, observed in self._participant_restore_evidence.items():
            assert (observed is not None) == (slot in self._restore_ids), slot

    def _observe_unreserved_participants(self) -> None:
        """Read real restored occupied/Empty state once before the original reservation."""
        assert not self._participant_restore_evidence
        for slot in self._participant_ids:
            if slot in self._restore_ids:
                assert slot in self._restore_loaded
                assert slot not in self.session.loading_sample_ids
                assert self._selection_ready(slot), (slot, self.diagnostics())
                observed = self._unreserved_source_evidence(slot)
            else:
                assert self.audio.current_input_runtime_pad_binding(slot) is None, slot
                observed = None
            self._participant_restore_evidence[slot] = observed

    def _selection_ready(self, slot: int) -> bool:
        entry = self.project.stem_cache[slot]
        assert entry is not None
        assert entry.pair is not None
        return self.app.stems._committed_residency.recovered_selection_ready(
            slot,
            entry,
            mode=self.project.pad_stem_mix_mode[slot],
            enabled_mask=self.project.pad_stem_intent[slot].enabled_mask,
            preload=self.project.preload_stem_loops_on_startup,
        )

    def _unreserved_source_evidence(self, slot: int) -> dict[str, object]:
        binding = self.audio.current_input_runtime_pad_binding(slot)
        assert binding is not None, slot
        assert binding.is_current(), slot
        assert self.audio.pad_timing_intent(slot) == "tap"
        manifest_json = self.audio.cold_source_manifest(slot)
        assert manifest_json is not None, slot
        manifest = json.loads(manifest_json)
        assert manifest["integrity"]["warm"]
        for name in (
            "source_copied_bytes",
            "original_copied_bytes",
            "assignment_copy_bytes",
            "stream_scratch_reserved_bytes",
        ):
            assert manifest["integrity"][name] == 0, (slot, name, manifest)
        return {"binding": binding.metadata(), "cold": manifest}

    def _assert_image(self, role: str) -> None:
        assert _musical_project(self.project) == self.proof[role], (role, self.diagnostics())
        for row in self.original.bank.projected().slots:
            expected = row.before if role == "before" else row.after
            assert musical_content_data(self.project, row.target_id) == expected.model_dump(
                mode="json"
            )

    def _ancestor_path(self) -> Path:
        return Path("samples/.material-migrations") / self.original.transaction_id

    def diagnostics(self) -> str:
        """Report actual stages and per-target controls if the fresh receipt cannot settle."""
        startup = self.controller._startup
        recovery = startup._after if startup else None
        return json.dumps({
            "stage": self.stage,
            "status": self.controller.status,
            "error": self.controller.error,
            "startup_status": startup.status if startup else None,
            "startup_error": startup.error if startup else None,
            "loading": sorted(self.session.loading_sample_ids),
            "restored": sorted(self._restore_loaded),
            "load_errors": self.session.sample_load_errors,
            "stem_errors": self.session.stem_generation_errors,
            "tasks": len(self.tasks),
            "receipt_count": len(self.receipts),
            "observed_unreserved": sorted(self._participant_restore_evidence),
            "outside_restored": sorted(self._outside_restore_evidence),
            "fresh_native": json.loads(recovery.acceptance.metadata_json())
            if recovery and recovery.acceptance
            else None,
        })

    def close(self) -> None:
        """Only the terminal receiver shuts down ordinary workers and test hooks."""
        assert self.stage == "recover_after"
        assert not self.controller.active
        self._transfer_patch.stop()
        self._release_patch.stop()
        if self._after_intents_patch is not None:
            self._after_intents_patch.stop()
        super().close()
