"""Accepted primary BPM intent survives separately refused derived publications."""

from typing import TYPE_CHECKING, Literal

import pytest

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import BeatGrid, SampleAnalysis
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding

if TYPE_CHECKING:
    from collections.abc import Callable
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


REFUSAL = "control queue is full"


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
    [("off", "set_pad_timing_metadata"), ("off", "set_bpm_lock"), ("on", "set_bpm_lock")],
)
def test_bpm_lock_rejected_primary_preserves_flag_anchor_and_identical_explicit_retry(
    controller: AppController,
    audio_engine_mock: Mock,
    error_type: type[Exception],
    initial: str,
    boundary: str,
) -> None:
    native = _loaded(controller, audio_engine_mock)
    native["intent"] = "legacy"
    controller.project.pad_timing_intent[0] = "legacy"
    controller.project.bpm_lock = initial == "on"
    if initial == "on":
        controller.session.bpm_lock_anchor_pad_id = 0
        controller.session.bpm_lock_anchor_bpm = 100.0
        controller.session.master_bpm = 100.0
        controller.session.master_period_seconds = 0.6
    target = initial == "off"
    controller.persistence.flush()
    before_project = controller.project.model_dump()
    before_session = controller.session.model_dump()
    before_config = controller.persistence.config_path.read_bytes()
    revision = controller.persistence.revision
    audio_engine_mock.reset_mock()
    refusal = getattr(audio_engine_mock, boundary)
    accepted = refusal.side_effect
    refusal.side_effect = error_type(REFUSAL)

    with pytest.raises(error_type, match=REFUSAL):
        controller.transport.global_params.set_bpm_lock(enabled=target)

    assert controller.project.model_dump() == before_project
    assert controller.session.model_dump() == before_session
    assert controller.persistence.revision == revision
    assert not controller.persistence.flush_if_dirty()
    assert controller.persistence.config_path.read_bytes() == before_config
    refusal.side_effect = accepted

    controller.transport.global_params.set_bpm_lock(enabled=target)

    assert refusal.call_count == 2
    assert controller.project.bpm_lock is target
    assert controller.persistence.flush_if_dirty()
    saved = ProjectPersistence.from_config_path(controller.persistence.config_path).project
    assert saved.bpm_lock is target
    assert controller.session.bpm_lock_anchor_pad_id == (0 if target else None)
    audio_engine_mock.set_bpm_lock.assert_called_with(enabled=target)


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
@pytest.mark.parametrize("boundary", ["set_master_bpm", "bootstrap_transport_from_pad"])
def test_bpm_lock_keeps_accepted_flag_and_master_partial_state_after_derived_rejection(
    controller: AppController,
    audio_engine_mock: Mock,
    error_type: type[Exception],
    boundary: str,
) -> None:
    native = _loaded(controller, audio_engine_mock)
    native["intent"] = "legacy"
    controller.project.pad_timing_intent[0] = "legacy"
    controller.persistence.flush()
    audio_engine_mock.reset_mock()
    refusal = getattr(audio_engine_mock, boundary)
    refusal.side_effect = error_type(REFUSAL)

    with pytest.raises(error_type, match=REFUSAL):
        controller.transport.global_params.set_bpm_lock(enabled=True)

    audio_engine_mock.set_bpm_lock.assert_called_once_with(enabled=True)
    assert controller.project.bpm_lock
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert controller.session.bpm_lock_anchor_bpm == 100.0
    assert controller.session.master_bpm == (100.0 if boundary.startswith("bootstrap") else None)
    assert controller.persistence.flush_if_dirty()
    saved = ProjectPersistence.from_config_path(controller.persistence.config_path).project
    assert saved.bpm_lock
    controller.transport.on_frame_render()
    refusal.assert_called_once()
    audio_engine_mock.set_bpm_lock.assert_called_once_with(enabled=True)


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
@pytest.mark.parametrize("intent", ["manual", "tap"])
@pytest.mark.parametrize("boundary", ["set_pad_timing_metadata", "set_bpm_lock"])
def test_lock_enable_mirrors_only_actual_grid_origin_authority_before_flag_rejection(
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
        controller.transport.global_params.set_bpm_lock(enabled=True)

    assert not controller.project.bpm_lock
    assert controller.session.bpm_lock_anchor_pad_id is None
    assert controller.project.manual_bpm[0] == 120.0
    if boundary == "set_pad_timing_metadata":
        assert native["intent"] == intent
        assert controller.project.model_dump() == before
        assert not controller.persistence.flush_if_dirty()
        audio_engine_mock.set_bpm_lock.assert_not_called()
    else:
        assert native["intent"] == "legacy"
        assert controller.project.pad_timing_intent[0] == "legacy"
        _assert_saved_bpm(controller, 120.0, "legacy")
    audio_engine_mock.set_master_bpm.assert_not_called()
    audio_engine_mock.bootstrap_transport_from_pad.assert_not_called()


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
