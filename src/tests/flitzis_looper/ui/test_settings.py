from contextlib import nullcontext
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import icons_fontawesome_6, imgui

from flitzis_looper.models import (
    STEM_SEPARATOR_LABELS,
    TRIGGER_QUANTIZATION_STEP_LABELS,
    TRIGGER_QUANTIZATION_STEPS,
)
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import settings
from flitzis_looper.ui.render.bottom_bar import settings_button_local_pos
from flitzis_looper.ui.render.settings import (
    SETTINGS_TOGGLE_BUTTON_SIZE,
    settings_surface_child_id,
    settings_toggle_button_label,
    settings_toggle_tooltip,
)

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.models import StemSeparator


def test_settings_toggle_uses_gear_when_closed() -> None:
    assert settings_toggle_button_label(settings_open=False) == (
        f"{icons_fontawesome_6.ICON_FA_GEAR}##settings_toggle"
    )
    assert settings_toggle_tooltip(settings_open=False) == "Open settings"


def test_settings_toggle_uses_x_when_open() -> None:
    assert settings_toggle_button_label(settings_open=True) == (
        f"{icons_fontawesome_6.ICON_FA_XMARK}##settings_toggle"
    )
    assert settings_toggle_tooltip(settings_open=True) == "Close settings"


def test_settings_overlay_replaces_main_surface_id() -> None:
    assert settings_surface_child_id(settings_open=False) == "looper_main"
    assert settings_surface_child_id(settings_open=True) == "settings_overlay"


def test_settings_quantize_grid_options_cover_loop_editor_grid() -> None:
    assert TRIGGER_QUANTIZATION_STEPS == ("1_64", "1_32", "1_16")
    assert TRIGGER_QUANTIZATION_STEP_LABELS["1_16"] == "1/16"
    assert TRIGGER_QUANTIZATION_STEP_LABELS["1_64"] == "1/64"


def test_settings_button_position_right_aligns_inside_bottom_bar() -> None:
    x, y = settings_button_local_pos(
        cursor_x=0.0,
        cursor_y=0.0,
        available_width=1538.0,
        available_height=55.0,
    )

    assert x + SETTINGS_TOGGLE_BUTTON_SIZE == 1538.0
    assert y == 9.5


def test_settings_button_position_handles_tiny_bottom_bar() -> None:
    x, y = settings_button_local_pos(
        cursor_x=4.0,
        cursor_y=6.0,
        available_width=24.0,
        available_height=20.0,
    )

    assert x == 4.0
    assert y == 6.0


@pytest.mark.parametrize("choice", ["bs-roformer:musdb18hq", "demucs:htdemucs"])
def test_settings_separator_combo_switches_and_persists_only_future_job_choice(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, choice: StemSeparator
) -> None:
    controller.project.stem_separator = (
        "demucs:htdemucs" if choice == "bs-roformer:musdb18hq" else "bs-roformer:musdb18hq"
    )
    ctx = UiContext(controller)
    flush = Mock(return_value=True)
    quality = Mock()
    target_label = f"{STEM_SEPARATOR_LABELS[choice]}##stem_separator_{choice}"
    monkeypatch.setattr(settings, "item_width", lambda _width: nullcontext())
    monkeypatch.setattr(imgui, "begin_combo", Mock(return_value=True))
    monkeypatch.setattr(
        imgui, "selectable", lambda label, selected: (label == target_label, selected)
    )
    monkeypatch.setattr(imgui, "set_item_default_focus", Mock())
    monkeypatch.setattr(imgui, "end_combo", Mock())
    monkeypatch.setattr(imgui, "text_colored", Mock())
    monkeypatch.setattr(settings, "_demucs_quality_controls", quality)
    monkeypatch.setattr(ctx.persistence, "flush_if_dirty", flush)

    settings._stem_generation_controls(ctx)

    assert controller.project.stem_separator == choice
    flush.assert_called_once_with()
    assert controller.session.stem_generating_sample_ids == set()
    if choice == "demucs:htdemucs":
        quality.assert_called_once_with(ctx)
    else:
        quality.assert_not_called()


def test_settings_renders_exact_pcm_and_pinned_over_budget_without_process_ram_claim(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = UiContext(controller)
    controller.project.resident_pcm_budget_automatic = False
    audio_engine_mock.get_resident_pcm_budget_status.return_value = {
        "budget_mib": 512,
        "budget_bytes": 512 * 1048576,
        "held_unique_pcm_bytes": 600 * 1048576,
        "pending_resident_bytes": 0,
        "admitted_total_bytes": 600 * 1048576,
        "unique_backing_count": 1,
        "pending_reservation_count": 0,
        "over_budget": True,
        "available_bytes": 0,
    }
    text: list[str] = []
    _resident_controls_widgets(monkeypatch, text)
    settings._resident_pcm_controls(ctx)
    assert any("629145600 B" in message for message in text)
    assert any("Pinned readers temporarily exceed" in message for message in text)
    assert any("PCM counters are not process RSS" in message for message in text)
    assert any("separate limits" in message for message in text)


def test_settings_accounting_failure_never_renders_invented_zero_bytes(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = UiContext(controller)
    audio_engine_mock.get_resident_pcm_budget_status.return_value = {}
    text: list[str] = []
    _resident_controls_widgets(monkeypatch, text)
    settings._resident_pcm_controls(ctx)
    assert any("Resident accounting unavailable" in message for message in text)
    assert not any("Exact unique resident PCM" in message for message in text)


def _resident_controls_widgets(
    monkeypatch: pytest.MonkeyPatch, text: list[str]
) -> tuple[Mock, Mock, Mock]:
    checkbox = Mock(return_value=(False, False))
    slider = Mock(return_value=(False, 1))
    disabled = Mock()
    monkeypatch.setattr(settings, "item_width", lambda _width: nullcontext())
    monkeypatch.setattr(imgui, "checkbox", checkbox)
    monkeypatch.setattr(imgui, "slider_int", slider)
    monkeypatch.setattr(imgui, "begin_disabled", disabled)
    monkeypatch.setattr(imgui, "end_disabled", Mock())
    monkeypatch.setattr(imgui, "is_item_deactivated_after_edit", Mock(return_value=False))
    monkeypatch.setattr(imgui, "text_wrapped", text.append)
    return checkbox, slider, disabled


def test_automatic_admission_disables_manual_limit_and_shows_os_headroom_separately(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = UiContext(controller)
    native = audio_engine_mock.get_resident_pcm_budget_status.return_value
    native.update({
        "automatic": True,
        "budget_mib": 0,
        "budget_bytes": 0,
        "available_bytes": 0,
        "host_pending_allocation_bytes": 123,
        "host_available_physical_bytes": 987654321,
        "host_available_commit_bytes": 876543210,
        "host_allocation_allowance_bytes": 765432109,
        "host_memory_error": None,
    })
    text: list[str] = []
    _, slider, disabled = _resident_controls_widgets(monkeypatch, text)
    slider.return_value = (True, 12)
    audio_engine_mock.set_resident_pcm_budget_mib.reset_mock()

    settings._resident_pcm_controls(ctx)

    disabled.assert_called_once()
    assert disabled.call_args.args == (True,)
    assert controller.project.resident_pcm_budget_mib == 512
    audio_engine_mock.set_resident_pcm_budget_mib.assert_not_called()
    assert any("automatic; no fixed resident PCM limit" in message for message in text)
    assert any("physical 987654321 B; commit 876543210 B" in message for message in text)
    assert any("Outstanding host allocation reservations 123 B" in message for message in text)
    assert any("PCM counters are not process RSS" in message for message in text)
    assert not any("Effective manual resident PCM limit" in message for message in text)


def test_settings_toggle_uses_retained_manual_limit_and_flushes_after_native_acceptance(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = UiContext(controller)
    controller.project.resident_pcm_budget_mib = 2048
    checkbox, _, disabled = _resident_controls_widgets(monkeypatch, [])
    checkbox.side_effect = lambda label, _value: (label == "Automatic memory admission", False)
    audio_engine_mock.set_resident_pcm_budget_mib.reset_mock()
    flush = Mock(return_value=True)
    monkeypatch.setattr(ctx.persistence, "flush_if_dirty", flush)

    settings._resident_pcm_controls(ctx)

    disabled.assert_called_once()
    assert disabled.call_args.args == (False,)
    audio_engine_mock.set_resident_pcm_budget_mib.assert_called_once_with(2048)
    assert not controller.project.resident_pcm_budget_automatic
    flush.assert_called_once_with()


def test_settings_failed_mode_toggle_keeps_automatic_and_exposes_native_error(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = UiContext(controller)
    text: list[str] = []
    checkbox, _, disabled = _resident_controls_widgets(monkeypatch, text)
    checkbox.side_effect = lambda label, _value: (label == "Automatic memory admission", False)
    audio_engine_mock.set_resident_pcm_budget_mib.side_effect = RuntimeError("Native stopped")
    flush = Mock(return_value=True)
    monkeypatch.setattr(ctx.persistence, "flush_if_dirty", flush)

    settings._resident_pcm_controls(ctx)

    disabled.assert_called_once()
    assert disabled.call_args.args == (True,)
    assert controller.project.resident_pcm_budget_automatic
    assert controller.project.resident_pcm_budget_mib == 512
    assert controller.session.resident_pcm_settings_error == "Native stopped"
    assert "Native stopped" in text
    flush.assert_not_called()


def test_rendering_gib_slider_preserves_exact_legacy_manual_limit(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = UiContext(controller)
    controller.project.resident_pcm_budget_mib = 9925
    controller.project.resident_pcm_budget_automatic = False
    text: list[str] = []
    _, slider, disabled = _resident_controls_widgets(monkeypatch, text)
    slider.return_value = (False, 10)
    audio_engine_mock.set_resident_pcm_budget_mib.reset_mock()

    settings._resident_pcm_budget_controls(ctx)

    slider.assert_called_once_with("Choose manual limit (GiB)", 10, 1, 16)
    disabled.assert_called_once()
    assert disabled.call_args.args == (False,)
    assert controller.project.resident_pcm_budget_mib == 9925
    assert "Retained manual limit: 9.69 GiB (9925 MiB)." in text
    audio_engine_mock.set_resident_pcm_budget_mib.assert_not_called()


def test_gib_slider_gesture_applies_whole_gib_before_persisting(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = UiContext(controller)
    controller.project.resident_pcm_budget_mib = 9925
    controller.project.resident_pcm_budget_automatic = False
    text: list[str] = []
    _, slider, _ = _resident_controls_widgets(monkeypatch, text)
    slider.return_value = (True, 12)

    def apply_native(value: int) -> None:
        assert controller.project.resident_pcm_budget_mib == 9925
        assert value == 12288

    audio_engine_mock.set_resident_pcm_budget_mib.reset_mock()
    audio_engine_mock.set_resident_pcm_budget_mib.side_effect = apply_native

    settings._resident_pcm_budget_controls(ctx)

    audio_engine_mock.set_resident_pcm_budget_mib.assert_called_once_with(12288)
    assert controller.project.resident_pcm_budget_mib == 12288
    assert "Retained manual limit: 12.00 GiB (12288 MiB)." in text
