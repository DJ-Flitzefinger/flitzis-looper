"""Requested-parent, recovery, admission and actual-audio interaction regressions."""

import json
from dataclasses import asdict
from itertools import product
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.accepted_publication import ACCEPTED_REFRESH_SIGNATURE_LENGTH
from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.input_mapping.actions import global_source_sync_toggle_action
from flitzis_looper.models import PadContentIdentity, ProjectState
from flitzis_looper.source_beatmap_intent import next_source_beatmap_intent
from flitzis_looper.source_sync_intent import PadSourceSyncIntent
from flitzis_looper.timing_policy import MAX_TIMING_POLICY_REVISION, GlobalTimingPolicy
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding, current_timing_metadata
from tests.flitzis_looper.controller.test_source_sync import _feedback

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController


BPM_REFERENCE_SOURCE_IDENTITY = "b" * 64


def _master_provenance(
    source_identity: str | None = BPM_REFERENCE_SOURCE_IDENTITY,
    accepted_revision: str | None = None,
    *,
    kind: str = "qualified",
) -> dict[str, object]:
    return {
        "kind": kind,
        "source_identity": source_identity,
        "accepted_revision": accepted_revision,
    }


def _bind_fixture_source_identity(audio: Mock, *, pad_id: int) -> None:
    """Give only one locally captured original source an explicit correlation value."""
    original_provider = audio.current_input_runtime_pad_binding.side_effect
    original_source: tuple[object, ...] | None = None

    def current_binding(sample_id: int) -> object:
        nonlocal original_source
        binding = (
            original_provider(sample_id)
            if callable(original_provider)
            else audio.current_input_runtime_pad_binding.return_value
        )
        if isinstance(binding, FakeInputRuntimePadBinding):
            metadata = binding.metadata()
            source = tuple(
                metadata.get(key)
                for key in (
                    "source_id",
                    "source_generation",
                    "source_sha256",
                    "sample_rate_hz",
                    "frame_count",
                    "channels",
                    "content_lifetime",
                )
            )
            if original_source is None and sample_id == pad_id:
                original_source = source
            if source == original_source:
                binding._metadata["source_identity"] = BPM_REFERENCE_SOURCE_IDENTITY
        return binding

    audio.current_input_runtime_pad_binding.side_effect = current_binding


def _policy(
    *, sync: bool | None = True, realtime: bool | None = True, revision: int = 0
) -> GlobalTimingPolicy:
    return GlobalTimingPolicy(schema_version=1, sync=sync, realtime_bpm=realtime, revision=revision)


def _configured(controller: AppController, audio: Mock, *, pad_id: int = 0) -> dict[str, object]:
    project = controller.project
    project.multi_loop = True
    project.sample_paths[pad_id] = project.sample_paths[pad_id] or "samples/hierarchy-reference.wav"
    project.sample_durations[pad_id] = project.sample_durations[pad_id] or 10.0
    project.manual_bpm[pad_id] = project.manual_bpm[pad_id] or 120.0
    project.pad_content[pad_id] = project.pad_content[pad_id] or PadContentIdentity(
        instance_id="a" * 32
    )
    project.selected_pad = pad_id
    receipt: dict[str, object] = {
        "request_revision": 0,
        "effect_revision": 0,
        "state": "applied",
        "requested_enabled": False,
        "effective_enabled": False,
        "reason": None,
        "execution_frame": None,
        "submission_available": True,
    }
    audio.bpm_lock_status = Mock(side_effect=receipt.copy)
    _bind_fixture_source_identity(audio, pad_id=pad_id)
    initial_speed = project.speed
    initial_period = 60.0 / (project.manual_bpm[pad_id] or 120.0) / initial_speed
    actual_provenance = _master_provenance(None, kind="absent")
    submitted_provenance: dict[str, object] | None = None
    submitted_revision: int | None = None

    def observe_rate() -> dict[str, object]:
        nonlocal actual_provenance
        if receipt["state"] == "applied" and receipt["effect_revision"] == submitted_revision:
            actual_provenance = (
                dict(submitted_provenance)
                if receipt["effective_enabled"] and submitted_provenance is not None
                else _master_provenance(None, kind="absent")
            )
        return {
            "request_revision": 0,
            "effect_revision": 0,
            "state": "applied",
            "requested_speed": project.speed,
            "requested_master_period": None,
            "effective_speed": initial_speed,
            "effective_master_period": (initial_period if receipt["effective_enabled"] else None),
            "execution_frame": 0,
            "error": None,
            "master_provenance": dict(actual_provenance),
        }

    audio.global_rate_status = Mock(side_effect=observe_rate)

    def submit(*, enabled: bool, anchor_binding: object = None) -> None:
        nonlocal submitted_provenance, submitted_revision
        if enabled:
            assert isinstance(anchor_binding, FakeInputRuntimePadBinding)
            metadata = anchor_binding.metadata()
            accepted = metadata["accepted_timing"]
            submitted_provenance = _master_provenance(
                cast("str", metadata["source_identity"]),
                cast("str", accepted["revision"]) if isinstance(accepted, dict) else None,
            )
        receipt.update(
            request_revision=cast("int", receipt["request_revision"]) + 1,
            state="queued",
            requested_enabled=enabled,
            reason=None,
            execution_frame=None,
        )
        submitted_revision = cast("int", receipt["request_revision"])

    audio.set_bpm_lock = Mock(side_effect=submit)
    return receipt


def _acknowledged_bpm_reference(controller: AppController) -> None:
    """Fixture-only coherent Native effect; real admission/audio has separate regressions."""
    parameters = controller.transport.global_params
    parameters._begin_bpm_lock_application(capture_anchor=False)
    qualified = parameters._qualified_bpm_anchor()
    assert qualified is not None
    application = parameters._bpm_application
    assert application is not None
    application.fingerprint = qualified[1]
    application.admitted_revision = 1
    application.master_projected = True
    parameters._bpm_feedback = {
        "request_revision": 1,
        "effect_revision": 1,
        "state": "applied",
        "requested_enabled": True,
        "effective_enabled": True,
        "reason": None,
        "execution_frame": 64,
        "submission_available": True,
    }


def _rate_image(
    audio: Mock,
    *,
    state: str,
    revision: int = 2,
    requested_period: float = 1.0 / 3.0,
    accepted_revision: str | None = None,
    source_identity: str | None = BPM_REFERENCE_SOURCE_IDENTITY,
    provenance_kind: str = "qualified",
) -> dict[str, object]:
    feedback: dict[str, object] = {
        "request_revision": revision,
        "effect_revision": 0 if state == "queued" else revision,
        "state": state,
        "requested_speed": 1.5,
        "requested_master_period": requested_period,
        "effective_speed": 1.0,
        "effective_master_period": 0.5,
        "execution_frame": None if state == "queued" else 128,
        "error": "Source history is unavailable." if state == "rejected" else None,
        "master_provenance": _master_provenance(
            source_identity, accepted_revision, kind=provenance_kind
        ),
    }
    audio.global_rate_status.side_effect = None
    audio.global_rate_status.return_value = feedback
    return feedback


def _enabled_reference(controller: AppController, audio: Mock) -> None:
    receipt = _configured(controller, audio)
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    receipt.update(state="applied", effect_revision=1, effective_enabled=True, execution_frame=64)
    parameters.poll_bpm_lock_application()


def _applied_reference(
    controller: AppController, audio: Mock, *, automatic: bool
) -> dict[str, object]:
    receipt = _configured(controller, audio)
    if automatic:
        controller.project.manual_bpm[0] = None
        controller.project.pad_timing_intent[0] = "automatic"
        audio.pad_timing_intent.return_value = "automatic"
        audio.current_constant_timing.return_value = current_timing_metadata(period=0.5)
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    receipt.update(state="applied", effect_revision=1, effective_enabled=True, execution_frame=64)
    parameters.poll_bpm_lock_application()
    application = parameters._bpm_application
    assert application is not None
    assert application.master_projected
    assert not parameters._bpm_lock_application_status().pending
    return receipt


@pytest.mark.parametrize("automatic", [False, True])
@pytest.mark.parametrize("transient", ["feedback", "hold"])
def test_applied_bpm_reference_recovers_transient_error_without_replaying_effects(
    controller: AppController,
    audio_engine_mock: Mock,
    *,
    automatic: bool,
    transient: str,
) -> None:
    receipt = _applied_reference(controller, audio_engine_mock, automatic=automatic)
    parameters = controller.transport.global_params
    original_guard = controller.transport._pad_edit_guard
    if transient == "feedback":
        audio_engine_mock.bpm_lock_status.side_effect = lambda: None
    else:
        controller.transport._pad_edit_guard = Mock(side_effect=ValueError("Original source held."))
    parameters.poll_bpm_lock_application()
    failed = parameters._bpm_lock_application_status()
    assert failed.pending
    assert failed.detail is not None
    parameters.poll_bpm_lock_application()
    assert parameters._bpm_lock_application_status().pending

    audio_engine_mock.bpm_lock_status.side_effect = receipt.copy
    controller.transport._pad_edit_guard = original_guard
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    recovered = parameters._bpm_lock_application_status()
    assert recovered.effective is True
    assert not recovered.pending
    assert recovered.detail is None
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert controller.session.master_period_seconds == 0.5
    audio_engine_mock.set_bpm_lock.assert_called_once()
    audio_engine_mock.set_master_period.assert_not_called()
    audio_engine_mock.set_master_bpm.assert_not_called()
    audio_engine_mock.bootstrap_transport_from_pad.assert_not_called()


@pytest.mark.parametrize("blocked", ["hold", "timing", "replacement", "foreign", "pending"])
def test_readable_bpm_feedback_cannot_clear_an_unqualified_or_unapplied_error(
    controller: AppController, audio_engine_mock: Mock, blocked: str
) -> None:
    receipt = _applied_reference(controller, audio_engine_mock, automatic=True)
    parameters = controller.transport.global_params
    audio_engine_mock.bpm_lock_status.side_effect = lambda: None
    parameters.poll_bpm_lock_application()
    assert parameters._bpm_lock_application_status().pending
    audio_engine_mock.bpm_lock_status.side_effect = receipt.copy
    if blocked == "hold":
        controller.transport._pad_edit_guard = Mock(side_effect=ValueError("Original source held."))
    elif blocked == "timing":
        audio_engine_mock.current_constant_timing.return_value = None
    elif blocked == "replacement":
        controller.project.pad_content[0] = PadContentIdentity(instance_id="b" * 32)
    elif blocked == "foreign":
        receipt.update(request_revision=2, effect_revision=2)
    else:
        receipt.update(state="pending", effect_revision=0, execution_frame=None)
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    status = parameters._bpm_lock_application_status()
    assert status.pending
    assert status.detail is not None
    audio_engine_mock.set_bpm_lock.assert_called_once()
    audio_engine_mock.set_master_period.assert_not_called()
    audio_engine_mock.bootstrap_transport_from_pad.assert_not_called()


def test_recovered_bpm_observation_preserves_new_manual_master_failure(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipt = _applied_reference(controller, audio_engine_mock, automatic=False)
    parameters = controller.transport.global_params
    audio_engine_mock.bpm_lock_status.side_effect = lambda: None
    parameters.poll_bpm_lock_application()
    audio_engine_mock.bpm_lock_status.side_effect = receipt.copy
    controller.project.manual_bpm[0] = 100.0
    audio_engine_mock.set_master_period.side_effect = RuntimeError("Master admission unavailable.")
    parameters.poll_bpm_lock_application()
    status = parameters._bpm_lock_application_status()
    assert status.pending
    assert status.detail == "Master admission unavailable."
    assert controller.session.master_period_seconds == 0.5
    audio_engine_mock.set_bpm_lock.assert_called_once()
    audio_engine_mock.set_master_period.assert_called_once()
    assert audio_engine_mock.set_master_period.call_args.args == (0.6,)
    assert "anchor_binding" in audio_engine_mock.set_master_period.call_args.kwargs
    audio_engine_mock.bootstrap_transport_from_pad.assert_not_called()


def test_manual_master_after_rejected_speed_uses_actual_factor_and_actual_ack_only(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _enabled_reference(controller, audio_engine_mock)
    controller.project.speed = 1.5
    feedback = _rate_image(audio_engine_mock, state="rejected")
    parameters = controller.transport.global_params
    parameters._poll_global_rate_status()
    controller.project.manual_bpm[0] = 100.0
    controller.transport.bpm.recompute_master_bpm()
    audio_engine_mock.set_master_period.assert_called_once()
    assert audio_engine_mock.set_master_period.call_args.args == (0.6,)
    assert "anchor_binding" in audio_engine_mock.set_master_period.call_args.kwargs
    audio_engine_mock.set_speed.assert_not_called()
    audio_engine_mock.set_speed_and_master_period.assert_not_called()
    assert controller.project.speed == 1.5
    assert controller.session.master_period_seconds == 0.5
    feedback.update(
        request_revision=3,
        effect_revision=3,
        state="applied",
        requested_speed=1.0,
        requested_master_period=0.6,
        effective_master_period=0.6,
        error=None,
    )
    parameters._poll_global_rate_status()
    assert controller.session.master_period_seconds == 0.6
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    audio_engine_mock.set_master_period.assert_called_once()
    audio_engine_mock.set_bpm_lock.assert_called_once()


@pytest.mark.parametrize("state", ["queued", "preparing"])
def test_manual_timing_waits_for_terminal_rate_then_reconciles_once(
    controller: AppController, audio_engine_mock: Mock, state: str
) -> None:
    _enabled_reference(controller, audio_engine_mock)
    controller.project.speed = 1.5
    feedback = _rate_image(audio_engine_mock, state=state)
    parameters = controller.transport.global_params
    parameters._poll_global_rate_status()
    controller.project.manual_bpm[0] = 100.0
    controller.transport.bpm.recompute_master_bpm()
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    audio_engine_mock.set_master_period.assert_not_called()
    feedback.update(
        request_revision=3,
        effect_revision=3,
        state="rejected",
        execution_frame=192,
        error="Source history is unavailable.",
    )
    parameters._poll_global_rate_status()
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    audio_engine_mock.set_master_period.assert_called_once()
    assert audio_engine_mock.set_master_period.call_args.args == (0.6,)
    assert controller.project.speed == 1.5
    audio_engine_mock.set_speed.assert_not_called()
    audio_engine_mock.set_speed_and_master_period.assert_not_called()


def test_periodic_rejected_master_dedup_preserves_explicit_retry_and_new_source_context(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _enabled_reference(controller, audio_engine_mock)
    parameters = controller.transport.global_params
    _rate_image(audio_engine_mock, state="rejected")
    parameters._poll_global_rate_status()
    controller.project.manual_bpm[0] = 100.0
    controller.transport.bpm.recompute_master_bpm()
    feedback = _rate_image(audio_engine_mock, state="rejected", revision=3, requested_period=0.6)
    parameters._poll_global_rate_status()
    for _ in range(4):
        parameters.poll_bpm_lock_application()
    audio_engine_mock.set_master_period.assert_called_once()
    # A real repeated gesture may qualify once; periodic polling cannot flood it.
    controller.transport.bpm.recompute_master_bpm()
    assert audio_engine_mock.set_master_period.call_count == 2
    feedback.update(request_revision=4, effect_revision=4)
    parameters._poll_global_rate_status()
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_master_period.call_count == 2
    original_binding = audio_engine_mock.current_input_runtime_pad_binding(0)
    original_binding._metadata["window_revision"] = 2
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = original_binding
    parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_master_period.call_count == 3
    assert audio_engine_mock.set_master_period.call_args.args == (0.6,)
    binding = audio_engine_mock.set_master_period.call_args.kwargs["anchor_binding"]
    assert binding.metadata()["window_revision"] == 2
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_master_period.call_count == 3
    audio_engine_mock.set_bpm_lock.assert_called_once()


def _master_session(controller: AppController) -> tuple[object, ...]:
    session = controller.session
    return (
        session.master_period_seconds,
        session.master_bpm,
        session.bpm_lock_anchor_bpm,
        session.bpm_lock_anchor_revision,
    )


def _switch_to_same_source_manual_timing(
    controller: AppController, audio: Mock, *, bpm: float
) -> None:
    original = audio.current_input_runtime_pad_binding(0).metadata()
    controller.project.manual_bpm[0] = bpm
    controller.project.pad_timing_intent[0] = "manual"
    audio.current_constant_timing.return_value = None
    audio.pad_timing_intent.return_value = "manual"

    def current_binding(sample_id: int) -> FakeInputRuntimePadBinding:
        binding = FakeInputRuntimePadBinding(sample_id)
        binding._metadata.update(
            original,
            pad_id=sample_id,
            intent="manual",
            accepted_timing=None,
        )
        return binding

    audio.current_input_runtime_pad_binding.side_effect = current_binding


@pytest.mark.parametrize("manual_period", [0.5, 0.6])
def test_manual_master_replaces_accepted_provenance_only_after_its_own_actual_effect(
    controller: AppController, audio_engine_mock: Mock, manual_period: float
) -> None:
    _applied_reference(controller, audio_engine_mock, automatic=True)
    parameters = controller.transport.global_params
    previous = _master_session(controller)
    accepted_revision = controller.session.bpm_lock_anchor_revision
    assert accepted_revision is not None
    _switch_to_same_source_manual_timing(controller, audio_engine_mock, bpm=60.0 / manual_period)
    rate = _rate_image(audio_engine_mock, state="applied", accepted_revision=accepted_revision)
    parameters._poll_global_rate_status()
    controller.transport.bpm.recompute_master_bpm()
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    assert _master_session(controller) == previous
    audio_engine_mock.set_master_period.assert_called_once()
    assert audio_engine_mock.set_master_period.call_args.args == (manual_period,)
    assert (
        audio_engine_mock.set_master_period.call_args.kwargs["anchor_binding"].metadata()[
            "source_identity"
        ]
        == BPM_REFERENCE_SOURCE_IDENTITY
    )
    rate.update(
        request_revision=3,
        effect_revision=3,
        requested_master_period=manual_period,
        effective_master_period=manual_period,
        master_provenance=_master_provenance(),
    )
    parameters._poll_global_rate_status()
    assert controller.session.master_period_seconds == manual_period
    assert controller.session.master_bpm == 60.0 / manual_period
    assert controller.session.bpm_lock_anchor_bpm == 60.0 / manual_period
    assert controller.session.bpm_lock_anchor_revision is None
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    audio_engine_mock.set_master_period.assert_called_once()
    audio_engine_mock.set_bpm_lock.assert_called_once()
    audio_engine_mock.set_master_bpm.assert_not_called()
    audio_engine_mock.bootstrap_transport_from_pad.assert_not_called()


@pytest.mark.parametrize("state", ["queued", "preparing", "rejected"])
def test_pending_or_rejected_request_preserves_older_own_actual_accepted_proof(
    controller: AppController, audio_engine_mock: Mock, state: str
) -> None:
    _applied_reference(controller, audio_engine_mock, automatic=True)
    parameters = controller.transport.global_params
    revision = controller.session.bpm_lock_anchor_revision
    assert revision is not None
    audio_engine_mock.current_constant_timing.return_value["revision"] = "requested-later-revision"
    rate = _rate_image(audio_engine_mock, state=state, accepted_revision=revision)
    parameters._poll_global_rate_status()
    assert parameters._bpm_lock_master_ready()
    assert not parameters._bpm_lock_application_status().pending
    assert controller.session.bpm_lock_anchor_revision == revision
    assert controller.session.master_period_seconds == rate["effective_master_period"]
    audio_engine_mock.set_master_period.assert_not_called()
    audio_engine_mock.set_bpm_lock.assert_called_once()


@pytest.mark.parametrize("failure", ["unknown", "absent", "foreign", "own_bool", "content_aba"])
@pytest.mark.parametrize("state", ["applied", "queued", "rejected"])
def test_unconfirmed_actual_master_preserves_the_entire_previous_session_projection(
    controller: AppController, audio_engine_mock: Mock, failure: str, state: str
) -> None:
    _applied_reference(controller, audio_engine_mock, automatic=True)
    parameters = controller.transport.global_params
    previous = _master_session(controller)
    kind = failure if failure in {"unknown", "absent"} else "qualified"
    source = (
        None
        if kind != "qualified"
        else ("c" * 64 if failure == "foreign" else BPM_REFERENCE_SOURCE_IDENTITY)
    )
    rate = _rate_image(
        audio_engine_mock,
        state=state,
        source_identity=source,
        provenance_kind=kind,
        accepted_revision="new-actual" if kind == "qualified" else None,
    )
    rate.update(effective_master_period=0.7, effective_speed=1.25)
    if failure == "own_bool":
        assert parameters._bpm_feedback is not None
        parameters._bpm_feedback.update(request_revision=7, effect_revision=7)
    elif failure == "content_aba":
        controller.project.pad_content[0] = PadContentIdentity(instance_id="c" * 32)
    parameters._poll_global_rate_status()
    assert _master_session(controller) == previous
    assert parameters.global_rate_status().effective_master_period == 0.7
    assert parameters.global_rate_status().effective_speed == 1.25
    assert parameters._bpm_lock_application_status().pending
    assert parameters._bpm_application_error is None
    audio_engine_mock.set_master_period.assert_not_called()
    audio_engine_mock.set_bpm_lock.assert_called_once()


@pytest.mark.parametrize("kind", ["unknown", "absent", "foreign"])
def test_same_numeric_manual_period_without_own_manual_proof_still_publishes_once(
    controller: AppController, audio_engine_mock: Mock, kind: str
) -> None:
    _applied_reference(controller, audio_engine_mock, automatic=True)
    parameters = controller.transport.global_params
    _switch_to_same_source_manual_timing(controller, audio_engine_mock, bpm=120.0)
    _rate_image(
        audio_engine_mock,
        state="applied",
        provenance_kind="qualified" if kind == "foreign" else kind,
        source_identity="c" * 64 if kind == "foreign" else None,
    )
    parameters._poll_global_rate_status()
    controller.transport.bpm.recompute_master_bpm()
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    audio_engine_mock.set_master_period.assert_called_once()
    assert audio_engine_mock.set_master_period.call_args.args == (0.5,)
    assert parameters._bpm_lock_application_status().pending


@pytest.mark.parametrize("failure", ["missing", "malformed"])
def test_same_actual_proof_recovers_after_unreadable_rate_without_replaying_effects(
    controller: AppController, audio_engine_mock: Mock, failure: str
) -> None:
    _applied_reference(controller, audio_engine_mock, automatic=True)
    parameters = controller.transport.global_params
    previous = _master_session(controller)
    valid = audio_engine_mock.global_rate_status()
    audio_engine_mock.global_rate_status.side_effect = None
    broken = dict(valid)
    broken.pop("master_provenance")
    audio_engine_mock.global_rate_status.return_value = None if failure == "missing" else broken
    parameters._poll_global_rate_status()
    assert parameters._bpm_lock_application_status().pending
    assert _master_session(controller) == previous
    assert parameters._bpm_application_error is None
    audio_engine_mock.global_rate_status.return_value = valid
    parameters._poll_global_rate_status()
    assert not parameters._bpm_lock_application_status().pending
    assert _master_session(controller) == previous
    audio_engine_mock.set_master_period.assert_not_called()
    audio_engine_mock.set_bpm_lock.assert_called_once()
    audio_engine_mock.bootstrap_transport_from_pad.assert_not_called()


@pytest.mark.parametrize("failure", ["binding", "hold", "retired"])
def test_fresh_binding_failure_only_unconfirms_the_derived_master_view(
    controller: AppController, audio_engine_mock: Mock, failure: str
) -> None:
    _applied_reference(controller, audio_engine_mock, automatic=True)
    parameters = controller.transport.global_params
    previous = _master_session(controller)
    original_provider = audio_engine_mock.current_input_runtime_pad_binding.side_effect
    original_guard = controller.transport._pad_edit_guard
    if failure == "binding":
        audio_engine_mock.current_input_runtime_pad_binding.side_effect = RuntimeError(
            "Read unavailable."
        )
    elif failure == "hold":
        controller.transport._pad_edit_guard = Mock(side_effect=ValueError("Original source held."))
    else:
        binding = original_provider(0)
        binding.is_current = lambda: False
        audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
        audio_engine_mock.current_input_runtime_pad_binding.return_value = binding
    parameters._poll_global_rate_status()
    assert parameters._bpm_lock_application_status().pending
    assert parameters._bpm_application_error is None
    assert parameters._rate_status_error is None
    assert _master_session(controller) == previous
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = original_provider
    controller.transport._pad_edit_guard = original_guard
    parameters._poll_global_rate_status()
    assert not parameters._bpm_lock_application_status().pending
    assert _master_session(controller) == previous
    audio_engine_mock.set_master_period.assert_not_called()
    audio_engine_mock.set_bpm_lock.assert_called_once()


def test_resident_window_change_preserves_original_actual_master_correlation(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _applied_reference(controller, audio_engine_mock, automatic=True)
    parameters = controller.transport.global_params
    previous = _master_session(controller)
    binding = audio_engine_mock.current_input_runtime_pad_binding(0)
    binding._metadata.update(
        window_revision=9,
        resident_start_frame=1_000,
        resident_end_frame=2_000,
        resident_pcm_identity=77,
        resident_context="new-window-context",
        authority_revision=99,
    )
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = binding
    parameters._poll_global_rate_status()
    assert not parameters._bpm_lock_application_status().pending
    assert _master_session(controller) == previous
    audio_engine_mock.set_master_period.assert_not_called()
    audio_engine_mock.set_bpm_lock.assert_called_once()


@pytest.mark.parametrize(
    ("multi", "bpm", "sync", "pad", "realtime", "recovery"),
    list(product([False, True], repeat=6)),
)
def test_complete_requested_parent_matrix_preserves_every_raw_choice(
    *, multi: bool, bpm: bool, sync: bool, pad: bool, realtime: bool, recovery: bool
) -> None:
    project = ProjectState(multi_loop=multi, bpm_lock=bpm)
    project.global_timing_policy = _policy(sync=sync, realtime=realtime, revision=7)
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=pad, revision=4)
    before = project.model_dump()
    if recovery:
        project.quarantine_timing_policy()
    parent = multi and bpm and not recovery
    assert project.resolve_bpm_lock() is parent
    assert project.resolve_pad_source_sync(0) is (parent and sync and pad)
    assert project.resolve_pad_realtime_bpm(0) is (parent and sync and pad and realtime)
    assert project.model_dump() == before
    assert "recovery" not in project.model_dump_json()


def test_neutral_realtime_default_is_on_only_below_available_parents() -> None:
    project = ProjectState(multi_loop=True, bpm_lock=True)
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=0)
    assert project.resolve_pad_realtime_bpm(0)
    project.multi_loop = False
    assert not project.resolve_pad_realtime_bpm(0)
    assert project.pad_source_sync_intent[0].requested


@pytest.mark.parametrize("parent", ["multi_loop", "bpm_lock"])
def test_parent_noop_and_exhaustion_have_no_native_or_preference_prefix(
    controller: AppController, audio_engine_mock: Mock, parent: str
) -> None:
    _configured(controller, audio_engine_mock)
    controller.project.bpm_lock = True
    controller.project.global_timing_policy = _policy(revision=MAX_TIMING_POLICY_REVISION)
    setter = getattr(controller.transport.global_params, f"set_{parent}")
    before = controller.project.model_dump()
    audio_engine_mock.reset_mock()
    setter(enabled=True)
    assert not audio_engine_mock.mock_calls
    with pytest.raises(ValueError, match="capacity"):
        setter(enabled=False)
    assert controller.project.model_dump() == before
    assert not audio_engine_mock.mock_calls


@pytest.mark.parametrize("reserved", [{0}, {0, 36, 215}])
def test_reserved_multiloop_cancellation_uses_exact_previous_whole_image_and_no_adoption(
    controller: AppController, audio_engine_mock: Mock, reserved: set[int]
) -> None:
    _configured(controller, audio_engine_mock)
    controller.project.bpm_lock = True
    controller.project.global_timing_policy = _policy(revision=9)
    mapping = controller.input_mapping
    assert mapping._refresh_rust_runtime_state()
    original = mapping._last_input_runtime_state_signature
    assert original is not None
    controller.transport._pair_deferred_sample_ids = frozenset(reserved)
    audio_engine_mock.reset_mock()
    controller.transport.global_params.set_multi_loop(enabled=False)
    staged = mapping._last_input_runtime_state_signature
    assert staged == (False, original[1], original[2], original[3])
    assert staged[2] is original[2]
    audio_engine_mock.request_input_runtime_policy_adoption.assert_not_called()
    assert audio_engine_mock.set_input_runtime_state.call_count == 1
    assert controller.project.global_timing_policy.revision == 10
    assert controller.project.global_timing_policy.sync is True
    controller.transport.global_params.poll_bpm_lock_application()
    assert audio_engine_mock.set_input_runtime_state.call_count == 1
    controller.transport.global_params.set_multi_loop(enabled=True)
    assert controller.project.global_timing_policy.revision == 11
    assert mapping._last_input_runtime_state_signature[0] is True
    assert audio_engine_mock.set_input_runtime_state.call_count == 2


@pytest.mark.parametrize("missing", [False, True])
def test_reserved_multiloop_missing_image_or_failed_stage_preserves_raw_revision(
    controller: AppController, audio_engine_mock: Mock, *, missing: bool
) -> None:
    _configured(controller, audio_engine_mock)
    if not missing:
        assert controller.input_mapping._refresh_rust_runtime_state()
    else:
        controller.input_mapping._last_input_runtime_state_signature = None
    controller.transport._pair_deferred_sample_ids = frozenset({0})
    audio_engine_mock.set_input_runtime_state.side_effect = RuntimeError("stage capacity")
    before = controller.project.model_dump()
    with pytest.raises(RuntimeError, match="cancellation"):
        controller.transport.global_params.set_multi_loop(enabled=False)
    assert controller.project.model_dump() == before


def test_late_anchor_ack_does_not_follow_selection_or_flood_admitted_native_target(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipt = _configured(controller, audio_engine_mock)
    controller.project.manual_bpm[0] = None
    controller.transport.global_params.set_bpm_lock(enabled=True)
    audio_engine_mock.set_bpm_lock.assert_not_called()
    controller.project.selected_pad = 5
    controller.project.manual_bpm[0] = 120.0
    parameters = controller.transport.global_params
    parameters.poll_bpm_lock_application()
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert audio_engine_mock.set_bpm_lock.call_count == 1
    assert "anchor_binding" in audio_engine_mock.set_bpm_lock.call_args.kwargs
    audio_engine_mock.set_master_bpm.assert_not_called()
    for _ in range(4):
        parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_bpm_lock.call_count == 1
    receipt.update(state="applied", effect_revision=1, effective_enabled=True, execution_frame=64)
    for _ in range(4):
        parameters.poll_bpm_lock_application()
    assert parameters._bpm_lock_application_status().effective is True
    assert not parameters._bpm_lock_application_status().pending
    audio_engine_mock.set_master_bpm.assert_not_called()
    audio_engine_mock.bootstrap_transport_from_pad.assert_not_called()
    assert controller.session.master_bpm == 120.0


def _empty_bpm_reference(controller: AppController, audio: Mock) -> dict[str, object]:
    receipt = _configured(controller, audio)
    project = controller.project
    project.pad_content[0] = None
    project.sample_paths[0] = None
    project.manual_bpm[0] = None
    project.sample_durations[0] = None
    controller.transport.global_params.set_bpm_lock(enabled=True)
    application = controller.transport.global_params._bpm_application
    assert application is not None
    assert application.original_content_id is None
    audio.set_bpm_lock.assert_not_called()
    return receipt


def _complete_reference_content(controller: AppController, *, bpm: float | None) -> None:
    """Project only a completed assignment; Native source/audio custody is separately tested."""
    controller.project.sample_paths[0] = "samples/first-reference.wav"
    controller.project.sample_durations[0] = 10.0
    controller.project.pad_content[0] = PadContentIdentity(instance_id="c" * 32)
    controller.project.manual_bpm[0] = bpm


def test_empty_activation_binds_first_captured_slot_before_late_timing_ack_once(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipt = _empty_bpm_reference(controller, audio_engine_mock)
    parameters = controller.transport.global_params
    assert parameters._bpm_anchor_waiting_for_first_content(0)
    assert not parameters._bpm_anchor_waiting_for_first_content(5)
    controller.project.selected_pad = 5
    controller.project.sample_paths[5] = "samples/unrelated.wav"
    controller.project.pad_content[5] = PadContentIdentity(instance_id="b" * 32)
    controller.project.manual_bpm[5] = 90.0
    parameters.poll_bpm_lock_application()
    application = parameters._bpm_application
    assert application is not None
    assert application.original_content_id is None
    _complete_reference_content(controller, bpm=None)
    controller.project.pad_timing_intent[0] = "automatic"
    audio_engine_mock.pad_timing_intent.side_effect = lambda sample_id: (
        "automatic" if sample_id == 0 else "legacy"
    )
    accepted: dict[str, object] | None = None
    audio_engine_mock.current_constant_timing.side_effect = lambda sample_id: (
        accepted if sample_id == 0 else None
    )
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = lambda sample_id: (
        FakeInputRuntimePadBinding(
            sample_id,
            accepted_timing=accepted if sample_id == 0 else None,
            intent="automatic" if sample_id == 0 else "legacy",
            source_identity=BPM_REFERENCE_SOURCE_IDENTITY if sample_id == 0 else None,
        )
    )
    parameters.poll_bpm_lock_application()
    assert application.original_content_id == "c" * 32
    assert not parameters._bpm_anchor_waiting_for_first_content(0)
    audio_engine_mock.set_bpm_lock.assert_not_called()
    accepted = current_timing_metadata(period=0.5)
    for _ in range(4):
        parameters.poll_bpm_lock_application()
    audio_engine_mock.set_bpm_lock.assert_called_once()
    binding = audio_engine_mock.set_bpm_lock.call_args.kwargs["anchor_binding"]
    assert binding.metadata()["pad_id"] == 0
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert controller.project.selected_pad == 5
    receipt.update(state="applied", effect_revision=1, effective_enabled=True, execution_frame=64)
    parameters.poll_bpm_lock_application()
    assert not parameters._bpm_lock_application_status().pending
    assert controller.session.master_period_seconds == 0.5


@pytest.mark.parametrize("guard", ["incoming", "loading", "hold"])
def test_empty_activation_cannot_bind_before_its_assignment_or_hold_guard_clears(
    controller: AppController, audio_engine_mock: Mock, guard: str
) -> None:
    _empty_bpm_reference(controller, audio_engine_mock)
    _complete_reference_content(controller, bpm=120.0)
    transport = controller.transport
    original_incoming = transport._incoming_load_request
    original_guard = transport._pad_edit_guard
    if guard == "incoming":
        transport._incoming_load_request = Mock(return_value=7)
    elif guard == "loading":
        controller.session.loading_sample_ids.add(0)
    else:
        transport._pad_edit_guard = Mock(side_effect=ValueError("Original assignment is held."))
    audio_engine_mock.reset_mock()
    transport.global_params.poll_bpm_lock_application()
    application = transport.global_params._bpm_application
    assert application is not None
    assert application.original_content_id is None
    audio_engine_mock.set_bpm_lock.assert_not_called()
    audio_engine_mock.current_input_runtime_pad_binding.assert_not_called()
    transport._incoming_load_request = original_incoming
    transport._pad_edit_guard = original_guard
    controller.session.loading_sample_ids.discard(0)
    transport.global_params.poll_bpm_lock_application()
    assert application.original_content_id == "c" * 32
    audio_engine_mock.set_bpm_lock.assert_called_once()


@pytest.mark.parametrize(
    ("guard", "activation"),
    [
        ("incoming", "bpm"),
        ("loading", "bpm"),
        ("hold", "reactivate"),
        ("incoming", "restore"),
    ],
)
def test_initial_activation_never_binds_predecessor_while_assignment_is_pending(
    controller: AppController, audio_engine_mock: Mock, guard: str, activation: str
) -> None:
    _configured(controller, audio_engine_mock)
    transport = controller.transport
    parameters = transport.global_params
    original_incoming = transport._incoming_load_request
    original_guard = transport._pad_edit_guard
    if guard == "incoming":
        transport._incoming_load_request = Mock(return_value=7)
    elif guard == "loading":
        controller.session.loading_sample_ids.add(0)
    else:
        transport._pad_edit_guard = Mock(side_effect=ValueError("Original assignment is held."))
    if activation == "bpm":
        parameters.set_bpm_lock(enabled=True)
    elif activation == "reactivate":
        controller.project.bpm_lock = True
        controller.project.multi_loop = False
        parameters.set_multi_loop(enabled=True)
    else:
        controller.project.bpm_lock = True
        transport.apply_project_state_to_audio(restored_sample_ids=frozenset())
    application = parameters._bpm_application
    assert application is not None
    assert application.original_content_id is None
    assert parameters._bpm_lock_application_status().pending
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert controller.project.bpm_lock is True
    audio_engine_mock.set_bpm_lock.assert_not_called()
    controller.project.selected_pad = 5
    controller.project.pad_content[0] = PadContentIdentity(instance_id="d" * 32)
    controller.project.sample_paths[0] = "samples/completed-replacement.wav"
    replacement_binding = FakeInputRuntimePadBinding(0, authority_revision=2)
    replacement_binding._metadata.update(source_id="replacement", source_generation=2)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = replacement_binding
    transport._incoming_load_request = original_incoming
    transport._pad_edit_guard = original_guard
    controller.session.loading_sample_ids.discard(0)
    for _ in range(4):
        parameters.poll_bpm_lock_application()
    assert application.original_content_id == "d" * 32
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert controller.project.selected_pad == 5
    audio_engine_mock.set_bpm_lock.assert_called_once()
    assert audio_engine_mock.set_bpm_lock.call_args.kwargs["anchor_binding"] is replacement_binding


def test_initial_bpm_setter_keeps_existing_hold_guard_before_parent_mutation(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _configured(controller, audio_engine_mock)
    controller.transport.bind_pad_edit_guard(
        Mock(side_effect=ValueError("Original assignment is held."))
    )
    before = controller.project.model_dump()
    application = controller.transport.global_params._bpm_application
    assert application is not None
    application_before = asdict(application)
    with pytest.raises(ValueError, match="held"):
        controller.transport.global_params.set_bpm_lock(enabled=True)
    assert controller.project.model_dump() == before
    assert controller.transport.global_params._bpm_application is application
    assert asdict(application) == application_before
    audio_engine_mock.set_bpm_lock.assert_not_called()


@pytest.mark.parametrize("failure", ["timing_pending", "runtime_refused"])
def test_first_completed_content_stays_bound_through_refusal_unload_and_replacement(
    controller: AppController, audio_engine_mock: Mock, failure: str
) -> None:
    _empty_bpm_reference(controller, audio_engine_mock)
    _complete_reference_content(controller, bpm=None if failure == "timing_pending" else 120.0)
    transport = controller.transport
    original_refresh = transport._refresh_input_runtime
    if failure == "runtime_refused":
        transport._refresh_input_runtime = Mock(return_value=False)
    transport.global_params.poll_bpm_lock_application()
    application = transport.global_params._bpm_application
    assert application is not None
    assert application.original_content_id == "c" * 32
    assert application.attempted is False
    if failure == "runtime_refused":
        assert transport.global_params._bpm_lock_application_status().detail == (
            "Audio control state is not ready for this playback choice."
        )
    audio_engine_mock.set_bpm_lock.assert_not_called()
    transport._refresh_input_runtime = original_refresh
    controller.project.pad_content[0] = None
    controller.project.sample_paths[0] = None
    transport.global_params.poll_bpm_lock_application()
    controller.project.pad_content[0] = PadContentIdentity(instance_id="d" * 32)
    controller.project.sample_paths[0] = "samples/replacement.wav"
    controller.project.manual_bpm[0] = 120.0
    transport.global_params.poll_bpm_lock_application()
    assert application.original_content_id == "c" * 32
    audio_engine_mock.set_bpm_lock.assert_not_called()
    assert transport.global_params._bpm_lock_application_status().pending
    controller.project.selected_pad = 0
    transport.global_params.set_bpm_lock(enabled=False)
    transport.global_params.set_bpm_lock(enabled=True)
    fresh = transport.global_params._bpm_application
    assert fresh is not None
    assert fresh.original_content_id == "d" * 32
    assert audio_engine_mock.set_bpm_lock.call_count == 2
    assert audio_engine_mock.set_bpm_lock.call_args.kwargs["enabled"] is True
    assert "anchor_binding" in audio_engine_mock.set_bpm_lock.call_args.kwargs


@pytest.mark.parametrize("state", ["attempted", "admitted", "unobserved", "disabled", "parent"])
def test_first_content_binding_cannot_bypass_an_existing_submission_or_disabled_parent(
    controller: AppController, audio_engine_mock: Mock, state: str
) -> None:
    _empty_bpm_reference(controller, audio_engine_mock)
    _complete_reference_content(controller, bpm=120.0)
    parameters = controller.transport.global_params
    application = parameters._bpm_application
    assert application is not None
    if state == "attempted":
        application.attempted = True
    elif state == "admitted":
        application.admitted_revision = 1
    elif state == "unobserved":
        application.unobserved_after_revision = 0
    elif state == "disabled":
        application.enabled = False
    else:
        controller.project.multi_loop = False
    assert not parameters._bpm_anchor_waiting_for_first_content(0)
    parameters._bind_first_bpm_content()
    assert application.original_content_id is None
    assert parameters._qualified_bpm_anchor() is None
    audio_engine_mock.set_bpm_lock.assert_not_called()


def test_same_observation_after_marker_busy_retries_once_per_fresh_available_poll(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipt = _configured(controller, audio_engine_mock)
    submit = audio_engine_mock.set_bpm_lock.side_effect
    audio_engine_mock.set_bpm_lock.side_effect = RuntimeError("marker busy")
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    assert audio_engine_mock.set_bpm_lock.call_count == 1
    receipt["submission_available"] = False
    parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_bpm_lock.call_count == 1
    # No receipt/value change accompanies a failed producer CAS.
    receipt["submission_available"] = True
    audio_engine_mock.set_bpm_lock.side_effect = submit
    parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_bpm_lock.call_count == 2
    parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_bpm_lock.call_count == 2


@pytest.mark.parametrize("enabled", [False, True])
def test_hierarchy_reference_change_requalifies_same_source_once_per_native_receipt(
    controller: AppController, audio_engine_mock: Mock, *, enabled: bool
) -> None:
    receipt = _configured(controller, audio_engine_mock)
    controller.project.bpm_lock = not enabled
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=enabled)
    assert audio_engine_mock.set_bpm_lock.call_count == 1
    receipt.update(state="pending", effect_revision=1, reason="reference_changed")
    parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_bpm_lock.call_count == 2
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_bpm_lock.call_count == 2


@pytest.mark.parametrize("replacement", [None, PadContentIdentity(instance_id="b" * 32)])
def test_pending_anchor_replacement_cannot_inherit_original_qualification(
    controller: AppController, audio_engine_mock: Mock, replacement: PadContentIdentity | None
) -> None:
    _configured(controller, audio_engine_mock)
    controller.project.manual_bpm[0] = None
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    controller.project.pad_content[0] = replacement
    controller.project.manual_bpm[0] = 120.0
    parameters.poll_bpm_lock_application()
    audio_engine_mock.set_bpm_lock.assert_not_called()
    assert parameters._bpm_lock_application_status().pending


def test_pending_anchor_uses_existing_mirrored_placement_of_same_content(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _configured(controller, audio_engine_mock)
    controller.project.manual_bpm[0] = None
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    project = controller.project
    project.sample_paths[215], project.sample_paths[0] = project.sample_paths[0], None
    project.pad_content[215], project.pad_content[0] = project.pad_content[0], None
    project.manual_bpm[215] = 120.0
    controller.session.bpm_lock_anchor_pad_id = 215
    parameters.poll_bpm_lock_application()
    audio_engine_mock.set_bpm_lock.assert_called_once()
    binding = audio_engine_mock.set_bpm_lock.call_args.kwargs["anchor_binding"]
    assert binding.metadata()["pad_id"] == 215


def test_missing_qualified_api_stays_pending_without_unqualified_fallback(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _configured(controller, audio_engine_mock)
    audio_engine_mock.set_bpm_lock.side_effect = TypeError("anchor_binding unsupported")
    controller.transport.global_params.set_bpm_lock(enabled=True)
    assert audio_engine_mock.set_bpm_lock.call_count == 1
    assert "anchor_binding" in audio_engine_mock.set_bpm_lock.call_args.kwargs
    assert controller.transport.global_params._bpm_lock_application_status().pending


def test_successful_admission_with_temporarily_unreadable_receipt_is_never_resent(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipt = _configured(controller, audio_engine_mock)
    readings = 0

    def observe() -> dict[str, object] | None:
        nonlocal readings
        readings += 1
        return None if readings == 2 else receipt.copy()

    audio_engine_mock.bpm_lock_status.side_effect = observe
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    assert audio_engine_mock.set_bpm_lock.call_count == 1
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_bpm_lock.call_count == 1


def test_full_parent_off_projection_retains_raw_map_wish_and_masks_every_pitch_row(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _configured(controller, audio_engine_mock)
    project = controller.project
    project.bpm_lock = True
    project.multi_loop = False
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=4)
    maps = controller.loader.source_beatmaps
    maps.playback_delivery = Mock(side_effect=AssertionError("inactive map custody"))
    assert controller.input_mapping._refresh_rust_runtime_state()
    kwargs = audio_engine_mock.set_input_runtime_state.call_args.kwargs
    assert kwargs["source_map_rows"][0] == {
        "sync_requested": True,
        "intent_revision": 4,
        "delivery": None,
        "exact_quarters": None,
    }
    assert all(row["sync"] is False for row in kwargs["pitch_rows"])
    project.multi_loop = True
    assert all(
        controller.input_mapping._input_pitch_projection(sample_id).sync is True
        for sample_id in range(len(project.sample_paths))
    )


def test_applied_same_content_timing_and_position_keep_bool_receipt_and_qualified_master_owner(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipt = _configured(controller, audio_engine_mock)
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    receipt.update(state="applied", effect_revision=1, effective_enabled=True, execution_frame=64)
    parameters.poll_bpm_lock_application()
    assert parameters._bpm_lock_master_ready()
    binding = audio_engine_mock.current_input_runtime_pad_binding(0)
    metadata = binding.metadata()
    project = controller.project
    project.sample_paths[215], project.sample_paths[0] = project.sample_paths[0], None
    project.pad_content[215], project.pad_content[0] = project.pad_content[0], None
    project.manual_bpm[215] = 120.0
    controller.session.bpm_lock_anchor_pad_id = 215
    moved = FakeInputRuntimePadBinding(215, authority_revision=99)
    moved._metadata.update(metadata, pad_id=215, authority_revision=99)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = moved
    assert parameters._bpm_lock_master_ready()
    parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_bpm_lock.call_count == 1
    project.manual_bpm[215] = 130.0
    assert parameters._bpm_lock_master_ready()
    parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_bpm_lock.call_count == 1
    parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_bpm_lock.call_count == 1


def test_parent_off_scalar_loop_edit_refuses_old_actual_mapped_custody_without_saved_prefix(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _configured(controller, audio_engine_mock)
    controller.project.bpm_lock = True
    controller.project.multi_loop = False
    controller.project.global_timing_policy = _policy(sync=True, realtime=True, revision=4)
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=2)
    _feedback(
        audio_engine_mock,
        active=True,
        effective=True,
        intent_revision=2,
        map_revision="c" * 64,
        policy_mode="mapped",
        policy_revision=3,
        policy_sync=True,
        policy_realtime_bpm=True,
        settled=True,
    )
    before = controller.project.model_dump()
    audio_engine_mock.set_pad_loop_region.reset_mock()
    with pytest.raises(ValueError, match="mapped voice"):
        controller.transport.loop.set_start(0, 0.5)
    assert controller.project.model_dump() == before
    audio_engine_mock.set_pad_loop_region.assert_not_called()


def test_parent_off_pending_retains_actual_master_until_matching_native_off_effect(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipt = _configured(controller, audio_engine_mock)
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    receipt.update(state="applied", effect_revision=1, effective_enabled=True, execution_frame=64)
    parameters.poll_bpm_lock_application()
    assert controller.session.master_bpm == 120.0
    parameters.set_multi_loop(enabled=False)
    assert not controller.project.resolve_bpm_lock()
    assert parameters._bpm_lock_application_status().effective is True
    assert parameters._bpm_lock_application_status().pending
    controller.transport.bpm.recompute_master_bpm()
    parameters._poll_global_rate_status()
    assert controller.session.master_bpm == 120.0
    assert controller.session.master_period_seconds == 0.5
    receipt.update(state="applied", effect_revision=2, effective_enabled=False, execution_frame=128)
    parameters.poll_bpm_lock_application()
    assert controller.session.master_bpm is None
    assert controller.session.master_period_seconds is None


def test_mode_ack_keeps_map_facts_and_actual_realtime_but_never_confirms_phase(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _configured(controller, audio_engine_mock)
    controller.project.bpm_lock = True
    controller.project.global_timing_policy = _policy(sync=True, realtime=False, revision=2)
    _feedback(
        audio_engine_mock,
        active=True,
        effective=True,
        intent_revision=0,
        map_revision=None,
        policy_mode="fixed",
        policy_revision=2,
        policy_sync=True,
        policy_realtime_bpm=False,
        settled=True,
        target_current=True,
    )
    status = controller.transport.source_sync.status(0)
    assert status.effective is True
    assert status.actual_realtime_bpm is False
    assert status.grid_phase_confirmed is None
    assert status.state == "pending"
    assert status.guidance == "phase"
    assert controller.transport.timing_policy.status("sync").effective is True
    assert controller.transport.timing_policy.status("sync").grid_phase_confirmed is None


@pytest.mark.parametrize("route", ["direct", "gui", "midi"])
def test_unavailable_parent_guards_agree_for_direct_gui_and_midi(
    controller: AppController, audio_engine_mock: Mock, route: str
) -> None:
    _configured(controller, audio_engine_mock)
    controller.project.multi_loop = False
    before = controller.project.model_dump()
    if route == "direct":
        with pytest.raises(ValueError, match="parents"):
            controller.transport.timing_policy.set_enabled("sync", enabled=True)
    elif route == "gui":
        UiContext(controller).audio.global_.toggle_source_sync()
    else:
        controller.input_mapping.set_enabled(enabled=True)
        before = controller.project.model_dump()
        controller.input_mapping._handle_rust_input_event({
            "source": "midi",
            "binding_key": "midi:note:1:60",
            "action_key": global_source_sync_toggle_action().key,
        })
    assert controller.project.model_dump() == before


@pytest.mark.parametrize("change", ["parent", "recovery", "policy", "pad_sync", "map"])
def test_accepted_refresh_signature_tracks_full_compact_request_context(
    controller: AppController, audio_engine_mock: Mock, change: str
) -> None:
    _configured(controller, audio_engine_mock)
    controller.project.bpm_lock = True
    owner = controller.accepted_timing
    before = owner._control_signature(0)
    if change == "parent":
        controller.project.multi_loop = False
    elif change == "recovery":
        controller.project.quarantine_timing_policy()
    elif change == "policy":
        controller.project.global_timing_policy = _policy(sync=True, realtime=False, revision=3)
    elif change == "pad_sync":
        controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(
            requested=False, revision=2
        )
    else:
        controller.project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, None)
    after = owner._control_signature(0)
    assert len(before) == len(after) == ACCEPTED_REFRESH_SIGNATURE_LENGTH
    assert before != after
    assert before[-1] == after[-1] == controller.session.bpm_lock_anchor_pad_id


@pytest.mark.parametrize("invalid_policy", [None, {}, {"sync": True}])
def test_malformed_global_policy_quarantines_both_on_parents_before_restore_and_preserves_bytes(
    tmp_path: Path, invalid_policy: object
) -> None:
    project = ProjectState(multi_loop=True, bpm_lock=True)
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=4)
    data = project.model_dump(mode="json")
    data["global_timing_policy"] = invalid_policy
    original = json.dumps(data).encode()
    path = tmp_path / "owned-policy-recovery.json"
    path.write_bytes(original)
    persistence = ProjectPersistence.from_config_path(path)
    persistence.config_path = path
    recovered = persistence.project
    assert recovered.multi_loop
    assert recovered.bpm_lock
    assert recovered.timing_policy_recovery
    assert not recovered.resolve_bpm_lock()
    assert not recovered.resolve_pad_realtime_bpm(0)
    recovered.global_timing_policy = _policy(sync=True, realtime=True, revision=9)
    recovered.multi_loop = False
    recovered.multi_loop = True
    assert not recovered.resolve_bpm_lock()
    assert "timing_policy_recovery" not in recovered.model_dump_json()
    persistence.mark_dirty()
    with pytest.raises(PersistenceFenceError):
        persistence.flush()
    assert path.read_bytes() == original


@pytest.mark.parametrize("kind", ["historical", "valid_false_false_zero", "unrelated_error"])
def test_valid_or_unrelated_metadata_never_fabricates_recovery_sentinel(
    tmp_path: Path, kind: str
) -> None:
    project = ProjectState(multi_loop=True, bpm_lock=True)
    data = project.model_dump(mode="json")
    if kind == "historical":
        data.pop("global_timing_policy")
    elif kind == "valid_false_false_zero":
        data["global_timing_policy"] = _policy(sync=False, realtime=False).model_dump()
    else:
        data["global_timing_policy"] = _policy(sync=True, realtime=True).model_dump()
        data["config_revision"] = -1
    path = tmp_path / "owned-valid-policy.json"
    path.write_text(json.dumps(data))
    recovered = ProjectPersistence.from_config_path(path).project
    assert not recovered.timing_policy_recovery
    assert recovered.resolve_bpm_lock()
