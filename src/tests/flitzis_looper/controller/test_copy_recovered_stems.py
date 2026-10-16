"""Controller restore ownership/readiness; scripted receipts are not Native ACK proof."""

from contextlib import nullcontext
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stems import StemController
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from tests.flitzis_looper.controller.test_committed_stem_residency import (
    _entry,
    _RegistrationTicket,
    _ResidencyReceipt,
)

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

    from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
    from flitzis_looper.models import StemCacheEntry
    from flitzis_looper_audio import AudioEngine, PreparedStemPair, ProjectAssetLease

_TARGET = 215


@dataclass(slots=True)
class _SourceTicket:
    current: bool = True

    def same_source_request(self, other: object) -> bool:
        return other is self and self.current


class _Prepared:
    def __init__(self, entry: StemCacheEntry) -> None:
        self.entry = entry
        self.selected = 0
        self.discarded = 0

    @property
    def native_view(self) -> PreparedStemPair:
        return cast("PreparedStemPair", self)

    def selection_json(self) -> str:
        assert self.entry.pair is not None
        return self.entry.pair.model_dump_json()

    def select(self) -> None:
        self.selected += 1

    def discard(self) -> None:
        self.discarded += 1

    @staticmethod
    def has_components() -> bool:
        return False


class _Lease:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.reader = (path / "vocals.wav").open("rb")
        self.released = False

    def release(self) -> None:
        self.reader.close()
        self.released = True


@dataclass(slots=True)
class _Retirement:
    closed: int = 0
    splits: list[_Retirement] = field(default_factory=list)

    def activate(self) -> nullcontext[_Retirement]:
        return nullcontext(self)

    def split(self, capacity: int) -> _Retirement:
        assert capacity == 16
        owner = _Retirement()
        self.splits.append(owner)
        return owner

    def close(self) -> None:
        self.closed += 1


class _Assets:
    def __init__(self) -> None:
        self.leases: list[_Lease] = []
        self.adoptions: list[tuple[int, tuple[Path, ProjectAssetLease]]] = []
        self.syncs = 0

    @staticmethod
    def reserve_pending(capacity: int) -> _Retirement:
        assert capacity == 16
        return _Retirement()

    def acquire(self, path: Path) -> _Lease:
        lease = _Lease(path)
        self.leases.append(lease)
        return lease

    def adopt_stems(self, sample_id: int, prepared: tuple[Path, ProjectAssetLease]) -> None:
        self.adoptions.append((sample_id, prepared))

    def sync_assignments(self) -> None:
        self.syncs += 1


@dataclass(slots=True)
class _Fixture:
    controller: StemController
    project: ProjectState
    session: SessionState
    entry: StemCacheEntry
    audio: Mock
    source: _SourceTicket
    prepared: _Prepared
    registration: _RegistrationTicket
    residency: _ResidencyReceipt
    jobs: list[Callable[[], None]]
    assets: _Assets
    changed: Mock


def _fixture(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> _Fixture:
    monkeypatch.chdir(tmp_path)
    entry = _entry().model_copy(update={"available": False})
    cache = tmp_path / entry.cache_dir
    cache.mkdir(parents=True)
    for kind in ("vocals", "melody", "bass", "drums", "instrumental"):
        (cache / f"{kind}.wav").write_bytes(f"held {kind}".encode())
    project = ProjectState(preload_stem_loops_on_startup=False)
    project.sample_paths[_TARGET] = entry.source_version.split("|", 1)[0]
    project.pad_content[_TARGET] = PadContentIdentity(instance_id="f" * 32, material_id="a" * 32)
    project.stem_cache[_TARGET] = entry
    session = SessionState()
    audio = Mock(
        spec=[
            "prepare_stem_pair",
            "capture_prepared_source",
            "register_committed_stem_pair",
            "request_committed_stem_residency",
        ]
    )
    source = _SourceTicket()
    prepared = _Prepared(entry)
    registration = _RegistrationTicket()
    residency = _ResidencyReceipt()
    audio.capture_prepared_source.return_value = source
    audio.prepare_stem_pair.return_value = prepared
    audio.register_committed_stem_pair.return_value = registration
    audio.request_committed_stem_residency.return_value = residency
    jobs: list[Callable[[], None]] = []
    changed = Mock()
    assets = _Assets()
    controller = StemController(
        project,
        session,
        cast("AudioEngine", audio),
        changed,
        stem_task_runner=jobs.append,
        asset_lifecycle=cast("ProjectAssetLifecycle", assets),
    )
    return _Fixture(
        controller=controller,
        project=project,
        session=session,
        entry=entry,
        audio=audio,
        source=source,
        prepared=prepared,
        registration=registration,
        residency=residency,
        jobs=jobs,
        assets=assets,
        changed=changed,
    )


def _prepare_restored(fixture: _Fixture) -> None:
    controller = fixture.controller
    assert not controller.recovered_copy_selection_ready(_TARGET, fixture.entry)
    controller._poll_residency_demands()
    assert len(fixture.jobs) == 1
    request = controller._pair_preparations.pending[_TARGET]
    assert request.verify_saved_selection
    fixture.jobs.pop()()
    controller._poll_pair_preparations()


def test_saved_pair_verifies_and_registers_under_real_copy_writer_without_config_commit(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    fixture = _fixture(tmp_path, monkeypatch)
    persistence = ProjectPersistence(fixture.project)
    persistence.config_path = tmp_path / "samples" / "copy.config.json"
    persistence.flush()
    persistence.capture_migration("a" * 32)
    fixture.controller.bind_disk_selection_commit(persistence.commit_stem_selection)
    before = persistence.config_path.read_bytes()
    original_entry = fixture.project.stem_cache[_TARGET]
    _prepare_restored(fixture)
    fixture.audio.prepare_stem_pair.assert_called_once()
    assert fixture.entry.pair is not None
    expected_arguments = (
        _TARGET,
        fixture.entry.source_version,
        fixture.entry.cache_dir,
        fixture.source,
        False,
        fixture.entry.pair.descriptor_reference,
        None,
    )
    assert fixture.audio.prepare_stem_pair.call_args.args == expected_arguments
    assert persistence._migration_owner == "a" * 32
    assert persistence.config_path.read_bytes() == before
    assert fixture.project.stem_cache[_TARGET] is original_entry
    fixture.changed.assert_not_called()
    assert not fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)
    assert len(fixture.assets.adoptions) == 1
    fixture.registration.state = "accepted"
    assert fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)
    fixture.audio.request_committed_stem_residency.assert_not_called()


def test_registration_pressure_retains_same_preparation_and_disk_lease_until_admission(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    fixture = _fixture(tmp_path, monkeypatch)
    fixture.audio.register_committed_stem_pair.side_effect = [
        RuntimeError("committed registration queue is full"),
        fixture.registration,
    ]
    _prepare_restored(fixture)
    controller = fixture.controller
    settlement = controller._disk_selection_settlements[_TARGET]
    retained_lease = settlement.lease
    assert settlement.request.prepared is fixture.prepared.native_view
    assert not retained_lease.released
    assert fixture.prepared.discarded == 0
    assert not controller.recovered_copy_selection_ready(_TARGET, fixture.entry)
    controller._poll_disk_selection_settlements()
    assert _TARGET not in controller._disk_selection_settlements
    assert not retained_lease.released
    assert len(fixture.assets.adoptions) == 1
    assert fixture.audio.prepare_stem_pair.call_count == 1
    calls = fixture.audio.register_committed_stem_pair.call_args_list
    assert calls[0] == calls[1]
    assert calls[0].args == (fixture.prepared, fixture.source)
    fixture.registration.state = "accepted"
    assert controller.recovered_copy_selection_ready(_TARGET, fixture.entry)


@pytest.mark.parametrize("state", ["captured", "pending", "rejected", "accepted"])
def test_exact_registered_selection_requires_current_own_ack(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, state: str
) -> None:
    fixture = _fixture(tmp_path, monkeypatch)
    _prepare_restored(fixture)
    fixture.registration.state = state
    assert fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry) is (
        state == "accepted"
    )
    assert fixture.controller.stems_available(_TARGET) is (state == "accepted")
    assert fixture.project.stem_cache[_TARGET] is fixture.entry
    assert fixture.entry.available is False
    fixture.registration.current = False
    assert not fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)
    assert not fixture.controller.stems_available(_TARGET)


def test_all_stems_waits_for_matching_mask_and_actual_current_component_ack(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    fixture = _fixture(tmp_path, monkeypatch)
    fixture.project.pad_stem_mix_mode[_TARGET] = "all_stems"
    _prepare_restored(fixture)
    fixture.registration.state = "accepted"
    assert fixture.controller.stems_available(_TARGET)
    fixture.controller._poll_residency_demands()
    receipt = fixture.residency
    assert not fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)
    receipt.residency = "accepted"
    assert fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)
    selection = fixture.controller._committed_residency._selections[_TARGET]
    selection.receipt_intent = ("all_stems", 1)
    assert not fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)
    selection.receipt_intent = ("all_stems", fixture.project.pad_stem_intent[_TARGET].enabled_mask)
    receipt.current = False
    assert not fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)
    receipt.current = True
    receipt.failure_reason = "native-preparation-failed"
    assert not fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)


def test_preload_interest_requires_its_own_ack_only_when_enabled(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    fixture = _fixture(tmp_path, monkeypatch)
    fixture.project.preload_stem_loops_on_startup = True
    _prepare_restored(fixture)
    fixture.registration.state = "accepted"
    fixture.controller._poll_residency_demands()
    fixture.audio.request_committed_stem_residency.assert_called_once_with(
        fixture.registration, mode="full_mix", interest="preload"
    )
    receipt = fixture.residency
    assert not fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)
    receipt.residency = "accepted"
    assert fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)
    receipt.current = False
    assert not fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)
    fixture.project.preload_stem_loops_on_startup = False
    assert fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)


@pytest.mark.parametrize("damage", ["pair", "source_version", "content", "path"])
def test_changed_saved_pair_or_binding_cannot_certify_recovery(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, damage: str
) -> None:
    fixture = _fixture(tmp_path, monkeypatch)
    _prepare_restored(fixture)
    fixture.registration.state = "accepted"
    if damage == "pair":
        fixture.project.stem_cache[_TARGET] = _entry(generation="c")
    elif damage == "source_version":
        fixture.project.stem_cache[_TARGET] = fixture.entry.model_copy(
            update={"source_version": "changed"}
        )
    elif damage == "content":
        fixture.project.pad_content[_TARGET] = PadContentIdentity(instance_id="8" * 32)
    else:
        fixture.project.sample_paths[_TARGET] = "samples/other.wav"
    assert not fixture.controller.recovered_copy_selection_ready(_TARGET, fixture.entry)


@pytest.mark.parametrize("held", [False, True])
@pytest.mark.parametrize("version", [None, "changed-source"])
def test_restore_guard_preserves_journal_held_stem_metadata_for_missing_or_changed_source(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    version: str | None,
    *,
    held: bool,
) -> None:
    fixture = _fixture(tmp_path, monkeypatch)
    fixture.controller.bind_restored_assignment_guard(
        lambda sample_id: held and sample_id == _TARGET
    )
    monkeypatch.setattr(fixture.controller, "source_version_for_pad", lambda sample_id: version)
    original = fixture.project.stem_cache[_TARGET]
    fixture.controller.restore_stem_cache_from_project_state()
    if held:
        assert fixture.project.stem_cache[_TARGET] is original
        fixture.changed.assert_not_called()
        assert "transaction recovery" in fixture.session.stem_generation_errors[_TARGET]
    else:
        assert fixture.project.stem_cache[_TARGET] is None
        fixture.changed.assert_called_once()


def test_absent_cache_is_ready_but_unknown_cached_pair_is_visible_and_fenced(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    fixture = _fixture(tmp_path, monkeypatch)
    assert fixture.controller.recovered_copy_selection_ready(_TARGET, None)
    unknown = fixture.entry.model_copy(update={"pair": None})
    fixture.project.stem_cache[_TARGET] = unknown
    assert not fixture.controller.recovered_copy_selection_ready(_TARGET, unknown)
    assert "unsupported" in fixture.session.stem_generation_errors[_TARGET]
    assert fixture.jobs == []
