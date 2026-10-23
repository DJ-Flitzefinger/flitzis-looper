"""Parameter gestures contain expected Native refusal without changing controller truth."""

from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.controller.transport.test_bpm_publication_coherence import (
    _assert_no_legacy_lock_effects,
    _assert_saved_lock_request,
    _bpm_publication,
    _loaded,
)

if TYPE_CHECKING:
    from collections.abc import Callable
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


REFUSAL = "control queue is full"
PREVIOUS = "previous gesture feedback"
OTHER_PAD = "independent pad feedback"
PRIMARY = [
    ("volume", "set_volume"),
    ("mute", "set_volume"),
    ("speed", "set_speed"),
    ("effective_bpm", "set_speed"),
    ("nudge", "set_speed"),
    ("reset", "set_speed"),
    ("increase", "set_speed"),
    ("decrease", "set_speed"),
    ("bpm_lock", "set_bpm_lock"),
    ("quantization", "set_trigger_quantization"),
    ("quantization_toggle", "set_trigger_quantization"),
    ("pad.gain", "set_pad_gain"),
    ("pad.eq", "set_pad_eq"),
    ("pad.eq_band", "set_pad_eq"),
    ("pad.bpm", "set_pad_bpm"),
    ("pad.clear_bpm", "set_pad_bpm"),
    ("pad.tap", "set_pad_bpm"),
]


def _context(
    controller: AppController, audio: Mock, monkeypatch: pytest.MonkeyPatch, action: str
) -> UiContext:
    native = _loaded(controller, audio)
    native["intent"] = "legacy"
    controller.project.pad_timing_intent[0] = "legacy"
    controller.project.sample_paths[2] = "samples/synthetic-other.wav"
    controller.project.sample_durations[2] = 30.0
    controller.project.pad_gain_db[2] = -7.0
    if action == "pad.clear_bpm":
        controller.project.manual_bpm[0] = 90.0
        controller.project.pad_timing_intent[0] = "manual"
        native["intent"] = "manual"
    if action == "pad.tap":
        controller.session.tap_bpm_pad_id = 0
        controller.session.tap_bpm_timestamps = [1.0]
        taps = iter([1.5, 2.0])
        monkeypatch.setattr("flitzis_looper.controller.transport.bpm.monotonic", lambda: next(taps))
    if action == "reset":
        controller.project.speed = 1.25
    controller.session.active_sample_ids = {0, 2}
    controller.session.paused_sample_ids = {2}
    controller.session.pad_playhead_s[0] = 4.2
    controller.session.pad_playhead_s[2] = 8.5
    controller.transport.residency.report_launch_error(2, OTHER_PAD)
    controller.persistence.flush()
    audio.poll_loader_events.return_value = None
    audio.reset_mock()
    return UiContext(controller)


def _actions(ctx: UiContext) -> dict[str, Callable[[], object]]:
    global_ = ctx.audio.global_
    pads = ctx.audio.pads
    return {
        "volume": lambda: global_.set_volume(0.6),
        "mute": lambda: global_.set_momentary_output_mute(enabled=True),
        "speed": lambda: global_.set_speed(1.25),
        "effective_bpm": lambda: global_.set_effective_bpm(125.0),
        "nudge": lambda: global_.nudge_speed_by_bpm_steps(2),
        "reset": global_.reset_speed,
        "increase": global_.increase_speed,
        "decrease": global_.decrease_speed,
        "bpm_lock": global_.toggle_bpm_lock,
        "quantization": lambda: global_.set_trigger_quantization("1_16"),
        "quantization_toggle": global_.toggle_trigger_quantization,
        "multi_loop": global_.toggle_multi_loop,
        "key_lock": global_.toggle_key_lock,
        "pad.gain": lambda: pads.set_pad_gain(0, -6.0),
        "pad.eq": lambda: pads.set_pad_eq(0, -3.0, 2.0, 1.0),
        "pad.eq_band": lambda: pads.set_pad_eq_band(0, "mid", 2.0),
        "pad.bpm": lambda: pads.set_manual_bpm(0, 120.0),
        "pad.clear_bpm": lambda: pads.clear_manual_bpm(0),
        "pad.tap": lambda: pads.tap_bpm(0),
    }


def _session(controller: AppController) -> dict[str, object]:
    state = controller.session.model_dump()
    state.pop("tap_bpm_pad_id")
    state.pop("tap_bpm_timestamps")
    return state


def _feedback(ctx: UiContext, action: str) -> str | None:
    if action.startswith("pad."):
        return ctx.audio.pads.residency_status(0)[1]
    return ctx.audio.global_.playback_error


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
@pytest.mark.parametrize(
    ("action", "native_method"), [entry for entry in PRIMARY if entry[0] != "bpm_lock"]
)
def test_parameter_primary_refusal_is_nonfatal_and_only_an_explicit_retry_clears_feedback(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    error_type: type[Exception],
    action: str,
    native_method: str,
) -> None:
    ctx = _context(controller, audio_engine_mock, monkeypatch, action)
    before_project = controller.project.model_dump()
    before_session = _session(controller)
    refusal = getattr(audio_engine_mock, native_method)
    accepted = refusal.side_effect
    refusal.side_effect = error_type(REFUSAL)
    execute = _actions(ctx)[action]

    execute()

    refusal.assert_called_once()
    assert _feedback(ctx, action) == REFUSAL
    assert controller.project.model_dump() == before_project
    assert _session(controller) == before_session
    assert not controller.persistence.flush_if_dirty()
    ctx.audio.poll.poll()
    controller.transport.on_frame_render()
    refusal.assert_called_once()
    assert _feedback(ctx, action) == REFUSAL
    refusal.side_effect = accepted

    execute()

    assert refusal.call_count == 2
    assert _feedback(ctx, action) is None
    assert ctx.audio.pads.residency_status(2) == (None, OTHER_PAD)
    assert controller.session.active_sample_ids == {0, 2}
    assert controller.session.paused_sample_ids == {2}
    audio_engine_mock.capture_pad_start.assert_not_called()
    audio_engine_mock.play_sample.assert_not_called()


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
@pytest.mark.parametrize("action", ["pad.bpm", "pad.clear_bpm", "pad.tap"])
def test_parameter_derived_refusal_keeps_already_accepted_intent_durable(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    error_type: type[Exception],
    action: str,
) -> None:
    ctx = _context(controller, audio_engine_mock, monkeypatch, action)
    refusal = audio_engine_mock.set_pad_timing_metadata
    refusal.side_effect = error_type(REFUSAL)

    _actions(ctx)[action]()

    assert _feedback(ctx, action) == REFUSAL
    assert controller.persistence.flush_if_dirty()
    saved = ProjectPersistence.from_config_path(controller.persistence.config_path).project
    assert saved.manual_bpm[0] == (None if action == "pad.clear_bpm" else 120.0)
    assert saved.pad_timing_intent[0] == "legacy"
    audio_engine_mock.set_pad_bpm.assert_called_once()
    controller.transport.on_frame_render()
    refusal.assert_called_once()
    assert ctx.audio.pads.residency_status(2) == (None, OTHER_PAD)


def _assert_ui_timing_neighbors(ctx: UiContext, controller: AppController, audio: Mock) -> None:
    assert ctx.audio.pads.residency_status(2) == (None, OTHER_PAD)
    assert controller.session.active_sample_ids == {0, 2}
    assert controller.session.paused_sample_ids == {2}
    assert controller.session.pad_playhead_s[0] == 4.2
    assert controller.session.pad_playhead_s[2] == 8.5
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert controller.transport.global_params._bpm_lock_master_ready()
    audio.capture_pad_start.assert_not_called()
    audio.play_sample.assert_not_called()
    _assert_no_legacy_lock_effects(audio)


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
def test_bpm_lock_ui_publication_refusal_keeps_durable_request_and_bounded_original_retry(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    error_type: type[Exception],
) -> None:
    ctx = _context(controller, audio_engine_mock, monkeypatch, "bpm_lock")
    publication = _bpm_publication(controller, audio_engine_mock)
    accepted = audio_engine_mock.set_bpm_lock.side_effect
    audio_engine_mock.set_bpm_lock.side_effect = error_type(REFUSAL)

    ctx.audio.global_.toggle_bpm_lock()

    audio_engine_mock.set_bpm_lock.assert_called_once()
    status = ctx.state.global_.bpm_lock_application_status()
    assert status.requested
    assert status.pending
    assert status.effective is False
    assert status.detail == REFUSAL
    assert ctx.audio.global_.playback_error is None
    assert controller.session.bpm_lock_anchor_pad_id == 0
    assert controller.session.master_period_seconds is None
    _assert_saved_lock_request(controller, enabled=True, intent="legacy")
    publication.receipt["submission_available"] = False
    ctx.audio.poll.poll()
    controller.transport.on_frame_render()
    audio_engine_mock.set_bpm_lock.assert_called_once()
    audio_engine_mock.set_bpm_lock.side_effect = accepted
    publication.receipt["submission_available"] = True
    ctx.audio.poll.poll()
    assert audio_engine_mock.set_bpm_lock.call_count == 2
    for _ in range(3):
        ctx.audio.poll.poll()
    assert audio_engine_mock.set_bpm_lock.call_count == 2
    assert controller.session.master_period_seconds is None
    publication.apply(enabled=True, period=0.6)
    assert controller.session.master_period_seconds == 0.6
    assert not ctx.state.global_.bpm_lock_application_status().pending
    _assert_ui_timing_neighbors(ctx, controller, audio_engine_mock)


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
def test_bpm_lock_ui_receipt_refusal_preserves_wish_and_waits_for_own_applied_effect(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    error_type: type[Exception],
) -> None:
    ctx = _context(controller, audio_engine_mock, monkeypatch, "bpm_lock")
    publication = _bpm_publication(controller, audio_engine_mock)
    ctx.audio.global_.toggle_bpm_lock()
    audio_engine_mock.bpm_lock_status.side_effect = error_type(REFUSAL)

    ctx.audio.poll.poll()

    status = ctx.state.global_.bpm_lock_application_status()
    assert status.requested
    assert status.pending
    assert status.detail == REFUSAL
    assert ctx.audio.global_.playback_error is None
    assert controller.session.master_period_seconds is None
    _assert_saved_lock_request(controller, enabled=True, intent="legacy")
    audio_engine_mock.bpm_lock_status.side_effect = publication.receipt.copy
    foreign = dict(publication.receipt)
    foreign.update(
        request_revision=2,
        effect_revision=2,
        state="applied",
        effective_enabled=True,
        execution_frame=64,
    )
    audio_engine_mock.bpm_lock_status.side_effect = foreign.copy
    ctx.audio.poll.poll()
    assert controller.session.master_period_seconds is None
    audio_engine_mock.bpm_lock_status.side_effect = publication.receipt.copy
    publication.apply(enabled=True, period=0.6)
    assert controller.session.master_period_seconds == 0.6
    assert not ctx.state.global_.bpm_lock_application_status().pending
    audio_engine_mock.set_bpm_lock.assert_called_once()
    _assert_ui_timing_neighbors(ctx, controller, audio_engine_mock)


def _assert_qualified_speed_call(audio: Mock, *, count: int) -> None:
    assert audio.set_speed_and_master_period.call_count == count
    assert audio.set_speed_and_master_period.call_args.args == (1.25, 0.48)
    binding = audio.set_speed_and_master_period.call_args.kwargs["anchor_binding"]
    assert binding.metadata()["pad_id"] == 0
    assert binding.metadata()["source_generation"] == 1
    audio.set_speed.assert_not_called()


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
def test_qualified_ui_speed_refusal_and_later_effect_rejection_preserve_requested_actual_split(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    error_type: type[Exception],
) -> None:
    ctx = _context(controller, audio_engine_mock, monkeypatch, "speed")
    publication = _bpm_publication(controller, audio_engine_mock)
    parameters = controller.transport.global_params
    parameters.set_bpm_lock(enabled=True)
    publication.apply(enabled=True, period=0.6)
    controller.persistence.flush()
    before_project, before_session = controller.project.model_dump(), _session(controller)
    audio_engine_mock.set_speed_and_master_period.side_effect = error_type(REFUSAL)
    ctx.audio.global_.set_speed(1.25)
    assert _feedback(ctx, "speed") == REFUSAL
    assert controller.project.model_dump() == before_project
    assert _session(controller) == before_session
    assert not controller.persistence.flush_if_dirty()
    _assert_qualified_speed_call(audio_engine_mock, count=1)

    def submit(speed: float, period: float, *, anchor_binding: object) -> None:
        assert anchor_binding is not None
        publication.rate.update(
            request_revision=cast("int", publication.rate["request_revision"]) + 1,
            state="queued",
            requested_speed=speed,
            requested_master_period=period,
            execution_frame=None,
            error=None,
        )

    audio_engine_mock.set_speed_and_master_period.side_effect = submit
    ctx.audio.global_.set_speed(1.25)
    assert _feedback(ctx, "speed") is None
    _assert_qualified_speed_call(audio_engine_mock, count=2)
    assert controller.session.master_period_seconds == 0.6
    assert controller.persistence.flush_if_dirty()
    saved = ProjectPersistence.from_config_path(controller.persistence.config_path).project
    assert saved.speed == 1.25
    assert saved.bpm_lock
    publication.rate.update(
        effect_revision=publication.rate["request_revision"],
        state="rejected",
        execution_frame=128,
        error=REFUSAL,
    )
    for _ in range(3):
        ctx.audio.poll.poll()
    status = ctx.state.global_.global_rate_status()
    assert status.state == "rejected"
    assert status.error == REFUSAL
    assert status.requested_speed == 1.25
    assert status.effective_speed == 1.0
    assert controller.session.master_period_seconds == 0.6
    _assert_qualified_speed_call(audio_engine_mock, count=2)
    ctx.audio.global_.set_speed(1.25)
    _assert_qualified_speed_call(audio_engine_mock, count=3)
    publication.rate.update(
        effect_revision=publication.rate["request_revision"],
        state="applied",
        effective_speed=1.25,
        effective_master_period=0.48,
        execution_frame=192,
        error=None,
    )
    ctx.audio.poll.poll()
    assert controller.session.master_period_seconds == 0.48
    assert ctx.state.global_.global_rate_status().state == "applied"
    _assert_ui_timing_neighbors(ctx, controller, audio_engine_mock)


@pytest.mark.parametrize(
    ("action", "learned"),
    [
        ("volume", "global.volume.delta"),
        ("speed", "global.speed.delta"),
        ("reset", "global.speed.reset"),
        ("increase", "global.speed.increase"),
        ("decrease", "global.speed.decrease"),
        ("bpm_lock", "global.bpm_lock.toggle"),
        ("quantization", "global.trigger_quantization:1_16"),
        ("quantization_toggle", "global.trigger_quantization.toggle"),
        ("multi_loop", "global.multi_loop.toggle"),
        ("key_lock", "global.key_lock.toggle"),
        ("pad.gain", "pad.gain.delta:0"),
        ("pad.eq_band", "pad.eq.selected.delta:mid"),
        ("pad.tap", "pad.tap_bpm.selected"),
    ],
)
def test_learn_consumes_original_parameter_action_without_execution_or_feedback_clear(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    action: str,
    learned: str,
) -> None:
    ctx = _context(controller, audio_engine_mock, monkeypatch, action)
    ctx.audio.global_.playback_error = PREVIOUS
    controller.transport.residency.report_launch_error(0, PREVIOUS)
    controller.session.input_learn_active = True
    controller.session.input_learn_pending_source = "midi"
    controller.session.input_learn_pending_binding_key = "midi:cc:1:7"
    before = controller.project.model_dump()

    _actions(ctx)[action]()

    assert controller.input_mapping._midi.mappings[0].action.key == learned
    assert controller.project.model_dump() == before
    assert ctx.audio.global_.playback_error == PREVIOUS
    assert ctx.audio.pads.residency_status(0) == (None, PREVIOUS)
    for native_method in {method for _, method in PRIMARY}:
        getattr(audio_engine_mock, native_method).assert_not_called()


@pytest.mark.parametrize(
    ("action", "native_method"), [("volume", "set_volume"), ("pad.gain", "set_pad_gain")]
)
def test_parameter_boundary_does_not_hide_programming_errors(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    action: str,
    native_method: str,
) -> None:
    ctx = _context(controller, audio_engine_mock, monkeypatch, action)
    getattr(audio_engine_mock, native_method).side_effect = TypeError("programming failure")

    with pytest.raises(TypeError, match="programming failure"):
        _actions(ctx)[action]()


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
def test_direct_parameter_controller_api_still_exposes_native_refusal(
    controller: AppController, audio_engine_mock: Mock, error_type: type[Exception]
) -> None:
    audio_engine_mock.set_volume.side_effect = error_type(REFUSAL)

    with pytest.raises(error_type, match=REFUSAL):
        controller.transport.global_params.set_volume(0.6)
