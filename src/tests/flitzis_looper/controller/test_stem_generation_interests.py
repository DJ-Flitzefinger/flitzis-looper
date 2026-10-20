"""Real controller/config cancellation boundaries; device/separator fixtures declared."""

import os
import threading
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.stem_job import (
    DIRECT_STEM_INTEREST,
    StemGenerationInterest,
    StemGenerationInterests,
)
from tests.flitzis_looper.controller import test_stem_disk_commit as disk_fixtures
from tests.flitzis_looper.controller.test_shared_stem_material import _shared

disk_commit = disk_fixtures.disk_commit

if TYPE_CHECKING:
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from tests.flitzis_looper.controller.test_stem_disk_commit import _DiskCommit


def test_batch_and_direct_share_the_exact_queued_controller_publication_row(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    tasks, _tickets = _shared(controller, audio_engine_mock, tmp_path)
    token = StemGenerationInterest("batch-one-item-zero")
    owner = controller.stems.request_generation_interest(0, token)
    assert owner is not None
    job = next(iter(controller.stems._jobs.values()))
    row = job.subscribers()[0]
    before = (row, row.source, row.request, row.intent, row.retirement, row.source_lease)

    assert controller.stems.generate_stems_async(0)
    assert controller.stems.request_generation_interest(0, token) is owner
    assert len(tasks) == len(controller.stems._jobs) == 1
    current = job.subscribers()[0]
    assert current is row
    assert (
        current,
        current.source,
        current.request,
        current.intent,
        current.retirement,
        current.source_lease,
    ) == before
    assert row.interests == frozenset({token, DIRECT_STEM_INTEREST})
    assert controller.stems.cancel_generation_interest(owner, token).disposition == "remaining"
    assert job.subscribers()[0] is row
    assert not job.cancel_event.is_set()
    assert controller.stems.generation_interest_is_pending(owner)


def test_final_queued_batch_cancel_has_no_backend_read_and_settles_its_original_route(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    tasks, _tickets = _shared(controller, audio_engine_mock, tmp_path)
    token = StemGenerationInterest("only-batch-item")
    owner = controller.stems.request_generation_interest(0, token)
    assert owner is not None
    job = next(iter(controller.stems._jobs.values()))
    assert controller.stems.cancel_generation_interest(owner, token).disposition == "detached"
    assert job.cancel_event.is_set()
    assert not job.running
    for task in tasks:
        task()
    controller.stems.on_frame_render()
    assert not controller.stems.generation_interest_is_pending(owner)
    assert not controller.stems.is_stem_generation_running(0)
    assert controller.stems.cancel_generation_interest(owner, token).disposition == "absent"
    assert not owner.committed


def _own_pair(rig: _DiskCommit) -> tuple[StemGenerationInterests, StemGenerationInterest]:
    # This fixture supplies the actual source/config writer and an independently
    # declared prepared-pair object; integrity/audio are proved in Native profiles.
    token = StemGenerationInterest("confirmed-batch-item")
    owner = StemGenerationInterests(frozenset({token}))
    rig.request.interest_owner = owner
    return owner, token


def test_final_batch_cancel_before_returned_pair_preserves_previous_config(
    disk_commit: _DiskCommit,
) -> None:
    rig = disk_commit
    owner, token = _own_pair(rig)
    cancellation = rig.controller.stems.cancel_generation_interest(owner, token)
    assert cancellation.disposition == "detached"
    assert rig.request.cancelled.is_set()
    assert rig.controller.stems.generation_cancellation_is_pending(cancellation)
    rig.jobs.pop()()
    rig.controller.stems.on_frame_render()
    assert not rig.controller.stems.generation_cancellation_is_pending(cancellation)
    assert rig.controller.project.stem_cache[0] is rig.previous
    assert rig.controller._persistence.config_path.read_bytes() == rig.encoded
    assert not owner.committed
    assert not rig.prepared.selected


def test_final_cancel_between_returned_pair_projection_and_disk_commit_is_rechecked(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch
) -> None:
    rig = disk_commit
    owner, token = _own_pair(rig)
    capture = rig.controller.stems._capture_source_ticket
    cancellations = []

    def cancel_during_fresh_capture(sample_id: int, source_version: str) -> object:
        assert not rig.controller.stems._pair_preparations.pending
        assert not rig.controller.stems._disk_selection_settlements
        cancellations.append(rig.controller.stems.cancel_generation_interest(owner, token))
        return capture(sample_id, source_version)

    monkeypatch.setattr(rig.controller.stems, "_capture_source_ticket", cancel_during_fresh_capture)
    rig.jobs.pop()()
    rig.controller.stems._poll_pair_preparations()
    assert len(cancellations) == 1
    assert cancellations[0].disposition == "detached"
    assert not owner.current
    assert not owner.committed
    assert rig.controller._persistence.config_path.read_bytes() == rig.encoded
    assert rig.controller.project.stem_cache[0] is rig.previous
    assert rig.prepared.discarded


def test_queued_preparation_with_final_interest_already_gone_performs_no_native_read(
    disk_commit: _DiskCommit,
) -> None:
    rig = disk_commit
    owner, token = _own_pair(rig)
    assert owner.detach(token) == "detached"
    assert not rig.request.cancelled.is_set()
    rig.jobs.pop()()
    rig.controller.stems._poll_pair_preparations()
    rig.audio.prepare_stem_pair.assert_not_called()
    assert rig.request.cancelled.is_set()
    assert not owner.committed
    assert rig.controller._persistence.config_path.read_bytes() == rig.encoded


def test_direct_interest_keeps_the_same_pair_and_real_config_commit(
    disk_commit: _DiskCommit,
) -> None:
    rig = disk_commit
    owner, token = _own_pair(rig)
    assert rig.controller.stems.request_generation_interest(0, DIRECT_STEM_INTEREST) is owner
    assert rig.controller.stems.cancel_generation_interest(owner, token).disposition == "remaining"
    assert not rig.request.cancelled.is_set()
    rig.jobs.pop()()
    rig.controller.stems.on_frame_render()
    assert owner.committed
    assert rig.controller._persistence.config_path.read_bytes() != rig.encoded
    assert rig.controller.project.stem_cache[0] is not rig.previous
    assert rig.prepared.selected


def test_existing_verified_result_reports_noop_without_claiming_a_new_commit(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tasks, _tickets = _shared(controller, audio_engine_mock, tmp_path)
    # Existing-set verification has its own production regressions. This case
    # checks only projection of its authoritative positive result to a new caller.
    monkeypatch.setattr(controller.stems, "_verified_set_current", lambda _sample_id: True)
    token = StemGenerationInterest("already-present-batch-item")
    owner = controller.stems.request_generation_interest(0, token)
    assert owner is not None
    assert owner.already_present
    assert not owner.committed
    assert not tasks
    assert not controller.stems.generation_interest_is_pending(owner)


def test_unbound_control_fixture_does_not_fabricate_durable_commit(
    disk_commit: _DiskCommit,
) -> None:
    rig = disk_commit
    owner, _token = _own_pair(rig)
    rig.controller.stems._commit_disk_selection = None
    rig.jobs.pop()()
    rig.controller.stems._poll_pair_preparations()
    assert not owner.committed
    assert rig.controller._persistence.config_path.read_bytes() == rig.encoded


def test_cancel_after_commit_preserves_exact_deferred_owner_settlement(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch
) -> None:
    rig = disk_commit
    owner, token = _own_pair(rig)
    restore = disk_fixtures._defer_owner_settlement(rig, monkeypatch, "select")
    rig.jobs.pop()()
    rig.controller.stems._poll_pair_preparations()
    settlement = rig.controller.stems._disk_selection_settlements[0]
    committed_bytes = rig.controller._persistence.config_path.read_bytes()
    assert committed_bytes != rig.encoded
    assert owner.committed
    assert owner.failure is None
    cancellation = rig.controller.stems.cancel_generation_interest(owner, token)
    assert cancellation.disposition == "detached"
    assert not cancellation.jobs
    assert not cancellation.preparations
    assert not rig.request.cancelled.is_set()
    assert rig.controller.stems._disk_selection_settlements[0] is settlement
    restore()
    rig.controller.stems._poll_disk_selection_settlements()
    assert not rig.controller.stems._disk_selection_settlements
    assert owner.committed
    assert owner.failure is None
    assert rig.prepared.selected
    assert not rig.prepared.discarded
    assert rig.controller._persistence.config_path.read_bytes() == committed_bytes


def test_final_interest_waits_for_its_actual_reader_and_does_not_wait_for_foreign_reads(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    _shared(controller, audio_engine_mock, tmp_path)
    queued_token = StemGenerationInterest("queued-batch")
    running_token = StemGenerationInterest("running-batch")
    queued_owner = controller.stems.request_generation_interest(0, queued_token)
    running_owner = controller.stems.request_generation_interest(215, running_token)
    assert queued_owner is not None
    assert running_owner is not None
    running_job = next(
        job for job in controller.stems._jobs.values() if job.request.sample_id == 215
    )
    entered = threading.Event()
    release = threading.Event()
    returned = threading.Event()
    read_bytes: list[bytes] = []

    def read_original() -> None:
        source = running_job.begin_resolution()
        assert source is not None
        try:
            # Real input/file lifetime, with separation deliberately outside this
            # control-ownership proof. No production callback or process is faked.
            with running_job.request.source_path.open("rb") as reader:
                read_bytes.append(reader.read(4))
                entered.set()
                assert release.wait(10)
                read_bytes.append(reader.read(4))
        finally:
            running_job.finish_resolution()
            returned.set()

    reader = threading.Thread(target=read_original)
    reader.start()
    try:
        assert entered.wait(5)
        running_cancel = controller.stems.cancel_generation_interest(running_owner, running_token)
        assert running_cancel.jobs == (running_job,)
        assert controller.stems.generation_cancellation_is_pending(running_cancel)
        assert running_job.source is not None
        assert not returned.is_set()
        queued_cancel = controller.stems.cancel_generation_interest(queued_owner, queued_token)
        assert queued_cancel.jobs == ()
        assert not controller.stems.generation_cancellation_is_pending(queued_cancel)
        assert controller.stems.generation_cancellation_is_pending(running_cancel)
    finally:
        release.set()
        reader.join(5)
    assert not reader.is_alive()
    assert returned.is_set()
    assert not controller.stems.generation_cancellation_is_pending(running_cancel)
    assert running_job.source is None
    assert read_bytes[0] == b"RIFF"
    assert len(read_bytes[1]) == 4


@pytest.mark.parametrize("cancel_before_replace", [True, False])
def test_exact_config_replacement_decides_cancel_versus_committed_success(
    disk_commit: _DiskCommit, monkeypatch: pytest.MonkeyPatch, *, cancel_before_replace: bool
) -> None:
    rig = disk_commit
    owner, token = _own_pair(rig)
    replace_file = os.replace
    fsync = os.fsync

    if cancel_before_replace:

        def fsync_then_cancel(descriptor: int) -> None:
            fsync(descriptor)
            rig.controller.stems.cancel_generation_interest(owner, token)

        monkeypatch.setattr(os, "fsync", fsync_then_cancel)
    else:

        def replace_then_cancel(source: str | Path, target: str | Path) -> None:
            replace_file(source, target)
            rig.controller.stems.cancel_generation_interest(owner, token)

        monkeypatch.setattr(os, "replace", replace_then_cancel)

    rig.jobs.pop()()
    rig.controller.stems.on_frame_render()
    assert owner.committed is (not cancel_before_replace)
    assert (
        rig.controller._persistence.config_path.read_bytes() == rig.encoded
    ) is cancel_before_replace
    assert (rig.controller.project.stem_cache[0] is rig.previous) is cancel_before_replace
    if not cancel_before_replace:
        assert rig.prepared.selected
        assert not rig.prepared.discarded
