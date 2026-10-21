"""Real Bank/Copy controllers around the enclosing Rust test's existing engine.

Only the App envelope and explicit control-thread staging are test fixtures.
Every source, preparation, ticket, acknowledgement, Config receipt, journal,
mirror and asset lease comes from the productive path. Rust alone advances the
callback and checks audio. This does not exercise App/UI or startup recovery.
"""

import hashlib
import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, Protocol, cast
from unittest.mock import patch

from flitzis_looper.controller import app as app_module
from flitzis_looper.controller.accepted_publication import AcceptedTimingController
from flitzis_looper.controller.bank_arrangement import BankArrangementController
from flitzis_looper.controller.bank_arrangement_journal import (
    BankJournalOwner,
    validated_bank_history,
    written_bank_image_matches,
)
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.pair_arrangement import PairArrangementController
from flitzis_looper.controller.persistence import PersistenceFenceError
from flitzis_looper.controller.stems import StemController
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.copy_snapshot import (
    CONTENT_FIELDS,
    copy_after_snapshot,
    musical_content_data,
    musical_snapshot_projection,
)
from flitzis_looper.key_intent import PadKeyIntent
from flitzis_looper.ui.context import WaveformEditorActions

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
    from flitzis_looper.controller.bank_arrangement import BankRequest, _Bank
    from flitzis_looper.controller.pad_arrangement import PadArrangementController, _Copy
    from flitzis_looper.controller.persistence import ConfigCommitReceipt, ProjectPersistence
    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import (
        AudioEngine,
        BankMutationTicket,
        CommittedStemSetTicket,
        InstrumentalStemReader,
        PadCopyTicket,
    )


class _PublicSeed(Protocol):
    """The existing public_complete_probe.Probe, supplied as one actual object."""

    audio: AudioEngine
    project: ProjectState
    session: SessionState
    persistence: ProjectPersistence
    assets: ProjectAssetLifecycle
    arrangement: PadArrangementController
    callbacks: list[int]
    identities: list[str]
    lifetimes: list[int]
    before_callbacks: list[int]

    def begin(self, target: int) -> None: ...

    def close(self) -> None: ...

    def _committed(self, target: int, ticket: PadCopyTicket) -> None: ...


@dataclass(slots=True)
class _AppEnvelope:
    """Share the seed's owners; do not create a second engine or persistence path."""

    _project: ProjectState
    _session: SessionState
    _audio: AudioEngine
    _assets: ProjectAssetLifecycle
    project: ProjectState
    session: SessionState
    persistence: ProjectPersistence
    transport: TransportController
    loader: LoaderController
    accepted_timing: AcceptedTimingController
    metering: MeteringController
    stems: StemController
    arrangement: PadArrangementController
    pairs: PairArrangementController | None = None
    banks: BankArrangementController | None = None
    _pair_waveform_views: list[WaveformEditorActions] = field(default_factory=list)

    def _sync_arrangement_observers(self) -> None:
        deferred = self.arrangement.observer_fenced_sample_ids
        if self.pairs is not None:
            deferred |= self.pairs.observer_fenced_sample_ids
        if self.banks is not None:
            deferred |= self.banks.observer_fenced_sample_ids
        for component in (self.transport, self.loader, self.accepted_timing, self.stems):
            component._pair_deferred_sample_ids = deferred


def _envelope(
    seed: _PublicSeed, runner: Callable[[Callable[[], None]], None] | None
) -> _AppEnvelope:
    transport = TransportController(
        seed.project, seed.session, seed.audio, seed.persistence.mark_dirty
    )
    loader = LoaderController(
        seed.project,
        seed.session,
        seed.audio,
        lambda _slot: None,
        on_project_changed=seed.persistence.mark_dirty,
    )
    loader.bind_asset_lifecycle(seed.assets)
    app = _AppEnvelope(
        _project=seed.project,
        _session=seed.session,
        _audio=seed.audio,
        _assets=seed.assets,
        project=seed.project,
        session=seed.session,
        persistence=seed.persistence,
        transport=transport,
        loader=loader,
        accepted_timing=AcceptedTimingController(transport),
        metering=MeteringController(seed.project, seed.session, seed.audio),
        stems=StemController(
            seed.project,
            seed.session,
            seed.audio,
            stem_task_runner=runner,
            asset_lifecycle=seed.assets,
        ),
        arrangement=seed.arrangement,
    )
    app.pairs = PairArrangementController(cast("AppController", app))
    assert not app.pairs.active
    view = WaveformEditorActions(cast("AppController", app))
    assert app._pair_waveform_views == [view]
    return app


class BankControllerProbe:
    """Expose real control stages without simulating audio or Native acceptance."""

    def __init__(self, seed: object, *, actual_workers: bool = False) -> None:
        self.seed = cast("_PublicSeed", seed)
        self.audio = self.seed.audio
        self.project = self.seed.project
        self.session = self.seed.session
        self.persistence = self.seed.persistence
        self.assets = self.seed.assets
        self.tasks: list[Callable[[], None]] = []
        self.receipts: list[ConfigCommitReceipt] = []
        self.receipt_owners: list[str] = []
        self._commit = self.persistence.commit_migration_receipt
        self._receipt_patch = patch.object(
            self.persistence, "commit_migration_receipt", self._record_receipt
        )
        self._receipt_patch.start()
        self.app = _envelope(self.seed, None if actual_workers else self.tasks.append)
        self.controller = BankArrangementController(cast("AppController", self.app))
        self.app.banks = self.controller
        self.app._sync_arrangement_observers()
        self.request: BankRequest | None = None
        self.bank: _Bank | None = None
        self.ticket: BankMutationTicket | None = None
        self._seed_copy: _Copy | None = None
        self._seed_registration: CommittedStemSetTicket | None = None
        self._seed_callback = self.seed._committed
        self.seed.arrangement._on_copy_committed = self._copied
        self.reader: InstrumentalStemReader | None = None
        self._reader_geometry: tuple[int, int, int, int] | None = None
        self._reader_digest: str | None = None
        self._receipt_start = 0
        self._before_revision = self.persistence.revision
        self._before_config = b""
        self._before_project: dict[str, object] = {}
        self._before_session: dict[str, object] = {}
        self._before_assignments = self.assets._assignments.copy()

    def _record_receipt(
        self, owner: str, revision: int, project: ProjectState
    ) -> ConfigCommitReceipt:
        receipt = self._commit(owner, revision, project)
        self.receipts.append(receipt)
        self.receipt_owners.append(owner)
        return receipt

    def prepare_full_mix(self, *, key_lock: bool = False, speed: float = 1.0) -> None:
        """Align the initial source with real default pitch and an audible mix.

        Rust drains these actual control commands and verifies the accepted rate,
        Key Lock and positive energy before taking any audio baseline. Decorated
        piano intent needs its own real pitch admission and is outside this probe.
        The enclosing headless Rust fixture enqueues the existing Key Lock and
        FullMix setup commands; those stream-bound public setters are excluded.
        """
        assert not self.controller.active
        assert not self.seed.arrangement.active
        self.project.pad_key_intent[0] = PadKeyIntent()
        self.project.key_lock = key_lock
        self.project.pad_key_lock[0] = key_lock
        self.project.speed = speed
        self.audio.set_pad_key_lock(0, key_lock)
        self.audio.set_speed(speed)
        entry = self.project.stem_cache[0]
        assert entry is not None
        self.project.pad_stem_mix_mode[0] = "full_mix"
        self.persistence.mark_dirty()
        self.persistence.flush()

    def _copied(self, target: int, ticket: PadCopyTicket) -> None:
        self._seed_callback(target, ticket)
        self._seed_registration = ticket.committed_stem_ticket()
        self.app.stems.project_copied_native_state(
            target,
            components_available=app_module._copied_components_available(target, ticket),
            selection=self._seed_registration,
        )

    def seed_begin(self, target: int) -> PadCopyTicket:
        """Admit the existing genuine complete Copy and return its pending ticket."""
        assert not self.controller.active
        assert self._seed_copy is None
        self.seed.begin(target)
        copy = self.seed.arrangement._copy
        assert copy is not None
        assert copy.ticket is not None
        assert copy.ticket.publication_status() in {"pending", "adopting"}
        self._seed_copy = copy
        return copy.ticket

    def seed_complete(self, target: int) -> None:
        """Settle that Copy after Rust's callback, adopting its own real selection."""
        copy = self._seed_copy
        assert copy is not None
        assert copy.target_id == target
        assert copy.ticket is not None
        self.seed.arrangement.poll()
        assert self.seed.arrangement.status == "completed", self.seed.arrangement.error
        assert not self.seed.arrangement.active
        assert self.seed.callbacks == [*self.seed.before_callbacks, target]
        assert copy.native_finished
        assert copy.writer_released
        assert copy.predecessor is None
        assert copy.journal is None
        assert copy.receipt is None
        assert copy.config_committed
        assert copy.committed_revision == self.persistence.revision
        written = json.loads(self.persistence.config_path.read_bytes())
        assert written["config_revision"] == copy.committed_revision
        expected = copy_after_snapshot(copy.snapshot, copy.identity).projected()
        assert musical_content_data(self.project, target) == musical_snapshot_projection(
            expected, mode="json"
        )
        binding = self.audio.current_input_runtime_pad_binding(target)
        assert binding is not None
        assert binding.is_current()
        self._assert_selection(target, self._seed_registration)
        selection = self.app.stems._committed_residency._selections.get(target)
        assert selection is not None
        assert selection.ticket is self._seed_registration
        self._assert_assignments()
        self._seed_copy = None
        self._seed_registration = None

    def capture(self, source_bank: int | None, target_bank: int) -> None:
        """Keep the original Native witness and fixed musical seal through begin."""
        assert not self.controller.active
        assert not self.tasks
        assert self._seed_copy is None
        self.forget_settled()
        self._seed_session_observations()
        self._receipt_start = len(self.receipts)
        self._before_revision = self.persistence.revision
        self._before_config = self.persistence.config_path.read_bytes()
        self._before_project = self.project.model_dump(mode="json", exclude_computed_fields=True)
        self._before_session = self.session.model_dump(mode="python")
        self._before_assignments = self.assets._assignments.copy()
        self.request = self.controller.capture_request(source_bank, target_bank)
        assert self.controller._request is self.request
        assert self.persistence._migration_owner is None
        assert self.persistence.revision == self._before_revision

    def _seed_session_observations(self) -> None:
        """Seed GUI observations; these grant no audio or callback authority."""
        for slot, content in enumerate(self.project.pad_content):
            if content is None:
                continue
            self.session.pressed_pads[slot] = True
            self.session.pad_peak[slot] = 0.75
            self.session.pad_peak_updated_at[slot] = 17.0
            self.session.pad_clip_hold_until[slot] = 19.0
            self.session.pad_playhead_s[slot] = 0.125
            self.session.pad_playhead_updated_at[slot] = 23.0

    def begin(self) -> None:
        """Forward the exact capture; retain a rejected original owner for cleanup."""
        assert self.request is not None
        assert self.bank is None
        assert self.controller.begin(self.request), self.controller.error
        self.bank = self.controller._bank
        assert self.bank is not None
        assert self.bank.request is self.request
        assert self.controller._request is None
        assert self.persistence._migration_owner == self.bank.owner
        assert self.bank.initial.bank == self.request.bank

    def run_worker(self) -> None:
        """Run only the actual preparation closure admitted by this Bank owner."""
        assert self.bank is not None
        assert self.bank.captured is not None
        assert not self.bank.worker_done
        assert len(self.tasks) == 1
        self.tasks.pop(0)()
        assert not self.tasks
        assert self.bank.ticket is None

    def publish(self) -> BankMutationTicket:
        """Collect the genuine worker result and publish through the controller."""
        assert self.bank is not None
        self.controller.poll()
        assert self.bank.worker_done
        self.ticket = self.bank.ticket
        assert self.ticket is not None, self.controller.error
        assert self.bank.published
        assert self.ticket.publication_status() in {"pending", "adopting"}
        self.assert_waiting()
        return self.ticket

    def assert_waiting(self) -> None:
        """Repeated normal polls cannot manufacture an ACK or prepublish mirrors."""
        bank, ticket = self.bank, self.ticket
        assert bank is not None
        assert ticket is not None
        for _ in range(3):
            self.controller.poll()
            assert self.controller._bank is bank
            assert bank.ticket is ticket
            assert ticket.acknowledgement() is None
            assert bank.acknowledgement is None
            assert bank.receipt is None
            assert bank.mirror is None
            assert bank.publication is None
            self._assert_unchanged()
        assert bank.journal is not None
        assert bank.journal.journal is not None
        assert bank.journal.journal.phase == "publication_pending"
        try:
            self.persistence.flush()
        except PersistenceFenceError:
            pass
        else:
            message = "Pending actual Bank parent did not fence the Config writer"
            raise AssertionError(message)

    def _assert_unchanged(self) -> None:
        assert self.persistence.config_path.read_bytes() == self._before_config
        assert self.persistence.revision == self._before_revision
        assert len(self.receipts) == self._receipt_start
        assert (
            self.project.model_dump(mode="json", exclude_computed_fields=True)
            == self._before_project
        )
        assert self.session.model_dump(mode="python") == self._before_session
        assert self.assets._assignments.keys() == self._before_assignments.keys()
        assert all(
            self.assets._assignments[key] is row for key, row in self._before_assignments.items()
        )

    def settle(self) -> None:
        """Consume the actual own ACK after Rust advances the real callback."""
        bank, ticket = self.bank, self.ticket
        assert bank is not None
        assert ticket is not None
        assert ticket.acknowledgement() is not None
        self.controller.poll()
        assert self.controller.status == "completed", self.controller.error
        assert not self.controller.active
        assert bank.acknowledgement is not None
        assert bank.ticket is ticket
        self._assert_durable(bank)
        self._assert_targets(bank)
        self._assert_preserved(bank)
        self._assert_assignments()
        self._assert_released(bank)

    def _assert_durable(self, bank: _Bank) -> None:
        receipt = bank.receipt
        assert receipt is not None
        assert self.receipts[self._receipt_start :] == [receipt]
        assert self.receipt_owners[self._receipt_start :] == [bank.owner]
        assert receipt.revision == self._before_revision
        assert receipt.revision == self.persistence.revision
        written = self.persistence.config_path.read_bytes()
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

    def _assert_selection(self, slot: int, ticket: CommittedStemSetTicket | None) -> None:
        entry = self.project.stem_cache[slot]
        selection = self.app.stems._committed_residency._selections.get(slot)
        if entry is None or entry.pair is None:
            assert ticket is None
            assert selection is None
            return
        assert selection is not None
        assert ticket is not None
        # The public Native method returns cloned opaque wrappers. Their shared
        # owner is checked by Native; Python wrapper identity is not that owner.
        assert ticket.is_current()
        assert ticket.publication_status() == "accepted"
        assert selection.ticket.is_current()
        assert selection.ticket.publication_status() == "accepted"
        assert selection.entry == entry

    def _assert_targets(self, bank: _Bank) -> None:
        assert bank.ticket is not None
        assert bank.mirror is not None
        data = bank.request.bank.projected()
        metadata = json.loads(bank.ticket.metadata_json())
        assert metadata["status"] == "accepted"
        assert metadata["control_mirrored"] is True
        registrations = bank.ticket.committed_stem_tickets()
        assert len(metadata["targets"]) == len(registrations) == 36
        for row, observation, registration in zip(
            data.slots, metadata["targets"], registrations, strict=True
        ):
            slot = row.target_id
            assert observation["slot"] == slot
            assert type(observation["components_available"]) is bool
            assert musical_content_data(self.project, slot) == musical_snapshot_projection(
                row.after, mode="json"
            )
            entry = self.project.stem_cache[slot]
            assert (entry.available if entry is not None else False) == observation[
                "components_available"
            ]
            binding = self.audio.current_input_runtime_pad_binding(slot)
            if row.after.source_identity is None:
                assert binding is None
            else:
                assert binding is not None
                assert binding.is_current()
            self._assert_selection(slot, registration)
            assert self.app.stems._committed_residency._selections.get(slot) is (
                bank.mirror._selections.get(slot)
            )
        self._assert_stopped(bank.target_ids)

    def _assert_stopped(self, targets: frozenset[int]) -> None:
        for name in (
            "active_sample_ids",
            "paused_sample_ids",
            "global_stop_restore_sample_ids",
            "loading_sample_ids",
            "analyzing_sample_ids",
            "stem_generating_sample_ids",
        ):
            assert not targets.intersection(getattr(self.session, name)), name
        assert all(self.session.pressed_pads[slot] is False for slot in targets)
        for name, default in (
            ("pad_peak", 0.0),
            ("pad_peak_updated_at", 0.0),
            ("pad_clip_hold_until", 0.0),
            ("pad_playhead_s", None),
            ("pad_playhead_updated_at", 0.0),
        ):
            table = getattr(self.session, name)
            assert all(table[slot] == default for slot in targets), name
        for name in ("pending_sample_paths", "sample_load_errors"):
            table = getattr(self.session, name)
            assert all(slot not in table for slot in targets), name

    def _assert_preserved(self, bank: _Bank) -> None:
        current = self.project.model_dump(mode="json", exclude_computed_fields=True)
        content_fields = {field for _, field in CONTENT_FIELDS}
        for name, previous in self._before_project.items():
            if name in content_fields:
                before_rows = cast("list[object]", previous)
                after_rows = cast("list[object]", current[name])
                for slot, before in enumerate(before_rows):
                    if slot not in bank.target_ids:
                        assert after_rows[slot] == before, (name, slot)
            else:
                assert current[name] == previous, name
        for key, previous in self._before_assignments.items():
            if key[1] not in bank.target_ids:
                assert self.assets._assignments[key] is previous
        for name in ("active_sample_ids", "paused_sample_ids", "global_stop_restore_sample_ids"):
            before = cast("set[int]", self._before_session[name])
            assert getattr(self.session, name) - bank.target_ids == before - bank.target_ids
        for name in (
            "pressed_pads",
            "pad_peak",
            "pad_peak_updated_at",
            "pad_clip_hold_until",
            "pad_playhead_s",
            "pad_playhead_updated_at",
        ):
            before_rows = cast("list[object]", self._before_session[name])
            after_rows = getattr(self.session, name)
            assert len(after_rows) == len(before_rows) == 216
            assert all(
                after_rows[slot] == previous
                for slot, previous in enumerate(before_rows)
                if slot not in bank.target_ids
            ), name

    def _assert_assignments(self) -> None:
        expected = self.assets._current_assignments()
        assert self.assets._assignments.keys() == expected.keys()
        for key, (path, lease) in self.assets._assignments.items():
            assert path == expected[key]
            assert not lease.released

    def _assert_released(self, bank: _Bank) -> None:
        assert bank.mirror is not None
        assert bank.mirror.applied
        assert bank.mirror.settled
        assert bank.publication is not None
        assert bank.publication.applied
        assert bank.publication.settled
        assert bank.native_finished
        assert bank.writer_released
        assert not bank.images
        assert not bank.custody.readers
        assert bank.retirement is not None
        assert bank.retirement.remaining == 0
        assert self.persistence._migration_owner is None
        assert not self.controller.observer_fenced_sample_ids
        for component in (
            self.app.transport,
            self.app.loader,
            self.app.accepted_timing,
            self.app.stems,
        ):
            assert not component._pair_deferred_sample_ids

    def assert_rejected(self) -> str:
        """Prove a stale original witness cancels without preparation or publication."""
        bank = self.bank
        assert bank is not None
        assert bank.captured is None
        assert bank.ticket is None
        assert bank.cancel_requested
        assert not self.tasks
        reason = self.controller.error
        assert reason
        self.controller.poll()
        assert not self.controller.active
        assert self.controller.status == "cancelled"
        assert bank.cancellation_proven
        assert bank.writer_released
        assert not bank.native_finished
        assert self.persistence._migration_owner is None
        assert bank.journal is not None
        assert bank.journal.journal is not None
        assert bank.journal.journal.phase == "cancelled"
        assert not bank.custody.readers
        self._assert_unchanged()
        return reason

    def settlement_json(self) -> str:
        """Return bounded actual receipt/terminal evidence for enclosing Rust assertions."""
        bank = self.bank
        assert bank is not None
        receipt = bank.receipt
        phase = bank.journal.journal.phase if bank.journal and bank.journal.journal else None
        publication = bank.publication
        return json.dumps({
            "commit_count": len(self.receipts) - self._receipt_start,
            "before_revision": self._before_revision,
            "current_revision": self.persistence.revision,
            "revision": receipt.revision if receipt is not None else None,
            "sha256": receipt.sha256 if receipt is not None else None,
            "config_sha256": hashlib.sha256(self.persistence.config_path.read_bytes()).hexdigest(),
            "owner": bank.owner,
            "receipt_owner": self.receipt_owners[-1] if receipt is not None else None,
            "phase": phase,
            "mirror_applied": bank.mirror is not None and bank.mirror.applied,
            "publication_applied": publication is not None and publication.applied,
            "publication_settled": publication is not None and publication.settled,
            "native_finished": bank.native_finished,
            "writer_released": bank.writer_released,
            "completed": self.controller.status == "completed" and not self.controller.active,
        })

    def forget_settled(self) -> None:
        """Drop only finished test holders; real runtime and asset owners remain intact."""
        assert not self.controller.active
        assert self._seed_copy is None
        assert not self.tasks
        self.bank = None
        self.request = None
        self.ticket = None

    def hold_reader(self, slot: int) -> None:
        """Retain an actual instrumental reader, independently of slot placement."""
        assert self.reader is None
        entry = self.project.stem_cache[slot]
        assert entry is not None
        assert entry.pair is not None
        self.reader = self.audio.open_instrumental_stem_reader(
            slot, entry.source_version, entry.pair.descriptor_reference
        )
        self._reader_geometry = self.reader.geometry()
        payload = self.reader.read_f32le()
        _, frames, _, channels = self._reader_geometry
        assert len(payload) == frames * channels * 4
        self._reader_digest = hashlib.sha256(payload).hexdigest()

    def assert_reader(self) -> None:
        """Check the retained real reader's complete bytes after all sharing clears."""
        assert self.reader is not None
        assert self.reader.geometry() == self._reader_geometry
        assert hashlib.sha256(self.reader.read_f32le()).hexdigest() == self._reader_digest

    def release_reader(self) -> None:
        """Drop that reader; productive weak-reader retirement still decides deletion."""
        self.reader = None
        self.assets.retry_retirements()

    def poll_retirements(self) -> None:
        """Use the productive control cleanup lanes without deleting fixture files."""
        self.app.loader.poll_loader_events()
        assert not self.session.sample_load_errors, self.session.sample_load_errors
        self.assets.retry_retirements()

    def close(self) -> None:
        """Release test-only callback cycles and process owners after terminal work."""
        assert not self.controller.active
        assert not self.seed.arrangement.active
        self.forget_settled()
        self.reader = None
        self.app.loader.shut_down()
        self.app.stems.shut_down()
        self.seed.arrangement._on_copy_committed = self._seed_callback
        self._receipt_patch.stop()
        self.assets.release_saved_assignments()
        self.app.banks = None
        self.app.pairs = None
        self.app._pair_waveform_views.clear()
        self.seed.close()
