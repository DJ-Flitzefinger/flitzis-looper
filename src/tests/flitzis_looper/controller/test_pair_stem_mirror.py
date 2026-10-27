"""Surviving control custody only; these fakes cannot prove a Native Pair or live DSP."""

import threading
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import committed_stem_residency, pair_stem_mirror, stems
from flitzis_looper.controller.pair_stem_mirror import prepare_pair_stem_mirror
from flitzis_looper.controller.stem_generation import AudioShape, StemGenerationRequest
from flitzis_looper.controller.stem_job import StemGenerationJob
from flitzis_looper.controller.stem_pair_preparation import PairPreparation
from flitzis_looper.controller.stems import StemController
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from flitzis_looper.stem_execution import StemExecution
from tests.flitzis_looper.controller.test_committed_stem_residency import (
    _entry,
    _RegistrationTicket,
    _ResidencyReceipt,
)
from tests.flitzis_looper.controller.test_stem_job import _Assets, _Lease, _Reservation

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        ProjectAssetLifecycle,
    )
    from flitzis_looper.controller.pair_stem_mirror import PreparedPairStemMirror
    from flitzis_looper_audio import (
        AudioEngine,
        CommittedStemSetTicket,
        OfflineStemSourceLease,
        PreparedSourceTicket,
        PreparedStemPair,
        ProjectAssetLease,
        ResidentStemTicket,
    )


def _controller(*, occupied: bool = False) -> tuple[StemController, _Assets, Mock]:
    project = ProjectState()
    for sample_id, content_id in ((0, "a" * 32), (215, "b" * 32)):
        if sample_id == 215 and not occupied:
            continue
        project.pad_content[sample_id] = PadContentIdentity(instance_id=content_id)
        project.sample_paths[sample_id] = f"samples/source-{sample_id}.wav"
        project.stem_cache[sample_id] = _entry(generation="b" if sample_id == 0 else "c")
    audio = Mock(
        spec=[
            "register_committed_stem_pair",
            "register_pending_committed_stem_pair",
            "request_committed_stem_residency",
            "offline_stem_source_is_current",
        ]
    )
    audio.offline_stem_source_is_current.return_value = True
    assets = _Assets()
    controller = StemController(
        project,
        SessionState(),
        cast("AudioEngine", audio),
        stem_task_runner=lambda _task: None,
        asset_lifecycle=cast("ProjectAssetLifecycle", assets),
    )
    return controller, assets, audio


def _add_job(controller: StemController, assets: _Assets, sample_id: int = 0) -> StemGenerationJob:
    identity = controller._project.pad_content[sample_id]
    assert identity is not None
    path = controller._project.sample_paths[sample_id]
    assert path is not None
    request = StemGenerationRequest(
        sample_id=sample_id,
        source_path=Path(path).absolute(),
        source_version=f"{path}|sha256-v1:{'1' * 64}",
        cache_dir=Path(f"private-generation-{sample_id}"),
        target_shape=AudioShape(48_000, 2, 512),
        model_cache_dir=Path(f"model-{sample_id}"),
        original_bytes=2092,
    )
    source = cast("OfflineStemSourceLease", Mock(name=f"own-source-{sample_id}"))
    intent = controller._intents.capture(request, controller._project.stem_processing_revision)
    job = StemGenerationJob(
        request,
        source,
        cast("ProjectAssetLifecycle", assets),
        identity.instance_id,
        intent=intent,
    )
    controller._jobs[request.cache_dir] = job
    controller._generation_sources[sample_id] = source
    controller._generation_intents[sample_id] = intent
    controller._session.stem_generating_sample_ids.add(sample_id)
    controller._session.stem_generation_source_versions[sample_id] = request.source_version
    controller._session.stem_generation_progress[sample_id] = 17.0
    controller._session.stem_generation_stage[sample_id] = "Retained inference"
    return job


def _prepare(
    controller: StemController,
    *,
    validator: Callable[[object], bool] | None = None,
) -> tuple[PreparedPairStemMirror, object]:
    project = controller._project
    source = project.pad_content[0]
    target = project.pad_content[215]
    assert source is not None
    path = project.sample_paths[0]
    assert path is not None
    acknowledgement = object()
    plan = prepare_pair_stem_mirror(
        controller,
        0,
        215,
        source_content_id=source.instance_id,
        target_content_id=target.instance_id if target is not None else None,
        source_path=path,
        target_path=project.sample_paths[215],
        source_signature=controller._generation_sources.get(0, object()),
        target_signature=controller._generation_sources.get(215, object()),
        validate_acknowledgement=(
            validator if validator is not None else lambda value: value is acknowledgement
        ),
    )
    return plan, acknowledgement


def _project_after(controller: StemController, plan: PreparedPairStemMirror) -> None:
    project = controller._project
    source, target = project.pad_content[0], project.pad_content[215]
    source_path, target_path = project.sample_paths[0], project.sample_paths[215]
    project.pad_content[0] = target
    project.pad_content[215] = source
    project.sample_paths[0] = target_path
    project.sample_paths[215] = source_path
    for sample_id, entry in plan.projected_entries:
        project.stem_cache[sample_id] = entry


def _pending_preparation(controller: StemController, assets: _Assets) -> PairPreparation:
    entry = controller._project.stem_cache[0]
    assert entry is not None
    request = PairPreparation(
        0,
        entry,
        cast("PreparedSourceTicket", Mock(name="own-preparation-source")),
        "a" * 32,
        entry,
        cast("AssetRetirementReservation", assets.reserve(16)),
        cast("ProjectAssetLease", assets.acquire(Path(entry.cache_dir))),
        threading.Event(),
        source_path=controller._project.sample_paths[0],
    )
    controller._pair_preparations.pending[0] = request
    return request


def test_running_last_subscriber_move_keeps_actual_reader_until_worker_return() -> None:
    controller, assets, _audio = _controller()
    execution = StemExecution(backend="demucs", model_name="htdemucs", device="cuda")
    controller._session.stem_generation_execution[0] = execution
    job = _add_job(controller, assets)
    subscriber = job.subscribers()[0]
    original_request = job.request
    job.resolved_request = job.request
    resolved_request = job.resolved_request
    plan, acknowledgement = _prepare(controller)
    entered = threading.Event()
    finish = threading.Event()

    def worker() -> None:
        assert job.begin() is subscriber.source
        entered.set()
        assert finish.wait(5.0)
        assert job.finish_read()

    thread = threading.Thread(target=worker)
    thread.start()
    try:
        assert entered.wait(5.0)
        _project_after(controller, plan)
        plan.apply(acknowledgement)
        moved = job.subscribers()[0]
        assert (
            controller._session.stem_generation_execution.get(215),
            0 in controller._session.stem_generation_execution,
        ) == (execution, False)
        assert moved.sample_id == 215
        assert moved.source is subscriber.source
        assert moved.request is subscriber.request
        assert moved.intent is subscriber.intent
        assert moved.source_lease is subscriber.source_lease
        assert moved.retirement is subscriber.retirement
        assert job.request is original_request
        assert job.resolved_request is resolved_request
        assert job.source is subscriber.source
        assert job.running
        assert not job.cancel_event.is_set()
        assert not any(lease.released for lease in assets.leases)
        assert not any(reservation.closed for reservation in assets.reservations)
        # Releasing the old slot cannot remove the moved last subscriber.
        assert job.remove_subscriber(0)
        assert not job.cancel_event.is_set()
    finally:
        finish.set()
        thread.join(5.0)
    assert not thread.is_alive()
    assert assets.leases[0].released
    assert not cast("_Lease", subscriber.source_lease).released
    assert not cast("_Reservation", subscriber.retirement).closed
    assert not job.remove_subscriber(215)
    assert all(lease.released for lease in assets.leases)
    assert all(reservation.closed for reservation in assets.reservations)


def test_swap_retains_both_subscribers_and_once_only_retry_recognizes_own_after_image() -> None:
    controller, assets, _audio = _controller(occupied=True)
    source_job = _add_job(controller, assets)
    target_job = _add_job(controller, assets, 215)
    source, target = source_job.subscribers()[0], target_job.subscribers()[0]
    source_ids = controller._generation_sources
    plan, acknowledgement = _prepare(controller)

    _project_after(controller, plan)
    plan.apply(acknowledgement)
    first_image = controller._generation_sources
    plan.apply(acknowledgement)

    assert plan.applied
    assert source_ids is not first_image
    assert controller._generation_sources is first_image
    assert first_image[215] is source.source
    assert first_image[0] is target.source
    assert source_job.subscribers()[0].request is source.request
    assert target_job.subscribers()[0].request is target.request
    assert source_job.request.sample_id == 0
    assert target_job.request.sample_id == 215
    assert controller._session.stem_generation_progress == {215: 17.0, 0: 17.0}
    with pytest.raises(RuntimeError, match="another transaction receipt"):
        plan.apply(object())
    source_job.cancel()
    target_job.cancel()


def test_swap_rekeys_two_interests_of_one_shared_job_without_cloning_processing_history() -> None:
    controller, assets, _audio = _controller(occupied=True)
    job = _add_job(controller, assets)
    source = job.subscribers()[0]
    target_path = controller._project.sample_paths[215]
    assert target_path is not None
    target_request = replace(
        job.request,
        sample_id=215,
        source_path=Path(target_path).absolute(),
        source_version=f"{target_path}|sha256-v1:{'1' * 64}",
    )
    target_source = cast("OfflineStemSourceLease", Mock(name="independent-shared-job-source"))
    target_intent = controller._intents.capture(target_request, 0)
    job.add_subscriber(215, target_source, "b" * 32, request=target_request, intent=target_intent)
    target = next(sub for sub in job.subscribers() if sub.sample_id == 215)
    controller._generation_sources[215] = target_source
    controller._generation_intents[215] = target_intent
    controller._session.stem_generating_sample_ids.add(215)
    plan, acknowledgement = _prepare(controller)
    reservations = len(assets.reservations)
    leases = len(assets.leases)

    plan.apply(acknowledgement)

    moved = {subscriber.sample_id: subscriber for subscriber in job.subscribers()}
    assert moved[215].source is source.source
    assert moved[215].request is source.request
    assert moved[215].retirement is source.retirement
    assert moved[0].source is target.source
    assert moved[0].request is target_request
    assert moved[0].retirement is target.retirement
    assert moved[0].source_lease is target.source_lease
    assert job.request.sample_id == 0
    assert len(assets.reservations) == reservations
    assert len(assets.leases) == leases
    assert not job.cancel_event.is_set()
    job.cancel()


def test_latest_unrelated_rows_and_new_unrelated_job_interest_survive_final_merge() -> None:
    controller, assets, _audio = _controller()
    job = _add_job(controller, assets)
    plan, acknowledgement = _prepare(controller)
    source = cast("OfflineStemSourceLease", Mock(name="later-unrelated-source"))
    request = replace(job.request, sample_id=36)
    job.add_subscriber(36, source, "c" * 32, request=request)
    unrelated = next(subscriber for subscriber in job.subscribers() if subscriber.sample_id == 36)
    controller._generation_sources[36] = source
    controller._session.stem_generation_progress[36] = 91.0
    controller._session.stem_generation_errors[36] = "Later unrelated error"
    controller._session.stem_generating_sample_ids.add(36)
    controller._project.pad_content[37] = PadContentIdentity(instance_id="d" * 32)
    controller._project.sample_paths[37] = "samples/later-unrelated.wav"
    later_job = _add_job(controller, assets, 37)
    later_subscriber = later_job.subscribers()[0]

    plan.apply(acknowledgement)

    assert controller._generation_sources[36] is source
    assert controller._session.stem_generation_progress[36] == 91.0
    assert controller._session.stem_generation_errors[36] == "Later unrelated error"
    assert 36 in controller._session.stem_generating_sample_ids
    assert next(sub for sub in job.subscribers() if sub.sample_id == 36) is unrelated
    assert not cast("_Lease", unrelated.source_lease).released
    assert not cast("_Reservation", unrelated.retirement).closed
    assert controller._jobs[later_job.request.cache_dir] is later_job
    assert later_job.subscribers()[0] is later_subscriber
    assert controller._generation_sources[37] is later_subscriber.source
    job.cancel()
    later_job.cancel()


@pytest.mark.parametrize("changed", ["native_source", "subscriber", "control_row", "new_job"])
def test_changed_participant_rejects_before_any_controller_or_subscriber_exchange(
    changed: str,
) -> None:
    controller, assets, _audio = _controller()
    job = _add_job(controller, assets)
    controller._demands[0] = stems._StemDemand(
        job.request.source_version, "a" * 32, controller._project.sample_paths[0], None
    )
    plan, acknowledgement = _prepare(controller)
    if changed == "native_source":
        controller._generation_sources[0] = cast(
            "OfflineStemSourceLease", Mock(name="foreign-owner")
        )
    elif changed == "subscriber":
        old = job.subscribers()[0]
        assert old.intent is not None
        job.update_intent(
            0, replace(old.intent, material_revision=old.intent.material_revision + 1)
        )
    elif changed == "control_row":
        controller._demands[0].explicit = True
    else:
        _add_job(controller, assets)
    sources = controller._generation_sources
    progress = controller._session.stem_generation_progress
    subscriber_image = job._subscribers
    reservations = tuple(reservation.closed for reservation in assets.reservations)

    with pytest.raises(RuntimeError, match=r"changed|newer"):
        plan.apply(acknowledgement)

    assert not plan.applied
    assert controller._generation_sources is sources
    assert controller._session.stem_generation_progress is progress
    assert job._subscribers is subscriber_image
    assert tuple(reservation.closed for reservation in assets.reservations) == reservations
    for current in set(controller._jobs.values()) | {job}:
        current.cancel()


def test_json_diagnostic_cannot_replace_own_receipt_and_all_job_locks_precede_validation() -> None:
    controller, assets, _audio = _controller(occupied=True)
    jobs = (_add_job(controller, assets), _add_job(controller, assets, 215))
    acknowledged = object()
    observations: list[tuple[bool, ...]] = []

    def validate(value: object) -> bool:
        observations.append(tuple(job._lock.locked() for job in jobs))
        return value is acknowledged

    plan, _unused = _prepare(controller, validator=validate)
    sources = controller._generation_sources
    with pytest.raises(RuntimeError, match="opaque own Native acknowledgement"):
        plan.apply({"status": "accepted", "source": "a" * 32, "target": "b" * 32})
    assert controller._generation_sources is sources
    plan.apply(acknowledged)
    assert observations == [(True, True), (True, True)]
    for job in jobs:
        job.cancel()


def test_committed_live_attempt_effective_and_preload_keep_their_exact_owners() -> None:
    controller, _assets, audio = _controller(occupied=True)
    entry = controller._project.stem_cache[0]
    assert entry is not None
    registration = _RegistrationTicket(state="accepted")
    receipt = _ResidencyReceipt(residency="accepted", effective="effective")
    preload = _ResidencyReceipt(residency="accepted")
    receipt_view = cast("ResidentStemTicket", receipt)
    preload_view = cast("ResidentStemTicket", preload)
    control = committed_stem_residency._CommittedSelection(
        entry,
        "a" * 32,
        controller._project.sample_paths[0],
        cast("CommittedStemSetTicket", registration),
        desired=("all_stems", 5),
        receipt=receipt_view,
        receipt_intent=("all_stems", 5),
        preload_receipt=preload_view,
        preload_enabled=True,
    )
    attempt = replace(control, candidate=True)
    disk = committed_stem_residency._DiskCandidate(
        entry,
        control.content_id,
        control.source_path,
        cast("PreparedStemPair", Mock(name="exact-prepared-pair")),
        cast("PreparedSourceTicket", Mock(name="exact-source-permit")),
        attempt,
        "accepted",
    )
    live = committed_stem_residency._LiveSelection(
        disk, control, "retained-effective-identity", attempt, ("all_stems", 5)
    )
    controller._committed_residency._live_selections[0] = live
    controller._demands[0] = stems._StemDemand(
        entry.source_version,
        control.content_id,
        control.source_path,
        "retained-selection",
        explicit=True,
        policy=True,
    )
    controller._resident_pairs.add(0)
    controller._pair_release_ordered.add(0)
    plan, acknowledgement = _prepare(controller)

    plan.apply(acknowledgement)

    assert controller._committed_residency._live_selections[215] is live
    assert live.disk is disk
    assert live.control is control
    assert live.attempt is attempt
    assert control.receipt is receipt_view
    assert control.preload_receipt is preload_view
    assert live.effective_identity == "retained-effective-identity"
    assert controller._demands[215].explicit
    assert controller._demands[215].policy
    assert controller._resident_pairs == {215}
    assert controller._pair_release_ordered == {215}
    assert (registration.cancellations, receipt.cancellations, preload.cancellations) == (0, 0, 0)
    audio.register_committed_stem_pair.assert_not_called()
    audio.register_pending_committed_stem_pair.assert_not_called()
    audio.request_committed_stem_residency.assert_not_called()


def test_pending_worker_keeps_original_request_id_until_exact_result_returns() -> None:
    controller, assets, _audio = _controller()
    request = _pending_preparation(controller, assets)
    plan, acknowledgement = _prepare(controller)
    # Real worker completion fields may advance without replacing control intent.
    request.prepared = cast("PreparedStemPair", Mock(name="returned-prepared-pair"))
    request.error = "Worker diagnostic"

    plan.apply(acknowledgement)

    assert controller._pair_preparations.pending[215] is request
    assert request.sample_id == 0
    assert not cast("_Lease", request.lease).released
    assert not cast("_Reservation", request.retirement).closed
    controller._pair_preparations._results.put(request)
    (returned,) = controller._pair_preparations.collect()
    controller._project_returned_pair_placement(returned)
    assert returned is request
    assert returned.sample_id == 215
    assert controller._pair_preparations.pending == {}
    assert not cast("_Lease", request.lease).released
    request.close()


def test_cancelled_participant_preparation_rejects_without_releasing_its_worker_custody() -> None:
    controller, assets, _audio = _controller()
    request = _pending_preparation(controller, assets)
    plan, acknowledgement = _prepare(controller)
    pending = controller._pair_preparations.pending
    request.cancelled.set()

    with pytest.raises(RuntimeError, match="cancellation changed"):
        plan.apply(acknowledgement)

    assert not plan.applied
    assert controller._pair_preparations.pending is pending
    assert pending[0] is request
    assert request.sample_id == 0
    assert not cast("_Lease", request.lease).released
    assert not cast("_Reservation", request.retirement).closed
    request.close()


def test_publication_retry_keeps_original_ticket_and_rollback_custody() -> None:
    controller, assets, _audio = _controller()
    entry = controller._project.stem_cache[0]
    assert entry is not None
    pending = stems._PendingStemPublication(
        0,
        cast("PreparedSourceTicket", Mock(name="retained-publication-source")),
        entry,
        None,
        None,
        cast("AssetRetirementReservation", assets.reserve(16)),
        content_id="a" * 32,
        source_path=controller._project.sample_paths[0],
    )
    controller._pending_stem_publications[0] = pending
    retry = controller._publication_retries.retain(pending, "a" * 32, pending.source_path)
    retry.attempts = 3
    plan, acknowledgement = _prepare(controller)

    plan.apply(acknowledgement)

    assert controller._pending_stem_publications[215] is pending
    assert controller._publication_retries.pending[215] is retry
    assert retry.pending is pending
    assert pending.sample_id == 215
    assert retry.attempts == 3
    assert not cast("_Reservation", pending.retirement).closed


def test_existing_dispatch_follows_moved_subscriber_and_historical_worker_event() -> None:
    controller, assets, _audio = _controller()
    job = _add_job(controller, assets)
    event = stems._StemBackendEvent(
        0,
        job.request.source_version,
        job.source,
        job.request.cache_dir,
        "progress",
        percent=63.0,
        stage="Own continuing worker",
        source_path=job.request.source_path,
    )
    plan, acknowledgement = _prepare(controller)
    _project_after(controller, plan)
    plan.apply(acknowledgement)

    controller._dispatch_job_event(event, job)

    assert controller._session.stem_generation_progress[215] == 63.0
    assert controller._session.stem_generation_stage[215] == "Own continuing worker"
    assert 0 not in controller._session.stem_generation_progress
    assert job.request.sample_id == event.sample_id == 0
    job.cancel()


def _interrupted_swap_context() -> SimpleNamespace:
    controller, assets, audio = _controller(occupied=True)
    job = _add_job(controller, assets)
    target_job = _add_job(controller, assets, 215)
    entry = controller._project.stem_cache[0]
    assert entry is not None
    publication = stems._PendingStemPublication(
        0,
        cast("PreparedSourceTicket", Mock(name="exact-publication-source")),
        entry,
        None,
        None,
        cast("AssetRetirementReservation", assets.reserve(16)),
        content_id="a" * 32,
        source_path=controller._project.sample_paths[0],
    )
    controller._pending_stem_publications[0] = publication
    retry = controller._publication_retries.retain(publication, "a" * 32, publication.source_path)
    retry.attempts = 3
    plan, acknowledgement = _prepare(controller)
    _project_after(controller, plan)
    return SimpleNamespace(
        controller=controller,
        assets=assets,
        audio=audio,
        job=job,
        target_job=target_job,
        source_subscriber=job.subscribers()[0],
        target_subscriber=target_job.subscribers()[0],
        publication=publication,
        retry=retry,
        plan=plan,
        acknowledgement=acknowledgement,
    )


def _assert_stem_swap(
    context: SimpleNamespace, newest: OfflineStemSourceLease, unrelated_subscriber: object
) -> None:
    moved_source = next(row for row in context.job.subscribers() if row.sample_id == 215)
    moved_target = context.target_job.subscribers()[0]
    assert moved_target.sample_id == 0
    assert moved_source.source is context.source_subscriber.source
    assert moved_source.request is context.source_subscriber.request
    assert moved_source.intent is context.source_subscriber.intent
    assert moved_source.retirement is context.source_subscriber.retirement
    assert moved_target.source is context.target_subscriber.source
    assert moved_target.request is context.target_subscriber.request
    assert moved_target.retirement is context.target_subscriber.retirement
    assert context.job._subscribers[36] is unrelated_subscriber
    assert context.controller._generation_sources[36] is newest
    assert context.controller._session.stem_generation_progress[36] == 87.0
    assert context.controller._session.stem_generation_stage[36] == "Newest unrelated progress"
    assert context.controller._pending_stem_publications[215] is context.publication
    assert context.publication.sample_id == 215
    assert context.controller._publication_retries.pending[215] is context.retry
    assert context.retry.attempts == 3
    assert not cast("_Reservation", context.publication.retirement).closed
    context.audio.register_committed_stem_pair.assert_not_called()
    context.audio.register_pending_committed_stem_pair.assert_not_called()
    context.audio.request_committed_stem_residency.assert_not_called()


@pytest.mark.parametrize(
    ("owner_name", "field_name"),
    [
        ("job", "_subscribers"),
        ("publication", "sample_id"),
        ("controller", "_generation_sources"),
        ("session", "stem_generation_progress"),
        ("retries", "pending"),
    ],
)
def test_interrupted_stem_swap_retains_own_images_and_newest_unrelated_rows(
    monkeypatch: pytest.MonkeyPatch, owner_name: str, field_name: str
) -> None:
    context = _interrupted_swap_context()
    interrupted_owner = {
        "job": context.job,
        "publication": context.publication,
        "controller": context.controller,
        "session": context.controller._session,
        "retries": context.controller._publication_retries,
    }[owner_name]
    store = pair_stem_mirror._store_pair_image
    interrupted = False

    def interrupt_after_store(owner: object, name: str, image: object) -> None:
        nonlocal interrupted
        store(owner, name, image)
        if owner is interrupted_owner and name == field_name and not interrupted:
            interrupted = True
            message = "interrupted own Stem store"
            raise RuntimeError(message)

    monkeypatch.setattr(pair_stem_mirror, "_store_pair_image", interrupt_after_store)
    with pytest.raises(RuntimeError, match="interrupted own Stem store"):
        context.plan.apply(context.acknowledgement)
    assert not context.plan.applied
    with pytest.raises(RuntimeError, match="another transaction receipt"):
        context.plan.apply(object())
    newest = cast("OfflineStemSourceLease", Mock(name="newest-unrelated-owner"))
    context.job.add_subscriber(36, newest, "c" * 32)
    unrelated_subscriber = next(row for row in context.job.subscribers() if row.sample_id == 36)
    context.controller._generation_sources[36] = newest
    context.controller._session.stem_generation_progress[36] = 87.0
    context.controller._session.stem_generation_stage[36] = "Newest unrelated progress"
    context.plan.apply(context.acknowledgement)
    assert context.plan.applied
    assert interrupted
    _assert_stem_swap(context, newest, unrelated_subscriber)
    subscribers = context.job._subscribers
    context.plan.apply(context.acknowledgement)
    assert context.job._subscribers is subscribers
    context.job.cancel()
    context.target_job.cancel()
