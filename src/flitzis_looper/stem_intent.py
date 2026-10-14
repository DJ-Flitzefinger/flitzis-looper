"""Pure durable component-stem selection, independent of playback eligibility."""

from typing import Literal, Self

from pydantic import BaseModel, ConfigDict, Field, model_validator

type StemMaskDisplayMode = Literal["custom", "instrumental", "all"]

STEM_MASK_VOCALS = 1 << 0
STEM_MASK_MELODY = 1 << 1
STEM_MASK_BASS = 1 << 2
STEM_MASK_DRUMS = 1 << 3
STEM_MASK_INSTRUMENTAL = 1 << 4
STEM_COMPONENT_MASK = STEM_MASK_VOCALS | STEM_MASK_MELODY | STEM_MASK_BASS | STEM_MASK_DRUMS
STEM_INSTRUMENTAL_PRESET_MASK = STEM_MASK_DRUMS | STEM_MASK_MELODY | STEM_MASK_BASS


class PadStemIntent(BaseModel):
    """Immutable component selection and remembered custom selection for content."""

    model_config = ConfigDict(frozen=True, extra="forbid", revalidate_instances="always")

    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    enabled_mask: int = Field(
        default=STEM_COMPONENT_MASK, strict=True, ge=0, le=STEM_COMPONENT_MASK
    )
    last_custom_mask: int = Field(
        default=STEM_COMPONENT_MASK, strict=True, ge=0, le=STEM_COMPONENT_MASK
    )
    display_mode: StemMaskDisplayMode = "all"

    @model_validator(mode="after")
    def validate_preset_mask(self) -> Self:
        """Require a displayed preset to select its corresponding components."""
        if (
            self.display_mode == "instrumental"
            and self.enabled_mask != STEM_INSTRUMENTAL_PRESET_MASK
        ):
            message = "instrumental stem display mode requires the instrumental component mask"
            raise ValueError(message)
        if self.display_mode == "all" and self.enabled_mask != STEM_COMPONENT_MASK:
            message = "all stem display mode requires all component stems"
            raise ValueError(message)
        return self


def transition_stem_intent(
    current: PadStemIntent,
    enabled_mask: int,
    display_mode: StemMaskDisplayMode = "custom",
) -> PadStemIntent:
    """Validate a new selection while retaining the existing preset-toggle behavior.

    Args:
        current: Existing durable selection, revalidated before use.
        enabled_mask: Requested four-component selection; zero mutes every component.
        display_mode: Requested custom or preset display mode.

    Returns:
        An independent validated intent with the remembered custom selection updated.
    """
    validated = PadStemIntent.model_validate(current)
    remembered = validated.last_custom_mask
    if display_mode == "custom":
        remembered = enabled_mask
    elif validated.display_mode == "custom":
        remembered = validated.enabled_mask
    return PadStemIntent(
        schema_version=validated.schema_version,
        enabled_mask=enabled_mask,
        last_custom_mask=remembered,
        display_mode=display_mode,
    )
