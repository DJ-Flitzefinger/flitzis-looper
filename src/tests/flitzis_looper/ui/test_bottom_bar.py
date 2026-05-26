from flitzis_looper.models import (
    STEM_COMPONENT_MASK,
    STEM_INSTRUMENTAL_PRESET_MASK,
    STEM_MASK_BASS,
    STEM_MASK_DRUMS,
    STEM_MASK_MELODY,
    STEM_MASK_VOCALS,
)
from flitzis_looper.ui.render.bottom_bar import (
    MASTER_VOLUME_WIDTH,
    stem_button_is_active,
    stem_button_learn_target_state,
    stem_button_solo_state,
    stem_button_target_state,
    stem_controls_accept_input,
    trigger_quantization_button_style,
)


def test_stem_preset_display_lights_only_preset_button() -> None:
    assert stem_button_is_active(
        "I",
        STEM_INSTRUMENTAL_PRESET_MASK,
        STEM_INSTRUMENTAL_PRESET_MASK,
        "instrumental",
    )
    assert not stem_button_is_active(
        "D",
        STEM_MASK_DRUMS,
        STEM_INSTRUMENTAL_PRESET_MASK,
        "instrumental",
    )

    assert stem_button_is_active("A", STEM_COMPONENT_MASK, STEM_COMPONENT_MASK, "all")
    assert not stem_button_is_active("V", STEM_MASK_VOCALS, STEM_COMPONENT_MASK, "all")


def test_trigger_quantization_button_uses_mode_colors() -> None:
    assert trigger_quantization_button_style(enabled=False) == "mode-off"
    assert trigger_quantization_button_style(enabled=True) == "mode-on"


def test_master_volume_slider_hit_target_is_wider() -> None:
    assert MASTER_VOLUME_WIDTH >= 300.0


def test_stem_custom_display_lights_component_buttons() -> None:
    assert stem_button_is_active("V", STEM_MASK_VOCALS, STEM_MASK_VOCALS, "custom")
    assert not stem_button_is_active("D", STEM_MASK_DRUMS, STEM_MASK_VOCALS, "custom")


def test_component_click_from_all_preset_starts_custom_single_component() -> None:
    target_mask, display_mode = stem_button_target_state(
        STEM_MASK_MELODY,
        STEM_COMPONENT_MASK,
        STEM_MASK_VOCALS | STEM_MASK_BASS,
        "all",
        "custom",
    )

    assert target_mask == STEM_MASK_MELODY
    assert display_mode == "custom"


def test_component_click_from_instrumental_preset_starts_custom_single_component() -> None:
    target_mask, display_mode = stem_button_target_state(
        STEM_MASK_VOCALS,
        STEM_INSTRUMENTAL_PRESET_MASK,
        STEM_MASK_DRUMS | STEM_MASK_MELODY,
        "instrumental",
        "custom",
    )

    assert target_mask == STEM_MASK_VOCALS
    assert display_mode == "custom"


def test_component_right_click_sets_non_momentary_solo() -> None:
    target_mask, display_mode = stem_button_solo_state(STEM_MASK_DRUMS)

    assert target_mask == STEM_MASK_DRUMS
    assert display_mode == "custom"


def test_pending_learn_input_allows_disabled_stem_buttons_as_targets() -> None:
    assert stem_controls_accept_input(enabled=False, learn_pending=True)
    assert not stem_controls_accept_input(enabled=False, learn_pending=False)


def test_stem_learn_target_uses_button_identity_not_current_toggle_state() -> None:
    target_mask, display_mode = stem_button_learn_target_state(STEM_COMPONENT_MASK, "all")

    assert target_mask == STEM_COMPONENT_MASK
    assert display_mode == "all"


def test_custom_all_components_does_not_light_all_preset() -> None:
    assert stem_button_is_active("V", STEM_MASK_VOCALS, STEM_COMPONENT_MASK, "custom")
    assert stem_button_is_active("D", STEM_MASK_DRUMS, STEM_COMPONENT_MASK, "custom")
    assert stem_button_is_active("M", STEM_MASK_MELODY, STEM_COMPONENT_MASK, "custom")
    assert stem_button_is_active("B", STEM_MASK_BASS, STEM_COMPONENT_MASK, "custom")
    assert not stem_button_is_active("A", STEM_COMPONENT_MASK, STEM_COMPONENT_MASK, "custom")


def test_custom_instrumental_equivalent_does_not_light_instrumental_preset() -> None:
    assert not stem_button_is_active(
        "V",
        STEM_MASK_VOCALS,
        STEM_INSTRUMENTAL_PRESET_MASK,
        "custom",
    )
    assert stem_button_is_active(
        "D",
        STEM_MASK_DRUMS,
        STEM_INSTRUMENTAL_PRESET_MASK,
        "custom",
    )
    assert stem_button_is_active(
        "M",
        STEM_MASK_MELODY,
        STEM_INSTRUMENTAL_PRESET_MASK,
        "custom",
    )
    assert stem_button_is_active(
        "B",
        STEM_MASK_BASS,
        STEM_INSTRUMENTAL_PRESET_MASK,
        "custom",
    )
    assert not stem_button_is_active(
        "I",
        STEM_INSTRUMENTAL_PRESET_MASK,
        STEM_INSTRUMENTAL_PRESET_MASK,
        "custom",
    )


def test_preset_click_from_custom_restores_preset_display_state() -> None:
    target_mask, display_mode = stem_button_target_state(
        STEM_INSTRUMENTAL_PRESET_MASK,
        STEM_MASK_VOCALS,
        STEM_MASK_VOCALS,
        "custom",
        "instrumental",
    )
    assert target_mask == STEM_INSTRUMENTAL_PRESET_MASK
    assert display_mode == "instrumental"

    target_mask, display_mode = stem_button_target_state(
        STEM_COMPONENT_MASK,
        STEM_MASK_MELODY,
        STEM_MASK_MELODY,
        "custom",
        "all",
    )
    assert target_mask == STEM_COMPONENT_MASK
    assert display_mode == "all"


def test_active_preset_click_restores_remembered_custom_components() -> None:
    remembered_mask = STEM_MASK_VOCALS | STEM_MASK_BASS

    target_mask, display_mode = stem_button_target_state(
        STEM_INSTRUMENTAL_PRESET_MASK,
        STEM_INSTRUMENTAL_PRESET_MASK,
        remembered_mask,
        "instrumental",
        "instrumental",
    )

    assert target_mask == remembered_mask
    assert display_mode == "custom"

    target_mask, display_mode = stem_button_target_state(
        STEM_COMPONENT_MASK,
        STEM_COMPONENT_MASK,
        remembered_mask,
        "all",
        "all",
    )

    assert target_mask == remembered_mask
    assert display_mode == "custom"


def test_switching_between_presets_does_not_restore_remembered_components() -> None:
    remembered_mask = STEM_MASK_DRUMS | STEM_MASK_MELODY

    target_mask, display_mode = stem_button_target_state(
        STEM_COMPONENT_MASK,
        STEM_INSTRUMENTAL_PRESET_MASK,
        remembered_mask,
        "instrumental",
        "all",
    )

    assert target_mask == STEM_COMPONENT_MASK
    assert display_mode == "all"

    target_mask, display_mode = stem_button_target_state(
        STEM_INSTRUMENTAL_PRESET_MASK,
        STEM_COMPONENT_MASK,
        remembered_mask,
        "all",
        "instrumental",
    )

    assert target_mask == STEM_INSTRUMENTAL_PRESET_MASK
    assert display_mode == "instrumental"
