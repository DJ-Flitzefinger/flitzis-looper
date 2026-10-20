"""Returned Pair custody across the real Config writer; Native proof is separate."""

from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_generation import AudioShape, StemGenerationRequest
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.conftest import FakePreparedSourceTicket
from tests.flitzis_looper.controller.test_committed_stem_residency import _RegistrationTicket
from tests.flitzis_looper.controller.test_stem_disk_commit import (
    _assert_independent_copy,
)
from tests.flitzis_looper.controller.test_stem_disk_commit import (
    disk_commit as disk_commit_fixture,
)
from tests.flitzis_looper.controller.test_stem_pair_preparation import _PreparedPair

if TYPE_CHECKING:
    from flitzis_looper_audio import PreparedSourceTicket
    from tests.flitzis_looper.controller.test_stem_disk_commit import _DiskCommit


disk_commit = disk_commit_fixture
_WRITER = "9" * 32


def _capture_writer(rig: _DiskCommit, *, components: bool = False) -> tuple[Mock, Mock, bytes]:
    app = rig.controller
    app.project.pad_stem_mix_mode[0] = "all_stems" if components else "full_mix"
    app.persistence.mark_dirty()
    app.persistence.flush(now=1.0)
    commit = Mock(wraps=app.persistence.commit_stem_selection)
    blocked = Mock(wraps=app.persistence.stem_selection_write_blocked)
    app.stems.bind_disk_selection_commit(commit, app.persistence.flush, writer_blocked=blocked)
    app.persistence.capture_migration(_WRITER)
    return commit, blocked, app.persistence.config_path.read_bytes()


def _assert_held(rig: _DiskCommit, encoded: bytes) -> None:
    app, request = rig.controller, rig.request
    queue = app.stems._pair_preparations
    assert queue.pending[0] is request
    assert len(queue._held_results) == 1
    assert queue._held_results[0] is request
    assert cast("object", request.prepared) is rig.prepared
    assert not request.lease.released
    assert request.retirement.remaining == app._assets._reserved == 16
    assert not request.cancelled.is_set()
    assert request.error is None
    assert not rig.prepared.selected
    assert not rig.prepared.discarded
    assert app.project.stem_cache[0] is rig.previous
    assert app.persistence.config_path.read_bytes() == encoded
    assert app.persistence._migration_owner == _WRITER
    assert not app.stems._disk_selection_settlements
    assert not app.stems._pending_stem_publications
    assert not app.session.stem_generation_errors
    _assert_independent_copy(rig)


def test_returned_disk_pair_keeps_exact_result_readers_and_reservation_until_writer_release(
    disk_commit: _DiskCommit,
) -> None:
    rig = disk_commit
    app, request = rig.controller, rig.request
    commit, _, encoded = _capture_writer(rig)
    revision = app.persistence.revision
    original_ticket = request.ticket
    original_lease = request.lease
    original_reservation = request.retirement
    app.stems._poll_pair_preparations()
    assert request.prepared is None
    assert not app.stems._pair_preparations._held_results
    rig.audio.prepare_stem_pair.assert_not_called()

    rig.jobs[0]()
    for _ in range(3):
        app.stems._poll_pair_preparations()
        _assert_held(rig, encoded)
        assert request.ticket is original_ticket
        assert request.lease is original_lease
        assert request.retirement is original_reservation
        assert app.persistence.revision == revision
        commit.assert_not_called()
    rig.audio.prepare_stem_pair.assert_called_once()
    assert len(rig.jobs) == 1

    app.persistence.release_migration(_WRITER)
    app.stems._poll_pair_preparations()

    commit.assert_called_once()
    rig.audio.prepare_stem_pair.assert_called_once()
    assert not app.stems._pair_preparations.pending
    assert not app.stems._pair_preparations._held_results
    assert request.lease.released
    assert request.retirement.remaining == app._assets._reserved == 0
    assert rig.prepared.selected
    assert not rig.prepared.discarded
    selected = app.project.stem_cache[0]
    assert selected is not None
    assert selected.pair is not None
    assert selected.pair.stem_set_identity == rig.prepared.selection["stem_set_identity"]
    assert app.persistence.revision == revision + 1
    reopened = ProjectPersistence.from_config_path(app.persistence.config_path)
    assert reopened.load_error is None
    assert reopened.project.stem_cache[0] == selected
    _assert_independent_copy(rig)


@pytest.mark.parametrize("change", ["cancel", "content", "source", "selection", "source_ticket"])
def test_returned_writer_wait_rechecks_cancellation_and_original_assignment_before_retry(
    disk_commit: _DiskCommit, change: str
) -> None:
    rig = disk_commit
    app, request = rig.controller, rig.request
    commit, _, encoded = _capture_writer(rig)
    rig.jobs[0]()
    app.stems._poll_pair_preparations()
    _assert_held(rig, encoded)
    if change == "cancel":
        app.stems._pair_preparations.cancel(0)
    elif change == "content":
        app.project.pad_content[0] = PadContentIdentity(instance_id="8" * 32)
    elif change == "source":
        app.project.sample_paths[0] = "samples/replacement.wav"
    elif change == "selection":
        app.project.stem_cache[0] = rig.previous.model_copy(deep=True)
    else:
        newer_source = FakePreparedSourceTicket()
        newer_source.source_request = 2
        rig.audio.capture_prepared_source.return_value = newer_source
    latest = app.project.model_dump(mode="json", exclude_computed_fields=True)

    # The writer remains busy: stale results must still reach normal cleanup.
    app.stems._poll_pair_preparations()

    assert not app.stems._pair_preparations.pending
    assert not app.stems._pair_preparations._held_results
    assert request.lease.released
    assert request.retirement.remaining == app._assets._reserved == 0
    assert rig.prepared.discarded
    assert not rig.prepared.selected
    assert app.project.model_dump(mode="json", exclude_computed_fields=True) == latest
    assert app.persistence.config_path.read_bytes() == encoded
    commit.assert_not_called()
    if change == "source_ticket":
        assert "source request changed" in app.session.stem_generation_errors[0]
    else:
        assert not app.session.stem_generation_errors
    app.persistence.release_migration(_WRITER)


def test_an_older_returned_waiter_cannot_replace_or_close_a_newer_pending_owner(
    disk_commit: _DiskCommit,
) -> None:
    rig = disk_commit
    app, old = rig.controller, rig.request
    commit, _, encoded = _capture_writer(rig)
    rig.jobs[0]()
    app.stems._poll_pair_preparations()
    _assert_held(rig, encoded)
    queue = app.stems._pair_preparations
    queue.pending.pop(0)
    app.stems._prepare_pair(0, old.entry, old.ticket, components=False)
    newer = queue.pending[0]
    assert newer is not old

    app.stems._poll_pair_preparations()

    assert queue.pending[0] is newer
    assert not queue._held_results
    assert not newer.cancelled.is_set()
    assert not newer.lease.released
    assert newer.retirement.remaining == app._assets._reserved == 16
    assert old.lease.released
    assert old.retirement.remaining == 0
    assert rig.prepared.discarded
    assert not rig.prepared.selected
    commit.assert_not_called()
    assert app.persistence.config_path.read_bytes() == encoded
    queue.cancel(0)
    rig.jobs[-1]()
    app.stems._poll_pair_preparations()
    assert not queue.pending
    assert newer.lease.released
    assert app._assets._reserved == 0
    app.persistence.release_migration(_WRITER)


def test_returned_writer_wait_does_not_revive_a_withdrawn_generation_intent(
    disk_commit: _DiskCommit,
) -> None:
    rig = disk_commit
    app = rig.controller
    source = app.project.sample_paths[0]
    assert source is not None
    generation = StemGenerationRequest(
        sample_id=0,
        source_path=Path(source),
        source_version=rig.previous.source_version,
        cache_dir=Path(rig.request.entry.cache_dir),
        target_shape=AudioShape(44_100, 1, 100),
        model_cache_dir=Path("declared-model"),
    )
    intent = app.stems._intents.capture(generation, app.project.stem_processing_revision)
    rig.request.generation_intent = intent
    commit, _, encoded = _capture_writer(rig)
    rig.jobs[0]()
    app.stems._poll_pair_preparations()
    _assert_held(rig, encoded)

    app.stems._intents.release(intent)
    app.stems._poll_pair_preparations()

    assert not app.stems._pair_preparations.pending
    assert not app.stems._pair_preparations._held_results
    assert rig.request.lease.released
    assert rig.request.retirement.remaining == app._assets._reserved == 0
    assert rig.prepared.discarded
    assert not rig.prepared.selected
    assert app.project.stem_cache[0] is rig.previous
    assert app.persistence.config_path.read_bytes() == encoded
    commit.assert_not_called()
    app.persistence.release_migration(_WRITER)


def test_writer_wait_does_not_hide_a_returned_worker_error(disk_commit: _DiskCommit) -> None:
    rig = disk_commit
    app = rig.controller
    commit, _, encoded = _capture_writer(rig)
    rig.jobs[0]()
    rig.request.error = "declared returned worker failure"

    app.stems._poll_pair_preparations()

    assert not app.stems._pair_preparations.pending
    assert not app.stems._pair_preparations._held_results
    assert rig.request.lease.released
    assert app._assets._reserved == 0
    assert rig.prepared.discarded
    assert "declared returned worker failure" in app.session.stem_generation_errors[0]
    assert app.persistence.config_path.read_bytes() == encoded
    commit.assert_not_called()
    app.persistence.release_migration(_WRITER)


def test_writer_advisory_cannot_bypass_the_actual_config_commit_fence(
    disk_commit: _DiskCommit,
) -> None:
    rig = disk_commit
    app = rig.controller
    commit, blocked, encoded = _capture_writer(rig)
    blocked.side_effect = lambda: False
    rig.jobs[0]()

    app.stems._poll_pair_preparations()

    commit.assert_called_once()
    assert "owns the config writer" in app.session.stem_generation_errors[0]
    assert app.persistence._migration_owner == _WRITER
    assert app.persistence.config_path.read_bytes() == encoded
    assert not app.stems._pair_preparations.pending
    assert not app.stems._pair_preparations._held_results
    assert rig.prepared.discarded
    assert not rig.prepared.selected
    assert app._assets._reserved == 0
    app.persistence.release_migration(_WRITER)


def test_current_all_component_publication_keeps_its_existing_path_under_a_writer(
    disk_commit: _DiskCommit,
) -> None:
    rig = disk_commit
    app = rig.controller
    commit, blocked, encoded = _capture_writer(rig, components=True)
    rig.prepared.components = True
    rig.jobs[0]()

    app.stems._poll_pair_preparations()

    rig.audio.publish_stem_pair.assert_called_once_with(rig.prepared, rig.request.ticket)
    assert not app.stems._pair_preparations.pending
    assert not app.stems._pair_preparations._held_results
    assert cast("object", app.stems._pending_stem_publications[0].prepared_pair) is rig.prepared
    assert not rig.prepared.selected
    assert not rig.prepared.discarded
    assert app.persistence.config_path.read_bytes() == encoded
    commit.assert_not_called()
    blocked.assert_not_called()
    app.persistence.release_migration(_WRITER)


def test_restored_selection_verification_does_not_wait_for_the_config_writer(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch
) -> None:
    rig = disk_commit
    app = rig.controller
    app.stems._pair_preparations.cancel(0)
    rig.jobs.pop(0)()
    app.stems._poll_pair_preparations()
    pair = rig.previous.pair
    assert pair is not None
    prepared = _PreparedPair(components=False, selection=pair.model_dump(mode="json"))
    monkeypatch.setattr(rig.audio, "prepare_stem_pair", Mock(return_value=prepared))
    registration = Mock(return_value=_RegistrationTicket(state="accepted"))
    monkeypatch.setattr(rig.audio, "register_committed_stem_pair", registration, raising=False)
    app.stems._restored_stem_candidates[0] = rig.previous
    ticket = cast("PreparedSourceTicket", FakePreparedSourceTicket())
    app.stems._prepare_pair(0, rig.previous, ticket, components=False)
    request = app.stems._pair_preparations.pending[0]
    assert request.verify_saved_selection
    current_ticket = cast(
        "FakePreparedSourceTicket", rig.audio.capture_prepared_source.return_value
    )
    assert current_ticket.same_source_request(cast("FakePreparedSourceTicket", ticket))
    commit, blocked, encoded = _capture_writer(rig)
    rig.jobs[-1]()

    app.stems._poll_pair_preparations()

    registration.assert_called_once_with(prepared, current_ticket)
    assert request.ticket is ticket
    assert current_ticket.same_source_request(cast("FakePreparedSourceTicket", request.ticket))
    assert prepared.selected
    assert not prepared.discarded
    assert request.lease.released
    assert not app.stems._pair_preparations.pending
    assert not app.stems._pair_preparations._held_results
    assert not app.stems._disk_selection_settlements
    assert app._assets._reserved == 0
    assert app.project.stem_cache[0] is rig.previous
    assert app.persistence.config_path.read_bytes() == encoded
    assert not app.session.stem_generation_errors
    commit.assert_not_called()
    blocked.assert_not_called()
    app.persistence.release_migration(_WRITER)
