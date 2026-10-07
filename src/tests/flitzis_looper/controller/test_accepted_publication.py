"""Productive App acceptance and two-phase native derived completion."""

import struct
from concurrent.futures import Future
from threading import Event
from typing import TYPE_CHECKING
from unittest.mock import Mock, patch

import pytest

from flitzis_looper.controller.accepted_publication import ExplicitTimingAssessment
from flitzis_looper.models import BeatGrid, SampleAnalysis
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding, current_timing_metadata

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.models import TimingIntent
    from flitzis_looper_audio import ConstantTimingTicket


PERIOD = 0.2245048325556449
ASSESSMENT = ExplicitTimingAssessment(
    hypotheses_json='[{"independent": "caller-supplied count mapping"}]',
    origin_seconds=-0.125,
    origin_provenance="independent signed origin",
    acceptance_policy_version="explicit-test-v1",
    acceptance_provenance="independent explicit assessment",
)


class _PublicationTicket:
    def __init__(self, status: str = "pending") -> None:
        self.status = status

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

    def capture(*_args: object) -> object:
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
