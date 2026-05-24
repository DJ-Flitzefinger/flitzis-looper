from flitzis_looper.models import (
    STEM_COMPONENT_MASK,
    STEM_INSTRUMENTAL_PRESET_MASK,
    STEM_MASK_DRUMS,
    STEM_MASK_VOCALS,
)
from flitzis_looper.ui.render.bottom_bar import stem_button_is_active


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


def test_stem_custom_display_lights_component_buttons() -> None:
    assert stem_button_is_active("V", STEM_MASK_VOCALS, STEM_MASK_VOCALS, "custom")
    assert not stem_button_is_active("D", STEM_MASK_DRUMS, STEM_MASK_VOCALS, "custom")
