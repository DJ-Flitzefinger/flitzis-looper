"""Accepted primary BPM intent survives separately refused derived publications."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import BeatGrid, PadContentIdentity, SampleAnalysis
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding
from tests.flitzis_looper.controller.bpm_lock_fixture import (
    _bind_fixture_source_identity,
    _master_provenance,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller import AppController


REFUSAL = "control queue is full"


@dataclass
class _BpmPublication:
    """Local control receipts; effects advance only through explicit test evidence."""

    controller: AppController
    receipt: dict[str, object]
    rate: dict[str, object]
    submitted_provenance: dict[str, object] | None = None

    def submit(self, *, enabled: bool, anchor_binding: object = None) -> None:
        if enabled:
            assert isinstance(anchor_binding, FakeInputRuntimePadBinding)
            metadata = anchor_binding.metadata()
            accepted = metadata["accepted_timing"]
            self.submitted_provenance = _master_provenance(
                cast("str", metadata["source_identity"]),
                cast("str", accepted["revision"]) if isinstance(accepted, dict) else None,
            )
        self.receipt.update(
            request_revision=cast("int", self.receipt["request_revision"]) + 1,
            state="queued",
            requested_enabled=enabled,
            reason=None,
            execution_frame=None,
        )

    def apply(self, *, enabled: bool, period: float | None) -> None:
        """Supply this fixture's own matching effect, then use the normal observer."""
        assert self.receipt["requested_enabled"] is enabled
        revision = self.receipt["request_revision"]
        application = self.controller.transport.global_params._bpm_application
        assert application is not None
        assert application.enabled is enabled
        assert application.admitted_revision == revision
        self.receipt.update(
            effect_revision=revision,
            state="applied",
            effective_enabled=enabled,
            execution_frame=64,
        )
        self.rate.update(
            request_revision=revision,
            effect_revision=revision,
            state="applied",
            requested_master_period=period,
            effective_master_period=period,
            execution_frame=64,
            master_provenance=self.submitted_provenance.copy()
            if enabled and self.submitted_provenance is not None
            else _master_provenance(None, kind="absent"),
        )
        self.controller.transport.global_params.poll_bpm_lock_application()


def _bpm_publication(controller: AppController, audio: Mock, *, pad_id: int = 0) -> _BpmPublication:
    """Configure one original source and queued boundary without inventing an ACK."""
    project = controller.project
    assert project.sample_paths[pad_id] is not None
    project.multi_loop = True
    project.selected_pad = pad_id
    project.pad_content[pad_id] = project.pad_content[pad_id] or PadContentIdentity(
        instance_id="a" * 32
    )
    _bind_fixture_source_identity(audio, pad_id=pad_id)
    publication = _BpmPublication(
        controller,
        {
            "request_revision": 0,
            "effect_revision": 0,
            "state": "applied",
            "requested_enabled": False,
            "effective_enabled": False,
            "reason": None,
            "execution_frame": None,
            "submission_available": True,
        },
        {
            "request_revision": 0,
            "effect_revision": 0,
            "state": "applied",
            "requested_speed": project.speed,
            "requested_master_period": None,
            "effective_speed": project.speed,
            "effective_master_period": None,
            "execution_frame": 0,
            "error": None,
            "master_provenance": _master_provenance(None, kind="absent"),
        },
    )
    audio.bpm_lock_status = Mock(side_effect=publication.receipt.copy)
    audio.global_rate_status = Mock(side_effect=publication.rate.copy)
    audio.set_bpm_lock = Mock(side_effect=publication.submit)
    audio.output_sample_rate.return_value = 48_000
    return publication


def _loaded(controller: AppController, audio: Mock) -> dict[str, str]:
    project = controller.project
    project.selected_pad = 0
    project.sample_paths[0] = "samples/synthetic-bpm.wav"
    project.sample_durations[0] = 20.0
    project.sample_analysis[0] = SampleAnalysis(
        bpm=100.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0, 0.6], downbeats=[0.0], bars=[0.0]),
    )
    project.pad_loop_auto[0] = False
    project.pad_loop_start_s[0] = 2.0
    project.pad_loop_end_s[0] = 5.0
    native = {"intent": "automatic"}
    audio.pad_timing_intent.side_effect = lambda _sample_id: native["intent"]
    audio.current_constant_timing.return_value = None
    audio.export_current_constant_timing.return_value = None
    audio.current_input_runtime_pad_binding.side_effect = lambda sample_id: (
        FakeInputRuntimePadBinding(sample_id, intent=native["intent"])
    )
    audio.loaded_residency.return_value = None

    def legacy(*_args: object, **_options: object) -> None:
        native["intent"] = "legacy"

    def declare(_sample_id: int, intent: str, **_options: object) -> None:
        native["intent"] = intent

    audio.set_pad_bpm.side_effect = legacy
    audio.set_pad_timing_metadata.side_effect = legacy
    audio.set_pad_timing_intent.side_effect = declare
    return native


def _bpm_action(controller: AppController, action: str) -> Callable[[], object]:
    return {
        "set": lambda: controller.transport.bpm.set_manual_bpm(0, 120.0),
        "clear": lambda: controller.transport.bpm.clear_manual_bpm(0),
        "tap": lambda: controller.transport.bpm.tap_bpm(0),
    }[action]


def _assert_saved_bpm(controller: AppController, value: float | None, intent: str) -> None:
    assert controller.persistence.flush_if_dirty()
    saved = ProjectPersistence.from_config_path(controller.persistence.config_path).project
    assert saved.manual_bpm[0] == value
    assert saved.pad_timing_intent[0] == intent
    assert saved.pad_loop_start_s[0] == 2.0
    assert saved.pad_loop_end_s[0] == 5.0


def _assert_saved_lock_request(controller: AppController, *, enabled: bool, intent: str) -> None:
    assert controller.persistence.flush_if_dirty()
    saved = ProjectPersistence.from_config_path(controller.persistence.config_path).project
    assert saved.bpm_lock is enabled
    assert saved.pad_timing_intent[0] == intent
    assert saved.manual_bpm[0] == controller.project.manual_bpm[0]
    assert saved.pad_loop_start_s[0] == 2.0
    assert saved.pad_loop_end_s[0] == 5.0


def _assert_no_legacy_lock_effects(audio: Mock) -> None:
    audio.set_pad_bpm.assert_not_called()
    audio.set_pad_timing_metadata.assert_not_called()
    audio.set_master_bpm.assert_not_called()
    audio.set_master_period.assert_not_called()


def _assert_pending_lock_publication(controller: AppController, boundary: str) -> None:
    status = controller.transport.global_params._bpm_lock_application_status()
    assert status.pending
    if boundary == "set_bpm_lock":
        assert status.detail == REFUSAL
    else:
        assert status.detail == "Audio control state is not ready for this playback choice."
        assert controller.input_mapping._last_input_runtime_state_error == REFUSAL


def _retry_original_lock_publication(
    publication: _BpmPublication, audio: Mock, *, boundary: str, target: bool
) -> None:
    controller = publication.controller
    parameters = controller.transport.global_params
    application = parameters._bpm_application
    original_content = controller.project.pad_content[0]
    if boundary == "set_bpm_lock":
        publication.receipt["submission_available"] = False
        parameters.poll_bpm_lock_application()
        audio.set_bpm_lock.assert_called_once()
    publication.receipt["submission_available"] = True
    controller.project.selected_pad = 5
    parameters.poll_bpm_lock_application()
    assert parameters._bpm_application is application
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert controller.project.pad_content[0] is original_content
    assert audio.set_bpm_lock.call_count == (2 if boundary == "set_bpm_lock" else 1)
    if target:
        binding = audio.set_bpm_lock.call_args.kwargs["anchor_binding"]
        assert binding.metadata()["pad_id"] == 0
        assert application is not None
        assert original_content is not None
        assert application.original_content_id == original_content.instance_id
    count = audio.set_bpm_lock.call_count
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    assert audio.set_bpm_lock.call_count == count


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
@pytest.mark.parametrize(
    ("action", "boundary"),
    [
        ("set", "set_pad_bpm"),
        ("clear", "set_pad_bpm"),
        ("tap", "set_pad_bpm"),
        ("set", "set_pad_timing_metadata"),
        ("clear", "set_pad_timing_metadata"),
        ("tap", "set_pad_timing_metadata"),
        ("set", "set_pad_timing_intent"),
        ("tap", "set_pad_timing_intent"),
        ("set", "set_pad_loop_region"),
        ("clear", "set_pad_loop_region"),
        ("tap", "set_pad_loop_region"),
    ],
)
def test_bpm_primary_and_derived_rejection_preserve_only_actually_admitted_intent(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    error_type: type[Exception],
    action: str,
    boundary: str,
) -> None:
    native = _loaded(controller, audio_engine_mock)
    controller.project.manual_bpm[0] = 90.0 if action == "clear" else None
    native["intent"] = "manual" if action == "clear" else "automatic"
    controller.project.pad_timing_intent[0] = "manual" if action == "clear" else "automatic"
    if action == "tap":
        controller.session.tap_bpm_pad_id = 0
        controller.session.tap_bpm_timestamps = [1.0]
        monkeypatch.setattr("flitzis_looper.controller.transport.bpm.monotonic", lambda: 1.5)
    controller.persistence.flush()
    before = controller.project.model_dump()
    revision = controller.persistence.revision
    audio_engine_mock.reset_mock()
    refusal = getattr(audio_engine_mock, boundary)
    refusal.side_effect = error_type(REFUSAL)

    with pytest.raises(error_type, match=REFUSAL):
        _bpm_action(controller, action)()

    refusal.assert_called_once()
    audio_engine_mock.set_pad_bpm.assert_called_once_with(
        0, 100.0 if action == "clear" else 120.0, expected_source_generation=1
    )
    if boundary == "set_pad_bpm":
        assert controller.project.model_dump() == before
        assert controller.persistence.revision == revision
        assert not controller.persistence.flush_if_dirty()
    else:
        value = None if action == "clear" else 120.0
        intent = "tap" if action == "tap" else "manual"
        if boundary != "set_pad_loop_region" or action == "clear":
            intent = "legacy"
        assert native["intent"] == intent
        assert controller.project.manual_bpm[0] == value
        assert controller.project.pad_timing_intent[0] == intent
        assert controller.persistence.revision > revision
        _assert_saved_bpm(controller, value, intent)
    controller.transport.on_frame_render()
    refusal.assert_called_once()
    audio_engine_mock.set_pad_bpm.assert_called_once()


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
@pytest.mark.parametrize(
    ("initial", "boundary"),
    [
        ("off", "set_input_runtime_state"),
        ("on", "set_input_runtime_state"),
        ("off", "set_bpm_lock"),
        ("on", "set_bpm_lock"),
    ],
)
def test_bpm_lock_publication_refusal_retains_requested_wish_and_original_retry_owner(
    controller: AppController,
    audio_engine_mock: Mock,
    error_type: type[Exception],
    initial: str,
    boundary: str,
) -> None:
    native = _loaded(controller, audio_engine_mock)
    native["intent"] = "legacy"
    controller.project.pad_timing_intent[0] = "legacy"
    publication = _bpm_publication(controller, audio_engine_mock)
    parameters = controller.transport.global_params
    if initial == "on":
        parameters.set_bpm_lock(enabled=True)
        publication.apply(enabled=True, period=0.6)
    target = initial == "off"
    previous_period = controller.session.master_period_seconds
    controller.persistence.flush()
    original_content = controller.project.pad_content[0]
    revision = controller.persistence.revision
    audio_engine_mock.reset_mock()
    refusal = getattr(audio_engine_mock, boundary)
    accepted = refusal.side_effect
    refusal.side_effect = error_type(REFUSAL)

    parameters.set_bpm_lock(enabled=target)

    refusal.assert_called_once()
    application = parameters._bpm_application
    assert application is not None
    assert controller.project.bpm_lock is target
    assert controller.session.master_period_seconds == previous_period
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert controller.project.pad_content[0] is original_content
    _assert_pending_lock_publication(controller, boundary)
    assert controller.persistence.revision > revision
    _assert_saved_lock_request(controller, enabled=target, intent="legacy")
    parameters.set_bpm_lock(enabled=target)
    refusal.assert_called_once()
    refusal.side_effect = accepted
    _retry_original_lock_publication(
        publication, audio_engine_mock, boundary=boundary, target=target
    )
    assert parameters._bpm_application is application
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert controller.session.master_period_seconds == previous_period
    publication.apply(enabled=target, period=0.6 if target else None)
    assert controller.session.bpm_lock_anchor_pad_id == (0 if target else None)
    assert controller.session.master_period_seconds == (0.6 if target else None)
    assert not parameters._bpm_lock_application_status().pending
    _assert_no_legacy_lock_effects(audio_engine_mock)


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
@pytest.mark.parametrize("boundary", ["bpm_lock_status", "global_rate_status"])
def test_bpm_lock_receipt_refusal_keeps_requested_intent_until_own_actual_confirmation(
    controller: AppController,
    audio_engine_mock: Mock,
    error_type: type[Exception],
    boundary: str,
) -> None:
    native = _loaded(controller, audio_engine_mock)
    native["intent"] = "legacy"
    controller.project.pad_timing_intent[0] = "legacy"
    publication = _bpm_publication(controller, audio_engine_mock)
    controller.persistence.flush()
    audio_engine_mock.reset_mock()
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    audio_engine_mock.set_bpm_lock.assert_called_once()
    binding = audio_engine_mock.set_bpm_lock.call_args.kwargs["anchor_binding"]
    assert binding.metadata()["pad_id"] == 0
    assert controller.session.master_period_seconds is None
    refusal = getattr(audio_engine_mock, boundary)
    accepted = refusal.side_effect
    refusal.side_effect = error_type(REFUSAL)
    if boundary == "global_rate_status":
        publication.receipt.update(
            state="applied", effect_revision=1, effective_enabled=True, execution_frame=64
        )
    parameters.poll_bpm_lock_application()
    assert parameters._bpm_lock_application_status().pending
    assert parameters._bpm_lock_application_status().detail is not None
    if boundary == "global_rate_status":
        assert parameters._rate_status_error == REFUSAL
    else:
        assert parameters._bpm_lock_application_status().detail == REFUSAL
    assert controller.session.master_period_seconds is None
    _assert_saved_lock_request(controller, enabled=True, intent="legacy")
    refusal.side_effect = accepted
    foreign = dict(publication.receipt)
    foreign.update(
        request_revision=2,
        effect_revision=2,
        state="applied",
        effective_enabled=True,
        execution_frame=64,
    )
    audio_engine_mock.bpm_lock_status.side_effect = foreign.copy
    parameters.poll_bpm_lock_application()
    assert controller.session.master_period_seconds is None
    audio_engine_mock.bpm_lock_status.side_effect = publication.receipt.copy
    publication.apply(enabled=True, period=0.6)
    assert controller.session.master_period_seconds == 0.6
    assert not parameters._bpm_lock_application_status().pending
    audio_engine_mock.set_bpm_lock.assert_called_once()
    _assert_no_legacy_lock_effects(audio_engine_mock)


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
@pytest.mark.parametrize("intent", ["manual", "tap"])
@pytest.mark.parametrize("boundary", ["set_input_runtime_state", "set_bpm_lock"])
def test_lock_publication_refusal_preserves_original_manual_or_tap_authority(
    controller: AppController,
    audio_engine_mock: Mock,
    error_type: type[Exception],
    intent: Literal["manual", "tap"],
    boundary: str,
) -> None:
    native = _loaded(controller, audio_engine_mock)
    native["intent"] = intent
    controller.project.manual_bpm[0] = 120.0
    controller.project.pad_timing_intent[0] = intent
    publication = _bpm_publication(controller, audio_engine_mock)
    controller.persistence.flush()
    audio_engine_mock.reset_mock()
    refusal = getattr(audio_engine_mock, boundary)
    accepted = refusal.side_effect
    refusal.side_effect = error_type(REFUSAL)
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)

    refusal.assert_called_once()
    assert controller.project.bpm_lock
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert controller.session.master_period_seconds is None
    assert controller.project.manual_bpm[0] == 120.0
    assert native["intent"] == intent
    assert controller.project.pad_timing_intent[0] == intent
    _assert_saved_lock_request(controller, enabled=True, intent=intent)
    refusal.side_effect = accepted
    parameters.poll_bpm_lock_application()
    count = audio_engine_mock.set_bpm_lock.call_count
    for _ in range(3):
        parameters.poll_bpm_lock_application()
    assert audio_engine_mock.set_bpm_lock.call_count == count
    assert controller.session.master_period_seconds is None
    publication.apply(enabled=True, period=0.5)
    assert controller.session.master_period_seconds == 0.5
    assert native["intent"] == intent
    assert controller.project.pad_timing_intent[0] == intent
    _assert_no_legacy_lock_effects(audio_engine_mock)


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
@pytest.mark.parametrize("intent", ["manual", "tap"])
@pytest.mark.parametrize("boundary", ["set_pad_bpm", "set_pad_timing_metadata"])
def test_shared_bpm_refresh_mirrors_admitted_primary_before_later_origin_refusal(
    controller: AppController,
    audio_engine_mock: Mock,
    error_type: type[Exception],
    intent: Literal["manual", "tap"],
    boundary: str,
) -> None:
    native = _loaded(controller, audio_engine_mock)
    native["intent"] = intent
    controller.project.manual_bpm[0] = 120.0
    controller.project.pad_timing_intent[0] = intent
    controller.persistence.flush()
    before = controller.project.model_dump()
    audio_engine_mock.reset_mock()
    getattr(audio_engine_mock, boundary).side_effect = error_type(REFUSAL)

    with pytest.raises(error_type, match=REFUSAL):
        controller.transport.bpm.on_pad_bpm_changed(0)

    if boundary == "set_pad_bpm":
        assert native["intent"] == intent
        assert controller.project.model_dump() == before
        assert not controller.persistence.flush_if_dirty()
        audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    else:
        assert native["intent"] == "legacy"
        assert controller.project.manual_bpm[0] == 120.0
        assert controller.project.pad_timing_intent[0] == "legacy"
        _assert_saved_bpm(controller, 120.0, "legacy")
    audio_engine_mock.set_pad_timing_intent.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()


@pytest.mark.parametrize("intent", ["manual", "tap"])
def test_successful_shared_refresh_redeclares_original_requested_authority_after_origin(
    controller: AppController, audio_engine_mock: Mock, intent: Literal["manual", "tap"]
) -> None:
    native = _loaded(controller, audio_engine_mock)
    native["intent"] = intent
    controller.project.manual_bpm[0] = 120.0
    controller.project.pad_timing_intent[0] = intent
    controller.persistence.flush()
    audio_engine_mock.reset_mock()

    controller.transport.bpm.on_pad_bpm_changed(0)

    audio_engine_mock.set_pad_bpm.assert_called_once_with(0, 120.0)
    audio_engine_mock.set_pad_timing_metadata.assert_called_once()
    audio_engine_mock.set_pad_timing_intent.assert_called_once_with(0, intent)
    audio_engine_mock.set_pad_loop_region.assert_called_once()
    assert native["intent"] == intent
    assert controller.project.pad_timing_intent[0] == intent
    assert controller.project.manual_bpm[0] == 120.0
