"""Productive App acceptance and two-phase native derived completion."""

import struct
from concurrent.futures import Future
from threading import Event
from typing import TYPE_CHECKING
from unittest.mock import Mock, patch

import pytest

from flitzis_looper.controller.accepted_publication import (
    DEFAULT_CONSTANT_TIMING_PCM_LIMIT_BYTES,
    MAX_CONSTANT_TIMING_PCM_LIMIT_BYTES,
    ExplicitTimingAssessment,
    validate_constant_timing_pcm_limit,
)
from flitzis_looper.models import BeatGrid, SampleAnalysis
from flitzis_looper.pair_snapshot import capture_pair_snapshot
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding, current_timing_metadata
from tests.flitzis_looper.controller.test_pair_control_mirror import _bundle

if TYPE_CHECKING:
    from collections.abc import Mapping
    from typing import Literal

    from flitzis_looper.controller import AppController
    from flitzis_looper.models import TimingIntent
    from flitzis_looper_audio import ConstantTimingTicket
    from tests.flitzis_looper.controller.test_pair_control_mirror import _Bundle


PERIOD = 0.2245048325556449
ASSESSMENT = ExplicitTimingAssessment(
    hypotheses_json='[{"independent": "caller-supplied count mapping"}]',
    origin_seconds=-0.125,
    origin_provenance="independent signed origin",
    acceptance_policy_version="explicit-test-v1",
    acceptance_provenance="independent explicit assessment",
)


class _PublicationTicket:
    def __init__(self, status: str = "pending", *, control_slot: int | None = 0) -> None:
        self.status = status
        self.control_slot = control_slot
        self.marker_pending = False

    def current_control_slot(self) -> int | None:
        return self.control_slot

    def placement_pending(self) -> bool:
        return self.marker_pending

    def publication_status(self) -> str:
        return self.status

    def metadata(self) -> dict[str, object]:
        return {"pad_id": 0, "request_id": 9}

    def accepted_metadata(self) -> dict[str, object]:
        # This history must never supply the period/origin used for refresh.
        return {
            "revision": "accepted-test-revision",
            "period_seconds_per_quarter": 99.0,
            "origin_seconds": 123.0,
        }


class _RefreshTicket:
    def __init__(self, status: str = "pending", *, current: bool = True) -> None:
        self.status = status
        self.current = current
        self.marker_pending = False

    def placement_pending(self) -> bool:
        return self.marker_pending

    def publication_status(self) -> str:
        return self.status

    def is_current(self) -> bool:
        return self.status == "accepted" and self.current


@pytest.fixture
def automatic_pad(controller: AppController, audio_engine_mock: Mock) -> dict[str, object]:
    project = controller.project
    project.sample_paths[0] = "samples/source.wav"
    project.sample_durations[0] = 20.0  # Accepted actual extent is deliberately independent.
    project.sample_analysis[0] = SampleAnalysis(
        bpm=135.0, key="C", beat_grid=BeatGrid(beats=[2.0], downbeats=[2.0], bars=[2.0])
    )
    project.pad_timing_intent[0] = "automatic"
    project.pad_loop_auto[0] = True
    project.pad_loop_bars[0] = 1.0
    project.pad_loop_start_s[0] = 10.000001
    project.pad_grid_offset_samples[0] = 200_000
    project.bpm_lock = True
    project.speed = 1.25
    controller.session.bpm_lock_anchor_pad_id = 0
    controller.session.master_period_seconds = 0.7
    controller.session.master_bpm = 60.0 / 0.7
    controller.session.bpm_lock_anchor_revision = "previous-effective"
    metadata = current_timing_metadata(period=PERIOD)
    audio_engine_mock.current_constant_timing.return_value = metadata
    audio_engine_mock.pad_timing_intent.return_value = "automatic"
    audio_engine_mock.refresh_current_constant_timing = Mock(return_value=_RefreshTicket())
    audio_engine_mock._content_placement_pending = Mock(return_value=False)
    audio_engine_mock.poll_loader_events.return_value = None
    controller.persistence._dirty = False
    return metadata


def _submit_publication(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    ticket: _PublicationTicket,
) -> tuple[Future[None], Mock]:
    worker = Mock()
    future: Future[None] = Future()
    worker.submit.return_value = future
    monkeypatch.setattr(
        "flitzis_looper.controller.accepted_publication.ThreadPoolExecutor",
        lambda **_kwargs: worker,
    )
    assert controller.accepted_timing.publish(0, ticket, ASSESSMENT) is future  # type: ignore[arg-type]
    worker.submit.assert_called_once_with(
        controller._audio.publish_constant_timing,
        ticket,
        ASSESSMENT.hypotheses_json,
        ASSESSMENT.origin_seconds,
        ASSESSMENT.origin_provenance,
        ASSESSMENT.acceptance_policy_version,
        ASSESSMENT.acceptance_provenance,
    )
    return future, worker


def test_productive_general_publication_needs_both_native_acknowledgements(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    publication = _PublicationTicket()
    future, _worker = _submit_publication(controller, monkeypatch, publication)
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    future.set_result(None)
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    publication.status = "accepted"
    controller.poll_runtime_events()
    args = audio_engine_mock.refresh_current_constant_timing.call_args.args
    binding, start_s, end_s, master_period = args
    assert binding.metadata()["accepted_timing"] == automatic_pad
    assert start_s == round(10.000001 * 48_000) / 48_000
    assert end_s == round((10.000001 + PERIOD * 4) * 48_000) / 48_000
    assert master_period == PERIOD / 1.25
    assert 60.0 / (60.0 / PERIOD) != PERIOD
    assert controller.session.master_period_seconds == 0.7
    assert controller.project.pad_grid_offset_samples[0] == 200_000
    assert not controller.persistence._dirty
    refresh = audio_engine_mock.refresh_current_constant_timing.return_value
    refresh.status = "accepted"
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.session.bpm_lock_anchor_revision == automatic_pad["revision"]
    assert controller.project.pad_grid_offset_samples[0] == round(PERIOD * 4 * 48_000)
    assert controller.persistence._dirty
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.set_master_period.assert_not_called()
    audio_engine_mock.bootstrap_transport_from_pad.assert_not_called()


@pytest.mark.parametrize("failure", ["rejected", "worker", "unavailable", "historical", "newer"])
def test_explicit_publication_failure_never_refreshes_from_ticket_history(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
    failure: str,
) -> None:
    publication = _PublicationTicket("rejected" if failure == "rejected" else "accepted")
    future, _worker = _submit_publication(controller, monkeypatch, publication)
    if failure == "worker":
        future.set_exception(RuntimeError("native command admission failed"))
    else:
        future.set_result(None)
    if failure in {"unavailable", "historical"}:
        audio_engine_mock.current_constant_timing.return_value = None
    if failure == "newer":
        audio_engine_mock.current_constant_timing.return_value = dict(
            automatic_pad, accepted_request_id=10
        )
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    assert controller.session.master_period_seconds == 0.7
    assert controller.project.pad_grid_offset_samples[0] == 200_000
    assert not controller.persistence._dirty
    assert not controller.accepted_timing._publications


@pytest.mark.parametrize("intent", ["manual", "tap", "legacy"])
def test_nonautomatic_cannot_publish_or_refresh_matching_numerical_values(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    intent: TimingIntent,
) -> None:
    audio_engine_mock.pad_timing_intent.return_value = intent
    controller.project.pad_timing_intent[0] = intent
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    with pytest.raises(RuntimeError, match="Automatic intent"):
        controller.accepted_timing.publish(0, _PublicationTicket(), ASSESSMENT)  # type: ignore[arg-type]
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    assert not controller.persistence._dirty
    assert controller.project.manual_bpm[0] is None


@pytest.mark.parametrize("change", ["source", "manual", "tap", "legacy", "revision", "signed_zero"])
def test_pending_refresh_current_identity_and_authority_changes_preserve_projection(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    change: str,
) -> None:
    if change == "signed_zero":
        automatic_pad["source_zero_seconds"] = -0.0
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    if change == "source":
        controller.project.sample_paths[0] = "samples/replaced.wav"
    elif change in {"manual", "tap", "legacy"}:
        audio_engine_mock.pad_timing_intent.return_value = change
    elif change == "revision":
        audio_engine_mock.current_constant_timing.return_value = dict(
            automatic_pad, revision="same-values-new-complete-evidence", publication_epoch=12
        )
    else:
        audio_engine_mock.current_constant_timing.return_value = dict(
            automatic_pad, source_zero_seconds=0.0
        )
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == 0.7
    assert controller.session.bpm_lock_anchor_revision == "previous-effective"
    assert controller.project.pad_grid_offset_samples[0] == 200_000
    assert not controller.persistence._dirty


@pytest.mark.parametrize(
    "change",
    [
        "loop_start",
        "loop_end",
        "loop_auto",
        "bars",
        "grid_anchor",
        "grid_offset",
        "speed",
        "lock",
        "anchor",
    ],
)
def test_pending_refresh_intervening_control_intent_never_commits_stale_projection(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    change: str,
) -> None:
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    project = controller.project
    if change == "loop_start":
        project.pad_loop_start_s[0] = 11.0
    elif change == "loop_end":
        project.pad_loop_end_s[0] = 19.0
    elif change == "loop_auto":
        project.pad_loop_auto[0] = False
    elif change == "bars":
        project.pad_loop_bars[0] = 2.0
    elif change == "grid_anchor":
        project.pad_grid_anchor_s[0] = -0.0
    elif change == "grid_offset":
        project.pad_grid_offset_samples[0] = -10
    elif change == "speed":
        project.speed = 1.5
    elif change == "lock":
        project.bpm_lock = False
    else:
        controller.session.bpm_lock_anchor_pad_id = 1
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == 0.7
    assert controller.session.bpm_lock_anchor_revision == "previous-effective"
    assert not controller.persistence._dirty


@pytest.mark.parametrize("status", ["pending", "rejected", "accepted_stale"])
def test_native_refresh_status_and_actual_current_fence_preserve_state(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    status: str,
) -> None:
    refresh = _RefreshTicket("accepted" if status == "accepted_stale" else status, current=False)
    audio_engine_mock.refresh_current_constant_timing.return_value = refresh
    controller.transport.bpm.on_pad_bpm_changed(0)
    controller.poll_runtime_events()
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == 0.7
    assert controller.project.pad_grid_offset_samples[0] == 200_000
    assert not controller.persistence._dirty
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.set_master_period.assert_not_called()


def test_capacity_admission_is_bounded_and_retryable_only_from_matching_current(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    audio_engine_mock.refresh_current_constant_timing.side_effect = RuntimeError(
        "full command ring"
    )
    controller.accepted_timing.refresh_current(0)
    for _ in range(4):
        controller.poll_runtime_events()
    assert audio_engine_mock.refresh_current_constant_timing.call_count == 3
    assert not controller.accepted_timing._waiting_refresh
    assert not controller.persistence._dirty
    audio_engine_mock.refresh_current_constant_timing.side_effect = None
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.persistence._dirty


def test_queue_pressure_retry_rechecks_full_current_before_admission(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    audio_engine_mock.refresh_current_constant_timing.side_effect = RuntimeError(
        "full command ring"
    )
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    audio_engine_mock.current_constant_timing.return_value = dict(
        automatic_pad, source_generation=8
    )
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.assert_called_once()
    assert not controller.accepted_timing._waiting_refresh
    assert not controller.persistence._dirty


def test_binding_capture_disagreement_cannot_admit_refresh(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = FakeInputRuntimePadBinding(
        accepted_timing=dict(automatic_pad, revision="different-full-revision"), intent="automatic"
    )
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    assert not controller.persistence._dirty


@pytest.mark.parametrize("anchor_id", [None, 1])
def test_nonanchor_current_refresh_does_not_publish_master(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    anchor_id: int | None,
) -> None:
    controller.session.bpm_lock_anchor_pad_id = anchor_id
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    assert audio_engine_mock.refresh_current_constant_timing.call_args.args[3] is None
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == 0.7
    assert controller.persistence._dirty


def test_restored_and_general_refresh_coalesce_without_losing_stem_completion(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    restored = Mock(return_value=True)
    controller.loader.set_restored_sample_loaded_callback(restored)
    controller.loader._finish_accepted_restore(0)
    controller.transport.bpm.on_pad_bpm_changed(0)
    controller.poll_runtime_events()
    controller.transport.bpm.on_pad_bpm_changed(0)
    controller.loader._finish_accepted_restore(0)
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.assert_called_once()
    restored.assert_not_called()
    assert not controller.persistence._dirty
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.poll_runtime_events()
    restored.assert_called_once_with(0)
    assert controller.persistence._dirty


def test_productive_unload_retires_explicit_refresh_observers(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    controller.loader.unload_sample(0)
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    before = controller.session.master_period_seconds
    controller.poll_runtime_events()
    assert not controller.accepted_timing._refreshes
    assert controller.session.master_period_seconds == before
    assert controller.project.sample_paths[0] is None


@pytest.mark.parametrize("prior_intent", ["manual", "tap", "legacy"])
def test_explicit_preparation_chooses_automatic_and_captures_before_worker(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
    prior_intent: TimingIntent,
) -> None:
    controller.project.pad_timing_intent[0] = prior_intent
    controller.project.manual_bpm[0] = 90.0 if prior_intent != "legacy" else None
    audio_engine_mock.pad_timing_intent.return_value = prior_intent
    order: list[str] = []

    def declare(_sample_id: int, intent: str) -> None:
        order.append("declare")
        audio_engine_mock.pad_timing_intent.return_value = intent
        audio_engine_mock.current_constant_timing.return_value = None

    def capture(*_args: object, **_kwargs: object) -> object:
        order.append("capture")
        return object()

    future: Future[ConstantTimingTicket] = Future()
    worker = Mock()

    def submit(*_args: object) -> Future[ConstantTimingTicket]:
        order.append("submit")
        return future

    worker.submit.side_effect = submit
    monkeypatch.setattr(
        "flitzis_looper.controller.accepted_publication.ThreadPoolExecutor",
        lambda **_kwargs: worker,
    )
    audio_engine_mock.set_pad_timing_intent.side_effect = declare
    audio_engine_mock.capture_current_constant_timing = Mock(side_effect=capture)
    audio_engine_mock.prepare_captured_constant_timing = Mock()
    result = controller.accepted_timing.prepare(0, 0.001, "independent bound", intent="automatic")
    assert result is future
    assert order == ["declare", "capture", "submit"]
    assert controller.project.manual_bpm[0] is None
    assert controller.project.pad_timing_intent[0] == "automatic"
    assert controller.persistence._dirty  # Intent alone is an explicit durable performer edit.
    audio_engine_mock.prepare_captured_constant_timing.assert_not_called()
    assert controller.session.master_period_seconds == 0.7
    assert controller.project.pad_grid_offset_samples[0] == 200_000


def test_automatic_replacement_preparation_failure_preserves_previous_current(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    future: Future[ConstantTimingTicket] = Future()
    worker = Mock()
    worker.submit.return_value = future
    monkeypatch.setattr(
        "flitzis_looper.controller.accepted_publication.ThreadPoolExecutor",
        lambda **_kwargs: worker,
    )
    audio_engine_mock.capture_current_constant_timing = Mock(return_value=object())
    audio_engine_mock.prepare_captured_constant_timing = Mock()
    controller.accepted_timing.prepare(0, 0.001, "independent bound", intent="automatic")
    controller.poll_runtime_events()
    future.set_exception(RuntimeError("stale captured source during worker preparation"))
    controller.poll_runtime_events()
    with pytest.raises(RuntimeError, match="stale captured source"):
        future.result()
    audio_engine_mock.set_pad_timing_intent.assert_not_called()
    assert audio_engine_mock.current_constant_timing(0) == automatic_pad
    assert controller.session.master_period_seconds == 0.7
    assert not controller.persistence._dirty


def test_failed_automatic_declaration_preserves_project_performer_intent(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    controller.project.manual_bpm[0] = 94.0
    controller.project.pad_timing_intent[0] = "manual"
    audio_engine_mock.pad_timing_intent.return_value = "manual"
    audio_engine_mock.set_pad_timing_intent.side_effect = RuntimeError("full command ring")
    with pytest.raises(RuntimeError, match="full command ring"):
        controller.accepted_timing.prepare(0, 0.001, "independent bound", intent="automatic")
    assert controller.project.manual_bpm[0] == 94.0
    assert controller.project.pad_timing_intent[0] == "manual"
    assert not controller.persistence._dirty


def test_acceptance_shutdown_drains_worker_before_native_stream_teardown(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    future, worker = _submit_publication(controller, monkeypatch, _PublicationTicket())
    order = Mock()
    order.attach_mock(worker.shutdown, "worker_shutdown")
    order.attach_mock(audio_engine_mock.shut_down, "native_shutdown")
    with patch.object(controller.persistence, "flush"):
        controller.shut_down()
    assert future.cancelled()
    assert [entry[0] for entry in order.mock_calls] == ["worker_shutdown", "native_shutdown"]
    worker.shutdown.assert_called_once_with(wait=True, cancel_futures=True)


def test_signed_zero_control_change_is_observed_before_completion(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    controller.project.pad_loop_start_s[0] = -0.0
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    controller.project.pad_loop_start_s[0] = 0.0
    assert struct.pack("!d", -0.0) != struct.pack("!d", 0.0)
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.poll_runtime_events()
    assert not controller.persistence._dirty


def test_async_preparation_uses_original_capture_after_same_path_source_replacement(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    entered = Event()
    proceed = Event()
    owner = [7]
    captured = object()
    audio_engine_mock.capture_current_constant_timing = Mock(return_value=captured)

    def prepare(source_capture: object) -> ConstantTimingTicket:
        assert source_capture is captured
        entered.set()
        assert proceed.wait(2.0)
        if owner[0] != 7:
            msg = "native captured source generation retired"
            raise RuntimeError(msg)
        msg = "test requires a retired source"
        raise AssertionError(msg)

    audio_engine_mock.prepare_captured_constant_timing = Mock(side_effect=prepare)
    try:
        future = controller.accepted_timing.prepare(
            0, 0.001, "independent bound", intent="automatic"
        )
        assert entered.wait(2.0)
        owner[0] = 8
        audio_engine_mock.current_constant_timing.return_value = dict(
            automatic_pad, source_generation=8, source_id="replacement-at-same-project-path"
        )
        proceed.set()
        with pytest.raises(RuntimeError, match="native captured source generation retired"):
            future.result(timeout=2.0)
        controller.poll_runtime_events()
        audio_engine_mock.capture_current_constant_timing.assert_called_once()
        audio_engine_mock.prepare_captured_constant_timing.assert_called_once_with(captured)
        audio_engine_mock.refresh_current_constant_timing.assert_not_called()
        assert controller.session.master_period_seconds == 0.7
        assert not controller.persistence._dirty
    finally:
        proceed.set()
        controller.accepted_timing.shut_down()


@pytest.mark.parametrize("change", ["source", "manual", "tap", "legacy"])
def test_pending_async_publication_retires_after_source_or_authority_change(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
    change: str,
) -> None:
    future, _worker = _submit_publication(controller, monkeypatch, _PublicationTicket("accepted"))
    if change == "source":
        controller.project.sample_paths[0] = "samples/replaced.wav"
    else:
        audio_engine_mock.pad_timing_intent.return_value = change
    controller.poll_runtime_events()
    assert future.cancelled()
    assert not controller.accepted_timing._publications
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    assert not controller.persistence._dirty


def test_transient_command_capacity_failure_retries_then_waits_for_native_ack(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    refresh = _RefreshTicket()
    audio_engine_mock.refresh_current_constant_timing.side_effect = [
        RuntimeError("full command ring"),
        refresh,
    ]
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    assert not controller.persistence._dirty
    controller.poll_runtime_events()
    assert audio_engine_mock.refresh_current_constant_timing.call_count == 2
    assert not controller.persistence._dirty
    refresh.status = "accepted"
    controller.poll_runtime_events()
    assert controller.persistence._dirty
    assert controller.session.master_period_seconds == PERIOD / 1.25


def test_callback_refresh_rejection_preserves_state_and_allows_explicit_retry(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    first = _RefreshTicket("rejected")
    second = _RefreshTicket()
    audio_engine_mock.refresh_current_constant_timing.side_effect = [first, second]
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    controller.poll_runtime_events()
    assert not controller.persistence._dirty
    assert controller.session.master_period_seconds == 0.7
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    second.status = "accepted"
    controller.poll_runtime_events()
    assert controller.persistence._dirty
    assert controller.session.master_period_seconds == PERIOD / 1.25


@pytest.mark.parametrize(
    ("halfwidth", "provenance"),
    [(-0.001, "independent bound"), (float("nan"), "bound"), (0.001, " ")],
)
def test_invalid_preparation_assertions_cannot_change_performer_authority(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    halfwidth: float,
    provenance: str,
) -> None:
    controller.project.manual_bpm[0] = 94.0
    controller.project.pad_timing_intent[0] = "manual"
    with pytest.raises(ValueError, match=r"finite|timing-error"):
        controller.accepted_timing.prepare(0, halfwidth, provenance, intent="automatic")
    audio_engine_mock.set_pad_timing_intent.assert_not_called()
    assert controller.project.manual_bpm[0] == 94.0
    assert controller.project.pad_timing_intent[0] == "manual"
    assert not controller.persistence._dirty


@pytest.mark.parametrize("value", [True, False, 0, -1, 1.5, "1073741824", 2**30 + 1])
def test_pcm_policy_requires_a_bounded_actual_integer(value: object) -> None:
    with pytest.raises(ValueError, match="PCM limit"):
        validate_constant_timing_pcm_limit(value)


@pytest.mark.parametrize("pcm_limit_bytes", [True, 0, -1, 2**30 + 1])
def test_invalid_pcm_policy_has_no_authority_or_capture_side_effects(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    pcm_limit_bytes: int,
) -> None:
    controller.project.manual_bpm[0] = 94.0
    controller.project.pad_timing_intent[0] = "manual"
    with pytest.raises(ValueError, match="PCM limit"):
        controller.accepted_timing.prepare(
            0, 0.001, "independent bound", intent="automatic", pcm_limit_bytes=pcm_limit_bytes
        )
    audio_engine_mock.set_pad_timing_intent.assert_not_called()
    audio_engine_mock.current_input_runtime_pad_binding.assert_not_called()
    audio_engine_mock.capture_current_constant_timing.assert_not_called()
    assert controller.accepted_timing._worker is None
    assert controller.project.manual_bpm[0] == 94.0
    assert controller.project.pad_timing_intent[0] == "manual"
    assert not controller.persistence._dirty


@pytest.mark.parametrize("pcm_limit_bytes", [None, MAX_CONSTANT_TIMING_PCM_LIMIT_BYTES])
def test_preparation_forwards_default_or_explicit_immutable_pcm_policy(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
    pcm_limit_bytes: int | None,
) -> None:
    worker = Mock()
    monkeypatch.setattr(
        "flitzis_looper.controller.accepted_publication.ThreadPoolExecutor",
        lambda **_kwargs: worker,
    )
    policy = {} if pcm_limit_bytes is None else {"pcm_limit_bytes": pcm_limit_bytes}
    controller.accepted_timing.prepare(0, 0.001, "independent bound", intent="automatic", **policy)
    expected = (
        DEFAULT_CONSTANT_TIMING_PCM_LIMIT_BYTES if pcm_limit_bytes is None else pcm_limit_bytes
    )
    assert audio_engine_mock.capture_current_constant_timing.call_args.kwargs == {
        "pcm_limit_bytes": expected
    }
    worker.submit.assert_called_once()
    audio_engine_mock.prepare_captured_constant_timing.assert_not_called()


def test_failed_replacement_capture_keeps_prior_native_refresh_completion(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    original = controller.accepted_timing._refreshes[0]
    audio_engine_mock.capture_current_constant_timing = Mock(
        side_effect=ValueError("timing bound exceeds source duration")
    )
    with pytest.raises(ValueError, match="timing bound exceeds source duration"):
        controller.accepted_timing.prepare(0, 0.001, "independent bound", intent="automatic")
    assert controller.accepted_timing._refreshes[0] is original
    original.ticket.status = "accepted"  # type: ignore[attr-defined]
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.session.bpm_lock_anchor_revision == automatic_pad["revision"]
    assert controller.persistence._dirty


def test_successful_replacement_preparation_keeps_prior_current_refresh_observer(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    controller.accepted_timing.refresh_current(0)
    controller.poll_runtime_events()
    original = controller.accepted_timing._refreshes[0]
    future: Future[ConstantTimingTicket] = Future()
    worker = Mock()
    worker.submit.return_value = future
    monkeypatch.setattr(
        "flitzis_looper.controller.accepted_publication.ThreadPoolExecutor",
        lambda **_kwargs: worker,
    )
    audio_engine_mock.capture_current_constant_timing = Mock(return_value=object())
    audio_engine_mock.prepare_captured_constant_timing = Mock()
    controller.accepted_timing.prepare(0, 0.001, "independent bound", intent="automatic")
    assert controller.accepted_timing._refreshes[0] is original
    assert not future.done()
    original.ticket.status = "accepted"  # type: ignore[attr-defined]
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.persistence._dirty


def test_overlapping_publication_is_rejected_without_losing_prior_publication_completion(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ticket = _PublicationTicket()
    future, worker = _submit_publication(controller, monkeypatch, ticket)
    original = controller.accepted_timing._publications[0]
    with pytest.raises(RuntimeError, match="completion is already pending"):
        controller.accepted_timing.publish(0, _PublicationTicket(), ASSESSMENT)  # type: ignore[arg-type]
    assert controller.accepted_timing._publications[0] is original
    assert not future.cancelled()
    assert worker.submit.call_count == 1
    future.set_result(None)
    ticket.status = "accepted"
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.persistence._dirty


@pytest.mark.parametrize("admitted", [False, True])
def test_overlapping_publication_preserves_prior_derived_completion_and_stem_callback(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    *,
    admitted: bool,
) -> None:
    completed = Mock()
    controller.accepted_timing.refresh_current(0, on_refreshed=completed)
    if admitted:
        controller.poll_runtime_events()
    with pytest.raises(RuntimeError, match="completion is already pending"):
        controller.accepted_timing.publish(0, _PublicationTicket(), ASSESSMENT)  # type: ignore[arg-type]
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.persistence._dirty
    completed.assert_called_once_with(0)


def test_native_pending_retry_keeps_original_ticket_assessment_and_attempt_future(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ticket = _PublicationTicket()
    captured_metadata = Mock(return_value={"pad_id": 0, "request_id": 9})
    monkeypatch.setattr(ticket, "metadata", captured_metadata)
    original_future, worker = _submit_publication(controller, monkeypatch, ticket)
    owner = controller.accepted_timing._publications[0]
    request_metadata = owner.request_metadata
    retry_future: Future[None] = Future()
    worker.submit.return_value = retry_future
    error = BlockingIOError("constant timing placement is pending")
    original_future.set_exception(error)
    controller.poll_runtime_events()
    assert owner.retry_requested
    worker.submit.assert_called_once()
    assert not controller.persistence._dirty
    captured_metadata.return_value = {"pad_id": 215, "request_id": 100}
    controller.poll_runtime_events()
    assert controller.accepted_timing._publications[0] is owner
    assert owner.ticket is ticket
    assert owner.assessment is ASSESSMENT
    assert owner.request_metadata is request_metadata
    assert request_metadata == {"request_id": 9}
    assert owner.original_future is original_future
    assert owner.future is retry_future
    assert original_future.exception() is error
    assert worker.submit.call_args_list[0] == worker.submit.call_args_list[1]
    captured_metadata.assert_called_once()
    audio_engine_mock.capture_current_constant_timing.assert_not_called()
    audio_engine_mock.prepare_captured_constant_timing.assert_not_called()
    audio_engine_mock.set_pad_timing_intent.assert_not_called()
    retry_future.set_result(None)
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    ticket.status = "accepted"
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == 0.7
    assert controller.project.pad_grid_offset_samples[0] == 200_000
    assert not controller.persistence._dirty
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.session.bpm_lock_anchor_revision == automatic_pad["revision"]
    assert controller.persistence._dirty


def test_already_completed_pending_attempts_submit_at_most_once_per_poll(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ticket = _PublicationTicket()
    original_future, worker = _submit_publication(controller, monkeypatch, ticket)
    first_retry: Future[None] = Future()
    first_retry.set_exception(BlockingIOError("placement still pending"))
    final_retry: Future[None] = Future()
    final_retry.set_result(None)
    worker.submit.side_effect = [first_retry, final_retry]
    original_future.set_exception(BlockingIOError("placement pending"))
    for expected_submissions in (1, 2, 2, 3):
        controller.poll_runtime_events()
        assert worker.submit.call_count == expected_submissions
        assert not controller.persistence._dirty
        audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    ticket.status = "accepted"
    controller.poll_runtime_events()
    assert worker.submit.call_count == 3
    audio_engine_mock.refresh_current_constant_timing.assert_called_once()
    assert not controller.persistence._dirty
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.poll_runtime_events()
    assert controller.persistence._dirty


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
def test_pending_wording_in_untyped_publication_error_does_not_authorize_retry(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
    error_type: type[RuntimeError | ValueError],
) -> None:
    future, worker = _submit_publication(controller, monkeypatch, _PublicationTicket())
    future.set_exception(error_type("constant timing placement is pending"))
    controller.poll_runtime_events()
    controller.poll_runtime_events()
    assert not controller.accepted_timing._publications
    worker.submit.assert_called_once()
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    assert controller.session.master_period_seconds == 0.7
    assert controller.project.pad_grid_offset_samples[0] == 200_000
    assert not controller.persistence._dirty


@pytest.mark.parametrize("marker", ["ticket", "global"])
def test_accepted_publication_reader_race_retains_original_until_both_acks(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
    marker: str,
) -> None:
    ticket = _PublicationTicket("accepted")
    future, worker = _submit_publication(controller, monkeypatch, ticket)
    owner = controller.accepted_timing._publications[0]
    future.set_result(None)
    global_pending = [False]
    audio_engine_mock._content_placement_pending.side_effect = lambda: global_pending[0]

    def unavailable_during_placement(_sample_id: int) -> None:
        if marker == "ticket":
            ticket.marker_pending = True
        else:
            global_pending[0] = True

    audio_engine_mock.current_constant_timing.side_effect = unavailable_during_placement
    controller.poll_runtime_events()
    controller.poll_runtime_events()
    assert controller.accepted_timing._publications[0] is owner
    assert owner.ticket is ticket
    assert owner.original_future is future
    worker.submit.assert_called_once()
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    assert not controller.persistence._dirty
    ticket.marker_pending = False
    global_pending[0] = False
    audio_engine_mock.current_constant_timing.side_effect = None
    controller.poll_runtime_events()
    assert not controller.accepted_timing._publications
    audio_engine_mock.refresh_current_constant_timing.assert_called_once()
    assert controller.session.master_period_seconds == 0.7
    assert not controller.persistence._dirty
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.persistence._dirty


@pytest.mark.parametrize("marker", ["ticket", "global"])
@pytest.mark.parametrize("race", ["current_reader", "current_fence"])
def test_active_refresh_marker_races_preserve_same_observer_and_completion_callback(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
    marker: str,
    race: str,
) -> None:
    completed = Mock()
    controller.accepted_timing.refresh_current(0, on_refreshed=completed)
    controller.poll_runtime_events()
    owner = controller.accepted_timing._refreshes[0]
    ticket = audio_engine_mock.refresh_current_constant_timing.return_value
    ticket.status = "accepted"
    global_pending = [False]
    audio_engine_mock._content_placement_pending.side_effect = lambda: global_pending[0]

    def begin_placement() -> None:
        if marker == "ticket":
            ticket.marker_pending = True
        else:
            global_pending[0] = True

    def unavailable_during_placement(_sample_id: int) -> None:
        begin_placement()

    def not_current_during_placement() -> bool:
        begin_placement()
        return False

    original_fence = ticket.is_current
    if race == "current_reader":
        audio_engine_mock.current_constant_timing.side_effect = unavailable_during_placement
    else:
        monkeypatch.setattr(ticket, "is_current", not_current_during_placement)
    controller.poll_runtime_events()
    controller.poll_runtime_events()
    assert controller.accepted_timing._refreshes[0] is owner
    audio_engine_mock.refresh_current_constant_timing.assert_called_once()
    assert controller.session.master_period_seconds == 0.7
    assert controller.project.pad_grid_offset_samples[0] == 200_000
    assert not controller.persistence._dirty
    completed.assert_not_called()
    ticket.marker_pending = False
    global_pending[0] = False
    audio_engine_mock.current_constant_timing.side_effect = None
    monkeypatch.setattr(ticket, "is_current", original_fence)
    controller.poll_runtime_events()
    assert not controller.accepted_timing._refreshes
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.project.pad_grid_offset_samples[0] == round(PERIOD * 4 * 48_000)
    assert controller.persistence._dirty
    completed.assert_called_once_with(0)


@pytest.mark.parametrize("marker", ["ticket", "global"])
@pytest.mark.parametrize("stale", ["current_reader", "current_fence"])
def test_refresh_pending_only_defers_genuine_stale_retirement_until_marker_clears(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    marker: str,
    stale: str,
) -> None:
    completed = Mock()
    controller.accepted_timing.refresh_current(0, on_refreshed=completed)
    controller.poll_runtime_events()
    owner = controller.accepted_timing._refreshes[0]
    ticket = audio_engine_mock.refresh_current_constant_timing.return_value
    ticket.status = "accepted"
    if stale == "current_reader":
        audio_engine_mock.current_constant_timing.return_value = None
    else:
        ticket.current = False
    if marker == "ticket":
        ticket.marker_pending = True
    else:
        audio_engine_mock._content_placement_pending.return_value = True
    controller.poll_runtime_events()
    assert controller.accepted_timing._refreshes[0] is owner
    ticket.marker_pending = False
    audio_engine_mock._content_placement_pending.return_value = False
    controller.poll_runtime_events()
    assert not controller.accepted_timing._refreshes
    completed.assert_not_called()
    assert controller.session.master_period_seconds == 0.7
    assert controller.project.pad_grid_offset_samples[0] == 200_000
    assert not controller.persistence._dirty


@pytest.mark.parametrize("stale_after_clear", [False, True])
def test_waiting_refresh_global_pending_retains_request_then_rechecks_current(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    *,
    stale_after_clear: bool,
) -> None:
    completed = Mock()
    controller.accepted_timing.refresh_current(0, on_refreshed=completed)
    owner = controller.accepted_timing._waiting_refresh[0]
    audio_engine_mock._content_placement_pending.return_value = True
    audio_engine_mock.current_constant_timing.return_value = None
    controller.poll_runtime_events()
    controller.poll_runtime_events()
    assert controller.accepted_timing._waiting_refresh[0] is owner
    assert owner.admission_failures == 0
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    completed.assert_not_called()
    assert not controller.persistence._dirty
    audio_engine_mock._content_placement_pending.return_value = False
    if not stale_after_clear:
        audio_engine_mock.current_constant_timing.return_value = automatic_pad
    controller.poll_runtime_events()
    assert not controller.accepted_timing._waiting_refresh
    if stale_after_clear:
        audio_engine_mock.refresh_current_constant_timing.assert_not_called()
        assert not controller.accepted_timing._refreshes
        completed.assert_not_called()
        assert not controller.persistence._dirty
    else:
        audio_engine_mock.refresh_current_constant_timing.assert_called_once()
        assert not controller.persistence._dirty
        audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
        controller.poll_runtime_events()
        assert controller.persistence._dirty
        completed.assert_called_once_with(0)


@pytest.mark.parametrize("race", ["current_reader", "binding_capture"])
def test_waiting_refresh_retains_callback_when_global_pending_appears_during_capture(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    race: str,
) -> None:
    completed = Mock()
    controller.accepted_timing.refresh_current(0, on_refreshed=completed)
    owner = controller.accepted_timing._waiting_refresh[0]

    def unavailable_during_placement(_sample_id: int) -> None:
        audio_engine_mock._content_placement_pending.return_value = True

    getter = (
        audio_engine_mock.current_constant_timing
        if race == "current_reader"
        else audio_engine_mock.current_input_runtime_pad_binding
    )
    original_getter = getter.side_effect
    getter.side_effect = unavailable_during_placement
    controller.poll_runtime_events()
    assert controller.accepted_timing._waiting_refresh[0] is owner
    assert owner.admission_failures == 0
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    completed.assert_not_called()
    assert not controller.persistence._dirty
    getter.side_effect = original_getter
    audio_engine_mock._content_placement_pending.return_value = False
    controller.poll_runtime_events()
    audio_engine_mock.refresh_current_constant_timing.assert_called_once()
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.poll_runtime_events()
    assert controller.persistence._dirty
    completed.assert_called_once_with(0)


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
def test_global_pending_during_refresh_admission_does_not_consume_failure_budget(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    error_type: type[RuntimeError | ValueError],
) -> None:
    completed = Mock()
    controller.accepted_timing.refresh_current(0, on_refreshed=completed)
    owner = controller.accepted_timing._waiting_refresh[0]

    def unavailable_during_placement(*_args: object) -> None:
        audio_engine_mock._content_placement_pending.return_value = True
        message = "placement became pending during admission"
        raise error_type(message)

    audio_engine_mock.refresh_current_constant_timing.side_effect = unavailable_during_placement
    controller.poll_runtime_events()
    assert controller.accepted_timing._waiting_refresh[0] is owner
    assert owner.admission_failures == 0
    assert not controller.persistence._dirty
    completed.assert_not_called()
    audio_engine_mock.refresh_current_constant_timing.side_effect = None
    audio_engine_mock._content_placement_pending.return_value = False
    controller.poll_runtime_events()
    assert audio_engine_mock.refresh_current_constant_timing.call_count == 2
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.poll_runtime_events()
    assert controller.persistence._dirty
    completed.assert_called_once_with(0)


@pytest.mark.parametrize("getter_owner", ["ticket", "global"])
@pytest.mark.parametrize("compatibility", ["missing", "truthy_non_boolean"])
def test_optional_pending_getters_require_exact_true_to_defer_completion(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
    getter_owner: str,
    compatibility: str,
) -> None:
    ticket = _PublicationTicket("accepted")
    getter = None if compatibility == "missing" else Mock(return_value=1)
    if getter_owner == "ticket":
        monkeypatch.setattr(ticket, "placement_pending", getter)
        monkeypatch.setattr(
            audio_engine_mock.refresh_current_constant_timing.return_value,
            "placement_pending",
            getter,
        )
    else:
        audio_engine_mock._content_placement_pending = getter
    future, worker = _submit_publication(controller, monkeypatch, ticket)
    future.set_result(None)
    controller.poll_runtime_events()
    worker.submit.assert_called_once()
    audio_engine_mock.refresh_current_constant_timing.assert_called_once()
    assert not controller.persistence._dirty
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.poll_runtime_events()
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.persistence._dirty


@pytest.mark.parametrize("retire", ["cancel", "shutdown"])
def test_cancel_and_shutdown_stop_deferred_publication_retry(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
    retire: str,
) -> None:
    future, worker = _submit_publication(controller, monkeypatch, _PublicationTicket())
    error = BlockingIOError("placement pending")
    future.set_exception(error)
    controller.poll_runtime_events()
    assert controller.accepted_timing._publications[0].retry_requested
    if retire == "cancel":
        controller.accepted_timing.cancel(0)
    else:
        controller.accepted_timing.shut_down()
        worker.shutdown.assert_called_once_with(wait=True, cancel_futures=True)
    controller.poll_runtime_events()
    controller.poll_runtime_events()
    assert not controller.accepted_timing._publications
    assert future.exception() is error
    worker.submit.assert_called_once()
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    assert not controller.persistence._dirty


def _automatic_pair_bundle(operation: Literal["move", "swap"]) -> tuple[_Bundle, Mock]:
    bundle = _bundle(occupied=operation == "swap")
    app = bundle.app
    project = app._project
    project.pad_timing_intent[0] = "automatic"
    project.pad_loop_auto[0] = True
    project.pad_loop_bars[0] = 1.0
    project.pad_loop_start_s[0] = 10.000001
    project.pad_grid_offset_samples[0] = 200_000
    project.bpm_lock = True
    project.speed = 1.25
    app._session.bpm_lock_anchor_pad_id = 0
    app._session.master_period_seconds = 0.7
    changed = Mock()
    app.transport._on_project_changed = changed
    bundle.audio.pad_timing_intent = Mock(return_value="automatic")
    bundle.audio._content_placement_pending = Mock(return_value=False)
    bundle.audio.publish_constant_timing = Mock()
    refresh = _RefreshTicket()
    bundle.audio.refresh_current_constant_timing = Mock(return_value=refresh)
    return bundle, changed


@pytest.mark.parametrize("operation", ["move", "swap"])
def test_pending_publication_retry_follows_pair_mirror_with_same_original_owners(
    monkeypatch: pytest.MonkeyPatch, operation: Literal["move", "swap"]
) -> None:
    bundle, changed = _automatic_pair_bundle(operation)
    app = bundle.app
    project = app._project
    ticket = _PublicationTicket()
    future, worker = _submit_publication(app, monkeypatch, ticket)
    owner = app.accepted_timing._publications[0]
    request_metadata = owner.request_metadata
    future.set_exception(BlockingIOError("placement pending"))
    app.accepted_timing.poll()
    assert owner.retry_requested
    ticket.marker_pending = True
    ticket.control_slot = None
    app.accepted_timing.poll()
    bundle.snapshot = capture_pair_snapshot(project, 0, 215, operation=operation)
    bundle.prepare().apply(bundle.acknowledgement)
    assert app.accepted_timing._publications[215] is owner
    assert 0 not in app.accepted_timing._publications
    assert owner.request_metadata is request_metadata
    assert owner.ticket is ticket
    assert owner.assessment is ASSESSMENT
    assert owner.original_future is future
    ticket.marker_pending = False
    ticket.control_slot = 215
    current = current_timing_metadata(sample_id=215, period=PERIOD)
    bundle.audio.current_constant_timing = Mock(return_value=current)
    bundle.audio.current_input_runtime_pad_binding = Mock(
        return_value=FakeInputRuntimePadBinding(215, accepted_timing=current, intent="automatic")
    )
    retry: Future[None] = Future()
    worker.submit.return_value = retry
    app.accepted_timing.poll()
    assert owner.future is retry
    assert worker.submit.call_args_list[0] == worker.submit.call_args_list[1]
    retry.set_result(None)
    ticket.status = "accepted"
    app.accepted_timing.poll()
    bundle.audio.refresh_current_constant_timing.assert_called_once()
    assert app._session.master_period_seconds == 0.7
    assert project.pad_grid_offset_samples[215] == 200_000
    changed.assert_not_called()
    bundle.audio.refresh_current_constant_timing.return_value.status = "accepted"
    app.accepted_timing.poll()
    assert app._session.bpm_lock_anchor_pad_id == 215
    assert app._session.master_period_seconds == PERIOD / 1.25
    assert project.pad_grid_offset_samples[215] == round(PERIOD * 4 * 48_000)
    changed.assert_called_once_with()


def test_publication_retains_original_when_reader_pending_closes_before_marker_probe(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ticket = _PublicationTicket("accepted")
    future, worker = _submit_publication(controller, monkeypatch, ticket)
    future.set_result(None)
    owner = controller.accepted_timing._publications[0]
    placement_token = [10]
    audio_engine_mock._content_placement_read_token = Mock(side_effect=lambda: placement_token[0])

    def unreadable_during_completed_placement(_sample_id: int) -> None:
        placement_token[0] = 11

    audio_engine_mock.current_constant_timing.side_effect = unreadable_during_completed_placement
    controller.accepted_timing.poll()
    assert controller.accepted_timing._publications[0] is owner
    assert owner.original_future is future
    assert owner.ticket is ticket
    assert not ticket.marker_pending
    assert audio_engine_mock._content_placement_pending() is False
    worker.submit.assert_called_once()
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    assert not controller.persistence._dirty
    audio_engine_mock.current_constant_timing.side_effect = None
    controller.accepted_timing.poll()
    assert not controller.accepted_timing._publications
    audio_engine_mock.refresh_current_constant_timing.assert_called_once()
    assert controller.session.master_period_seconds == 0.7
    assert controller.project.pad_grid_offset_samples[0] == 200_000
    assert not controller.persistence._dirty
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.accepted_timing.poll()
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.persistence._dirty


@pytest.mark.parametrize("race", ["current_reader", "binding_capture"])
def test_waiting_refresh_retains_request_when_unreadable_placement_has_already_closed(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    race: str,
) -> None:
    completed = Mock()
    controller.accepted_timing.refresh_current(0, on_refreshed=completed)
    owner = controller.accepted_timing._waiting_refresh[0]
    placement_token = [10]
    audio_engine_mock._content_placement_read_token = Mock(side_effect=lambda: placement_token[0])

    def unreadable_during_completed_placement(_sample_id: int) -> None:
        placement_token[0] = 11

    getter = (
        audio_engine_mock.current_constant_timing
        if race == "current_reader"
        else audio_engine_mock.current_input_runtime_pad_binding
    )
    original_getter = getter.side_effect
    getter.side_effect = unreadable_during_completed_placement
    controller.accepted_timing.poll()
    assert controller.accepted_timing._waiting_refresh[0] is owner
    assert owner.admission_failures == 0
    assert audio_engine_mock._content_placement_pending() is False
    audio_engine_mock.refresh_current_constant_timing.assert_not_called()
    completed.assert_not_called()
    assert not controller.persistence._dirty
    getter.side_effect = original_getter
    controller.accepted_timing.poll()
    assert controller.accepted_timing._refreshes[0].request is owner
    audio_engine_mock.refresh_current_constant_timing.assert_called_once()
    completed.assert_not_called()
    assert not controller.persistence._dirty
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.accepted_timing.poll()
    assert controller.persistence._dirty
    completed.assert_called_once_with(0)


@pytest.mark.parametrize("race", ["current_reader", "current_fence"])
@pytest.mark.parametrize("stale_next_poll", [False, True])
def test_active_refresh_rechecks_after_closed_placement_with_new_poll_token(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
    race: str,
    *,
    stale_next_poll: bool,
) -> None:
    completed = Mock()
    placement_token = [10]
    audio_engine_mock._content_placement_read_token = Mock(side_effect=lambda: placement_token[0])
    controller.accepted_timing.refresh_current(0, on_refreshed=completed)
    controller.accepted_timing.poll()
    owner = controller.accepted_timing._refreshes[0]
    ticket = audio_engine_mock.refresh_current_constant_timing.return_value
    ticket.status = "accepted"

    def unreadable_during_completed_placement(_sample_id: int) -> None:
        placement_token[0] = 11

    def not_current_during_completed_placement() -> bool:
        placement_token[0] = 11
        return False

    original_fence = ticket.is_current
    if race == "current_reader":
        audio_engine_mock.current_constant_timing.side_effect = (
            unreadable_during_completed_placement
        )
    else:
        monkeypatch.setattr(ticket, "is_current", not_current_during_completed_placement)
    controller.accepted_timing.poll()
    assert controller.accepted_timing._refreshes[0] is owner
    assert not ticket.marker_pending
    assert audio_engine_mock._content_placement_pending() is False
    assert controller.session.master_period_seconds == 0.7
    assert controller.project.pad_grid_offset_samples[0] == 200_000
    assert not controller.persistence._dirty
    completed.assert_not_called()
    audio_engine_mock.current_constant_timing.side_effect = None
    monkeypatch.setattr(ticket, "is_current", original_fence)
    if stale_next_poll:
        if race == "current_reader":
            audio_engine_mock.current_constant_timing.return_value = None
        else:
            ticket.current = False
    controller.accepted_timing.poll()
    assert not controller.accepted_timing._refreshes
    audio_engine_mock.refresh_current_constant_timing.assert_called_once()
    if stale_next_poll:
        completed.assert_not_called()
        assert controller.session.master_period_seconds == 0.7
        assert not controller.persistence._dirty
    else:
        completed.assert_called_once_with(0)
        assert controller.session.master_period_seconds == PERIOD / 1.25
        assert controller.persistence._dirty


@pytest.mark.parametrize("phase", ["publication", "waiting_refresh", "active_refresh"])
def test_unreadable_poll_token_defers_owned_operation_without_reading_current(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
    phase: str,
) -> None:
    owners: Mapping[int, object]
    if phase == "publication":
        future, _worker = _submit_publication(
            controller, monkeypatch, _PublicationTicket("accepted")
        )
        future.set_result(None)
        owners = controller.accepted_timing._publications
    else:
        controller.accepted_timing.refresh_current(0)
        owners = controller.accepted_timing._waiting_refresh
        if phase == "active_refresh":
            controller.accepted_timing.poll()
            owners = controller.accepted_timing._refreshes
            audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    owner = owners[0]
    audio_engine_mock._content_placement_read_token = Mock(return_value=None)
    audio_engine_mock.current_constant_timing.reset_mock()
    controller.accepted_timing.poll()
    assert owners[0] is owner
    audio_engine_mock.current_constant_timing.assert_not_called()
    assert controller.session.master_period_seconds == 0.7
    assert controller.project.pad_grid_offset_samples[0] == 200_000
    assert not controller.persistence._dirty
    audio_engine_mock._content_placement_read_token.return_value = 10
    controller.accepted_timing.poll()
    if phase != "active_refresh":
        assert not controller.persistence._dirty
        audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
        controller.accepted_timing.poll()
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.persistence._dirty


@pytest.mark.parametrize("compatibility", ["absent", "boolean", "string", "mock"])
def test_unsupported_poll_token_getter_preserves_older_native_completion(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    monkeypatch: pytest.MonkeyPatch,
    compatibility: str,
) -> None:
    invalid_token = {"boolean": True, "string": "10", "mock": Mock()}
    audio_engine_mock._content_placement_read_token = (
        None if compatibility == "absent" else Mock(return_value=invalid_token[compatibility])
    )
    future, worker = _submit_publication(controller, monkeypatch, _PublicationTicket("accepted"))
    future.set_result(None)
    controller.accepted_timing.poll()
    worker.submit.assert_called_once()
    audio_engine_mock.refresh_current_constant_timing.assert_called_once()
    assert not controller.persistence._dirty
    audio_engine_mock.refresh_current_constant_timing.return_value.status = "accepted"
    controller.accepted_timing.poll()
    assert controller.session.master_period_seconds == PERIOD / 1.25
    assert controller.persistence._dirty
