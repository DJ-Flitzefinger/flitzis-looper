"""One Controller owner survives actual Config writes and opaque Native loss points."""

import hashlib
import json
from contextlib import nullcontext
from dataclasses import dataclass
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller import pair_arrangement, pair_arrangement_journal
from flitzis_looper.controller.pair_arrangement import PairArrangementController
from flitzis_looper.controller.pair_arrangement_journal import PairArrangementAfterImage
from flitzis_looper.controller.pair_arrangement_recovery import inspect_pair_recovery
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.copy_snapshot import CONTENT_FIELDS
from flitzis_looper.models import PadContentIdentity, ProjectState
from flitzis_looper_audio import MaterialMigrationJournalStore

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.pair_snapshot import PairSnapshot


class _Captured:
    """Opaque retained owner: numeric slots cannot reconstruct this object."""

    def __init__(self, source: int, target: int) -> None:
        self.source = source
        self.target = target
        self.claimed = False
        self.acknowledged = False
        self.cancelled = False
        self.ack = object()


class _Ticket:
    def __init__(self, captured: _Captured) -> None:
        self.captured = captured

    def publication_status(self) -> str:
        return (
            "rejected"
            if self.captured.cancelled
            else "accepted"
            if self.captured.acknowledged
            else "pending"
        )

    def own_acknowledgement(self) -> object | None:
        return self.captured.ack if self.captured.acknowledged else None


class _Audio:
    def __init__(self) -> None:
        self.captures: list[_Captured] = []
        self.tickets: list[_Ticket] = []
        self.publish_calls: list[_Ticket] = []
        self.cancel_calls: list[_Captured] = []
        self.finish_calls: list[tuple[_Ticket, object]] = []
        self.publish_failures = 0
        self.finish_failures = 0
        self.auto_ack = True

    def _capture_pad_pair(self, source: int, target: int) -> _Captured:
        captured = _Captured(source, target)
        self.captures.append(captured)
        return captured

    def _prepare_pad_pair(self, captured: _Captured) -> _Ticket:
        assert captured is self.captures[-1]
        ticket = _Ticket(captured)
        self.tickets.append(ticket)
        return ticket

    def _publish_pad_pair(self, ticket: _Ticket) -> None:
        self.publish_calls.append(ticket)
        if self.publish_failures:
            self.publish_failures -= 1
            message = "bounded queue full"
            raise RuntimeError(message)
        assert not ticket.captured.claimed
        ticket.captured.claimed = True
        ticket.captured.acknowledged = self.auto_ack

    def _reconcile_pad_pair(self, ticket: _Ticket) -> bool:
        return ticket.captured.acknowledged

    def _validate_pad_pair_acknowledgement(self, ticket: _Ticket, ack: object) -> bool:
        return (
            ticket is self.tickets[-1]
            and ack is ticket.captured.ack
            and ticket.captured.acknowledged
        )

    def _cancel_pad_pair(self, captured: _Captured) -> bool:
        self.cancel_calls.append(captured)
        if captured.claimed:
            return False
        captured.cancelled = True
        return True

    def _finish_pad_pair(self, ticket: _Ticket, ack: object) -> None:
        assert self._validate_pad_pair_acknowledgement(ticket, ack)
        self.finish_calls.append((ticket, ack))
        if self.finish_failures:
            self.finish_failures -= 1
            message = "Native finish temporarily unavailable"
            raise RuntimeError(message)


class _Retirement:
    def __init__(self) -> None:
        self.closed = 0
        self.activations = 0

    def activate(self) -> nullcontext:
        self.activations += 1
        return nullcontext(self)

    def close(self) -> None:
        self.closed += 1


class _Assets:
    def __init__(self) -> None:
        self.retirement = _Retirement()
        self.prepared: list[tuple[object, ProjectState]] = []
        self.published: list[object] = []
        self.released: list[object] = []

    def reserve(self, capacity: int) -> _Retirement:
        assert capacity == 64
        return self.retirement

    def prepare_assignment_image(self, project: ProjectState) -> object:
        image = object()
        self.prepared.append((image, project.model_copy(deep=True)))
        return image

    def publish_assignment_image(self, image: object) -> None:
        assert any(image is prepared for prepared, _ in self.prepared)
        self.published.append(image)

    def prepare_assignment_publication(self, image: object) -> _Publication:
        return _Publication(self, image)

    def release_assignment_image(self, image: object) -> None:
        assert image not in self.published
        self.released.append(image)


class _Publication:
    def __init__(self, assets: _Assets, image: object) -> None:
        self.assets, self.image = assets, image
        self.applied = False
        self.settled = False

    @property
    def owns_unpublished_image(self) -> bool:
        return not self.applied

    def apply(self) -> None:
        if not self.applied:
            self.assets.publish_assignment_image(self.image)
            self.applied = True
        self.settled = True


class _Mirror:
    """Retain one original sealed plan, including through a partial apply failure."""

    def __init__(self, app: SimpleNamespace, snapshot: PairSnapshot) -> None:
        self.app = app
        self.after = PairArrangementAfterImage.from_before(snapshot)
        self.apply_calls: list[object] = []
        self.apply_failures = 0
        self.partial = False

    def projected_project(self) -> ProjectState:
        current = self.app.project.model_dump(mode="python", exclude_computed_fields=True)
        data = self.after.projected()
        for sample_id, slot in ((data.source_id, data.source), (data.target_id, data.target)):
            projection = slot.model_dump(mode="python")
            for name, field in CONTENT_FIELDS:
                current[field][sample_id] = projection[name]
        return ProjectState.model_validate(current)

    def apply(self, ack: object) -> None:
        assert ack is self.app._audio.captures[-1].ack
        self.apply_calls.append(ack)
        projected = self.projected_project()
        if self.apply_failures:
            self.apply_failures -= 1
            if self.partial:
                self.app.project.sample_paths = projected.sample_paths
            message = "interrupted original mirror"
            raise RuntimeError(message)
        for field in ProjectState.model_fields:
            setattr(self.app.project, field, getattr(projected, field))


class _StemMirror:
    projected_entries: tuple = ()

    def __init__(self) -> None:
        self.acks: list[object] = []

    def apply(self, ack: object) -> None:
        self.acks.append(ack)


@dataclass
class _Rig:
    app: SimpleNamespace
    controller: PairArrangementController
    mirrors: list[_Mirror]
    stem_mirrors: list[_StemMirror]


def _rig(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> _Rig:
    monkeypatch.chdir(tmp_path)
    project = ProjectState()
    project.sample_paths[0] = str(tmp_path / "samples" / "A.wav")
    project.sample_paths[215] = "samples/B.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="1" * 32)
    project.pad_content[215] = PadContentIdentity(instance_id="2" * 32, copied_from="1" * 32)
    project.pad_gain_db[0], project.pad_gain_db[215] = -6.0, -12.0
    project.pad_gain_db[36] = -3.0
    persistence = ProjectPersistence(project)
    persistence.config_path = tmp_path / "samples" / "owning.config.json"
    persistence.flush()
    app = SimpleNamespace(
        project=project,
        persistence=persistence,
        _audio=_Audio(),
        _assets=_Assets(),
        arrangement=SimpleNamespace(active=False),
        stems=SimpleNamespace(_stem_task_runner=lambda run: run()),
        _pair_waveform_views=[],
    )
    mirrors: list[_Mirror] = []
    stem_mirrors: list[_StemMirror] = []

    def prepare_mirror(app: SimpleNamespace, snapshot: PairSnapshot, **kwargs: object) -> _Mirror:
        mirror = _Mirror(app, snapshot)
        mirrors.append(mirror)
        return mirror

    def prepare_stems(*args: object, **kwargs: object) -> _StemMirror:
        mirror = _StemMirror()
        stem_mirrors.append(mirror)
        return mirror

    monkeypatch.setattr(pair_arrangement, "prepare_pair_control_mirror", prepare_mirror)
    monkeypatch.setattr(pair_arrangement, "prepare_pair_stem_mirror", prepare_stems)
    controller = PairArrangementController(cast("AppController", app))
    return _Rig(app, controller, mirrors, stem_mirrors)


def _poll_complete(rig: _Rig) -> None:
    for _ in range(12):
        rig.controller.poll()
        if not rig.controller.active:
            break
    assert rig.controller.status == "completed", rig.controller.error


def test_queue_retry_preserves_original_native_owner_and_latest_unrelated_intent(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    rig.app._audio.publish_failures = 1
    assert rig.controller.begin(0, 215, operation="swap")
    original = rig.controller._pair
    assert original is not None
    rig.controller.poll()
    assert rig.controller.active
    assert rig.controller.status == "settlement_pending"
    assert rig.app._assets.retirement.closed == 0
    assert rig.app.persistence._migration_owner == original.owner
    rig.app.project.pad_gain_db[36] = -21.0
    rig.app.project.volume = 0.25
    rig.app.persistence.mark_dirty()
    _poll_complete(rig)

    assert len(rig.app._audio.captures) == len(rig.app._audio.tickets) == 1
    assert len(rig.app._audio.publish_calls) == 2
    assert rig.app._audio.publish_calls[0] is rig.app._audio.publish_calls[1]
    assert rig.app._audio.cancel_calls == []
    assert rig.app.project.pad_content[0].instance_id == "2" * 32
    assert rig.app.project.pad_content[215].instance_id == "1" * 32
    saved = ProjectState.model_validate_json(rig.app.persistence.config_path.read_bytes())
    assert saved.pad_gain_db[36] == -21.0
    assert saved.volume == 0.25
    assert saved.config_revision == rig.app.persistence.revision == 1
    assert (
        original.receipt.sha256
        == hashlib.sha256(rig.app.persistence.config_path.read_bytes()).hexdigest()
    )
    assert (
        original.receipt.written_text.encode("utf-8")
        == rig.app.persistence.config_path.read_bytes()
    )
    assert rig.app._assets.retirement.closed == 1


def test_mirror_factory_failure_preserves_captured_ticket_and_own_ack_for_retry(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    prepare = pair_arrangement.prepare_pair_control_mirror
    attempts = 0

    def fail_once(*args: object, **kwargs: object) -> object:
        nonlocal attempts
        attempts += 1
        if attempts == 1:
            message = "prepared mirror temporarily unavailable"
            raise RuntimeError(message)
        return prepare(*args, **kwargs)

    monkeypatch.setattr(pair_arrangement, "prepare_pair_control_mirror", fail_once)
    assert rig.controller.begin(0, 215, operation="swap")
    rig.controller.poll()
    original = rig.controller._pair
    assert original is not None
    assert original.acknowledgement is rig.app._audio.captures[0].ack
    ticket, ack = original.ticket, original.acknowledgement
    _poll_complete(rig)

    assert original.ticket is ticket
    assert original.acknowledgement is ack
    assert len(rig.app._audio.captures) == len(rig.app._audio.publish_calls) == 1
    assert len(rig.mirrors) == len(rig.stem_mirrors) == 1
    assert rig.mirrors[0].apply_calls == [ack]


def test_partial_mirror_retry_recommits_newer_unrelated_intent_without_another_swap(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    prepare = pair_arrangement.prepare_pair_control_mirror

    def partial_mirror(*args: object, **kwargs: object) -> _Mirror:
        mirror = prepare(*args, **kwargs)
        mirror.apply_failures = 1
        mirror.partial = True
        return mirror

    monkeypatch.setattr(pair_arrangement, "prepare_pair_control_mirror", partial_mirror)
    assert rig.controller.begin(0, 215, operation="swap")
    rig.controller.poll()
    original = rig.controller._pair
    assert original is not None
    assert original.receipt is not None
    first_revision = original.receipt.revision
    first_assignment = original.assignment
    assert first_assignment not in rig.app._assets.released
    rig.app.project.pad_gain_db[36] = -23.0
    rig.app.project.volume = 0.2
    rig.app.persistence.mark_dirty()
    _poll_complete(rig)

    assert len(rig.mirrors) == 1
    assert len(rig.mirrors[0].apply_calls) == 2
    assert rig.mirrors[0].apply_calls[0] is rig.mirrors[0].apply_calls[1]
    assert len(rig.app._audio.publish_calls) == 1
    assert original.receipt.revision == first_revision + 1 == rig.app.persistence.revision
    saved = original.receipt.projected()
    assert saved.pad_content[0].instance_id == "2" * 32
    assert saved.pad_content[215].instance_id == "1" * 32
    assert saved.pad_gain_db[36] == -23.0
    assert saved.volume == 0.2
    assert first_assignment in rig.app._assets.released
    assert rig.app._assets.published == [original.assignment]
    assert original.journal.journal.phase == "settled"


@pytest.mark.parametrize("partial", [False, True])
def test_owned_journal_append_loss_reconciles_complete_file_or_retains_partial_claim(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    partial: bool,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    native_store = MaterialMigrationJournalStore

    class LossyStore:
        failed = False

        def __init__(self, samples: str, transaction: str) -> None:
            self.native = native_store(samples, transaction)
            self.samples = samples
            self.transaction = transaction

        @staticmethod
        def scan(samples: str) -> list[str]:
            return native_store.scan(samples)

        @classmethod
        def open(cls, samples: str, transaction: str) -> object:
            store = object.__new__(cls)
            store.native = native_store.open(samples, transaction)
            store.samples, store.transaction = samples, transaction
            return store

        def history(self) -> list[str]:
            return self.native.history()

        def append(self, text: str) -> str:
            fail = json.loads(text)["phase"] == "config_committed" and not type(self).failed
            if fail and partial:
                ordinal = len(self.native.history())
                path = (
                    Path(self.samples)
                    / ".material-migrations"
                    / self.transaction
                    / f"{ordinal:02}.json"
                )
                path.write_text(text[:100], encoding="utf-8")
            else:
                path = self.native.append(text)
            if fail:
                type(self).failed = True
                message = "lost append return after actual file write"
                raise OSError(message)
            return str(path)

    monkeypatch.setattr(pair_arrangement_journal, "MaterialMigrationJournalStore", LossyStore)
    assert rig.controller.begin(0, 215, operation="swap")
    original = rig.controller._pair
    assert original is not None
    rig.controller.poll()

    if partial:
        assert rig.controller.active
        assert rig.controller.status == "settlement_pending"
        assert original.journal.intended is not None
        assert original.receipt is not None
        assert original.acknowledgement is not None
        assert original.journal.journal.phase == "native_ack_observed"
        assert rig.app._assets.published == []
        assert rig.app._assets.retirement.closed == 0
        assert rig.app.persistence._migration_owner == original.owner
        rig.controller.poll()
        assert rig.controller._pair is original
        assert original.journal.intended is not None
        assert rig.app._audio.finish_calls == []
    else:
        assert rig.controller.status == "completed", rig.controller.error
        assert original.journal.intended is None
        assert len(original.journal.encoded) == 5
        assert original.journal.journal.phase == "settled"
    assert len(rig.app._audio.captures) == len(rig.app._audio.publish_calls) == 1


def test_config_replace_exception_keeps_native_owner_then_actual_after_recovery_and_retry(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    write = rig.app.persistence._atomic_write_text
    fail = True

    def replace_then_fail(text: str) -> None:
        nonlocal fail
        write(text)
        if fail:
            fail = False
            message = "lost Config commit return after atomic replacement"
            raise OSError(message)

    monkeypatch.setattr(rig.app.persistence, "_atomic_write_text", replace_then_fail)
    assert rig.controller.begin(0, 215, operation="swap")
    rig.controller.poll()
    original = rig.controller._pair
    assert original is not None
    assert original.receipt is None
    assert original.journal.journal.phase == "native_ack_observed"
    decision = inspect_pair_recovery(
        original.journal.journal, rig.app.persistence.config_path.read_bytes()
    )
    assert decision.disposition == "recognized_after"
    assert rig.app.persistence._migration_owner == original.owner
    assert rig.app._audio.cancel_calls == []
    assert rig.app._assets.retirement.closed == 0
    assignment = original.assignment
    publication = original.assignment_publication
    assert assignment is not None
    assert publication is not None
    assert not publication.applied
    assert len(rig.app._assets.prepared) == 1
    _poll_complete(rig)
    assert len(rig.app._audio.captures) == len(rig.app._audio.publish_calls) == 1
    assert original.receipt.revision == rig.app.persistence.revision
    assert len(rig.app._assets.prepared) == 1
    assert rig.app._assets.published == [assignment]
    assert rig.app._assets.released == []
    assert publication.applied
    assert publication.settled


def test_preclaim_shutdown_cancels_original_capture_and_preserves_config_bytes(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    before = rig.app.persistence.config_path.read_bytes()
    assert rig.controller.begin(0, 215, operation="swap")
    original = rig.controller._pair
    assert original is not None
    assert rig.controller.prepare_shutdown()
    assert rig.controller.status == "cancelled"
    assert rig.app._audio.cancel_calls == [original.captured]
    assert rig.app._audio.publish_calls == []
    assert original.journal.journal.phase == "cancelled"
    assert rig.app.persistence._migration_owner is None
    assert rig.app.persistence.config_path.read_bytes() == before
    assert rig.app._assets.retirement.closed == 1


def test_shutdown_after_claim_before_ack_keeps_owner_and_polls_genuine_own_ack(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    rig.app._audio.auto_ack = False
    assert rig.controller.begin(0, 215, operation="swap")
    rig.controller.poll()
    original = rig.controller._pair
    assert original is not None
    assert original.published
    assert original.acknowledgement is None
    assert not rig.controller.prepare_shutdown()
    assert rig.app._assets.retirement.closed == 0
    assert rig.app.persistence._migration_owner == original.owner
    original.captured.acknowledged = True
    _poll_complete(rig)
    assert original.acknowledgement is original.captured.ack
    assert original.journal.journal.phase == "settled"
    assert len(rig.app._audio.publish_calls) == 1


def test_native_finish_retry_keeps_exact_assignment_and_receipt_until_settled(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    rig.app._audio.finish_failures = 1
    assert rig.controller.begin(0, 215, operation="swap")
    rig.controller.poll()
    original = rig.controller._pair
    assert original is not None
    assert original.assets_published
    assert original.receipt is not None
    assignment, receipt, ack = original.assignment, original.receipt, original.acknowledgement
    assert rig.app._assets.retirement.closed == 0
    _poll_complete(rig)
    assert original.assignment is assignment
    assert original.receipt is receipt
    assert rig.app._assets.published == [assignment]
    assert rig.app._audio.finish_calls == [(original.ticket, ack), (original.ticket, ack)]
    assert original.journal.journal.phase == "settled"
