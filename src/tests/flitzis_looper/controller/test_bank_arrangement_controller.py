"""Real Bank journals, Config, mirrors and asset owners around opaque Native doubles.

The Bank/asset facade opens no device and proves no Native callback, DSP,
resource admission, startup composition or Human listening acceptance.
"""

import hashlib
import json
import threading
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller.accepted_publication import AcceptedTimingController
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.bank_arrangement import BankArrangementController
from flitzis_looper.controller.bank_arrangement_journal import (
    BankArrangementJournal,
    BankJournalOwner,
    validated_bank_history,
)
from flitzis_looper.controller.bank_arrangement_recovery import inspect_bank_recovery
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_generation import AudioShape, StemGenerationRequest
from flitzis_looper.controller.stem_job import StemGenerationJob
from flitzis_looper.controller.stem_pair_preparation import PairPreparation
from flitzis_looper.controller.stems import StemController
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from flitzis_looper.ui.context import WaveformEditorActions
from tests.flitzis_looper.controller.test_bank_arrangement_mirror import _FailingLock
from tests.flitzis_looper.controller.test_committed_stem_residency import (
    _entry,
    _RegistrationTicket,
)
from tests.flitzis_looper.controller.test_pair_arrangement_controller import (
    _AppFixture,
    _Arrangement,
    _project,
)
from tests.flitzis_looper.controller.test_pair_arrangement_controller import (
    _Native as _AssetNative,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.asset_lifecycle import AssignmentImage
    from flitzis_looper.controller.bank_arrangement import _Bank
    from flitzis_looper_audio import (
        AudioEngine,
        CommittedStemSetTicket,
        MaterialMigrationJournalStore,
        OfflineStemSourceLease,
        PreparedSourceTicket,
    )


@dataclass(eq=False)
class _Participants:
    source_bank: int | None
    target_bank: int
    current: bool = True


@dataclass(eq=False)
class _CapturedBank:
    participants: _Participants
    intents: list[str | None]
    phase: str = "captured"
    ack: object = field(default_factory=object)
    mirrored: bool = False


@dataclass(eq=False)
class _BankTicket:
    captured: _CapturedBank
    registrations: list[CommittedStemSetTicket | None]
    components: list[bool]

    def publication_status(self) -> str:
        return self.captured.phase

    def acknowledgement(self) -> object | None:
        return self.captured.ack if self.captured.phase == "accepted" else None

    def committed_stem_tickets(self) -> list[CommittedStemSetTicket | None]:
        return self.registrations.copy()

    def metadata_json(self) -> str:
        capture = self.captured
        targets = []
        for offset, intent in enumerate(capture.intents):
            slot = capture.participants.target_bank * 36 + offset
            occupied = intent is not None
            targets.append({
                "slot": slot,
                "after_lifetime": 1000 + slot if occupied else 0,
                "source_generation": 100 + slot if occupied else 0,
                "components_available": self.components[offset],
            })
        return json.dumps({
            "status": capture.phase,
            "control_mirrored": capture.mirrored,
            "source_bank": capture.participants.source_bank,
            "target_bank": capture.participants.target_bank,
            "targets": targets,
        })


class _Native(_AssetNative):
    """Only opaque Bank admission/ACK and existing asset-lease observations are scripted."""

    def __init__(self) -> None:
        super().__init__()
        self.participants: list[_Participants] = []
        self.bank_captures: list[_CapturedBank] = []
        self.bank_tickets: list[_BankTicket] = []
        self.bank_publications: list[_BankTicket] = []
        self.bank_cancellations: list[_CapturedBank] = []
        self.bank_finishes: list[tuple[_BankTicket, object]] = []
        self.bank_validations: list[tuple[_BankTicket, object]] = []
        self.auto_ack = True

    def capture_bank_participants(self, source_bank: int | None, target_bank: int) -> _Participants:
        witness = _Participants(source_bank, target_bank)
        self.participants.append(witness)
        return witness

    def capture_bank_mutation(
        self,
        source_bank: int | None,
        target_bank: int,
        intents: list[str | None],
        *,
        participants: _Participants,
    ) -> _CapturedBank:
        if (
            not any(participants is original for original in self.participants)
            or not participants.current
            or (participants.source_bank, participants.target_bank) != (source_bank, target_bank)
        ):
            message = "original Native Bank participants are stale"
            raise ValueError(message)
        assert len(intents) == 36
        captured = _CapturedBank(participants, intents.copy())
        self.bank_captures.append(captured)
        return captured

    def prepare_bank_mutation(self, captured: _CapturedBank) -> _BankTicket:
        assert any(captured is original for original in self.bank_captures)
        registrations: list[CommittedStemSetTicket | None] = []
        components: list[bool] = []
        for intent in captured.intents:
            stems = json.loads(intent)["snapshot"]["stem_cache"] if intent is not None else None
            components.append(stems is not None)
            registrations.append(
                cast("CommittedStemSetTicket", _RegistrationTicket(state="accepted"))
                if stems is not None and stems["pair"] is not None
                else None
            )
        if captured.phase != "rejected":
            captured.phase = "prepared"
        ticket = _BankTicket(captured, registrations, components)
        self.bank_tickets.append(ticket)
        return ticket

    def publish_bank_mutation(self, ticket: _BankTicket) -> None:
        assert ticket.captured.phase == "prepared"
        self.bank_publications.append(ticket)
        ticket.captured.phase = "accepted" if self.auto_ack else "adopting"

    def validate_bank_mutation(self, ticket: _BankTicket, ack: object) -> bool:
        self.bank_validations.append((ticket, ack))
        return (
            ticket in self.bank_tickets
            and ack is ticket.captured.ack
            and (ticket.captured.phase == "accepted")
        )

    def reconcile_bank_mutation(self, ticket: _BankTicket, ack: object) -> bool:
        if not self.validate_bank_mutation(ticket, ack):
            return False
        ticket.captured.mirrored = True
        return True

    def finish_bank_mutation(self, ticket: _BankTicket, ack: object) -> None:
        assert self.validate_bank_mutation(ticket, ack)
        self.bank_finishes.append((ticket, ack))

    def cancel_bank_mutation(self, captured: _CapturedBank) -> bool:
        self.bank_cancellations.append(captured)
        if captured.phase in {"adopting", "accepted"}:
            return False
        captured.phase = "rejected"
        return True

    def retire_project_asset(self, path: str, *, recursive: bool = False) -> None:
        # Real ProjectAssetLifecycle determines containment, leases and retirement timing.
        # This existing Native boundary records admission without deleting fixture inputs.
        if recursive:
            assert Path(path).name.startswith((".ready-", ".generation-"))
        self.retired_paths.append(path)


@dataclass(slots=True)
class _App(_AppFixture):
    pairs: _Arrangement
    banks: BankArrangementController | None = None

    def _sync_arrangement_observers(self) -> None:
        deferred = frozenset() if self.banks is None else self.banks.observer_fenced_sample_ids
        for controller in (self.transport, self.loader, self.accepted_timing, self.stems):
            controller._pair_deferred_sample_ids = deferred


@dataclass
class _Harness:
    app: _App
    native: _Native
    controller: BankArrangementController
    tasks: list[Callable[[], None]]
    images: list[AssignmentImage]

    def current(self) -> _Bank:
        bank = self.controller._bank
        assert bank is not None
        return bank

    def run_worker(self) -> None:
        assert len(self.tasks) == 1
        self.tasks.pop(0)()

    def complete(self) -> None:
        for _ in range(16):
            while self.tasks:
                self.tasks.pop(0)()
            self.controller.poll()
            if not self.controller.active:
                break
        assert self.controller.status == "completed", self.controller.error


def _harness(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, full: bool = False, stems: bool = False
) -> _Harness:
    monkeypatch.chdir(tmp_path)
    (tmp_path / "samples").mkdir()
    project = _project(tmp_path)
    source_ids = range(36) if full else (0,)
    for sample_id in (*source_ids, 36, 37, 71):
        if sample_id != 0:
            path = tmp_path / "samples" / f"source-{sample_id}.wav"
            path.write_bytes(b"bounded Bank original fixture")
            project.sample_paths[sample_id] = path.relative_to(tmp_path).as_posix()
            project.pad_content[sample_id] = PadContentIdentity(
                instance_id=f"{sample_id + 4096:032x}"
            )
            project.sample_durations[sample_id] = 12.0
    if stems:
        for sample_id in source_ids:
            entry = _entry()
            Path(entry.cache_dir).mkdir(parents=True, exist_ok=True)
            project.stem_cache[sample_id] = entry
    session = SessionState(active_sample_ids={0, 36, 71, 215}, paused_sample_ids={71})
    native = _Native()
    audio = cast("AudioEngine", native)
    assets = ProjectAssetLifecycle(project, audio)
    assets.sync_assignments()
    transport = TransportController(project, session, audio)
    persistence = ProjectPersistence(project)
    persistence.config_path = tmp_path / "samples" / "bank.config.json"
    persistence.flush()
    tasks: list[Callable[[], None]] = []
    app = _App(
        _project=project,
        _session=session,
        _audio=native,
        _assets=assets,
        project=project,
        session=session,
        persistence=persistence,
        transport=transport,
        loader=LoaderController(project, session, audio, lambda _slot: None),
        accepted_timing=AcceptedTimingController(transport),
        metering=MeteringController(project, session, audio),
        stems=StemController(
            project, session, audio, stem_task_runner=tasks.append, asset_lifecycle=assets
        ),
        arrangement=_Arrangement(),
        _pair_waveform_views=[],
        pairs=_Arrangement(),
    )
    WaveformEditorActions(cast("AppController", app))
    images: list[AssignmentImage] = []
    prepare_image = assets.prepare_assignment_image

    def remember_image(candidate: ProjectState) -> AssignmentImage:
        image = prepare_image(candidate)
        images.append(image)
        return image

    monkeypatch.setattr(assets, "prepare_assignment_image", remember_image)
    controller = BankArrangementController(cast("AppController", app))
    app.banks = controller
    return _Harness(app, native, controller, tasks, images)


def _begin(harness: _Harness, *, clear: bool = False) -> _Bank:
    request = harness.controller.capture_request(None if clear else 0, 1)
    assert harness.controller.begin(request)
    return harness.current()


def _records(bank: _Bank) -> list[BankArrangementJournal]:
    owner = BankJournalOwner.open(Path.cwd() / "samples", bank.owner)
    return validated_bank_history(owner.encoded, bank.owner)


def _assert_settled(harness: _Harness, bank: _Bank) -> None:
    receipt = bank.receipt
    assert receipt is not None
    written = harness.app.persistence.config_path.read_bytes()
    assert receipt.written_text.encode("utf-8") == written
    assert receipt.sha256 == hashlib.sha256(written).hexdigest()
    assert receipt.revision == harness.app.persistence.revision
    records = _records(bank)
    assert records[-1].phase == "settled"
    assert records[-1].committed_config_sha256 == receipt.sha256
    assert bank.mirror is not None
    assert bank.mirror.applied
    assert bank.mirror.settled
    assert bank.publication is not None
    assert bank.publication.settled
    assert bank.native_finished
    assert bank.writer_released
    assert not bank.images
    assert not bank.custody.readers
    assert harness.app.persistence._migration_owner is None
    assert bank.retirement is not None
    assert bank.retirement.remaining == 0
    assert len(harness.native.bank_captures) == len(harness.native.bank_publications) == 1
    ticket = harness.native.bank_tickets[0]
    assert harness.native.bank_finishes == [(ticket, ticket.captured.ack)]
    assert all(
        owned is ticket and acknowledgement is ticket.captured.ack
        for owned, acknowledgement in harness.native.bank_validations
    )
    assert all(not lease.released for _, lease in harness.app._assets._assignments.values())


def test_confirmation_retains_exact_original_native_witness_without_writer_or_reservation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    request = harness.controller.capture_request(0, 1)
    witness = cast("_Participants", request.participants)
    fixed = request.bank
    assert witness is harness.native.participants[0]
    assert not harness.controller.active
    assert harness.app.persistence._migration_owner is None
    assert not harness.native.bank_captures
    assert not harness.tasks
    assert harness.controller.begin(request)
    bank = harness.current()
    assert bank.request is request
    assert bank.initial.bank == fixed
    assert harness.native.bank_captures[0].participants is witness
    assert not harness.controller.begin(request)
    assert len(harness.native.participants) == len(harness.native.bank_captures) == 1
    harness.complete()
    _assert_settled(harness, bank)


def test_detached_capture_migration_image_must_match_original_confirmation_before_native(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    request = harness.controller.capture_request(0, 1)
    capture = harness.app.persistence.capture_migration

    def changed_capture(owner: str) -> tuple[int, ProjectState, str | None]:
        revision, image, digest = capture(owner)
        image.pad_gain_db[35] = -23.0
        return revision, image, digest

    monkeypatch.setattr(harness.app.persistence, "capture_migration", changed_capture)
    before = harness.app.project.model_dump_json()
    with pytest.raises(ValueError, match="participant musical content changed"):
        harness.controller.begin(request)
    assert harness.app.project.model_dump_json() == before
    assert harness.app.persistence._migration_owner is None
    assert not harness.controller.active
    assert not harness.native.bank_captures
    assert not harness.tasks
    assert not (tmp_path / "samples" / ".material-migrations").exists()


def test_completed_same_file_aba_rejects_original_opaque_witness_without_recapture(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    request = harness.controller.capture_request(0, 1)
    cast("_Participants", request.participants).current = False
    assert harness.controller.begin(request)
    bank = harness.current()
    assert bank.captured is None
    assert len(harness.native.participants) == 1
    assert not harness.native.bank_captures
    assert harness.app.persistence._migration_owner == bank.owner
    assert harness.controller.cancel()
    assert harness.controller.status == "cancelled"
    assert harness.app.persistence._migration_owner is None
    assert _records(bank)[-1].phase == "cancelled"


@pytest.mark.parametrize("case", ["mixed", "full", "clear"])
def test_one_own_ack_settles_fixed_complete_bank_config_journal_mirrors_and_assets(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, case: str
) -> None:
    harness = _harness(tmp_path, monkeypatch, full=case == "full")
    source = harness.app.project.pad_content[0]
    outside = harness.app.project.pad_content[215]
    bank = _begin(harness, clear=case == "clear")
    fixed = tuple(row.after.source_identity for row in bank.initial.bank.projected().slots)
    assert harness.controller.fenced_sample_ids == (
        frozenset(range(36, 72)) if case == "clear" else frozenset(range(72))
    )
    harness.complete()
    _assert_settled(harness, bank)
    assert len(_records(bank)) == 5
    assert harness.app.project.pad_content[0] is source
    assert harness.app.project.pad_content[215] is outside
    assert tuple(harness.app.project.pad_content[36:72]) == fixed
    assert harness.app.session.active_sample_ids == {0, 215}
    assert not harness.app.session.paused_sample_ids
    assert harness.app.project.selected_bank == 0
    if case == "full":
        identities = [identity.instance_id for identity in fixed if identity is not None]
        assert len(set(identities)) == 36
    elif case == "clear":
        assert all(identity is None for identity in fixed)
    else:
        assert fixed[0] is not None
        assert source is not None
        assert fixed[0].copied_from == source.instance_id
        assert fixed[1] is None


def test_worker_admitted_cancel_keeps_writer_readers_and_reservation_until_actual_return(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    before = harness.app.persistence.config_path.read_bytes()
    bank = _begin(harness)
    captured = bank.captured
    assert captured is not None
    assert not bank.worker_done
    assert not harness.controller.cancel()
    assert bank.cancellation_proven
    assert bank.custody.readers
    assert harness.app.persistence._migration_owner == bank.owner
    assert bank.retirement is not None
    assert bank.retirement.remaining > 0
    assert not bank.worker_done
    assert _records(bank)[-1].phase == "captured"
    assert not harness.controller.cancel()
    harness.run_worker()
    harness.controller.poll()
    assert not harness.controller.active
    assert harness.controller.status == "cancelled"
    assert bank.worker_done
    assert bank.writer_released
    assert _records(bank)[-1].phase == "cancelled"
    assert not bank.custody.readers
    assert harness.app.persistence._migration_owner is None
    assert harness.app.persistence.config_path.read_bytes() == before
    assert not harness.native.bank_publications
    assert not harness.native.bank_finishes
    assert all(actual is captured for actual in harness.native.bank_cancellations)


def test_adopting_cancel_refusal_retains_original_owner_until_its_own_ack_finishes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    harness.native.auto_ack = False
    bank = _begin(harness)
    harness.run_worker()
    harness.controller.poll()
    captured = harness.native.bank_captures[0]
    assert captured.phase == "adopting"
    assert bank.acknowledgement is None
    assert not harness.controller.cancel()
    assert not harness.controller.cancel()
    assert harness.current() is bank
    assert not bank.cancellation_proven
    assert harness.app.persistence._migration_owner == bank.owner
    assert bank.custody.readers
    captured.phase = "accepted"
    harness.complete()
    _assert_settled(harness, bank)
    assert all(original is captured for original in harness.native.bank_cancellations)


@pytest.mark.parametrize("after_replace", [False, True])
def test_actual_config_failure_retains_same_candidate_ack_and_unpublished_owners_until_retry(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, after_replace: bool
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    write = harness.app.persistence._atomic_write_text
    failed = False

    def fail_once(text: str, *, before_replace: Callable[[], None] | None = None) -> None:
        nonlocal failed
        if not failed:
            failed = True
            if after_replace:
                write(text, before_replace=before_replace)
            message = "actual Bank Config replacement return failed"
            raise OSError(message)
        write(text, before_replace=before_replace)

    monkeypatch.setattr(harness.app.persistence, "_atomic_write_text", fail_once)
    before = harness.app.project.model_dump_json()
    assignments = harness.app._assets._assignments
    bank = _begin(harness)
    harness.run_worker()
    harness.controller.poll()
    assert failed
    assert bank.receipt is None
    assert bank.mirror is not None
    assert not bank.mirror.applied
    assert harness.app.project.model_dump_json() == before
    assert harness.app._assets._assignments is assignments
    retained = (bank.candidate, bank.assignment, bank.publication, bank.acknowledgement)
    assert bank.publication is not None
    assert bank.publication.owns_unpublished_image
    assert harness.app.persistence._migration_owner == bank.owner
    assert not harness.native.bank_finishes
    if after_replace:
        assert bank.journal is not None
        assert bank.journal.journal is not None
        decision = inspect_bank_recovery(
            bank.journal.journal,
            harness.app.persistence.config_path.read_bytes(),
            history=bank.journal.encoded,
        )
        assert decision.disposition == "recognized_after"
        assert decision.receipt is not None
    harness.complete()
    assert all(
        actual is original
        for actual, original in zip(
            (bank.candidate, bank.assignment, bank.publication, bank.acknowledgement),
            retained,
            strict=True,
        )
    )
    _assert_settled(harness, bank)


class _LostAppendReturn:
    def __init__(self, store: MaterialMigrationJournalStore) -> None:
        self.store = store
        self.failed = False

    def history(self) -> list[str]:
        return self.store.history()

    def append(self, text: str) -> str:
        result = self.store.append(text)
        if not self.failed and json.loads(text)["phase"] == "publication_pending":
            self.failed = True
            message = "actual Native Bank ordinal written before lost return"
            raise OSError(message)
        return result


def test_exact_uncertain_native_append_adopts_existing_ordinal_without_republishing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    bank = _begin(harness)
    assert bank.journal is not None
    uncertain = _LostAppendReturn(bank.journal.store)
    bank.journal.store = cast("MaterialMigrationJournalStore", uncertain)
    harness.complete()
    assert uncertain.failed
    assert bank.journal.intended is None
    records = _records(bank)
    assert [record.phase for record in records] == [
        "captured",
        "publication_pending",
        "native_ack_observed",
        "config_committed",
        "settled",
    ]
    _assert_settled(harness, bank)


@pytest.mark.parametrize("after_append", [False, True])
def test_terminal_append_failure_never_repeats_native_finish_or_loses_actual_receipt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, after_append: bool
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    append = BankJournalOwner.append
    failed = False

    def fail_terminal(
        owner: BankJournalOwner, record: BankArrangementJournal
    ) -> BankArrangementJournal:
        nonlocal failed
        if record.phase == "settled" and not failed:
            failed = True
            if after_append:
                append(owner, record)
            message = "Bank terminal ordinal return interrupted"
            raise OSError(message)
        return append(owner, record)

    monkeypatch.setattr(BankJournalOwner, "append", fail_terminal)
    bank = _begin(harness)
    harness.run_worker()
    harness.controller.poll()
    assert failed
    assert bank.native_finished
    assert bank.receipt is not None
    assert len(harness.native.bank_finishes) == 1
    receipt, mirror, publication = bank.receipt, bank.mirror, bank.publication
    assert harness.app.persistence._migration_owner == bank.owner
    harness.complete()
    assert bank.receipt is receipt
    assert bank.mirror is mirror
    assert bank.publication is publication
    _assert_settled(harness, bank)


def test_newer_source_outside_and_navigation_intent_get_second_receipt_with_same_fixed_after(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    persistence = harness.app.persistence
    write = persistence._atomic_write_text
    changed = False

    def write_then_edit(text: str, *, before_replace: Callable[[], None] | None = None) -> None:
        nonlocal changed
        write(text, before_replace=before_replace)
        if not changed:
            changed = True
            harness.app.project.pad_gain_db[0] = -24.0
            harness.app.project.pad_gain_db[108] = -17.0
            harness.app.project.volume = 0.25
            harness.app.project.selected_pad = 108
            persistence.mark_dirty()

    monkeypatch.setattr(persistence, "_atomic_write_text", write_then_edit)
    bank = _begin(harness)
    fixed = bank.initial.bank.projected().slots[0].after.source_identity
    harness.run_worker()
    harness.controller.poll()
    first = bank.receipt
    assert first is not None
    assert first.revision < persistence.revision
    assert bank.mirror is not None
    assert not bank.mirror.applied
    acknowledgement = bank.acknowledgement
    harness.complete()
    _assert_settled(harness, bank)
    assert bank.acknowledgement is acknowledgement
    assert bank.receipt is not None
    assert bank.receipt is not first
    saved = bank.receipt.projected()
    assert saved.pad_content[36] == fixed
    assert saved.pad_gain_db[36] == -6.0
    assert saved.pad_gain_db[0] == -24.0
    assert saved.pad_gain_db[108] == -17.0
    assert (saved.volume, saved.selected_pad) == (0.25, 108)
    assert len([record for record in _records(bank) if record.phase == "config_committed"]) == 2


def test_final_registration_failure_retains_controls_config_and_writer_until_same_ack_retry(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch, full=True, stems=True)
    old = harness.app.project.pad_content
    written = harness.app.persistence.config_path.read_bytes()
    assignments = harness.app._assets._assignments
    bank = _begin(harness)
    harness.run_worker()
    ticket = harness.native.bank_tickets[0]
    last = cast("_RegistrationTicket", ticket.registrations[-1])
    last.state = "rejected"
    harness.controller.poll()
    assert bank.acknowledgement is ticket.captured.ack
    assert bank.receipt is None
    assert harness.app.project.pad_content is old
    assert harness.app._assets._assignments is assignments
    assert harness.app.persistence.config_path.read_bytes() == written
    assert harness.app.persistence._migration_owner == bank.owner
    assert not harness.native.bank_finishes
    last.state = "accepted"
    harness.complete()
    _assert_settled(harness, bank)


def test_parent_observed_unavailable_components_remain_false_despite_own_registration(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch, stems=True)
    bank = _begin(harness)
    harness.run_worker()
    ticket = harness.native.bank_tickets[0]
    assert ticket.registrations[0] is not None
    ticket.components[0] = False
    harness.complete()
    _assert_settled(harness, bank)
    assert bank.receipt is not None
    copied = bank.receipt.projected().stem_cache[36]
    assert copied is not None
    assert copied.available is False
    assert harness.app.project.stem_cache[36] is not None
    assert harness.app.project.stem_cache[36].available is False


def test_repeated_same_config_failure_retains_one_bounded_history_and_original_native_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    write = harness.app.persistence._atomic_write_text

    def unavailable(_text: str, *, before_replace: Callable[[], None] | None = None) -> None:
        message = "same bounded Bank Config failure"
        raise OSError(message)

    monkeypatch.setattr(harness.app.persistence, "_atomic_write_text", unavailable)
    bank = _begin(harness)
    harness.run_worker()
    for _ in range(40):
        harness.controller.poll()
    assert harness.current() is bank
    assert bank.journal is not None
    assert len(bank.journal.encoded) < 16
    assert bank.receipt is None
    assert bank.custody.readers
    assert harness.app.persistence._migration_owner == bank.owner
    assert len(harness.native.bank_publications) == 1
    assert not harness.native.bank_finishes
    monkeypatch.setattr(harness.app.persistence, "_atomic_write_text", write)
    harness.complete()
    _assert_settled(harness, bank)


def test_pending_target_collector_rejects_before_native_reservation_and_keeps_original_readers(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    request = harness.controller.capture_request(0, 1)
    entry = _entry()
    Path(entry.cache_dir).mkdir(parents=True)
    identity = harness.app.project.pad_content[36]
    assert identity is not None
    preparation = PairPreparation(
        36,
        entry,
        cast("PreparedSourceTicket", object()),
        identity.instance_id,
        None,
        harness.app._assets.reserve_pending(16),
        harness.app._assets.acquire(Path(entry.cache_dir)),
        threading.Event(),
        source_path=harness.app.project.sample_paths[36],
    )
    harness.app.stems._pair_preparations.pending[36] = preparation
    with pytest.raises(RuntimeError, match="unsettled preparation"):
        harness.controller.begin(request)
    assert harness.app.stems._pair_preparations.pending[36] is preparation
    assert not preparation.cancelled.is_set()
    assert not preparation.lease.released
    assert preparation.retirement.remaining > 0
    assert not harness.native.bank_captures
    assert not harness.tasks
    assert harness.app.persistence._migration_owner is None
    preparation.close()


def _target_job(harness: _Harness, slot: int) -> StemGenerationJob:
    project = harness.app.project
    identity = project.pad_content[slot]
    path = project.sample_paths[slot]
    assert identity is not None
    assert path is not None
    cache = Path("samples/stems") / f"#{slot + 1}" / f".generation-{slot + 1:032x}"
    cache.mkdir(parents=True)
    request = StemGenerationRequest(
        sample_id=slot,
        source_path=Path(path).absolute(),
        source_version=f"{path}|sha256-v1:{'1' * 64}",
        cache_dir=cache,
        target_shape=AudioShape(48_000, 2, 512),
        model_cache_dir=Path("models"),
        original_bytes=32,
    )
    source = cast("OfflineStemSourceLease", object())
    intent = harness.app.stems._intents.capture(request, project.stem_processing_revision)
    job = StemGenerationJob(
        request, source, harness.app._assets, identity.instance_id, intent=intent
    )
    harness.app.stems._jobs[cache] = job
    harness.app.stems._generation_sources[slot] = source
    harness.app.stems._generation_intents[slot] = intent
    return job


def test_36th_job_lock_failure_after_actual_receipt_keeps_every_control_store_unapplied(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    for slot in range(36, 72):
        if harness.app.project.pad_content[slot] is None:
            path = tmp_path / "samples" / f"source-{slot}.wav"
            path.write_bytes(b"owned target job fixture")
            harness.app.project.sample_paths[slot] = path.relative_to(tmp_path).as_posix()
            harness.app.project.pad_content[slot] = PadContentIdentity(
                instance_id=f"{slot + 8192:032x}"
            )
    harness.app._assets.sync_assignments()
    harness.app.persistence.mark_dirty()
    jobs = tuple(_target_job(harness, slot) for slot in range(36, 72))
    subscriber_images = tuple(job._subscribers for job in jobs)
    before = harness.app.project.pad_content
    write = harness.app.persistence._atomic_write_text
    entered = [0]
    installed = False
    with monkeypatch.context() as locks:

        def write_then_block(
            text: str, *, before_replace: Callable[[], None] | None = None
        ) -> None:
            nonlocal installed
            write(text, before_replace=before_replace)
            if not installed:
                installed = True
                for job in jobs:
                    locks.setattr(job, "_lock", _FailingLock(job._lock, entered))

        locks.setattr(harness.app.persistence, "_atomic_write_text", write_then_block)
        bank = _begin(harness)
        harness.run_worker()
        harness.controller.poll()
        assert installed
        assert entered[0] == 36
        assert bank.receipt is not None
        assert bank.mirror is not None
        assert not bank.mirror.applied
        assert harness.app.project.pad_content is before
        assert all(
            job._subscribers is image for job, image in zip(jobs, subscriber_images, strict=True)
        )
        assert harness.app.persistence._migration_owner == bank.owner
        assert not harness.native.bank_finishes
    harness.complete()
    _assert_settled(harness, bank)
