"""Physical channel labels must belong to the explicitly selected driver."""

from contextlib import nullcontext
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.audio_output_options import AudioOutputOptions
from flitzis_looper.ui.render import audio_output as output_render

if TYPE_CHECKING:
    from flitzis_looper.controller.audio_output import AudioOutputController


@pytest.mark.parametrize("selected", [None, "driver-b", "driver-a"])
def test_named_channels_are_not_borrowed_from_another_or_auto_driver(selected: str | None) -> None:
    channels = [{"index": 2, "name": "Original physical output", "pcm_supported": True}]
    control = Mock()
    control.draft = AudioOutputOptions(backend="asio", device_id=selected)
    control.observed = {"asio_control": {"device_id": "driver-a", "channels": channels}}
    result = output_render._installed_channels(cast("AudioOutputController", control))
    assert result == (channels if selected == "driver-a" else [])


def _control(options: AudioOutputOptions | None = None) -> Mock:
    control = Mock()
    control.draft = options or AudioOutputOptions()
    control.numeric_edits = {}
    control.phase = "ready"
    return control


@pytest.fixture
def fake_imgui(monkeypatch: pytest.MonkeyPatch) -> Mock:
    ui = Mock()
    ui.checkbox.return_value = (False, True)
    ui.input_text.return_value = (False, "")
    ui.button.return_value = False
    monkeypatch.setattr(output_render, "imgui", ui)
    monkeypatch.setattr(output_render, "item_width", lambda _width: nullcontext())
    return ui


@pytest.mark.parametrize(
    ("field", "maximum", "value"),
    [
        ("sample_rate_hz", 0xFFFF_FFFF, 96000),
        ("sample_rate_hz", 0xFFFF_FFFF, 44117),
        ("sample_rate_hz", 0xFFFF_FFFF, 0xFFFF_FFFF),
        ("buffer_frames", 0x7FFF_FFFF, 192),
        ("buffer_frames", 0x7FFF_FFFF, 256),
    ],
)
def test_manual_value_requires_explicit_input_before_apply(
    fake_imgui: Mock, field: str, maximum: int, value: int
) -> None:
    control = _control()
    fake_imgui.checkbox.return_value = (True, False)
    output_render._automatic_number(control, "Value", field, maximum)
    assert getattr(control.draft, field) is None
    assert control.numeric_edits == {field: ""}
    fake_imgui.button.return_value = True  # Even a spurious click cannot apply an incomplete edit.
    output_render._apply_controls(control)
    control.apply.assert_not_called()

    fake_imgui.checkbox.return_value = (False, False)
    fake_imgui.input_text.return_value = (True, str(value))
    output_render._automatic_number(control, "Value", field, maximum)
    assert getattr(control.draft, field) == value
    assert control.numeric_edits == {}
    output_render._apply_controls(control)
    control.apply.assert_called_once_with(control.draft)


@pytest.mark.parametrize("text", ["", "0", "-1", "abc", "2147483648", "9" * 5000])
def test_invalid_manual_edit_blocks_previous_valid_value(fake_imgui: Mock, text: str) -> None:
    control = _control(AudioOutputOptions(buffer_frames=192))
    fake_imgui.input_text.return_value = (True, text)
    output_render._automatic_number(control, "Buffer", "buffer_frames", 0x7FFF_FFFF)
    assert control.draft.buffer_frames == 192
    assert control.numeric_edits == {"buffer_frames": text}
    fake_imgui.button.return_value = True
    output_render._apply_controls(control)
    control.apply.assert_not_called()


def test_returning_to_auto_discards_incomplete_edit(fake_imgui: Mock) -> None:
    control = _control(AudioOutputOptions(buffer_frames=256))
    control.numeric_edits["buffer_frames"] = "invalid"
    fake_imgui.checkbox.return_value = (True, True)
    output_render._automatic_number(control, "Buffer", "buffer_frames", 0x7FFF_FFFF)
    assert control.draft.buffer_frames is None
    assert control.numeric_edits == {}


def test_changing_driver_clears_device_specific_geometry_and_routing(
    fake_imgui: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    control = _control(
        AudioOutputOptions(
            backend="asio",
            device_id="driver-a",
            sample_rate_hz=96000,
            buffer_frames=192,
            left_channel=2,
            right_channel=3,
        )
    )
    control.inventory = [{"id": "asio", "name": "ASIO", "devices": []}]
    control.numeric_edits["buffer_frames"] = "invalid"
    choices = iter([(False, "asio"), (True, "driver-b")])
    monkeypatch.setattr(output_render, "_choice", lambda *_args: next(choices))
    output_render._backend_device_controls(control)
    assert control.draft == AudioOutputOptions(backend="asio", device_id="driver-b")
    assert control.numeric_edits == {}


@pytest.mark.parametrize("draft_value", [None, 192])
def test_routing_change_does_not_accept_unfinished_numeric_edit(
    fake_imgui: Mock, draft_value: int | None
) -> None:
    control = _control(AudioOutputOptions(buffer_frames=draft_value))
    control.numeric_edits["buffer_frames"] = ""
    fake_imgui.checkbox.return_value = (True, False)
    fake_imgui.input_int.return_value = (False, 1)
    output_render._routing(control)
    assert control.numeric_edits == {"buffer_frames": ""}
    fake_imgui.button.return_value = True
    output_render._apply_controls(control)
    control.apply.assert_not_called()


def test_changing_backend_clears_device_specific_geometry_and_pending_edit(
    fake_imgui: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    control = _control(AudioOutputOptions(backend="asio", device_id="driver-a", buffer_frames=192))
    control.inventory = []
    control.numeric_edits["sample_rate_hz"] = ""
    choices = iter([(True, "wasapi"), (False, None)])
    monkeypatch.setattr(output_render, "_choice", lambda *_args: next(choices))
    output_render._backend_device_controls(control)
    assert control.draft == AudioOutputOptions(backend="wasapi")
    assert control.numeric_edits == {}


def test_reselecting_same_backend_and_driver_preserves_edits(
    fake_imgui: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    options = AudioOutputOptions(
        backend="asio",
        device_id="driver-a",
        sample_rate_hz=96000,
        buffer_frames=192,
        left_channel=2,
        right_channel=3,
    )
    control = _control(options)
    control.inventory = [{"id": "asio", "name": "ASIO", "devices": []}]
    control.numeric_edits["buffer_frames"] = "invalid"
    choices = iter([(True, "asio"), (True, "driver-a")])
    monkeypatch.setattr(output_render, "_choice", lambda *_args: next(choices))
    output_render._backend_device_controls(control)
    assert control.draft == options
    assert control.numeric_edits == {"buffer_frames": "invalid"}
    fake_imgui.button.return_value = True
    output_render._apply_controls(control)
    control.apply.assert_not_called()


def test_two_manual_fields_remain_independently_blocking_across_frames(fake_imgui: Mock) -> None:
    control = _control()
    fake_imgui.checkbox.return_value = (True, False)
    output_render._automatic_number(control, "Rate", "sample_rate_hz", 0xFFFF_FFFF)
    output_render._automatic_number(control, "Buffer", "buffer_frames", 0x7FFF_FFFF)
    assert control.numeric_edits == {"sample_rate_hz": "", "buffer_frames": ""}

    fake_imgui.checkbox.return_value = (False, False)
    fake_imgui.input_text.return_value = (True, "9")
    output_render._automatic_number(control, "Rate", "sample_rate_hz", 0xFFFF_FFFF)
    assert control.draft.sample_rate_hz == 9
    assert control.numeric_edits == {"buffer_frames": ""}
    fake_imgui.button.return_value = True
    output_render._apply_controls(control)
    control.apply.assert_not_called()

    fake_imgui.input_text.return_value = (True, "96000")
    output_render._automatic_number(control, "Rate", "sample_rate_hz", 0xFFFF_FFFF)
    fake_imgui.checkbox.return_value = (True, True)
    fake_imgui.input_text.return_value = (False, "")
    output_render._automatic_number(control, "Buffer", "buffer_frames", 0x7FFF_FFFF)
    assert control.draft.sample_rate_hz == 96000
    assert control.draft.buffer_frames is None
    assert control.numeric_edits == {}


def test_sample_rate_overflow_is_not_applied(fake_imgui: Mock) -> None:
    control = _control(AudioOutputOptions(sample_rate_hz=96000))
    fake_imgui.input_text.return_value = (True, "4294967296")
    output_render._automatic_number(control, "Rate", "sample_rate_hz", 0xFFFF_FFFF)
    assert control.draft.sample_rate_hz == 96000
    assert control.numeric_edits == {"sample_rate_hz": "4294967296"}
    fake_imgui.button.return_value = True
    output_render._apply_controls(control)
    control.apply.assert_not_called()


def _asio_snapshot() -> dict[str, object]:
    return {
        "device_id": "driver-a",
        "backend_released": False,
        "failure": None,
        "info_query_error": None,
        "status": {"notifications": 0, "callback_error": 0},
        "initialized_sample_rate_hz": 96000.0,
        "last_control_query_rate_hz": 96000.0,
        "driver": {
            "sample_rate": 96000.0,
            "min_frames": 96,
            "max_frames": 96,
            "preferred_frames": 96,
            "granularity": 0,
        },
        "common_rate_probes": [
            {"sample_rate_hz": 48000, "supported": True, "driver_status": 0},
            {"sample_rate_hz": 44100, "supported": False, "driver_status": -1},
            {"sample_rate_hz": 48000, "supported": True, "driver_status": 0},
        ],
    }


def _asio_control() -> Mock:
    control = _control(AudioOutputOptions(backend="asio", device_id="driver-a"))
    control.available = True
    control.busy = False
    control.observed = {"asio_control": _asio_snapshot()}
    return control


def test_reported_capabilities_are_bound_to_selected_healthy_driver() -> None:
    control = _asio_control()
    snapshot = output_render._current_asio_snapshot(control)
    assert snapshot is not None
    assert output_render._reported_rates(snapshot) == [48000, 96000]
    assert output_render._reported_buffers(control, snapshot) == ([96], False)
    control.draft = AudioOutputOptions(backend="asio", device_id="driver-b")
    assert output_render._current_asio_snapshot(control) is None
    control.draft = AudioOutputOptions(backend="asio")
    assert output_render._current_asio_snapshot(control) is None


def test_rate_suggestions_reject_invalid_probes_and_invalid_initialized_rate() -> None:
    snapshot = _asio_snapshot()
    snapshot["initialized_sample_rate_hz"] = float("nan")
    snapshot["common_rate_probes"] = [
        {"sample_rate_hz": True, "supported": True, "driver_status": 0},
        {"sample_rate_hz": 48000.0, "supported": True, "driver_status": 0},
        {"sample_rate_hz": 96000, "supported": True, "driver_status": -1},
        {"sample_rate_hz": 44100, "supported": True, "driver_status": False},
        {"sample_rate_hz": 88200, "supported": True, "driver_status": 0},
    ]
    assert output_render._reported_rates(snapshot) == [88200]


@pytest.mark.parametrize(
    "status",
    [
        {"notifications": 1, "callback_error": 0},
        {"notifications": 2, "callback_error": 0},
        {"notifications": 4, "callback_error": 0},
        {"notifications": 32, "callback_error": 0},
        {"notifications": 0, "callback_error": -1},
        {"notifications": 0, "callback_error": False},
        {},
    ],
)
def test_faulted_or_incomplete_driver_snapshot_is_not_used(status: dict[str, object]) -> None:
    control = _asio_control()
    control.observed["asio_control"]["status"] = status
    assert output_render._current_asio_snapshot(control) is None


@pytest.mark.parametrize(
    ("minimum", "maximum", "preferred", "granularity", "expected"),
    [
        (96, 768, 192, -1, ([96, 192, 384, 768], False)),
        (96, 257, 160, 32, ([96, 128, 160, 192, 224, 256], False)),
        (96, 512, 192, 0, ([192], False)),
        (1, 10000, 192, 1, ([1, 192, 10000], True)),
        (96, 768, 200, -1, ([], False)),
        (96, 257, 161, 32, ([], False)),
        (96, 257, 160, -2, ([], False)),
        (1, 0x7FFF_FFFF, 192, 1, ([1, 192, 0x7FFF_FFFF], True)),
        (0, 512, 192, 1, ([], False)),
        (1, 0x8000_0000, 192, 1, ([], False)),
        (1, 512, 192, True, ([], False)),
    ],
)
def test_buffer_choices_follow_reported_driver_geometry(
    minimum: int, maximum: int, preferred: int, granularity: int, expected: tuple[list[int], bool]
) -> None:
    control = _asio_control()
    snapshot = control.observed["asio_control"]
    snapshot["driver"].update(
        min_frames=minimum, max_frames=maximum, preferred_frames=preferred, granularity=granularity
    )
    assert output_render._reported_buffers(control, snapshot) == expected


def test_buffer_choices_are_withheld_for_a_different_or_unfinished_rate() -> None:
    control = _asio_control()
    snapshot = control.observed["asio_control"]
    control.draft = AudioOutputOptions(backend="asio", device_id="driver-a", sample_rate_hz=48000)
    assert output_render._reported_buffers(control, snapshot) == ([], False)
    control.draft = AudioOutputOptions(backend="asio", device_id="driver-a")
    control.numeric_edits["sample_rate_hz"] = ""
    assert output_render._reported_buffers(control, snapshot) == ([], False)


@pytest.mark.parametrize("rate", [None, True, float("nan"), float("inf"), 48000, 10**1000])
def test_buffer_choices_require_matching_valid_query_rate(rate: object) -> None:
    control = _asio_control()
    snapshot = control.observed["asio_control"]
    snapshot["last_control_query_rate_hz"] = rate
    assert output_render._reported_buffers(control, snapshot) == ([], False)


def test_explicit_reported_choice_only_finishes_its_own_field(
    fake_imgui: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    control = _asio_control()
    control.numeric_edits = {"sample_rate_hz": "", "buffer_frames": ""}
    monkeypatch.setattr(output_render, "_choice", lambda *_args: (True, "96000"))
    output_render._reported_choice(control, "sample_rate_hz", "Rate", [48000, 96000])
    assert control.draft.sample_rate_hz == 96000
    assert control.numeric_edits == {"buffer_frames": ""}
    fake_imgui.button.return_value = True
    output_render._apply_controls(control)
    control.apply.assert_not_called()


def test_selecting_another_rate_withholds_old_buffer_in_the_same_frame(
    fake_imgui: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    control = _asio_control()
    shown: list[str] = []

    def choose(label: str, _current: str | None, _values: object) -> tuple[bool, str | None]:
        shown.append(label)
        return True, "48000"

    monkeypatch.setattr(output_render, "_choice", choose)
    output_render._reported_geometry(control)
    assert control.draft.sample_rate_hz == 48000
    assert shown == ["Initialized and probed rates (Hz)"]
