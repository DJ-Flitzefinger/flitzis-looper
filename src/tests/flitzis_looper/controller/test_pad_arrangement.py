"""Copy control ownership and failures; scripted adapters do not prove Native execution."""

import json
import threading
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller.pad_arrangement import (
    NativeAdapterProtocol,
    PadArrangementController,
)
from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from flitzis_looper.stem_intent import PadStemIntent

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        AssignmentPublicationPlan,
        ProjectAssetLifecycle,
    )
    from flitzis_looper.controller.pad_arrangement import AssignmentImage
    from flitzis_looper_audio import (
        CapturedPadCopy,
        InputRuntimePadBinding,
        PadCopyTicket,
        ProjectAssetLease,
    )


@pytest.fixture(autouse=True)
def _isolated_copy_samples(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    (samples / "source.wav").write_bytes(b"source custody fixture")
    (samples / "target.wav").write_bytes(b"predecessor custody fixture")


class _Capture:
    def __init__(self) -> None:
        self.phase = "captured"
        self.cancelled = False


class _Ticket:
    def __init__(self, captured: _Capture) -> None:
        self.captured = captured

    def publication_status(self) -> str:
        return self.captured.phase


class _ScriptedNative:
    """A control contract double; complete ACK is explicitly injected by each test."""

    def __init__(self) -> None:
        self.bindings = {0: cast("InputRuntimePadBinding", object())}
        self.captured = _Capture()
        self.captured_view = cast("CapturedPadCopy", self.captured)
        self.ticket = _Ticket(self.captured)
        self.ticket_view = cast("PadCopyTicket", self.ticket)
        self.envelope: dict[str, object] | None = None
        self.complete_ack = False
        self.ordinary_source_ack = False
        self.prepare_error: ValueError | None = None
        self.prepared_status = "pending"
        self.publish_error: RuntimeError | None = None
        self.cancel_error: RuntimeError | None = None
        self.cancel_allowed = True
        self.prepare_thread: int | None = None
        self.capture_count = 0
        self.publish_count = 0
        self.cancel_count = 0
        self.finish_count = 0
        self.target_binding: InputRuntimePadBinding | None = None

    def current_input_runtime_pad_binding(self, sample_id: int) -> InputRuntimePadBinding | None:
        return self.bindings.get(sample_id)

    def capture_pad_copy(
        self,
        source_binding: InputRuntimePadBinding,
        target_id: int,
        intent_json: str,
        *,
        target_binding: InputRuntimePadBinding | None = None,
    ) -> CapturedPadCopy:
        assert source_binding is self.bindings[0]
        assert target_id == 215
        self.capture_count += 1
        self.target_binding = target_binding
        self.envelope = json.loads(intent_json)
        return self.captured_view

    def prepare_pad_copy(self, captured: CapturedPadCopy) -> PadCopyTicket:
        assert captured is self.captured_view
        self.prepare_thread = threading.get_ident()
        if self.prepare_error is not None:
            raise self.prepare_error
        if self.captured.cancelled:
            message = "Native Copy cancelled before preparation returned"
            raise ValueError(message)
        self.captured.phase = self.prepared_status
        return self.ticket_view

    def publish_pad_copy(self, ticket: PadCopyTicket) -> None:
        assert ticket is self.ticket_view
        self.publish_count += 1
        if self.publish_error is not None:
            raise self.publish_error
        self.captured.phase = "adopting"

    def reconcile_pad_copy(self, ticket: PadCopyTicket) -> bool:
        assert ticket is self.ticket_view
        if self.complete_ack:
            self.captured.phase = "accepted"
        return self.complete_ack

    def finish_pad_copy(self, ticket: PadCopyTicket) -> None:
        assert ticket is self.ticket_view
        assert self.complete_ack is True
        self.finish_count += 1

    def cancel_pad_copy(self, captured: CapturedPadCopy) -> bool:
        assert captured is self.captured_view
        self.cancel_count += 1
        if self.cancel_error is not None:
            raise self.cancel_error
        if not self.cancel_allowed or self.captured.phase in {"adopting", "accepted"}:
            return False
        self.captured.cancelled = True
        self.captured.phase = "rejected"
        return True


class _Lease:
    def __init__(self) -> None:
        self.released = False

    def release(self) -> None:
        self.released = True


class _Retirement:
    def __init__(self) -> None:
        self.closed = False
        self.activations = 0

    @contextmanager
    def activate(self) -> Iterator[None]:
        self.activations += 1
        yield

    def close(self) -> None:
        self.closed = True


class _Assets:
    def __init__(self) -> None:
        self.retirement = _Retirement()
        self.images: list[AssignmentImage] = []
        self.leases: list[_Lease] = []
        self.published: list[AssignmentImage] = []
        self.prepare_error: RuntimeError | None = None
        self.publish_failures = 0
        self.publication_plans: list[_Publication] = []
        self.planned_retirement_failures = 0

    def reserve(self) -> AssetRetirementReservation:
        return cast("AssetRetirementReservation", self.retirement)

    def acquire(self, path: Path) -> ProjectAssetLease:
        assert path.is_file() or path.is_dir()
        lease = _Lease()
        self.leases.append(lease)
        return cast("ProjectAssetLease", lease)

    def prepare_assignment_image(self, project: ProjectState) -> AssignmentImage:
        if self.prepare_error is not None:
            raise self.prepare_error
        paths: dict[tuple[str, int], Path] = {}
        owners: dict[tuple[str, int], tuple[Path, ProjectAssetLease]] = {}
        for sample_id, path in enumerate(project.sample_paths):
            if path is not None:
                key = ("original", sample_id)
                lease = _Lease()
                self.leases.append(lease)
                paths[key] = Path(path)
                owners[key] = (Path(path), cast("ProjectAssetLease", lease))
        image = paths, owners
        self.images.append(image)
        return image

    def publish_assignment_image(self, image: AssignmentImage) -> None:
        if self.publish_failures:
            self.publish_failures -= 1
            message = "asset image transfer unavailable"
            raise RuntimeError(message)
        self.published.append(image)

    def prepare_assignment_publication(self, image: AssignmentImage) -> AssignmentPublicationPlan:
        plan = _Publication(self, image)
        plan.retirement_failures = self.planned_retirement_failures
        self.planned_retirement_failures = 0
        self.publication_plans.append(plan)
        return cast("AssignmentPublicationPlan", plan)

    @staticmethod
    def release_assignment_image(image: AssignmentImage) -> None:
        for _, lease in image[1].values():
            lease.release()


class _Publication:
    def __init__(self, assets: _Assets, image: AssignmentImage) -> None:
        self.assets = assets
        self.image = image
        self.applied = False
        self.settled = False
        self.retirement_failures = 0

    @property
    def owns_unpublished_image(self) -> bool:
        return not self.applied

    def apply(self) -> None:
        if not self.applied:
            self.assets.publish_assignment_image(self.image)
            self.applied = True
        if self.retirement_failures:
            self.retirement_failures -= 1
            message = "original retirement settlement pending"
            raise RuntimeError(message)
        self.settled = True


class _Jobs:
    def __init__(self) -> None:
        self.pending: list[Callable[[], None]] = []

    def __call__(self, job: Callable[[], None]) -> None:
        self.pending.append(job)

    def run(self) -> None:
        self.pending.pop(0)()


@dataclass(slots=True)
class _Harness:
    project: ProjectState
    persistence: ProjectPersistence
    session: SessionState
    native: _ScriptedNative
    assets: _Assets
    jobs: _Jobs
    controller: PadArrangementController


def _harness(
    tmp_path: Path,
    *,
    callback: Callable[[int, PadCopyTicket], None] | None = None,
    removed: Callable[[int, object], None] | None = None,
) -> _Harness:
    project = ProjectState(key_lock=True, selected_pad=0, volume=0.7)
    project.sample_paths[0] = "samples/source.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="a" * 32, material_id="b" * 32)
    project.pad_stem_mix_mode[0] = "all_stems"
    project.pad_stem_intent[0] = PadStemIntent(
        enabled_mask=0, last_custom_mask=0, display_mode="custom"
    )
    project.manual_bpm[0] = 123.45
    project.pad_timing_intent[0] = "tap"
    project.pad_gain_db[0] = -7.0
    project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(version=7, raw_key="Bb"),
        correction="Fm",
        base_shift=-4,
        extra_shift=12,
        retrigger=True,
    )
    session = SessionState(
        active_sample_ids={0},
        paused_sample_ids={0},
        waveform_editor_open=True,
        waveform_editor_pad_id=0,
        waveform_pause_hold_pad_id=0,
        global_stop_restore_sample_ids={0},
        tap_bpm_pad_id=0,
        tap_bpm_timestamps=[1.0, 1.5],
    )
    session.pressed_pads[0] = True
    persistence = ProjectPersistence(project)
    persistence.config_path = tmp_path / "project.json"
    native = _ScriptedNative()
    assets = _Assets()
    jobs = _Jobs()
    controller = PadArrangementController(
        persistence,
        session,
        cast("NativeAdapterProtocol", native),
        cast("ProjectAssetLifecycle", assets),
        stem_task_runner=jobs,
        on_copy_committed=callback,
        on_target_removed=removed,
    )
    return _Harness(project, persistence, session, native, assets, jobs, controller)


def _source_session(session: SessionState) -> dict[str, object]:
    return session.model_dump(
        exclude={"loading_sample_ids", "sample_load_stage", "sample_load_errors"}
    )


def _assert_writer_fenced(harness: _Harness) -> None:
    with pytest.raises(PersistenceFenceError, match="owns the config writer"):
        harness.persistence.flush()


def _assert_complete_copy(harness: _Harness) -> PadContentIdentity:
    assert harness.controller.status == "completed"
    assert harness.controller.active is False
    target = harness.project.pad_content[215]
    assert target is not None
    assert target.instance_id != "a" * 32
    assert target.copied_from == "a" * 32
    assert target.material_id == "b" * 32
    assert harness.project.sample_paths[215] == "samples/source.wav"
    assert harness.project.pad_stem_intent[215].enabled_mask == 0
    assert harness.project.pad_key_intent[215].retrigger is True
    assert harness.project.pad_timing_intent[215] == "tap"
    assert harness.native.finish_count == 1
    assert harness.assets.published
    assert harness.assets.retirement.closed is True
    return target


@pytest.mark.parametrize(("source", "target"), [(0, 0), (-1, 215), (0, 216), (True, 215), (1, 215)])
def test_self_invalid_and_empty_source_are_side_effect_free_noops(
    tmp_path: Path, source: int, target: int
) -> None:
    harness = _harness(tmp_path)
    before = harness.project.model_dump_json()
    session_before = harness.session.model_dump_json()

    assert harness.controller.copy_to_empty(source, target) is False

    assert harness.project.model_dump_json() == before
    assert harness.session.model_dump_json() == session_before
    assert harness.native.capture_count == 0
    assert harness.controller.active is False


@pytest.mark.parametrize(
    "blocked", ["occupied", "pending_load", "native_occupied", "no_source_binding"]
)
def test_unsupported_source_or_target_is_not_reserved(tmp_path: Path, blocked: str) -> None:
    harness = _harness(tmp_path)
    if blocked == "occupied":
        harness.project.sample_paths[215] = "samples/other.wav"
    elif blocked == "pending_load":
        harness.session.loading_sample_ids.add(215)
    elif blocked == "native_occupied":
        harness.native.bindings[215] = cast("InputRuntimePadBinding", object())
    else:
        harness.native.bindings.clear()

    assert harness.controller.copy_to_empty(0, 215) is False

    assert harness.native.capture_count == 0
    assert not harness.jobs.pending


def test_worker_only_prepares_and_full_ack_precedes_config_model_assets_callback_and_finish(
    tmp_path: Path,
) -> None:
    callbacks: list[tuple[int, PadCopyTicket]] = []

    def callback(sample_id: int, ticket: PadCopyTicket) -> None:
        assert harness.persistence.config_path.exists()
        assert harness.project.pad_content[sample_id] is not None
        assert harness.assets.published
        assert harness.native.finish_count == 0
        callbacks.append((sample_id, ticket))

    harness = _harness(tmp_path, callback=callback)
    project_identity = harness.project
    paths_identity = harness.project.sample_paths
    source_session = _source_session(harness.session)
    assert harness.controller.copy_to_empty(0, 215) is True
    project_before_worker = harness.project.model_dump_json()
    session_before_worker = harness.session.model_dump_json()
    worker = threading.Thread(target=harness.jobs.run)
    worker.start()
    worker.join(timeout=2)
    assert not worker.is_alive()

    assert harness.native.prepare_thread != threading.get_ident()
    assert harness.project.model_dump_json() == project_before_worker
    assert harness.session.model_dump_json() == session_before_worker
    assert harness.assets.images == []
    harness.controller.poll()
    assert harness.controller.active is True
    assert harness.native.publish_count == 1
    assert harness.project.sample_paths[215] is None
    assert not harness.persistence.config_path.exists()
    assert callbacks == []
    harness.native.ordinary_source_ack = True
    harness.controller.poll()
    assert harness.project.sample_paths[215] is None
    assert callbacks == []
    harness.native.complete_ack = True

    harness.controller.poll()

    target = _assert_complete_copy(harness)
    assert harness.project is project_identity
    assert harness.project.sample_paths is paths_identity
    assert _source_session(harness.session) == source_session
    assert harness.project.selected_pad == 0
    assert callbacks == [(215, harness.native.ticket_view)]
    saved = json.loads(harness.persistence.config_path.read_text(encoding="utf-8"))
    assert saved["pad_content"][215]["instance_id"] == target.instance_id


@pytest.mark.parametrize("override", [None, False, True])
def test_capture_envelope_resolves_only_key_lock_and_preserves_snapshot_intent(
    tmp_path: Path, *, override: bool | None
) -> None:
    harness = _harness(tmp_path)
    harness.project.pad_key_lock[0] = override

    assert harness.controller.copy_to_empty(0, 215) is True

    envelope = harness.native.envelope
    assert envelope is not None
    assert set(envelope) == {"schema_version", "snapshot", "resolved_key_lock"}
    assert envelope["resolved_key_lock"] is (True if override is None else override)
    snapshot = envelope["snapshot"]
    assert isinstance(snapshot, dict)
    assert snapshot["key_lock"] is override
    assert snapshot["stem_intent"]["enabled_mask"] == 0
    assert harness.controller.fenced_sample_ids == frozenset({215})
    if override is None:
        with pytest.raises(RuntimeError, match="inheritance is reserved"):
            harness.controller.ensure_global_key_lock_edit_allowed()
    else:
        harness.controller.ensure_global_key_lock_edit_allowed()


def test_known_invalid_config_rejects_before_native_capture_or_writer_reservation(
    tmp_path: Path,
) -> None:
    harness = _harness(tmp_path)
    harness.persistence.load_error = "Unsupported stem selection metadata retained on disk"
    before = harness.project.model_dump()

    assert harness.controller.copy_to_empty(0, 215) is False

    assert harness.controller.active is False
    assert harness.native.capture_count == 0
    assert not harness.jobs.pending
    assert not harness.assets.images
    assert harness.persistence._migration_owner is None
    assert harness.project.model_dump() == before
    assert "config is fenced" in harness.session.sample_load_errors[215]


def test_cancel_waits_for_actual_worker_return_before_releasing_resources(tmp_path: Path) -> None:
    harness = _harness(tmp_path)
    assert harness.controller.copy_to_empty(0, 215) is True

    assert harness.controller.cancel() is False

    assert harness.controller.active is True
    assert harness.native.cancel_count == 0
    assert harness.assets.retirement.closed is False
    _assert_writer_fenced(harness)
    harness.jobs.run()
    assert harness.native.cancel_count == 0

    harness.controller.poll()

    assert harness.controller.status == "cancelled"
    assert harness.native.captured.cancelled is True
    assert harness.native.publish_count == 0
    assert harness.assets.retirement.closed is True
    assert harness.project.sample_paths[215] is None
    harness.persistence.flush()


def test_shutdown_without_copy_closes_admission_permanently(tmp_path: Path) -> None:
    harness = _harness(tmp_path)
    project_before = harness.project.model_dump_json()
    session_before = harness.session.model_dump_json()

    assert harness.controller.prepare_shutdown() is True
    assert harness.controller.prepare_shutdown() is True
    assert harness.controller.copy_to_empty(0, 215) is False

    assert harness.native.capture_count == 0
    assert harness.project.model_dump_json() == project_before
    assert harness.session.model_dump_json() == session_before
    assert harness.jobs.pending == []


@pytest.mark.parametrize("worker_running", [False, True])
def test_shutdown_eagerly_cancels_native_but_retains_owners_until_worker_returns(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, worker_running: bool
) -> None:
    harness = _harness(tmp_path)
    harness.persistence.flush()
    saved_before = harness.persistence.config_path.read_bytes()
    project_before = harness.project.model_dump_json()
    session_before = _source_session(harness.session)
    preparing = threading.Event()
    finish_preparing = threading.Event()
    prepare = harness.native.prepare_pad_copy

    def blocked_prepare(captured: CapturedPadCopy) -> PadCopyTicket:
        preparing.set()
        assert finish_preparing.wait(timeout=2)
        return prepare(captured)

    assert harness.controller.copy_to_empty(0, 215) is True
    worker = None
    if worker_running:
        monkeypatch.setattr(harness.native, "prepare_pad_copy", blocked_prepare)
        worker = threading.Thread(target=harness.jobs.run)
        worker.start()
    try:
        if worker is not None:
            assert preparing.wait(timeout=2)
        assert harness.controller.prepare_shutdown() is False
        assert harness.native.cancel_count == 1
        assert harness.native.captured.cancelled is True
        assert harness.controller.copy_to_empty(0, 36) is False
        assert harness.assets.retirement.closed is False
        assert harness.session.loading_sample_ids == {215}
        _assert_writer_fenced(harness)
        assert harness.controller.prepare_shutdown() is False
        assert harness.native.cancel_count == 1
    finally:
        finish_preparing.set()
        if worker is not None:
            worker.join(timeout=2)
    if worker is None:
        harness.jobs.run()
    else:
        assert not worker.is_alive()
    assert harness.assets.retirement.closed is False

    assert harness.controller.prepare_shutdown() is True

    assert harness.controller.status == "cancelled"
    assert harness.controller.error is None
    assert harness.native.cancel_count == 1
    assert harness.native.publish_count == 0
    assert harness.native.finish_count == 0
    assert harness.assets.retirement.closed is True
    assert harness.project.model_dump_json() == project_before
    assert harness.persistence.config_path.read_bytes() == saved_before
    assert _source_session(harness.session) == session_before
    harness.persistence.flush()


@pytest.mark.parametrize("cancel_error", [False, True])
def test_shutdown_requires_actual_native_cancellation_and_preserves_retry(
    tmp_path: Path, *, cancel_error: bool
) -> None:
    harness = _harness(tmp_path)
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.native.cancel_allowed = False
    if cancel_error:
        harness.native.cancel_error = RuntimeError("Native cancellation temporarily unavailable")

    assert harness.controller.prepare_shutdown() is False

    assert harness.native.cancel_count == 1
    assert harness.native.captured.cancelled is False
    assert harness.assets.retirement.closed is False
    _assert_writer_fenced(harness)
    harness.jobs.run()
    harness.native.cancel_error = None
    harness.native.cancel_allowed = True

    assert harness.controller.prepare_shutdown() is True

    assert harness.controller.active is False
    assert harness.native.captured.cancelled is True
    assert harness.native.publish_count == 0
    assert harness.native.finish_count == 0
    assert harness.assets.retirement.closed is True


@pytest.mark.parametrize("cancel_error", [False, True])
def test_shutdown_claimed_copy_waits_for_own_complete_ack_and_durable_callback(
    tmp_path: Path, *, cancel_error: bool
) -> None:
    callbacks: list[int] = []

    def committed(sample_id: int, _ticket: PadCopyTicket) -> None:
        assert harness.persistence.config_path.exists()
        assert harness.assets.published
        assert harness.assets.retirement.closed is False
        assert harness.native.finish_count == 0
        callbacks.append(sample_id)

    harness = _harness(tmp_path, callback=committed)
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()
    harness.controller.poll()
    harness.native.ordinary_source_ack = True
    if cancel_error:
        harness.native.cancel_error = RuntimeError("Native cancellation temporarily unavailable")

    for _ in range(3):
        assert harness.controller.prepare_shutdown() is False

    assert callbacks == []
    assert harness.native.finish_count == 0
    assert harness.assets.retirement.closed is False
    assert not any(lease.released for lease in harness.assets.leases)
    assert harness.project.sample_paths[215] is None
    assert not harness.persistence.config_path.exists()
    _assert_writer_fenced(harness)
    harness.native.complete_ack = True

    assert harness.controller.prepare_shutdown() is True

    _assert_complete_copy(harness)
    assert callbacks == [215]
    assert harness.controller.copy_to_empty(0, 36) is False
    assert harness.controller.prepare_shutdown() is True
    assert harness.native.finish_count == 1


@pytest.mark.parametrize("failure", ["config", "callback"])
def test_shutdown_acknowledged_copy_keeps_owners_until_commit_retry_succeeds(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    failures = [True]

    def fail_once() -> None:
        if failures:
            failures.pop()
            message = "Copy shutdown publication temporarily unavailable"
            raise OSError(message)

    def committed(_sample_id: int, _ticket: PadCopyTicket) -> None:
        if failure == "callback":
            fail_once()

    harness = _harness(tmp_path, callback=committed)
    write = harness.persistence._atomic_write_text

    def write_once(content: str) -> None:
        if failure == "config":
            fail_once()
        write(content)

    monkeypatch.setattr(harness.persistence, "_atomic_write_text", write_once)
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()
    harness.controller.poll()
    harness.native.complete_ack = True

    assert harness.controller.prepare_shutdown() is False

    assert harness.controller.active is True
    assert harness.native.finish_count == 0
    assert harness.native.captured.cancelled is False
    assert harness.assets.retirement.closed is False
    _assert_writer_fenced(harness)
    cancellations_before_retry = harness.native.cancel_count

    assert harness.controller.prepare_shutdown() is True

    _assert_complete_copy(harness)
    assert harness.native.cancel_count == cancellations_before_retry


def test_preparation_failure_cancels_only_after_worker_completion(tmp_path: Path) -> None:
    harness = _harness(tmp_path)
    harness.native.prepare_error = ValueError("captured source became stale")
    source_session = _source_session(harness.session)
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()
    assert harness.controller.active is True

    harness.controller.poll()

    assert harness.controller.status == "failed"
    assert harness.native.captured.cancelled is True
    assert harness.project.sample_paths[215] is None
    assert "stale" in harness.session.sample_load_errors[215]
    assert _source_session(harness.session) == source_session


def test_bounded_worker_queue_rejection_releases_unclaimed_native_and_writer(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path)

    def reject(_job: Callable[[], None]) -> None:
        message = "existing stem queue full"
        raise RuntimeError(message)

    monkeypatch.setattr(harness.controller, "_runner", reject)

    assert harness.controller.copy_to_empty(0, 215) is False

    assert harness.controller.active is False
    assert harness.native.captured.cancelled is True
    assert harness.assets.retirement.closed is True
    assert harness.project.sample_paths[215] is None
    harness.persistence.flush()


@pytest.mark.parametrize("failure", ["asset_prepare", "native_publish"])
def test_preclaim_image_or_queue_failure_leaves_old_project_and_config_intact(
    tmp_path: Path, failure: str
) -> None:
    harness = _harness(tmp_path)
    harness.persistence.flush()
    before = harness.persistence.config_path.read_bytes()
    project_before = harness.project.model_dump_json()
    if failure == "asset_prepare":
        harness.assets.prepare_error = RuntimeError("asset capacity unavailable")
    else:
        harness.native.publish_error = RuntimeError("Native queue full")
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()

    harness.controller.poll()

    assert harness.controller.status == "failed"
    assert harness.native.captured.cancelled is True
    assert harness.project.model_dump_json() == project_before
    assert harness.persistence.config_path.read_bytes() == before
    assert all(lease.released for lease in harness.assets.leases)
    assert harness.native.finish_count == 0


def test_adopting_without_complete_ack_retains_fence_pins_and_never_timeout_rolls_back(
    tmp_path: Path,
) -> None:
    harness = _harness(tmp_path)
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()
    harness.controller.poll()

    assert harness.controller.cancel() is False
    for _ in range(8):
        harness.controller.poll()

    assert harness.controller.active is True
    assert harness.controller.pending is True
    assert harness.project.sample_paths[215] is None
    assert harness.assets.retirement.closed is False
    assert not any(lease.released for lease in harness.assets.leases)
    assert len(harness.assets.images) == 1
    assert not harness.persistence.config_path.exists()
    _assert_writer_fenced(harness)
    harness.native.complete_ack = True

    harness.controller.poll()

    assert harness.controller.status == "completed"
    assert harness.native.finish_count == 1


def test_terminal_rejection_releases_unadopted_owners_without_inventing_ack(tmp_path: Path) -> None:
    harness = _harness(tmp_path)
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()
    harness.controller.poll()
    harness.native.captured.phase = "rejected"

    harness.controller.poll()

    assert harness.controller.status == "failed"
    assert harness.controller.active is False
    assert harness.native.finish_count == 0
    assert harness.native.cancel_count == 1
    assert harness.native.captured.cancelled is True
    assert all(lease.released for lease in harness.assets.leases)
    assert harness.project.sample_paths[215] is None
    assert not harness.persistence.config_path.exists()


def test_returned_native_captured_ticket_is_ready_for_full_publication(tmp_path: Path) -> None:
    harness = _harness(tmp_path)
    harness.native.prepared_status = "captured"
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()

    harness.controller.poll()

    assert harness.native.publish_count == 1
    assert harness.controller.active is True
    assert harness.project.sample_paths[215] is None
    assert not harness.persistence.config_path.exists()
    harness.native.complete_ack = True

    harness.controller.poll()

    _assert_complete_copy(harness)


def test_terminal_rejection_retains_fence_until_native_reservation_is_cleared(
    tmp_path: Path,
) -> None:
    harness = _harness(tmp_path)
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()
    harness.controller.poll()
    harness.native.captured.phase = "rejected"
    harness.native.cancel_allowed = False

    harness.controller.poll()

    assert harness.controller.active is True
    assert harness.native.cancel_count == 1
    assert harness.native.captured.cancelled is False
    assert harness.assets.retirement.closed is False
    assert not any(lease.released for lease in harness.assets.leases)
    _assert_writer_fenced(harness)
    harness.native.cancel_allowed = True

    harness.controller.poll()

    assert harness.controller.status == "failed"
    assert harness.native.captured.cancelled is True
    assert harness.assets.retirement.closed is True


def test_config_failure_keeps_claimed_target_and_rebases_unrelated_edits_on_retry(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path)
    real_write = harness.persistence._atomic_write_text
    failures = [True]

    def write(content: str) -> None:
        if failures:
            failures.pop()
            message = "atomic config replacement unavailable"
            raise OSError(message)
        real_write(content)

    monkeypatch.setattr(harness.persistence, "_atomic_write_text", write)
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()
    harness.controller.poll()
    harness.native.complete_ack = True
    harness.controller.poll()

    assert harness.controller.active is True
    assert harness.project.sample_paths[215] is None
    assert harness.native.finish_count == 0
    assert harness.assets.retirement.closed is False
    assert len([lease for lease in harness.assets.leases if not lease.released]) == 4
    _assert_writer_fenced(harness)
    harness.project.volume = 0.35
    harness.project.pad_gain_db[36] = -9.0
    harness.persistence.mark_dirty()

    harness.controller.poll()

    assert harness.controller.status == "completed"
    assert harness.project.volume == 0.35
    assert harness.project.pad_gain_db[36] == -9.0
    assert harness.project.pad_gain_db[215] == -7.0
    saved = json.loads(harness.persistence.config_path.read_text(encoding="utf-8"))
    assert saved["volume"] == 0.35
    assert saved["pad_gain_db"][36] == -9.0
    assert saved["sample_paths"][215] == "samples/source.wav"
    assert len([lease for lease in harness.assets.leases if not lease.released]) == 2


def test_newer_intent_during_atomic_write_is_retained_and_recommitted(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path)
    real_write = harness.persistence._atomic_write_text
    supersede = [True]

    def write(content: str) -> None:
        real_write(content)
        if supersede:
            supersede.pop()
            harness.project.volume = 0.4
            harness.persistence.mark_dirty()

    monkeypatch.setattr(harness.persistence, "_atomic_write_text", write)
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()
    harness.controller.poll()
    harness.native.complete_ack = True
    harness.controller.poll()

    assert harness.controller.active is True
    assert harness.project.sample_paths[215] is None
    assert harness.project.volume == 0.4
    assert harness.native.finish_count == 0
    _assert_writer_fenced(harness)

    harness.controller.poll()

    assert harness.controller.status == "completed"
    saved = json.loads(harness.persistence.config_path.read_text(encoding="utf-8"))
    assert saved["volume"] == 0.4
    assert saved["sample_paths"][215] == "samples/source.wav"


def test_prepublication_target_conflict_preserves_the_new_occupant(tmp_path: Path) -> None:
    harness = _harness(tmp_path)
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()
    harness.project.sample_paths[215] = "samples/newer.wav"
    harness.project.pad_content[215] = PadContentIdentity(instance_id="c" * 32)

    harness.controller.poll()

    assert harness.controller.status == "failed"
    assert harness.project.sample_paths[215] == "samples/newer.wav"
    assert harness.native.publish_count == 0
    assert harness.native.captured.cancelled is True


def test_postclaim_target_conflict_remains_visible_and_cannot_rollback_or_overwrite(
    tmp_path: Path,
) -> None:
    harness = _harness(tmp_path)
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()
    harness.controller.poll()
    harness.project.sample_paths[215] = "samples/newer.wav"
    harness.project.pad_content[215] = PadContentIdentity(instance_id="c" * 32)
    harness.native.complete_ack = True

    harness.controller.poll()

    assert harness.controller.active is True
    assert harness.project.sample_paths[215] == "samples/newer.wav"
    assert "target changed" in harness.session.sample_load_errors[215]
    assert harness.controller.cancel() is False
    assert harness.native.finish_count == 0
    assert harness.assets.retirement.closed is False
    _assert_writer_fenced(harness)


@pytest.mark.parametrize("failure", ["callback", "assets"])
def test_postcommit_publication_failure_retains_native_and_writer_until_retry(
    tmp_path: Path, failure: str
) -> None:
    calls: list[int] = []

    def callback(sample_id: int, _ticket: PadCopyTicket) -> None:
        calls.append(sample_id)
        if failure == "callback" and len(calls) == 1:
            message = "Session binding projection unavailable"
            raise RuntimeError(message)

    harness = _harness(tmp_path, callback=callback)
    if failure == "assets":
        harness.assets.publish_failures = 1
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()
    harness.controller.poll()
    harness.native.complete_ack = True
    harness.controller.poll()

    assert harness.controller.active is True
    assert harness.project.sample_paths[215] == "samples/source.wav"
    assert harness.persistence.config_path.exists()
    assert harness.native.finish_count == 0
    assert harness.controller.cancel() is False
    assert harness.assets.retirement.closed is False
    _assert_writer_fenced(harness)

    harness.controller.poll()

    assert harness.controller.status == "completed"
    assert harness.native.finish_count == 1
    assert len(calls) == (2 if failure == "callback" else 1)
    assert harness.assets.retirement.closed is True


def test_inherited_global_change_before_publication_cancels_without_native_or_config_effects(
    tmp_path: Path,
) -> None:
    harness = _harness(tmp_path)
    assert harness.controller.copy_to_empty(0, 215) is True
    harness.jobs.run()
    harness.project.key_lock = False

    harness.controller.poll()

    assert harness.controller.status == "failed"
    assert harness.native.publish_count == 0
    assert harness.native.captured.cancelled is True
    assert harness.project.key_lock is False
    assert harness.project.sample_paths[215] is None


def test_another_writer_fence_blocks_copy_without_releasing_its_owner(tmp_path: Path) -> None:
    harness = _harness(tmp_path)
    owner = "f" * 32
    harness.persistence.capture_migration(owner)

    assert harness.controller.copy_to_empty(0, 215) is False

    assert harness.controller.active is False
    assert harness.native.capture_count == 0
    _assert_writer_fenced(harness)
    harness.persistence.release_migration(owner)


def _occupy_target(harness: _Harness) -> PadContentIdentity:
    identity = PadContentIdentity(instance_id="c" * 32, material_id="d" * 32)
    harness.project.sample_paths[215] = "samples/target.wav"
    harness.project.pad_content[215] = identity
    harness.project.manual_bpm[215] = 88.75
    harness.project.pad_timing_intent[215] = "manual"
    harness.project.pad_gain_db[215] = 3.0
    harness.native.bindings[215] = cast("InputRuntimePadBinding", object())
    harness.session.active_sample_ids.add(215)
    harness.session.paused_sample_ids.add(215)
    return identity


def test_occupied_copy_retains_original_binding_and_waits_for_own_ack_before_retirement(
    tmp_path: Path,
) -> None:
    events: list[str] = []

    def removed(sample_id: int, predecessor: object) -> None:
        assert sample_id == 215
        assert harness.native.complete_ack
        assert harness.project.pad_content[215] == original
        assert not harness.persistence.config_path.exists()
        pending = harness.controller._copy
        assert pending is not None
        assert predecessor is pending.predecessor
        events.append("removed")

    def committed(sample_id: int, ticket: PadCopyTicket) -> None:
        assert harness.project.pad_content[sample_id] != original
        assert harness.persistence.config_path.exists()
        assert events == ["removed"]
        events.append("committed")

    harness = _harness(tmp_path, callback=committed, removed=removed)
    original = _occupy_target(harness)
    source_before = harness.project.pad_content[0]
    assert harness.controller.copy_overwrite(0, 215)
    assert harness.native.target_binding is harness.native.bindings[215]
    assert harness.controller.fenced_sample_ids == frozenset({215})
    harness.jobs.run()
    harness.controller.poll()
    assert harness.project.pad_content[215] == original
    assert 215 in harness.session.active_sample_ids
    assert not events
    _assert_writer_fenced(harness)
    harness.native.complete_ack = True
    harness.controller.poll()
    _assert_complete_copy(harness)
    assert events == ["removed", "committed"]
    assert harness.project.pad_content[0] == source_before


@pytest.mark.parametrize("operation", ["cancel", "changed_identity", "removed", "changed_intent"])
def test_occupied_copy_preclaim_failure_preserves_both_contents_and_never_retires_target(
    tmp_path: Path,
    operation: str,
) -> None:
    retired: list[int] = []
    harness = _harness(tmp_path, removed=lambda sample_id, _old: retired.append(sample_id))
    _occupy_target(harness)
    assert harness.controller.copy_overwrite(0, 215)
    harness.jobs.run()
    if operation == "changed_identity":
        harness.project.pad_content[215] = PadContentIdentity(
            instance_id="e" * 32, material_id="d" * 32
        )
    elif operation == "removed":
        harness.project.pad_content[215] = None
        harness.project.sample_paths[215] = None
    elif operation == "changed_intent":
        harness.project.manual_bpm[215] = 91.25
    before = harness.project.model_dump_json()
    if operation == "cancel":
        assert not harness.controller.cancel()
        harness.controller.poll()
    else:
        harness.controller.poll()
    assert not harness.controller.active
    assert harness.project.model_dump_json() == before
    assert not retired
    assert harness.native.publish_count == 0
    assert not harness.persistence.config_path.exists()


def test_occupied_copy_postclaim_cleanup_failure_retains_own_retry_and_original_custody(
    tmp_path: Path,
) -> None:
    attempts: list[int] = []

    def removed(sample_id: int, _predecessor: object) -> None:
        attempts.append(sample_id)
        if len(attempts) == 1:
            message = "original owner settlement pending"
            raise RuntimeError(message)

    harness = _harness(tmp_path, removed=removed)
    original = _occupy_target(harness)
    assert harness.controller.copy_overwrite(0, 215)
    harness.jobs.run()
    harness.controller.poll()
    harness.native.complete_ack = True
    harness.controller.poll()
    assert harness.controller.active
    assert harness.project.pad_content[215] == original
    assert harness.native.cancel_count == 0
    assert not harness.assets.retirement.closed
    _assert_writer_fenced(harness)
    harness.controller.poll()
    _assert_complete_copy(harness)
    assert attempts == [215, 215]
    assert harness.native.publish_count == 1


@pytest.mark.parametrize("newer_intent", [False, True])
def test_occupied_copy_retries_actual_transferred_asset_plan_without_republishing_old_owners(
    tmp_path: Path,
    *,
    newer_intent: bool,
) -> None:
    harness = _harness(tmp_path)
    _occupy_target(harness)
    harness.assets.planned_retirement_failures = 1
    assert harness.controller.copy_overwrite(0, 215)
    harness.jobs.run()
    harness.controller.poll()
    harness.native.complete_ack = True
    harness.controller.poll()
    assert harness.controller.active
    first = harness.assets.publication_plans[0]
    assert first.applied
    assert not first.settled
    assert harness.assets.published == [first.image]
    target_identity = harness.project.pad_content[215]
    first_leases = [lease for _, lease in first.image[1].values()]
    assert all(not lease.released for lease in first_leases)
    _assert_writer_fenced(harness)
    if newer_intent:
        harness.project.volume = 0.25
        harness.persistence.mark_dirty()
    harness.controller.poll()
    _assert_complete_copy(harness)
    assert first.settled
    assert harness.project.pad_content[215] == target_identity
    assert sum(image is first.image for image in harness.assets.published) == 1
    assert len(harness.assets.publication_plans) == (2 if newer_intent else 1)
    assert all(not lease.released for lease in first_leases)
    if newer_intent:
        assert json.loads(harness.persistence.config_path.read_text())["volume"] == 0.25


@pytest.mark.parametrize("blocked", ["empty", "missing_binding", "loading", "self"])
def test_occupied_copy_requires_loaded_original_target_without_reserving_unsupported_target(
    tmp_path: Path,
    blocked: str,
) -> None:
    harness = _harness(tmp_path)
    _occupy_target(harness)
    target = 215
    if blocked == "empty":
        harness.project.pad_content[215] = None
        harness.project.sample_paths[215] = None
    elif blocked == "missing_binding":
        harness.native.bindings.pop(215)
    elif blocked == "loading":
        harness.session.loading_sample_ids.add(215)
    else:
        target = 0
    project_before = harness.project.model_dump_json()
    session_before = harness.session.model_dump_json()
    assert not harness.controller.copy_overwrite(0, target)
    assert harness.native.capture_count == 0
    assert not harness.controller.active
    assert harness.project.model_dump_json() == project_before
    assert harness.session.model_dump_json() == session_before
