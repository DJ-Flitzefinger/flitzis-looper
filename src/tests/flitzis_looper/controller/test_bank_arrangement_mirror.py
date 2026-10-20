"""Whole-bank Python ownership only; opaque doubles do not prove Native acceptance."""

import json
import threading
from concurrent.futures import Future
from dataclasses import dataclass, replace
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.bank_snapshot import capture_bank_snapshot
from flitzis_looper.constants import NUM_PADS
from flitzis_looper.controller import (
    bank_arrangement_mirror,
    committed_stem_residency,
    loader,
    pair_control_mirror,
    stems,
)
from flitzis_looper.controller.bank_arrangement_journal import BankArrangementJournal
from flitzis_looper.controller.bank_arrangement_mirror import (
    prepare_bank_arrangement_mirror,
    validate_bank_mirror_admission,
)
from flitzis_looper.controller.pad_selection import record_pad_selection
from flitzis_looper.controller.stem_pair_preparation import PairPreparation
from flitzis_looper.input_mapping.actions import LooperAction
from flitzis_looper.input_mapping.bindings import MidiBinding
from flitzis_looper.input_mapping.storage import MidiMappingEntry, MidiMappingFile
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.controller.test_committed_stem_residency import (
    _entry,
    _RegistrationTicket,
)
from tests.flitzis_looper.controller.test_pair_control_mirror import _bundle, _pending
from tests.flitzis_looper.controller.test_pair_stem_mirror import _add_job
from tests.flitzis_looper.controller.test_stem_job import _Assets, _Lease, _Reservation

if TYPE_CHECKING:
    from collections.abc import Callable
    from contextlib import AbstractContextManager
    from types import TracebackType

    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        ProjectAssetLifecycle,
    )
    from flitzis_looper.controller.bank_arrangement_mirror import PreparedBankMirror
    from flitzis_looper.input_mapping.controller import InputMappingController
    from flitzis_looper_audio import (
        BankMutationTicket,
        CommittedStemSetTicket,
        ConstantTimingTicket,
        OfflineStemSourceLease,
        PreparedSourceTicket,
        ProjectAssetLease,
    )
    from tests.flitzis_looper.controller.test_pair_control_mirror import _Bundle


@dataclass
class _BankTicket:
    """Observe an ordered accepted image, without creating a Native permission."""

    metadata: dict[str, object]
    registrations: list[CommittedStemSetTicket | None]

    def metadata_json(self) -> str:
        return json.dumps(self.metadata)

    def committed_stem_tickets(self) -> list[CommittedStemSetTicket | None]:
        return self.registrations.copy()


@dataclass
class _Harness:
    controls: _Bundle
    assets: _Assets
    journal: BankArrangementJournal
    ticket: _BankTicket

    def prepare(self, validator: Callable[[object], bool] | None = None) -> PreparedBankMirror:
        return prepare_bank_arrangement_mirror(
            self.controls.app,
            self.journal,
            cast("BankMutationTicket", self.ticket),
            validate_acknowledgement=(
                validator
                if validator is not None
                else lambda value: value is self.controls.acknowledgement
            ),
            waveform_views=(self.controls.view,),
        )


def _harness(tmp_path: Path, *, full: bool = False, clear: bool = False) -> _Harness:
    controls = _bundle(occupied=True)
    app = controls.app
    project = app._project
    source_ids = range(NUM_PADS) if full else (0,)
    for sample_id in (*source_ids, *range(NUM_PADS, 2 * NUM_PADS)):
        if sample_id != 0:
            project.pad_content[sample_id] = PadContentIdentity(
                instance_id=f"{sample_id + 4096:032x}"
            )
            project.sample_paths[sample_id] = f"samples/source-{sample_id}.wav"
        project.stem_cache[sample_id] = _entry()
    project.selected_bank = 0
    assets = _Assets()
    # Both the ordinary controllers and the real Stem controller share these exact models.
    app.stems = stems.StemController(
        project,
        app._session,
        app._audio,
        stem_task_runner=lambda _task: None,
        asset_lifecycle=cast("ProjectAssetLifecycle", assets),
    )
    snapshot = capture_bank_snapshot(project, None if clear else 0, 1)
    image = project.model_dump_json()
    journal = BankArrangementJournal(
        transaction_id="e" * 32,
        config_reference=str(tmp_path / "bank.config.json"),
        captured_revision=project.config_revision,
        intent_revision=project.config_revision,
        captured_config_sha256=None,
        bank=snapshot,
        captured_project_image_json=image,
        project_image_json=image,
        image_role="before",
        phase="captured",
    )
    rows: list[dict[str, object]] = []
    registrations: list[CommittedStemSetTicket | None] = []
    for row in snapshot.projected().slots:
        occupied = row.after.source_identity is not None
        rows.append({
            "slot": row.target_id,
            "source_generation": row.target_id + 100 if occupied else 0,
            "after_lifetime": row.target_id + 1000 if occupied else 0,
            "components_available": occupied,
        })
        registrations.append(
            cast("CommittedStemSetTicket", _RegistrationTicket(state="accepted"))
            if occupied
            else None
        )
    return _Harness(
        controls,
        assets,
        journal,
        _BankTicket(
            {
                "status": "accepted",
                "control_mirrored": True,
                "source_bank": None if clear else 0,
                "target_bank": 1,
                "targets": rows,
            },
            registrations,
        ),
    )


def _owned_images(harness: _Harness) -> tuple[object, ...]:
    app = harness.controls.app
    return (
        app._project.pad_content,
        app._project.sample_paths,
        app._session.active_sample_ids,
        app._session.pad_peak,
        app.transport.residency._pending,
        app.accepted_timing._publications,
        app.stems._generation_sources,
        app.stems._committed_residency._selections,
    )


def _assert_same_images(harness: _Harness, before: tuple[object, ...]) -> None:
    assert all(
        current is original
        for current, original in zip(_owned_images(harness), before, strict=True)
    )


@pytest.mark.parametrize("clear", [False, True])
def test_complete_bank_store_stops_all_targets_and_preserves_source_and_outside(
    tmp_path: Path, *, clear: bool
) -> None:
    harness = _harness(tmp_path, clear=clear)
    app = harness.controls.app
    source = app._project.pad_content[0]
    outside = app._project.pad_content[215]
    defaults = type(app._session)()
    for sample_id in range(NUM_PADS, 2 * NUM_PADS):
        app._session.active_sample_ids.add(sample_id)
        app._session.paused_sample_ids.add(sample_id)
        app._session.global_stop_restore_sample_ids.add(sample_id)
        app._session.pressed_pads[sample_id] = True
        app._session.pad_peak[sample_id] = 0.8
        app._session.pad_peak_updated_at[sample_id] = 10.0
        app._session.pad_clip_hold_until[sample_id] = 11.0
        app._session.pad_playhead_s[sample_id] = 2.0
        app._session.pad_playhead_updated_at[sample_id] = 12.0
    app._session.active_sample_ids.update({0, 215})
    source_pending = _pending()
    app.transport.residency._pending[0] = source_pending
    plan = harness.prepare()

    candidate = plan.projected_project()
    assert app._project.pad_content[36] != candidate.pad_content[36]
    plan.apply(harness.controls.acknowledgement)

    assert plan.applied
    assert app._project.pad_content[0] is source
    assert app._project.pad_content[215] is outside
    assert app.transport.residency._pending[0] is source_pending
    assert app._session.active_sample_ids == {0, 215}
    assert not app._session.paused_sample_ids
    assert not app._session.global_stop_restore_sample_ids
    for sample_id in range(NUM_PADS, 2 * NUM_PADS):
        assert app._session.pressed_pads[sample_id] is False
        assert app._session.pad_peak[sample_id] == defaults.pad_peak[sample_id]
        assert (
            app._session.pad_peak_updated_at[sample_id] == (defaults.pad_peak_updated_at[sample_id])
        )
        assert (
            app._session.pad_clip_hold_until[sample_id] == (defaults.pad_clip_hold_until[sample_id])
        )
        assert app._session.pad_playhead_s[sample_id] == defaults.pad_playhead_s[sample_id]
        assert (
            app._session.pad_playhead_updated_at[sample_id]
            == (defaults.pad_playhead_updated_at[sample_id])
        )
        assert app._project.pad_content[sample_id] == candidate.pad_content[sample_id]
    assert app._project.pad_content[37] is None
    if clear:
        assert all(value is None for value in app._project.pad_content[36:72])
    else:
        copied = app._project.pad_content[36]
        assert copied is not None
        assert source is not None
        assert copied.instance_id != source.instance_id
        assert copied.copied_from == source.instance_id
        assert app._project.stem_cache[36] is not None
        assert app._project.stem_cache[36].available is True
    assert plan.settle_retired()
    assert plan.settled
    assert not harness.controls.audio.mock_calls


def test_latest_source_outside_navigation_and_physical_midi_rows_survive(tmp_path: Path) -> None:
    harness = _harness(tmp_path)
    app = harness.controls.app
    plan = harness.prepare()
    source_identity = app._project.pad_content[0]
    assert source_identity is not None
    newest_source = source_identity.model_copy(update={"instance_id": "9" * 32})
    app._project.pad_content[0] = newest_source
    app._project.pad_gain_db[0] = -23.0
    app._project.pad_gain_db[108] = -17.0
    app._project.volume = 0.25
    app._project.selected_bank = 3
    record_pad_selection(app._project, app._session, 108)
    newer_pending = _pending()
    app.transport.residency._pending[108] = newer_pending
    app._session.master_output_peak = 0.91
    app._session.sample_analysis_stage[108] = "Latest unrelated analysis"
    midi = MidiMappingFile(
        mappings=[
            MidiMappingEntry(
                input=MidiBinding(kind="note", channel=1, number=60),
                action=LooperAction.trigger_pad(36),
            ),
        ]
    )
    app.input_mapping = cast("InputMappingController", SimpleNamespace(_midi=midi))
    midi_image = midi.model_dump()
    before_source_key = app._project.pad_key_intent[0]

    candidate = plan.projected_project()
    plan.apply(harness.controls.acknowledgement)

    assert app._project.pad_content[0] is newest_source
    assert app._project.pad_key_intent[0] is before_source_key
    assert app._project.pad_gain_db[0] == candidate.pad_gain_db[0] == -23.0
    assert app._project.pad_gain_db[108] == -17.0
    assert (app._project.selected_pad, app._project.selected_bank) == (108, 3)
    assert app._project.volume == 0.25
    assert app.input_mapping._midi is midi
    assert midi.model_dump() == midi_image
    assert midi.mappings[0].action == LooperAction.trigger_pad(36)
    assert app.transport.residency._pending[108] is newer_pending
    assert app._session.master_output_peak == 0.91
    assert app._session.sample_analysis_stage[108] == "Latest unrelated analysis"
    copied = app._project.pad_content[36]
    assert copied is not None
    assert copied.copied_from == source_identity.instance_id
    assert plan.settle_retired()


def test_registration_presence_does_not_infer_effective_component_availability(
    tmp_path: Path,
) -> None:
    harness = _harness(tmp_path)
    rows = cast("list[dict[str, object]]", harness.ticket.metadata["targets"])
    rows[0]["components_available"] = False
    assert harness.ticket.registrations[0] is not None
    plan = harness.prepare()

    plan.apply(harness.controls.acknowledgement)

    entry = harness.controls.app._project.stem_cache[36]
    assert entry is not None
    assert entry.available is False
    assert plan.settle_retired()


def test_foreign_or_json_ack_cannot_exchange_any_original_image(tmp_path: Path) -> None:
    harness = _harness(tmp_path)
    plan = harness.prepare()
    before = _owned_images(harness)
    for acknowledgement in (object(), {"status": "accepted", "control_mirrored": True}):
        with pytest.raises(RuntimeError, match=r"ACK|acknowledgement|receipt"):
            plan.apply(acknowledgement)
        _assert_same_images(harness, before)
        assert not plan.applied
    plan.apply(harness.controls.acknowledgement)
    images = _owned_images(harness)
    plan.apply(harness.controls.acknowledgement)
    _assert_same_images(harness, images)
    assert plan.settle_retired()


@pytest.mark.parametrize("after_store", [False, True])
@pytest.mark.parametrize("owner_name", ["project", "session", "stems"])
def test_interrupted_store_retries_its_own_image_without_losing_newer_unrelated_rows(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, owner_name: str, *, after_store: bool
) -> None:
    harness = _harness(tmp_path)
    app = harness.controls.app
    _add_job(app.stems, harness.assets, 36)
    app._session.active_sample_ids.add(36)
    plan = harness.prepare()
    owner, field = {
        "project": (app._project, "pad_content"),
        "session": (app._session, "active_sample_ids"),
        "stems": (app.stems, "_generation_sources"),
    }[owner_name]
    store = pair_control_mirror._store
    interrupted = False

    def interrupt(current: object, name: str, image: object) -> None:
        nonlocal interrupted
        if current is owner and name == field and not interrupted:
            interrupted = True
            if after_store:
                store(current, name, image)
            message = "interrupted Bank mirror store"
            raise RuntimeError(message)
        store(current, name, image)

    monkeypatch.setattr(bank_arrangement_mirror, "_store", interrupt)
    with pytest.raises(RuntimeError, match="interrupted Bank mirror store"):
        plan.apply(harness.controls.acknowledgement)
    assert interrupted
    assert not plan.applied
    newest = _pending()
    app.transport.residency._pending[108] = newest
    app._project.pad_gain_db[108] = -29.0
    record_pad_selection(app._project, app._session, 108)
    with pytest.raises(RuntimeError, match=r"ACK|acknowledgement|receipt"):
        plan.apply(object())

    plan.apply(harness.controls.acknowledgement)

    assert plan.applied
    assert app.transport.residency._pending[108] is newest
    assert app._project.pad_gain_db[108] == -29.0
    assert app._project.selected_pad == 108
    assert app._project.pad_content[36] == (
        harness.journal.bank.projected().slots[0].after.source_identity
    )
    assert plan.settle_retired()


def test_the_36th_copied_registration_failure_precedes_every_store(tmp_path: Path) -> None:
    harness = _harness(tmp_path, full=True)
    registration = cast("_RegistrationTicket", harness.ticket.registrations[-1])
    plan = harness.prepare()
    registration.state = "rejected"
    before = _owned_images(harness)
    musical = harness.controls.app._project.model_dump_json()

    with pytest.raises(RuntimeError, match=r"registration|accepted|current"):
        plan.apply(harness.controls.acknowledgement)

    _assert_same_images(harness, before)
    assert harness.controls.app._project.model_dump_json() == musical
    assert all(
        cast("_RegistrationTicket", value).cancellations == 0
        for value in harness.ticket.registrations
    )
    registration.state = "accepted"
    plan.apply(harness.controls.acknowledgement)
    assert plan.applied
    assert plan.settle_retired()


@pytest.mark.parametrize("damage", ["availability", "missing", "last_slot", "duplicate", "size"])
def test_incomplete_native_target_observation_cannot_become_a_mirror(
    tmp_path: Path, damage: str
) -> None:
    harness = _harness(tmp_path)
    rows = cast("list[dict[str, object]]", harness.ticket.metadata["targets"])
    if damage == "availability":
        rows[0]["components_available"] = 1
    elif damage == "missing":
        rows[-1].pop("components_available")
    elif damage == "last_slot":
        rows[-1]["slot"] = 215
    elif damage == "duplicate":
        rows[-1]["slot"] = rows[0]["slot"]
    else:
        rows.pop()
    before = _owned_images(harness)
    with pytest.raises(ValueError, match=r"Bank|bank"):
        harness.prepare()
    _assert_same_images(harness, before)


@pytest.mark.parametrize("damage", ["target_music", "outside_identity"])
def test_sealed_target_and_whole_project_identity_guards_precede_stores(
    tmp_path: Path, damage: str
) -> None:
    harness = _harness(tmp_path)
    app = harness.controls.app
    plan = harness.prepare()
    if damage == "target_music":
        app._project.pad_key_intent[71].__dict__["extra_shift"] = 7
    else:
        identity = harness.journal.bank.projected().slots[0].after.source_identity
        assert identity is not None
        app._project.pad_content[108] = identity
    before = _owned_images(harness)
    with pytest.raises((RuntimeError, ValueError)):
        plan.apply(harness.controls.acknowledgement)
    _assert_same_images(harness, before)


class _FailingLock:
    def __init__(self, lock: AbstractContextManager[object], entered: list[int]) -> None:
        self._lock = lock
        self._entered = entered

    def __enter__(self) -> object:
        self._entered[0] += 1
        if self._entered[0] == NUM_PADS:
            message = "36th Bank subscriber lock unavailable"
            raise RuntimeError(message)
        return self._lock.__enter__()

    def __exit__(
        self,
        exception_type: type[BaseException] | None,
        exception: BaseException | None,
        traceback: TracebackType | None,
    ) -> bool | None:
        return self._lock.__exit__(exception_type, exception, traceback)


def test_the_36th_job_lock_failure_leaves_every_subscriber_and_controller_image_owned(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path)
    app = harness.controls.app
    jobs = tuple(_add_job(app.stems, harness.assets, sample_id) for sample_id in range(36, 72))
    plan = harness.prepare()
    before = _owned_images(harness)
    subscriber_images = tuple(job._subscribers for job in jobs)
    entered = [0]
    for job in jobs:
        monkeypatch.setattr(job, "_lock", _FailingLock(job._lock, entered))

    with pytest.raises(RuntimeError, match="36th Bank subscriber lock unavailable"):
        plan.apply(harness.controls.acknowledgement)

    assert entered[0] == NUM_PADS
    _assert_same_images(harness, before)
    assert all(
        job._subscribers is image for job, image in zip(jobs, subscriber_images, strict=True)
    )
    assert all(not lease.released for lease in harness.assets.leases)
    assert not plan.applied
    monkeypatch.undo()
    plan.apply(harness.controls.acknowledgement)
    assert plan.settle_retired()


def test_shared_job_loses_only_target_interest_without_cloning_the_source_worker(
    tmp_path: Path,
) -> None:
    harness = _harness(tmp_path)
    app = harness.controls.app
    source_content = app._project.pad_content[0]
    target_content = app._project.pad_content[36]
    assert source_content is not None
    assert target_content is not None
    app._project.pad_content[36] = source_content.model_copy(
        update={
            "instance_id": target_content.instance_id,
            "copied_from": source_content.instance_id,
        }
    )
    app._project.sample_paths[36] = app._project.sample_paths[0]
    image = app._project.model_dump_json()
    harness.journal = harness.journal.changed(
        bank=capture_bank_snapshot(app._project, 0, 1),
        captured_project_image_json=image,
        project_image_json=image,
    )
    job = _add_job(app.stems, harness.assets)
    original = job.subscribers()[0]
    target_content = app._project.pad_content[36]
    assert target_content is not None
    job.add_subscriber(
        36,
        cast("OfflineStemSourceLease", object()),
        target_content.instance_id,
        request=replace(job.request, sample_id=36),
    )
    target = next(subscriber for subscriber in job.subscribers() if subscriber.sample_id == 36)
    app.stems._generation_sources[36] = target.source
    job.resolved_request = job.request
    assert job.begin() is job.source
    physical = harness.assets.leases[0]
    job_image = app.stems._jobs
    plan = harness.prepare()

    plan.apply(harness.controls.acknowledgement)
    assert plan.settle_retired()

    assert app.stems._jobs is job_image
    assert app.stems._jobs[job.request.cache_dir] is job
    assert job.subscribers() == (original,)
    assert original.source is job.source
    assert not job.cancel_event.is_set()
    assert not physical.released
    assert cast("_Lease", target.source_lease).released
    assert not cast("_Lease", original.source_lease).released
    assert 36 not in app.stems._generation_sources
    assert app.stems._generation_sources[0] is original.source
    assert job.finish_read()
    job.cancel()


def test_failed_target_interest_release_retains_reader_and_retries_exact_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path)
    app = harness.controls.app
    job = _add_job(app.stems, harness.assets, 36)
    job.resolved_request = job.request
    reader = job.source
    assert job.begin() is reader
    physical = harness.assets.leases[0]
    subscriber = job.subscribers()[0]
    lease = cast("_Lease", subscriber.source_lease)
    release = lease.release
    failed = False

    def fail_once() -> None:
        nonlocal failed
        if not failed:
            failed = True
            message = "captured target lease release interrupted"
            raise RuntimeError(message)
        release()

    monkeypatch.setattr(lease, "release", fail_once)
    plan = harness.prepare()
    plan.apply(harness.controls.acknowledgement)

    with pytest.raises(RuntimeError, match="captured target lease release interrupted"):
        plan.settle_retired()
    assert failed
    assert not plan.settled
    assert job.running
    assert job.source is reader
    assert not physical.released
    assert not lease.released
    successor = _pending()
    app.transport.residency._pending[36] = successor
    assert plan.settle_retired()
    assert plan.settled
    assert app.transport.residency._pending[36] is successor
    assert lease.released
    assert job.source is reader
    assert not physical.released
    assert not job.finish_read()
    assert physical.released


def _preparation(harness: _Harness) -> PairPreparation:
    app = harness.controls.app
    entry = app._project.stem_cache[36]
    identity = app._project.pad_content[36]
    assert entry is not None
    assert identity is not None
    return PairPreparation(
        36,
        entry,
        cast("PreparedSourceTicket", object()),
        identity.instance_id,
        entry,
        cast("AssetRetirementReservation", _Reservation()),
        cast("ProjectAssetLease", harness.assets.acquire(Path(entry.cache_dir))),
        threading.Event(),
        source_path=app._project.sample_paths[36],
    )


@pytest.mark.parametrize(
    "blocked", ["pair_preparation", "disk_selection", "incoming_load", "load_retirement"]
)
def test_public_preflight_preserves_pending_worker_or_durable_retirement_owners(
    tmp_path: Path, blocked: str
) -> None:
    harness = _harness(tmp_path)
    app = harness.controls.app
    preparation = _preparation(harness)
    if blocked == "pair_preparation":
        app.stems._pair_preparations.pending[36] = preparation
    elif blocked == "disk_selection":
        app.stems._disk_selection_settlements[36] = stems._DiskSelectionSettlement(
            preparation,
            preparation.entry,
            preparation.ticket,
            preparation.lease,
            None,
            preparation.retirement,
        )
    elif blocked == "incoming_load":
        app.loader._incoming_loads[36] = loader._IncomingLoad(
            loader._IncomingIntent(request_id=19, revision=2, override=None, key_lock=True)
        )
    else:
        app.loader._load_retirements[36, 19] = preparation.retirement
    before = _owned_images(harness)
    musical = app._project.model_dump_json()

    with pytest.raises(RuntimeError, match=r"pending|settlement|retirement|preparation"):
        validate_bank_mirror_admission(app, harness.journal)

    _assert_same_images(harness, before)
    assert app._project.model_dump_json() == musical
    assert not preparation.cancelled.is_set()
    assert not cast("_Reservation", preparation.retirement).closed
    assert not cast("_Lease", preparation.lease).released


def test_a_new_target_collector_blocks_apply_before_any_prepared_store(tmp_path: Path) -> None:
    harness = _harness(tmp_path)
    plan = harness.prepare()
    app = harness.controls.app
    preparation = _preparation(harness)
    app.stems._pair_preparations.pending[36] = preparation
    before = _owned_images(harness)

    with pytest.raises(RuntimeError, match="unsettled preparation"):
        plan.apply(harness.controls.acknowledgement)

    _assert_same_images(harness, before)
    assert not plan.applied
    assert app.stems._pair_preparations.pending[36] is preparation
    assert not preparation.cancelled.is_set()
    assert not cast("_Lease", preparation.lease).released
    assert not cast("_Reservation", preparation.retirement).closed


def test_retirement_cancels_captured_future_without_cancelling_a_new_target_future(
    tmp_path: Path,
) -> None:
    harness = _harness(tmp_path)
    app = harness.controls.app
    original: Future[ConstantTimingTicket] = Future()
    app.accepted_timing._preparations[36] = original
    entry = app._project.stem_cache[36]
    content = app._project.pad_content[36]
    assert entry is not None
    assert content is not None
    old_registration = _RegistrationTicket(state="accepted")
    app.stems._committed_residency._selections[36] = committed_stem_residency._CommittedSelection(
        entry,
        content.instance_id,
        app._project.sample_paths[36],
        cast("CommittedStemSetTicket", old_registration),
    )
    plan = harness.prepare()
    plan.apply(harness.controls.acknowledgement)
    successor: Future[ConstantTimingTicket] = Future()
    app.accepted_timing._preparations[36] = successor

    assert plan.settle_retired()

    assert original.cancelled()
    assert app.accepted_timing._preparations[36] is successor
    assert not successor.cancelled()
    assert old_registration.cancellations > 0
    assert cast("_RegistrationTicket", harness.ticket.registrations[0]).cancellations == 0
