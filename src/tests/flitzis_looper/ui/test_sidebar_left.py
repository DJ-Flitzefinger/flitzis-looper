from contextlib import contextmanager
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.controller.committed_stem_residency import StemResidencyStatus
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import sidebar_left
from flitzis_looper.ui.render.bpm_entry import filtered_bpm_entry_char, sanitize_bpm_entry_text
from flitzis_looper.ui.render.sidebar_left import (
    MANUAL_BPM_ENTRY_MAX,
    MANUAL_BPM_ENTRY_MIN,
    filtered_eq_entry_char,
    parse_eq_entry_text,
    parse_manual_bpm_entry_text,
    sanitize_eq_entry_text,
)
from tests.flitzis_looper.ui.test_key_lock_render import _imgui_context

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper.controller import AppController


def test_stem_generation_diagnostics_do_not_render_in_sidebar(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = UiContext(controller)
    diagnostic = r"Private stem artifact cleanup failed: D:\private\song.wav"
    monkeypatch.setattr(ctx.state.stems, "stem_generation_diagnostic", lambda _pad: diagnostic)
    labels: list[str] = []
    original_text = imgui.text_colored

    def record(color: imgui.ImVec4Like, text: str) -> None:
        labels.append(text)
        original_text(color, text)

    monkeypatch.setattr(imgui, "text_colored", record)
    with _imgui_context():
        imgui.new_frame()
        imgui.begin("Stem diagnostics", flags=imgui.WindowFlags_.no_saved_settings)
        sidebar_left._render_generate_stems(ctx, 0)
        imgui.end()
        imgui.render()
    assert all(diagnostic not in label for label in labels)


class _Point:
    __slots__ = ("x", "y")

    def __init__(self, x: float, y: float) -> None:
        self.x = x
        self.y = y


class _Io:
    __slots__ = ("mouse_delta", "mouse_pos")

    def __init__(self, mouse_x: float) -> None:
        self.mouse_pos = _Point(mouse_x, 0.0)
        self.mouse_delta = _Point(0.0, 0.0)


class _GainPads:
    __slots__ = ("calls",)

    def __init__(self) -> None:
        self.calls: list[tuple[int, float]] = []

    def set_pad_gain(self, pad_id: int, gain_db: float) -> None:
        self.calls.append((pad_id, gain_db))


class _GainAudio:
    __slots__ = ("pads",)

    def __init__(self) -> None:
        self.pads = _GainPads()


class _GainProject:
    __slots__ = ("pad_gain_db",)

    def __init__(self) -> None:
        self.pad_gain_db = [0.0]


class _GainState:
    __slots__ = ("project",)

    def __init__(self) -> None:
        self.project = _GainProject()


class _GainContext:
    __slots__ = ("audio", "state")

    def __init__(self) -> None:
        self.audio = _GainAudio()
        self.state = _GainState()


class _SidebarPads:
    __slots__ = ("_loaded", "_loading")

    def __init__(self, *, loaded: bool, loading: bool) -> None:
        self._loaded = loaded
        self._loading = loading

    def is_loaded(self, _pad_id: int) -> bool:
        return self._loaded

    def is_loading(self, _pad_id: int) -> bool:
        return self._loading


class _SidebarProject:
    __slots__ = ("selected_pad",)

    def __init__(self) -> None:
        self.selected_pad = 0


class _SidebarState:
    __slots__ = ("pads", "project")

    def __init__(self, *, loaded: bool, loading: bool) -> None:
        self.project = _SidebarProject()
        self.pads = _SidebarPads(loaded=loaded, loading=loading)


class _SidebarContext:
    __slots__ = ("state",)

    def __init__(self, *, loaded: bool, loading: bool) -> None:
        self.state = _SidebarState(loaded=loaded, loading=loading)


@contextmanager
def _noop_style_var(*_args: object, **_kwargs: object) -> Iterator[None]:
    yield None


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("123,456abc", "123.456"),
        ("12..3", "12.3"),
        ("a1b2c3", "123"),
        ("3,5", "3.5"),
        ("-6.0", "-6.0"),
        ("--60", "-60"),
        ("6-0", "60"),
        ("\u00dc1", "1"),
    ],
)
def test_sanitize_eq_entry_text(raw: str, expected: str) -> None:
    assert sanitize_eq_entry_text(raw) == expected


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("123,456abc", "123.456"),
        ("12..3", "12.3"),
        ("a1b2c3", "123"),
        ("120,5", "120.5"),
        ("0\n1 2", "012"),
        ("\u00dc1", "1"),
    ],
)
def test_sanitize_manual_bpm_entry_text(raw: str, expected: str) -> None:
    assert sanitize_bpm_entry_text(raw) == expected


@pytest.mark.parametrize(
    ("char", "current", "cursor", "expected"),
    [
        ("1", "", 0, ord("1")),
        (",", "3", 1, ord(".")),
        (".", "3", 1, ord(".")),
        (".", "3.5", 3, None),
        ("-", "", 0, ord("-")),
        ("-", "3", 0, ord("-")),
        ("-", "3", 1, None),
        ("-", "-3", 0, None),
        ("\u00dc", "3", 1, None),
    ],
)
def test_filtered_eq_entry_char(
    char: str,
    current: str,
    cursor: int,
    expected: int | None,
) -> None:
    assert (
        filtered_eq_entry_char(
            ord(char),
            current,
            cursor,
            has_selection=False,
        )
        == expected
    )


@pytest.mark.parametrize(
    ("char", "current", "cursor", "expected"),
    [
        ("1", "", 0, ord("1")),
        (",", "120", 3, ord(".")),
        (".", "120", 3, ord(".")),
        (".", "120.1", 5, None),
        ("5", "120.12", 6, ord("5")),
        ("5", "120.12", 2, ord("5")),
        ("\u00dc", "120", 3, None),
    ],
)
def test_filtered_manual_bpm_entry_char(
    char: str,
    current: str,
    cursor: int,
    expected: int | None,
) -> None:
    assert (
        filtered_bpm_entry_char(
            ord(char),
            current,
            cursor,
            has_selection=False,
        )
        == expected
    )


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("3,5", 3.5),
        ("-6", -6.0),
        ("-6.0", -6.0),
        ("-60", -60.0),
        ("-999", -60.0),
        ("999", 6.0),
        ("001.2", 1.2),
    ],
)
def test_parse_eq_entry_text(raw: str, expected: float) -> None:
    assert parse_eq_entry_text(raw) == pytest.approx(expected)


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("123,456abc", 123.456),
        ("120.1", 120.1),
        ("001.20", 1.2),
        ("0", MANUAL_BPM_ENTRY_MIN),
        ("0.49", MANUAL_BPM_ENTRY_MIN),
        ("0,5", MANUAL_BPM_ENTRY_MIN),
        ("400", MANUAL_BPM_ENTRY_MAX),
        ("400.01", MANUAL_BPM_ENTRY_MAX),
        ("500", MANUAL_BPM_ENTRY_MAX),
    ],
)
def test_parse_manual_bpm_entry_text(raw: str, expected: float) -> None:
    assert parse_manual_bpm_entry_text(raw) == pytest.approx(expected)


@pytest.mark.parametrize("raw", ["", ".", "-", "-."])
def test_parse_eq_entry_text_ignores_empty_values(raw: str) -> None:
    assert parse_eq_entry_text(raw) is None


@pytest.mark.parametrize("raw", ["", "."])
def test_parse_manual_bpm_entry_text_ignores_empty_values(raw: str) -> None:
    assert parse_manual_bpm_entry_text(raw) is None


def test_filtered_eq_entry_char_accepts_replacing_selected_negative_sign() -> None:
    assert filtered_eq_entry_char(
        ord("-"),
        "-3",
        0,
        has_selection=True,
    ) == ord("-")


def test_gain_left_drag_tracks_absolute_pointer_position(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = _GainContext()
    sidebar_left._GAIN_DRAG.start(
        pad_id=0,
        button=imgui.MouseButton_.left,
        mouse_pos=imgui.ImVec2(10.0, 0.0),
    )

    monkeypatch.setattr("flitzis_looper.ui.render.sidebar_left.imgui.is_mouse_down", lambda _: True)
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left.imgui.get_item_rect_min",
        lambda: _Point(10.0, 0.0),
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left.imgui.get_item_rect_max",
        lambda: _Point(110.0, 0.0),
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left.imgui.get_io",
        lambda: _Io(85.0),
    )

    try:
        sidebar_left._apply_gain_drag(cast("UiContext", ctx), 0)
    finally:
        sidebar_left._GAIN_DRAG.clear()

    assert len(ctx.audio.pads.calls) == 1
    pad_id, gain_db = ctx.audio.pads.calls[0]
    assert pad_id == 0
    assert gain_db == pytest.approx(6.0)


@pytest.mark.parametrize(
    ("pad_state", "expected_key_lock_calls"),
    [
        ("loaded", [0]),
        ("loading", []),
        ("empty", []),
    ],
)
def test_sidebar_renders_pad_key_lock_only_for_loaded_pads(
    monkeypatch: pytest.MonkeyPatch,
    pad_state: str,
    expected_key_lock_calls: list[int],
) -> None:
    key_lock_calls: list[int] = []

    monkeypatch.setattr("flitzis_looper.ui.render.sidebar_left.style_var", _noop_style_var)
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left.imgui.get_content_region_avail",
        lambda: _Point(240.0, 0.0),
    )
    monkeypatch.setattr("flitzis_looper.ui.render.sidebar_left.imgui.separator", lambda: None)
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_pad_header",
        lambda _ctx, _info: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_bpm",
        lambda _ctx, _info: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_adjust_loop",
        lambda _ctx, _pad_id: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_key",
        lambda _ctx, _info: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_loaded_gain",
        lambda _ctx, _pad_id: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_loaded_eq",
        lambda _ctx, _info: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_loaded_actions",
        lambda _ctx, _pad_id: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_loading_status",
        lambda _ctx, _pad_id: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_unloaded_actions",
        lambda _ctx, _pad_id: None,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.sidebar_left._render_pad_key_lock",
        lambda _ctx, pad_id: key_lock_calls.append(pad_id),
    )

    sidebar_left.sidebar_left(
        cast(
            "UiContext",
            _SidebarContext(
                loaded=pad_state == "loaded",
                loading=pad_state == "loading",
            ),
        )
    )

    assert key_lock_calls == expected_key_lock_calls


@pytest.mark.parametrize(
    ("label", "factor"), [("BPM /2##half_bpm", 0.5), ("BPM x2##double_bpm", 2.0)]
)
@pytest.mark.parametrize("available", [False, True])
def test_half_double_buttons_route_factors_and_disable_unavailable_bpm(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    label: str,
    factor: float,
    *,
    available: bool,
) -> None:
    ctx = UiContext(controller)
    action = Mock()
    disabled = False

    def begin(value: object) -> None:
        nonlocal disabled
        disabled = bool(value)

    monkeypatch.setattr(ctx.audio.pads, "scale_bpm", action)
    monkeypatch.setattr(imgui, "begin_disabled", begin)
    monkeypatch.setattr(imgui, "end_disabled", lambda: None)
    monkeypatch.setattr(imgui, "same_line", lambda: None)
    monkeypatch.setattr(imgui, "button", lambda text: not disabled and text == label)
    sidebar_left._render_bpm_scale(ctx, 3, 94.0 if available else None)
    if available:
        action.assert_called_once_with(3, factor)
    else:
        action.assert_not_called()


@pytest.mark.parametrize("manual", [False, True])
@pytest.mark.parametrize("submitted", [False, True])
def test_untouched_pad_bpm_entry_preserves_effective_value_and_override(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    *,
    manual: bool,
    submitted: bool,
) -> None:
    ctx = UiContext(controller)
    bpm = 120.00128936767578
    if manual:
        controller.project.manual_bpm[0] = bpm
    monkeypatch.setattr(ctx.state.pads, "effective_bpm", lambda _pad: bpm)
    set_manual = Mock()
    clear_manual = Mock()
    monkeypatch.setattr(ctx.audio.pads, "set_manual_bpm", set_manual)
    monkeypatch.setattr(ctx.audio.pads, "clear_manual_bpm", clear_manual)
    monkeypatch.setattr(sidebar_left, "_BPM_ENTRY", sidebar_left._BpmEntryState())
    monkeypatch.setattr(imgui, "text_colored", lambda *_args: None)
    monkeypatch.setattr(imgui, "same_line", lambda *_args: None)
    monkeypatch.setattr(imgui, "set_next_item_width", lambda _width: None)
    monkeypatch.setattr(imgui, "input_text", lambda _id, text, *_args: (submitted, text))
    monkeypatch.setattr(imgui, "is_item_deactivated_after_edit", lambda: True)
    monkeypatch.setattr(imgui, "is_item_deactivated", lambda: True)
    monkeypatch.setattr(imgui, "is_item_active", lambda: False)
    monkeypatch.setattr(imgui, "is_item_hovered", lambda: True)
    monkeypatch.setattr(imgui, "is_mouse_clicked", lambda _button: False)
    tooltip = Mock()
    monkeypatch.setattr(imgui, "set_tooltip", tooltip)
    monkeypatch.setattr(imgui, "button", lambda *_args: False)
    monkeypatch.setattr(imgui, "begin_disabled", lambda *_args: None)
    monkeypatch.setattr(imgui, "end_disabled", lambda: None)

    sidebar_left._render_bpm(
        ctx, sidebar_left._SidebarPadInfo(0, 240.0, is_loaded=True, is_loading=False)
    )

    assert sidebar_left._BPM_ENTRY.text == str(bpm)
    set_manual.assert_not_called()
    clear_manual.assert_not_called()
    assert controller.project.manual_bpm[0] == (bpm if manual else None)
    tooltip.assert_called_once_with(f"Effective BPM: {bpm}")


def test_deliberate_pad_bpm_edit_preserves_fractional_override(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = UiContext(controller)
    monkeypatch.setattr(ctx.state.pads, "effective_bpm", lambda _pad: 120.00128936767578)
    set_manual = Mock()
    monkeypatch.setattr(ctx.audio.pads, "set_manual_bpm", set_manual)
    monkeypatch.setattr(sidebar_left, "_BPM_ENTRY", sidebar_left._BpmEntryState())
    monkeypatch.setattr(imgui, "text_colored", lambda *_args: None)
    monkeypatch.setattr(imgui, "same_line", lambda *_args: None)
    monkeypatch.setattr(imgui, "set_next_item_width", lambda _width: None)
    monkeypatch.setattr(imgui, "input_text", lambda *_args: (True, "119,9991234567"))
    monkeypatch.setattr(imgui, "is_item_active", lambda: False)
    monkeypatch.setattr(imgui, "is_item_hovered", lambda: False)
    monkeypatch.setattr(imgui, "button", lambda *_args: False)
    monkeypatch.setattr(imgui, "begin_disabled", lambda *_args: None)
    monkeypatch.setattr(imgui, "end_disabled", lambda: None)

    sidebar_left._render_bpm(
        ctx, sidebar_left._SidebarPadInfo(0, 240.0, is_loaded=True, is_loading=False)
    )

    set_manual.assert_called_once_with(0, 119.9991234567)


def test_pad_bpm_entry_baseline_tracks_pad_switches_and_inactive_metadata() -> None:
    state = sidebar_left._BpmEntryState()
    state.sync(pad_id=0, display_text="120.00128936767578")
    state.active = True
    state.sync(pad_id=0, display_text="119.999")
    assert state.text == state.initial_text == "120.00128936767578"

    state.sync(pad_id=1, display_text="123.456")
    assert state.text == state.initial_text == "123.456"
    assert not state.active

    state.sync(pad_id=1, display_text="119.999")
    assert state.text == state.initial_text == "119.999"


@pytest.mark.parametrize(
    ("phase", "expected"),
    [
        ("saved", "New stems awaiting audio switch"),
        ("captured", "Audio: stem selection pending"),
        ("waiting", "Audio: ALL STEMS pending"),
        ("transitioning", "Audio: ALL STEMS pending"),
        ("queue-failed", "Audio mode unconfirmed"),
        ("native-failed", "Audio mode unconfirmed"),
        ("own-effective", "Audio: ALL STEMS"),
        ("reopen", "Stems saved; audio mode unconfirmed"),
        ("full-mix", "Audio: FULL MIX"),
        ("new-launch", "Audio: ALL STEMS pending"),
    ],
)
def test_actual_stem_render_separates_saved_replacement_and_own_audio_confirmation(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    phase: str,
    expected: str,
) -> None:
    """Render declared controller projections; this does not measure Native audio."""
    previous = phase not in {"own-effective", "reopen", "full-mix", "new-launch"}
    active_attempt = phase in {"captured", "waiting", "transitioning", "native-failed"}
    status = StemResidencyStatus(
        registration="captured"
        if phase == "captured"
        else "withdrawn"
        if phase == "full-mix"
        else "accepted",
        residency=None if phase == "reopen" else "pending" if phase == "captured" else "accepted",
        effective=(
            "waiting"
            if phase in {"captured", "waiting"}
            else "transitioning"
            if phase == "transitioning"
            else "rejected"
            if phase == "native-failed"
            else None
            if phase in {"reopen", "new-launch"}
            else "effective"
        ),
        requested_mode=(
            None if phase == "reopen" else "full_mix" if phase == "full-mix" else "all_stems"
        ),
        disk_stem_set_identity="c" * 64,
        control_stem_set_identity="b" * 64
        if previous or phase in {"new-launch", "full-mix"}
        else "c" * 64,
        effective_stem_set_identity="b" * 64
        if previous
        else "c" * 64
        if phase == "own-effective"
        else None,
        attempt_stem_set_identity=None
        if phase == "reopen"
        else "c" * 64
        if active_attempt or phase == "own-effective"
        else "b" * 64,
        attempt_failed=phase in {"queue-failed", "native-failed"},
    )
    selector = Mock(return_value=status)
    monkeypatch.setattr(controller.stems, "stem_residency_status", selector)
    monkeypatch.setattr(controller.stems, "stems_available", Mock(return_value=True))
    generation = Mock(side_effect=AssertionError("render must not start a separator"))
    monkeypatch.setattr(controller.stems, "generate_stems_async", generation)
    controller.project.pad_stem_mix_mode[0] = (
        "full_mix" if phase in {"reopen", "full-mix"} else "all_stems"
    )
    ctx = UiContext(controller)
    native_text = imgui.text_colored
    descriptions: list[str] = []
    rects: list[tuple[str, float]] = []

    def text(color: imgui.ImVec4Like, value: str) -> None:
        descriptions.append(value)
        native_text(color, value)
        rects.append((value, imgui.get_item_rect_max().x))

    monkeypatch.setattr(imgui, "text_colored", text)
    with _imgui_context():
        for _ in range(2):
            descriptions.clear()
            rects.clear()
            imgui.new_frame()
            imgui.set_next_window_pos((0, 0))
            imgui.set_next_window_size((280, 350))
            imgui.begin("Stem status render", flags=imgui.WindowFlags_.no_saved_settings)
            wrap_right = imgui.get_cursor_screen_pos().x + 240.0
            sidebar_left._render_stem_mix_mode(
                ctx, sidebar_left._SidebarPadInfo(0, 240.0, is_loaded=True, is_loading=False)
            )
            imgui.end()
            imgui.render()
            assert imgui.get_draw_data().total_vtx_count > 0
        assert expected in descriptions
        assert ("New stems saved" in descriptions) is previous
        assert ("Last confirmed audio: previous stems" in descriptions) is previous
        if previous:
            assert "Audio: ALL STEMS" not in descriptions
        if phase in {"reopen", "new-launch"}:
            assert "Audio: FULL MIX" not in descriptions
            assert "Audio: ALL STEMS" not in descriptions
        assert all(right <= wrap_right + 1.0 for label, right in rects if label != "Stem Mix")
        assert not any("b" * 64 in label or "c" * 64 in label for label in descriptions)
    assert selector.call_args_list == [((0,), {}), ((0,), {})]
    generation.assert_not_called()


class _StemMixRender:
    """Observe real disabled buttons, rectangles and labels while retaining mouse behavior."""

    def __init__(self, ctx: UiContext, monkeypatch: pytest.MonkeyPatch) -> None:
        self.ctx = ctx
        self.disabled = Mock(wraps=imgui.begin_disabled)
        self.buttons: dict[str, tuple[bool, float, float]] = {}
        self.descriptions: list[str] = []
        self._native_button = imgui.button
        self._native_text = imgui.text_colored
        monkeypatch.setattr(imgui, "begin_disabled", self.disabled)
        monkeypatch.setattr(imgui, "button", self.button)
        monkeypatch.setattr(imgui, "text_colored", self.text)

    def button(self, label: str, size: imgui.ImVec2Like) -> bool:
        clicked = self._native_button(label, size)
        minimum = imgui.get_item_rect_min()
        maximum = imgui.get_item_rect_max()
        self.buttons[label] = (
            self.disabled.call_args.kwargs["disabled"],
            (minimum.x + maximum.x) / 2,
            (minimum.y + maximum.y) / 2,
        )
        return clicked

    def text(self, color: imgui.ImVec4Like, value: str) -> None:
        self.descriptions.append(value)
        self._native_text(color, value)

    def frame(self) -> None:
        self.disabled.reset_mock()
        self.buttons.clear()
        self.descriptions.clear()
        imgui.new_frame()
        imgui.set_next_window_pos((0, 0))
        imgui.set_next_window_size((280, 350))
        imgui.begin("Stem retry render", flags=imgui.WindowFlags_.no_saved_settings)
        sidebar_left._render_stem_mix_mode(
            self.ctx, sidebar_left._SidebarPadInfo(0, 240.0, is_loaded=True, is_loading=False)
        )
        imgui.end()
        imgui.render()
        assert imgui.get_draw_data().total_vtx_count > 0


@pytest.mark.parametrize(
    ("source_present", "can_request"),
    [(True, True), (True, False), (False, False)],
    ids=["unavailable-paired-retry", "unavailable-no-pair", "missing-source"],
)
def test_actual_all_stems_button_requests_retry_without_claiming_availability(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    *,
    source_present: bool,
    can_request: bool,
) -> None:
    """Prove UI eligibility and intent forwarding, without manufacturing Native adoption."""
    controller.project.sample_paths[0] = "samples/retry.wav" if source_present else None
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    status = StemResidencyStatus(
        registration="accepted", residency=None, effective=None, requested_mode=None
    )
    available = Mock(return_value=False)
    eligibility = Mock(return_value=can_request)
    set_mode = Mock()
    generation = Mock(side_effect=AssertionError("a mix retry must not start a separator"))
    monkeypatch.setattr(controller.stems, "stems_available", available)
    monkeypatch.setattr(controller.stems, "can_request_all_stems", eligibility, raising=False)
    monkeypatch.setattr(controller.stems, "stem_residency_status", Mock(return_value=status))
    monkeypatch.setattr(controller.stems, "set_stem_mix_mode", set_mode)
    monkeypatch.setattr(controller.stems, "generate_stems_async", generation)
    before_project = controller.project.model_dump()
    before_session = controller.session.model_dump()
    render = _StemMixRender(UiContext(controller), monkeypatch)
    with _imgui_context():
        render.frame()
        render.frame()
        all_stems_label = "ALL STEMS##stem_mix_all_stems"
        disabled, x, y = render.buttons[all_stems_label]
        assert disabled is not can_request
        assert not render.buttons["FULL MIX##stem_mix_full_mix"][0]
        set_mode.assert_not_called()
        io = imgui.get_io()
        io.add_mouse_pos_event(x, y)
        pressed, released = True, False
        io.add_mouse_button_event(0, pressed)
        render.frame()
        io.add_mouse_button_event(0, released)
        render.frame()
        if can_request:
            set_mode.assert_called_once_with(0, "all_stems")
        else:
            set_mode.assert_not_called()
        assert "All stems pending current cache" in render.descriptions
        assert "Stems saved; audio mode unconfirmed" in render.descriptions
        assert "Audio: ALL STEMS" not in render.descriptions
        assert "Audio: FULL MIX" not in render.descriptions
    assert available.call_args_list == [((0,), {})] * 4
    assert eligibility.call_args_list == [((0,), {})] * 4
    assert status.residency is None
    assert status.effective is None
    assert controller.project.model_dump() == before_project
    assert controller.session.model_dump() == before_session
    generation.assert_not_called()
