"""Restored verification keeps exact owners until superseding config is durable."""

from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.controller.test_committed_stem_residency import _RegistrationTicket
from tests.flitzis_looper.controller.test_stem_disk_commit import (
    _assert_independent_copy,
)
from tests.flitzis_looper.controller.test_stem_disk_commit import (
    disk_commit as disk_commit_fixture,
)
from tests.flitzis_looper.controller.test_stem_pair_preparation import _PreparedPair

if TYPE_CHECKING:
    from flitzis_looper.controller.stems import _DiskSelectionSettlement
    from flitzis_looper.models import StemCacheEntry
    from tests.flitzis_looper.conftest import FakeProjectAssetLease
    from tests.flitzis_looper.controller.test_stem_disk_commit import _DiskCommit


REGISTRATION_PRESSURE = "committed registration queue is full"
NEWER_FEEDBACK = "replacement pad feedback"
disk_commit = disk_commit_fixture


@dataclass(slots=True)
class _Restored:
    rig: _DiskCommit
    prepared: _PreparedPair
    settlement: _DiskSelectionSettlement
    adopt: Mock
    register: Mock


def _verify_restored(rig: _DiskCommit, monkeypatch: pytest.MonkeyPatch, boundary: str) -> _Restored:
    app = rig.controller
    app.stems._pair_preparations.cancel(0)
    rig.jobs.pop(0)()
    app.stems._poll_pair_preparations()
    assert not app.stems._pair_preparations.pending
    assert app._assets._reserved == 0
    pair = rig.previous.pair
    assert pair is not None
    prepared = _PreparedPair(components=False, selection=pair.model_dump(mode="json"))
    monkeypatch.setattr(rig.audio, "prepare_stem_pair", Mock(return_value=prepared))
    register = Mock(return_value=_RegistrationTicket())
    monkeypatch.setattr(rig.audio, "register_committed_stem_pair", register, raising=False)
    monkeypatch.setattr(rig.audio, "request_committed_stem_residency", Mock(), raising=False)
    adopt = Mock(wraps=app._assets.adopt_stems)
    monkeypatch.setattr(app._assets, "adopt_stems", adopt)
    failure = adopt if boundary == "adopt" else register
    failure.side_effect = RuntimeError(REGISTRATION_PRESSURE)
    app.project.pad_stem_mix_mode[0] = "full_mix"
    app.project.preload_stem_loops_on_startup = False
    app.persistence.mark_dirty()
    app.persistence.flush()
    assert not app.stems.recovered_copy_selection_ready(0, rig.previous)
    app.stems._poll_residency_demands()
    assert len(rig.jobs) == 1
    request = app.stems._pair_preparations.pending[0]
    assert request.verify_saved_selection
    assert request.previous_entry is rig.previous
    rig.jobs.pop(0)()
    app.stems._poll_pair_preparations()
    settlement = app.stems._disk_selection_settlements[0]
    assert settlement.verified_restore
    assert settlement.request is request
    assert cast("object", settlement.request.prepared) is prepared
    assert app.project.stem_cache[0] is rig.previous
    assert app._assets._reserved == settlement.retirement.remaining == 16
    assert prepared.selected
    assert not prepared.discarded
    return _Restored(rig, prepared, settlement, adopt, register)


def _supersede(restored: _Restored, change: str) -> StemCacheEntry | None:
    app = restored.rig.controller
    replacement = None
    if change == "unload":
        app.loader.unload_sample(0)
    else:
        app.stems.invalidate_stem_cache(0)
        if change == "content":
            app.project.pad_content[0] = PadContentIdentity(instance_id="8" * 32)
            replacement = restored.rig.previous.model_copy(deep=True)
            app.project.stem_cache[0] = replacement
            app._assets.sync_assignments()
            app.persistence.mark_dirty()
    app.session.stem_generation_errors[0] = NEWER_FEEDBACK
    return replacement


def _assert_settled(restored: _Restored, replacement: StemCacheEntry | None) -> None:
    app = restored.rig.controller
    assert 0 not in app.stems._disk_selection_settlements
    assert app._assets._reserved == 0
    assert restored.settlement.retirement.remaining == 0
    assert cast("FakeProjectAssetLease", restored.settlement.lease).released
    assert app.project.stem_cache[0] is replacement
    assert not app.stems._committed_residency.contains(0)
    assert app.session.stem_generation_errors[0] == NEWER_FEEDBACK
    assert not restored.prepared.discarded
    reopened = ProjectPersistence.from_config_path(app.persistence.config_path)
    assert reopened.load_error is None
    assert reopened.project.stem_cache[0] == replacement
    assert reopened.project.pad_content[0] == app.project.pad_content[0]
    assert reopened.project.sample_paths[0] == app.project.sample_paths[0]
    _assert_independent_copy(restored.rig)


@pytest.mark.parametrize("boundary", ["adopt", "register"])
@pytest.mark.parametrize("change", ["cache", "content", "unload"])
def test_superseded_restored_verification_retires_only_old_owner_after_durable_config(
    disk_commit: _DiskCommit,
    monkeypatch: pytest.MonkeyPatch,
    boundary: str,
    change: str,
) -> None:
    restored = _verify_restored(disk_commit, monkeypatch, boundary)
    app = disk_commit.controller
    replacement = _supersede(restored, change)
    before = ProjectPersistence.from_config_path(app.persistence.config_path).project
    assert before.stem_cache[0] == disk_commit.previous
    restored.adopt.side_effect = None
    restored.register.side_effect = None

    app.stems._poll_disk_selection_settlements()

    _assert_settled(restored, replacement)
    assert restored.register.call_count == (0 if boundary == "adopt" else 1)
    app.stems._poll_disk_selection_settlements()
    assert restored.register.call_count == (0 if boundary == "adopt" else 1)
    assert Path(disk_commit.independent.cache_dir).is_dir()


def test_superseded_restored_pair_keeps_same_lease_until_newer_config_write_succeeds(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch
) -> None:
    restored = _verify_restored(disk_commit, monkeypatch, "adopt")
    app = disk_commit.controller
    lease = cast("FakeProjectAssetLease", restored.settlement.lease)
    assert not lease.released
    replacement = _supersede(restored, "content")
    initial = app.persistence.config_path.read_bytes()
    write = Mock(
        wraps=app.persistence._atomic_write_text,
        side_effect=OSError("newer config remains unwritten"),
    )
    monkeypatch.setattr(app.persistence, "_atomic_write_text", write)
    restored.adopt.side_effect = None

    app.stems._poll_disk_selection_settlements()

    assert app.stems._disk_selection_settlements[0] is restored.settlement
    assert write.call_count == 1
    assert app.persistence.config_path.read_bytes() == initial
    assert not lease.released
    assert app._assets._reserved == restored.settlement.retirement.remaining == 16
    assert "newer config remains unwritten" in (restored.settlement.last_error or "")
    assert app.session.stem_generation_errors[0] == NEWER_FEEDBACK
    restored.register.assert_not_called()
    release = lease.release
    releases: list[bool] = []

    def release_after_durable_replacement() -> None:
        reopened = ProjectPersistence.from_config_path(app.persistence.config_path).project
        assert reopened.stem_cache[0] == replacement
        assert reopened.pad_content[0] == app.project.pad_content[0]
        releases.append(True)
        release()

    monkeypatch.setattr(lease, "release", release_after_durable_replacement)
    write.side_effect = None

    app.stems._poll_disk_selection_settlements()

    assert write.call_count == 2
    assert releases == [True]
    _assert_settled(restored, replacement)
    restored.register.assert_not_called()


def test_current_restored_registration_pressure_keeps_exact_preparation_until_retry(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch
) -> None:
    restored = _verify_restored(disk_commit, monkeypatch, "register")
    app = disk_commit.controller
    before = app.persistence.config_path.read_bytes()

    app.stems._poll_disk_selection_settlements()

    assert app.stems._disk_selection_settlements[0] is restored.settlement
    assert app._assets._reserved == 16
    assert restored.register.call_count == 2
    assert app.persistence.config_path.read_bytes() == before
    restored.register.side_effect = None

    app.stems._poll_disk_selection_settlements()

    assert 0 not in app.stems._disk_selection_settlements
    assert app._assets._reserved == 0
    assert restored.register.call_count == 3
    calls = restored.register.call_args_list
    assert calls[0] == calls[1] == calls[2]
    assert app.stems._committed_residency.contains(0)
    assert app.project.stem_cache[0] is disk_commit.previous
    assert app.persistence.config_path.read_bytes() == before
    assert not restored.prepared.discarded
    _assert_independent_copy(disk_commit)
