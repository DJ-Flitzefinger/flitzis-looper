"""Negative admission/lifetime proofs; no fake Native capture or format success.

The real Thread and namespace run below. The audio substitute can only reject
preparation; it never returns a ticket, writes PCM or acknowledges an effect.
Actual Native/group/codec positive composition belongs to the Native fixtures.
The synthetic outcome cases exercise only owner fencing after a real worker
Thread returns; their metadata is not capture, codec or publication evidence.
"""

import tempfile
from pathlib import Path
from threading import Event, Thread
from time import monotonic, sleep
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller import recording
from flitzis_looper.controller.recording import RecordingController
from flitzis_looper.models import ProjectState
from flitzis_looper.recording_capture_checks import RecordingCaptureExtent
from flitzis_looper.recording_codec import RecordingCodecResult
from flitzis_looper.recording_finalize import RecordingCompletion
from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.recording_paths import create_recording_namespace
from flitzis_looper.recording_pcm import RecordingPcmStats
from flitzis_looper.recording_worker import (
    RecordingTakeWorker,
    RecordingWorkerOutcome,
    recording_native_state,
)

if TYPE_CHECKING:
    from contextlib import AbstractContextManager

    from flitzis_looper.controller.recording import RecordingGroupGate
    from flitzis_looper.recording_paths import FileIdentity
    from flitzis_looper.recording_worker import _TakeResources
    from flitzis_looper_audio import AudioEngine


@pytest.fixture
def owned_record_root() -> Path:
    workspace = Path(__file__).resolve().parents[5]
    parent = workspace / "test-artifacts" / "recording-controller-20261020"
    parent.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="negative-owner-", dir=parent))
    (root / "OWNER.md").write_text(
        "Owned recording-controller negative admission/real Thread and metadata discovery tests. "
        "No positive Native substitute, device, model or Human inputs. Preserve unknown data; "
        "remove only after all own workers and handles are terminal.\n",
        encoding="utf-8",
        newline="\n",
    )
    return root


def missing_live_witness(path: Path) -> AbstractContextManager[FileIdentity]:
    message = f"required live-spool witness is deliberately unavailable: {path.name}"
    raise NotImplementedError(message)


class RejectingPreparation:
    def __init__(self, *, blocked: bool = False) -> None:
        self.entered = Event()
        self.release = Event()
        self.calls = 0
        if not blocked:
            self.release.set()

    def prepare_mix_capture(self, spool_path: str) -> None:
        assert Path(spool_path).is_absolute()
        self.calls += 1
        self.entered.set()
        assert self.release.wait(5), "negative preparation gate was not released"
        message = "explicit test-only Native preparation rejection; no ticket created"
        raise RuntimeError(message)


class RejectedOriginalGroup:
    def __init__(self) -> None:
        self.cancelled = 0

    def is_current(self) -> bool:
        return False

    def publish(self, ticket: object) -> bool:
        pytest.fail("a rejected original group must never publish")

    def cancel(self) -> None:
        self.cancelled += 1


class BlockedCancellationGroup(RejectedOriginalGroup):
    def __init__(self) -> None:
        super().__init__()
        self.allow_cancellation = False

    def cancel(self) -> None:
        super().cancel()
        if not self.allow_cancellation:
            message = "explicit original-group cancellation rejection"
            raise RuntimeError(message)


class SyntheticPublishedOutcomeWorker(RecordingTakeWorker):
    """Metadata-only fencing seam using the actual Thread/outer release protocol.

    No Native prepare, ticket, start, PCM, finalizer or file publication occurs.
    The only synthetic facts are a started terminal projection and its outcome.
    """

    def __init__(
        self,
        root: Path,
        audio: RejectingPreparation,
        outcome: RecordingWorkerOutcome,
    ) -> None:
        super().__init__(
            cast("AudioEngine", audio),
            root,
            RecordingOptions(),
            live_spool_witness=missing_live_witness,
        )
        self.before_cleanup = Event()
        self.release_cleanup = Event()
        self.synthetic_outcome = outcome

    def _prepare(self, resources: _TakeResources) -> None:
        metadata = native_metadata()
        metadata["worker_joined"] = True
        self._update(native=recording_native_state(metadata))

    def _capture_and_finalize(self, resources: _TakeResources) -> RecordingWorkerOutcome:
        return self.synthetic_outcome

    def _release(
        self,
        resources: _TakeResources,
        outcome: RecordingWorkerOutcome,
    ) -> RecordingWorkerOutcome:
        self.before_cleanup.set()
        assert self.release_cleanup.wait(5), "synthetic owner cleanup gate was not released"
        return super()._release(resources, outcome)


def wait_worker_joined(worker: RecordingTakeWorker) -> None:
    deadline = monotonic() + 5
    while not worker.join_if_done():
        assert monotonic() < deadline, worker.snapshot()
        sleep(0.005)


def unverified_completion_metadata(root: Path) -> RecordingCompletion:
    """A DTO for failure-projection retention only; no corresponding file exists."""
    digest = "0" * 64
    return RecordingCompletion(
        path=root / "synthetic-owner-projection-only.wav",
        extent=RecordingCaptureExtent(
            first_frame=10,
            end_frame=20,
            frames=10,
            rate=48000,
            channels=2,
            session_id=1,
            payload_sha256=digest,
        ),
        pcm=RecordingPcmStats(
            frames=10,
            channels=2,
            peak_abs=0.0,
            source_overrange_samples=0,
            source_overrange_frames=0,
            source_payload_sha256=digest,
            output_payload_sha256=digest,
        ),
        codec=RecordingCodecResult(
            output_identity=(0, 1),
            output_bytes=80,
            output_sha256=digest,
            decoded_frames=10,
            decoded_payload_sha256=digest,
        ),
        warning=None,
    )


def controller_for(root: Path, audio: RejectingPreparation) -> RecordingController:
    return RecordingController(
        ProjectState(),
        cast("AudioEngine", audio),
        root,
        live_spool_witness=missing_live_witness,
    )


def wait_retired(controller: RecordingController) -> None:
    deadline = monotonic() + 5
    while True:
        controller.poll()
        if controller.status().retired:
            return
        assert monotonic() < deadline, controller.status()
        sleep(0.005)


def test_empty_left_group_creates_no_namespace_or_worker(owned_record_root: Path) -> None:
    audio = RejectingPreparation()
    controller = controller_for(owned_record_root, audio)
    controller.record_group(lambda: None)
    assert controller.status().phase == "Idle"
    assert controller.status().notice == "No active or remembered recording group"
    assert audio.calls == 0
    assert not tuple(owned_record_root.glob(".incomplete-*"))


def test_rejected_original_group_is_cancelled_without_native_admission(
    owned_record_root: Path,
) -> None:
    audio = RejectingPreparation()
    controller = controller_for(owned_record_root, audio)
    gate = RejectedOriginalGroup()
    controller.record_group(lambda: cast("RecordingGroupGate", gate))
    assert gate.cancelled == 1
    assert audio.calls == 0
    assert controller.status().phase == "Failed"
    assert controller.status().retired
    assert controller.acknowledge_failure()


def test_failed_group_cancellation_cannot_be_acknowledged_or_discarded(
    owned_record_root: Path,
) -> None:
    audio = RejectingPreparation()
    controller = controller_for(owned_record_root, audio)
    original = BlockedCancellationGroup()
    controller.record_group(lambda: cast("RecordingGroupGate", original))
    assert original.cancelled == 1
    assert controller.status().phase == "Failed"
    assert controller.status().pending == "retire"
    assert controller.status().busy
    assert not controller.status().retired
    assert not controller.acknowledge_failure()
    controller.record_mix()
    assert audio.calls == 0
    assert controller.status().busy
    original.allow_cancellation = True
    controller.poll()
    assert controller.status().retired
    assert not controller.status().busy
    assert controller.acknowledge_failure()
    assert controller.status().phase == "Idle"
    assert not tuple(owned_record_root.glob(".incomplete-*"))


def test_synthetic_started_gate_cancel_failure_survives_actual_worker_join(
    owned_record_root: Path,
) -> None:
    audio = RejectingPreparation()
    controller = controller_for(owned_record_root, audio)
    original = BlockedCancellationGroup()
    worker = SyntheticPublishedOutcomeWorker(
        owned_record_root,
        audio,
        RecordingWorkerOutcome(error="synthetic failed take"),
    )
    owner = recording._TakeOwner(
        worker,
        RecordingOptions(),
        cast("RecordingGroupGate", original),
        start_requested=True,
    )
    controller._owner = owner
    worker.start()
    try:
        assert worker.before_cleanup.wait(5)
        worker.release_cleanup.set()
        wait_worker_joined(worker)
        assert worker.snapshot().cleanup_complete
        controller.poll()
        assert controller.status().phase == "Failed"
        assert controller.status().busy
        assert controller.status().retirement_uncertain
        assert not controller.status().retired
        assert owner.gate is original
        assert owner.gate_cancel_error is not None
        assert not controller.acknowledge_failure()
        controller.record_mix()
        assert controller._owner is owner
        assert owner.gate is original
        assert audio.calls == 0
        original.allow_cancellation = True
        controller.poll()
        assert owner.gate_cancelled
        assert owner.gate_cancel_error is None
        assert owner.gate is None
        assert controller.status().retired
        assert not controller.status().retirement_uncertain
        assert controller.acknowledge_failure()
    finally:
        worker.release_cleanup.set()
        wait_worker_joined(worker)


def test_synthetic_success_outcome_cannot_erase_later_shutdown_failure(
    owned_record_root: Path,
) -> None:
    audio = RejectingPreparation()
    controller = controller_for(owned_record_root, audio)
    completion = unverified_completion_metadata(owned_record_root)
    worker = SyntheticPublishedOutcomeWorker(
        owned_record_root,
        audio,
        RecordingWorkerOutcome(completion=completion),
    )
    owner = recording._TakeOwner(worker, RecordingOptions(), None, start_requested=True)
    controller._owner = owner
    worker.start()
    try:
        assert worker.before_cleanup.wait(5)
        assert worker.snapshot().outcome is not None
        assert not worker.snapshot().cleanup_complete
        assert not controller.prepare_shutdown(deadline=monotonic() - 1)
        assert controller.status().phase == "Failed"
        assert controller.status().last_completion is completion
        assert controller.status().busy
        assert not controller.acknowledge_failure()
        worker.release_cleanup.set()
        wait_worker_joined(worker)
        controller.poll()
        assert controller.status().retired
        assert not controller.status().busy
        assert controller.status().phase == "Failed"
        assert controller.status().error == (
            "Recording shutdown deadline expired; incomplete evidence retained"
        )
        assert controller.status().last_completion is completion
        assert controller.acknowledge_failure()
        assert controller.status().phase == "Idle"
        assert controller.status().last_completion is completion
        assert not completion.path.exists()
        assert audio.calls == 0
    finally:
        worker.release_cleanup.set()
        wait_worker_joined(worker)


def test_busy_press_never_recaptures_group_and_close_waits_real_thread(
    owned_record_root: Path,
) -> None:
    audio = RejectingPreparation(blocked=True)
    controller = controller_for(owned_record_root, audio)

    def forbidden_recapture() -> RecordingGroupGate | None:
        pytest.fail("busy left press must not capture a new group or target")

    controller.record_mix()
    try:
        assert audio.entered.wait(5)
        controller.record_group(forbidden_recapture)
        assert audio.calls == 1
        assert controller.status().busy
        assert not controller.acknowledge_failure()
        assert not controller.prepare_shutdown(deadline=monotonic() - 1)
        with pytest.raises(RuntimeError, match="actual Native, worker and custody retirement"):
            controller.shut_down()
        assert not controller.status().retired
    finally:
        audio.release.set()
        wait_retired(controller)
    assert controller.prepare_shutdown(deadline=monotonic() + 1)
    controller.shut_down()


def test_take_keeps_original_options_and_failure_requires_explicit_ack(
    owned_record_root: Path,
) -> None:
    project = ProjectState()
    audio = RejectingPreparation()
    controller = RecordingController(
        project,
        cast("AudioEngine", audio),
        owned_record_root,
        live_spool_witness=missing_live_witness,
    )
    controller.record_mix()
    project.recording_options = RecordingOptions(format="mp3")
    wait_retired(controller)
    assert controller.status().options == RecordingOptions()
    assert controller.status().phase == "Failed"
    controller.record_mix()
    assert audio.calls == 1
    assert controller.acknowledge_failure()
    controller.record_mix()
    wait_retired(controller)
    assert audio.calls == 2
    assert controller.status().options == RecordingOptions(format="mp3")


def test_thread_creation_rejection_retires_without_any_native_work(
    owned_record_root: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def reject_thread_start(thread: Thread) -> None:
        message = "explicit test-only thread creation rejection"
        raise RuntimeError(message)

    monkeypatch.setattr(Thread, "start", reject_thread_start)
    audio = RejectingPreparation()
    controller = controller_for(owned_record_root, audio)
    controller.record_mix()
    controller.poll()
    assert audio.calls == 0
    assert controller.status().phase == "Failed"
    assert controller.status().retired
    assert controller.acknowledge_failure()
    assert not tuple(owned_record_root.glob(".incomplete-*"))


def test_foreign_control_thread_cannot_begin_recording(owned_record_root: Path) -> None:
    audio = RejectingPreparation()
    controller = controller_for(owned_record_root, audio)
    errors: list[str] = []

    def foreign_press() -> None:
        try:
            controller.record_mix()
        except RuntimeError as error:
            errors.append(str(error))

    thread = Thread(target=foreign_press)
    thread.start()
    thread.join(timeout=5)
    assert not thread.is_alive()
    assert errors == ["Recording intent and projections belong to the control thread"]
    assert audio.calls == 0


def test_recovery_uses_same_slot_and_bounded_released_metadata_window(
    owned_record_root: Path,
) -> None:
    for _ in range(35):
        create_recording_namespace(owned_record_root, RecordingOptions())
    audio = RejectingPreparation()
    controller = controller_for(owned_record_root, audio)
    controller.discover_recovery()
    wait_retired(controller)
    assert controller.status().phase == "Idle"
    assert controller.status().recovery_observed_count == 35
    observations = controller.recovery_observations()
    assert len(observations) == 32
    assert all(item.original_provenance_authenticated is False for item in observations)
    assert all(item.publication_verified is False for item in observations)
    assert all(item.retirement_verified is False for item in observations)
    assert audio.calls == 0


def native_metadata() -> dict[str, object]:
    # Strict metadata parser only; this dictionary grants no Native permission.
    return {
        "session_id": 1,
        "sample_rate_hz": 48000,
        "channels": 2,
        "writer_ready": False,
        "started": True,
        "rt_sealed": True,
        "rt_released": True,
        "worker_closed": True,
        "worker_joined": False,
        "first_frame": 10,
        "end_frame": 20,
        "submitted_frames": 10,
        "written_frames": 10,
        "empty": False,
        "failure": None,
    }


def test_projection_cannot_turn_closed_file_into_joined_worker() -> None:
    assert not recording_native_state(native_metadata()).quiescent


@pytest.mark.parametrize("name", ["session_id", "sample_rate_hz", "channels", "written_frames"])
def test_projection_rejects_boolean_counters(name: str) -> None:
    status = native_metadata()
    status[name] = True
    with pytest.raises(ValueError, match="strict unsigned extent"):
        recording_native_state(status)
