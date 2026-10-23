"""Requested global controls cannot certify a callback rate effect or a newer receipt."""

from contextlib import contextmanager
from dataclasses import FrozenInstanceError
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.ui.constants import SIDEBAR_RIGHT_PX
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import sidebar_right

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper.controller import AppController


def _feedback(
    *,
    state: str = "applied",
    request: int = 2,
    effect: int = 2,
    effective_speed: float = 2.0,
    error: str | None = None,
) -> dict[str, object]:
    return {
        "request_revision": request,
        "effect_revision": effect,
        "state": state,
        "requested_speed": 1.8,
        "requested_master_period": 0.25,
        "effective_speed": effective_speed,
        "effective_master_period": 0.5,
        "execution_frame": None if state == "queued" else 4096,
        "error": error,
        "master_provenance": {
            "kind": "unknown",
            "source_identity": None,
            "accepted_revision": None,
        },
    }


def test_runtime_tick_reads_one_receipt_without_replacing_requested_settings(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.speed = 1.7
    controller.session.master_period_seconds = 0.3
    observe = Mock(return_value=_feedback(state="preparing"))
    audio_engine_mock.global_rate_status = observe
    audio_engine_mock.poll_loader_events.return_value = None
    audio_engine_mock.receive_msg.return_value = None

    controller.poll_runtime_events()
    status = controller.transport.global_params.global_rate_status()

    observe.assert_called_once_with()
    assert status.state == "preparing"
    assert status.requested_speed == 1.8
    assert status.effective_speed == 2.0
    assert status.requested_master_period == 0.25
    assert status.effective_master_period == 0.5
    assert status.execution_frame == 4096
    assert controller.project.speed == 1.7
    assert controller.session.master_period_seconds == 0.3
    assert UiContext(controller).state.global_.global_rate_status() is status
    observe.assert_called_once_with()


def test_repeated_same_target_cannot_reuse_old_applied_receipt(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.speed = 1.8
    observe = Mock(return_value=_feedback(effective_speed=1.8))
    audio_engine_mock.global_rate_status = observe
    controls = controller.transport.global_params
    controls._poll_global_rate_status()
    assert controls.global_rate_status().state == "applied"

    controls.set_speed(1.8)
    unobserved = controls.global_rate_status()
    assert unobserved.state == "queued"
    assert unobserved.request_revision is None
    assert unobserved.effect_revision == 2
    assert unobserved.effective_speed == 1.8
    assert unobserved.execution_frame is None
    observe.assert_called_once_with()

    controls._poll_global_rate_status()
    assert controls.global_rate_status().state == "queued"
    assert controls.global_rate_status().request_revision is None

    observe.return_value = _feedback(state="queued", request=3, effect=2, effective_speed=1.8)
    controls._poll_global_rate_status()
    queued = controls.global_rate_status()
    assert queued.state == "queued"
    assert queued.request_revision == 3
    assert queued.effect_revision == 2
    assert queued.execution_frame is None
    assert controller.project.speed == 1.8


def test_refused_receipt_preserves_newer_requested_speed_and_old_effective_tuple(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.speed = 1.8
    audio_engine_mock.global_rate_status = Mock(
        return_value=_feedback(state="rejected", error="participant changed")
    )
    controls = controller.transport.global_params
    controls._poll_global_rate_status()

    rejected = controls.global_rate_status()
    assert rejected.state == "rejected"
    assert rejected.effective_speed == 2.0
    assert rejected.effective_master_period == 0.5
    assert rejected.error == "participant changed"
    assert controller.project.speed == 1.8

    controls.set_speed(1.7)
    newer = controls.global_rate_status()
    assert newer.state == "queued"
    assert newer.requested_speed == 1.7
    assert newer.error is None
    assert newer.effective_speed == 2.0
    assert controller.project.speed == 1.7


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("request_revision", True),
        ("effect_revision", 3),
        ("state", "queued"),
        ("effective_speed", float("nan")),
        ("effective_master_period", 0.0),
        ("execution_frame", -1),
        ("effective_speed", True),
        ("effective_speed", 10**1000),
        ("state", []),
        ("error", "unexpected success error"),
        ("error", ""),
    ],
)
def test_invalid_receipt_cannot_confirm_audio(
    controller: AppController, audio_engine_mock: Mock, field: str, value: object
) -> None:
    feedback = _feedback()
    feedback[field] = value
    audio_engine_mock.global_rate_status = Mock(return_value=feedback)
    controller.project.speed = 1.8
    controls = controller.transport.global_params
    controls._poll_global_rate_status()

    status = controls.global_rate_status()
    assert status.state is None
    assert status.effective_speed is None
    assert status.error
    assert controller.project.speed == 1.8


@pytest.mark.parametrize("field", list(_feedback()))
def test_incomplete_receipt_cannot_confirm_audio(
    controller: AppController, audio_engine_mock: Mock, field: str
) -> None:
    feedback = _feedback()
    del feedback[field]
    audio_engine_mock.global_rate_status = Mock(return_value=feedback)
    controls = controller.transport.global_params

    controls._poll_global_rate_status()

    status = controls.global_rate_status()
    assert status.state is None
    assert status.effective_speed is None
    assert status.error


@pytest.mark.parametrize(
    "provenance",
    [
        None,
        {},
        {"kind": [], "source_identity": None, "accepted_revision": None},
        {"kind": "other", "source_identity": None, "accepted_revision": None},
        {"kind": "unknown", "source_identity": "a" * 64, "accepted_revision": None},
        {"kind": "absent", "source_identity": None, "accepted_revision": "old"},
        {"kind": "qualified", "source_identity": None, "accepted_revision": None},
        {"kind": "qualified", "source_identity": "z" * 64, "accepted_revision": None},
        {"kind": "qualified", "source_identity": "a" * 63, "accepted_revision": None},
        {"kind": "qualified", "source_identity": "a" * 64, "accepted_revision": ""},
        {"kind": "qualified", "source_identity": "a" * 64, "accepted_revision": 1},
    ],
)
def test_invalid_master_provenance_cannot_certify_an_actual_rate(
    controller: AppController, audio_engine_mock: Mock, provenance: object
) -> None:
    feedback = _feedback()
    feedback["master_provenance"] = provenance
    audio_engine_mock.global_rate_status = Mock(return_value=feedback)
    parameters = controller.transport.global_params
    parameters._poll_global_rate_status()
    status = parameters.global_rate_status()
    assert status.state is None
    assert status.master_provenance is None
    assert status.error


def test_qualified_master_provenance_requires_an_actual_master_period(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    feedback = _feedback()
    feedback.update(
        effective_master_period=None,
        master_provenance={
            "kind": "qualified",
            "source_identity": "a" * 64,
            "accepted_revision": None,
        },
    )
    audio_engine_mock.global_rate_status = Mock(return_value=feedback)
    parameters = controller.transport.global_params
    parameters._poll_global_rate_status()
    assert parameters.global_rate_status().master_provenance is None
    assert parameters.global_rate_status().error


@pytest.mark.parametrize("accepted_revision", [None, "opaque-current-timing-revision"])
def test_actual_master_provenance_is_an_immutable_copy_of_one_rate_image(
    controller: AppController, audio_engine_mock: Mock, accepted_revision: str | None
) -> None:
    feedback = _feedback()
    provenance = {
        "kind": "qualified",
        "source_identity": "a" * 64,
        "accepted_revision": accepted_revision,
    }
    feedback["master_provenance"] = provenance
    audio_engine_mock.global_rate_status = Mock(return_value=feedback)
    parameters = controller.transport.global_params
    parameters._poll_global_rate_status()
    status = parameters.global_rate_status()
    actual = status.master_provenance
    assert actual is not None
    provenance.update(source_identity="c" * 64, accepted_revision="later")
    assert actual.source_identity == "a" * 64
    assert actual.accepted_revision == accepted_revision
    with pytest.raises(FrozenInstanceError):
        actual.source_identity = "d" * 64


@pytest.mark.parametrize("kind", ["absent", "unknown"])
def test_no_qualified_reference_does_not_claim_a_transport_clock_reset(
    controller: AppController, audio_engine_mock: Mock, kind: str
) -> None:
    feedback = _feedback()
    feedback["master_provenance"] = {
        "kind": kind,
        "source_identity": None,
        "accepted_revision": None,
    }
    audio_engine_mock.global_rate_status = Mock(return_value=feedback)
    parameters = controller.transport.global_params
    parameters._poll_global_rate_status()
    status = parameters.global_rate_status()
    assert status.effective_master_period == 0.5
    assert status.effective_speed == 2.0
    assert status.master_provenance is not None
    assert status.master_provenance.kind == kind


def test_older_request_receipt_cannot_replace_a_newer_observation(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    observe = Mock(return_value=_feedback())
    audio_engine_mock.global_rate_status = observe
    controls = controller.transport.global_params
    controls._poll_global_rate_status()

    observe.return_value = _feedback(request=1, effect=1)
    controls._poll_global_rate_status()

    status = controls.global_rate_status()
    assert status.state is None
    assert status.effective_speed is None
    assert status.error


@pytest.mark.parametrize("unavailable", ["missing", "not-callable", "no-callback", "failure"])
def test_absent_confirmation_never_falls_back_to_project_speed(
    controller: AppController, audio_engine_mock: Mock, unavailable: str
) -> None:
    controller.project.speed = 1.8
    if unavailable == "missing":
        audio_engine_mock.global_rate_status = Mock()
        del audio_engine_mock.global_rate_status
    elif unavailable == "not-callable":
        audio_engine_mock.global_rate_status = None
    elif unavailable == "failure":
        failure = RuntimeError("snapshot unavailable")
        audio_engine_mock.global_rate_status = Mock(side_effect=failure)
    else:
        audio_engine_mock.global_rate_status = Mock(return_value=None)
    controls = controller.transport.global_params
    controls._poll_global_rate_status()

    status = controls.global_rate_status()
    assert status.requested_speed == 1.8
    assert status.effective_speed is None
    assert status.state is None
    assert bool(status.error) is (unavailable != "no-callback")


@contextmanager
def _imgui_context() -> Iterator[None]:
    previous = imgui.get_current_context()
    owned = imgui.create_context()
    imgui.set_current_context(owned)
    io = imgui.get_io()
    io.set_ini_filename(None)
    io.display_size = imgui.ImVec2(600, 450)
    io.delta_time = 1 / 60
    io.backend_flags |= imgui.BackendFlags_.renderer_has_textures
    try:
        yield
    finally:
        imgui.destroy_context(owned)
        if previous is not None:
            imgui.set_current_context(previous)


@pytest.mark.parametrize(
    ("state", "effect", "message", "error"),
    [
        ("queued", 1, "Rate change queued", None),
        ("preparing", 2, "Preparing rate change…", None),
        ("applied", 2, "Rate applied", None),
        ("rejected", 2, None, "participant changed"),
    ],
)
def test_actual_rate_hover_distinguishes_requested_from_confirmed_audio_without_error_details(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    state: str,
    effect: int,
    message: str | None,
    error: str | None,
) -> None:
    controller.project.speed = 1.8
    audio_engine_mock.global_rate_status = Mock(
        return_value=_feedback(state=state, effect=effect, error=error)
    )
    controller.transport.global_params._poll_global_rate_status()
    ctx = UiContext(controller)
    monkeypatch.setattr(ctx.state.global_, "effective_bpm", lambda: 240.0)
    descriptions: list[str] = []
    original_tooltip = imgui.set_tooltip

    def tooltip(value: str) -> None:
        descriptions.append(value)
        original_tooltip(value)

    monkeypatch.setattr(imgui, "set_tooltip", tooltip)
    with _imgui_context():
        for frame in range(4):
            imgui.new_frame()
            imgui.set_next_window_pos((0, 0))
            imgui.set_next_window_size((SIDEBAR_RIGHT_PX, 400))
            imgui.begin("Global rate confirmation", flags=imgui.WindowFlags_.no_saved_settings)
            sidebar_right._bpm_display(ctx)
            minimum, maximum = imgui.get_item_rect_min(), imgui.get_item_rect_max()
            midpoint = ((minimum.x + maximum.x) / 2, (minimum.y + maximum.y) / 2)
            imgui.end()
            imgui.render()
            if frame == 1:
                assert not descriptions
                imgui.get_io().add_mouse_pos_event(*midpoint)
        assert imgui.get_draw_data().total_vtx_count > 0
    assert descriptions
    lines = descriptions[-1].splitlines()
    assert "Requested speed: 1.80x" in lines
    assert "Audio speed: 2.00x" in lines
    assert "Requested BPM: 240.0" in lines
    assert "Audio BPM: 120.00" in lines
    if message is not None:
        assert message in lines
    else:
        assert not any("rejected" in text.lower() for text in descriptions)
    if error is not None:
        assert not any(error in text for text in descriptions)
