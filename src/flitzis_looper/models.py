from typing import TYPE_CHECKING, Annotated

from pydantic import AfterValidator, BaseModel, ConfigDict, Field, field_validator

from flitzis_looper.constants import (
    NUM_BANKS,
    NUM_SAMPLES,
    SPEED_MAX,
    SPEED_MIN,
    VOLUME_MAX,
    VOLUME_MIN,
)

if TYPE_CHECKING:
    from collections.abc import Iterable


def _default_sample_paths() -> list[str | None]:
    return [None] * NUM_SAMPLES


def validate_sample_id(sample_id: int) -> int:
    if not 0 <= sample_id < NUM_SAMPLES:
        msg = f"sample_id must be >= 0 and < {NUM_SAMPLES}, got {sample_id}"
        raise ValueError(msg)
    return sample_id


class ProjectState(BaseModel):
    """Persistent state. Saved to disk."""

    model_config = ConfigDict(validate_assignment=True)

    sample_paths: list[str | None] = Field(default_factory=_default_sample_paths)
    """Maps pad IDs to file paths."""

    # Global Audio Settings
    multi_loop: bool = False
    """Allow to play multiple loops at the same time."""
    key_lock: bool = False
    """Key lock state."""
    bpm_lock: bool = False
    """BPM lock state."""
    volume: float = Field(default=1.0, ge=VOLUME_MIN, le=VOLUME_MAX)
    """Global volume."""
    speed: float = Field(default=1.0, ge=SPEED_MIN, le=SPEED_MAX)
    """Global speed."""

    # UI State
    selected_pad: Annotated[int, AfterValidator(validate_sample_id)] = 0
    """The currently selected pad ID."""
    selected_bank: int = Field(default=0, ge=0, lt=NUM_BANKS)
    """Currently selected pad bank."""
    sidebar_left_expanded: bool = False
    """Right sidebar expaneded/collapsed state."""
    sidebar_right_expanded: bool = True
    """Right sidebar expanded/collapsed state."""


class SessionState(BaseModel):
    """Runtime/UI state. Recreated on app launch."""

    model_config = ConfigDict(validate_assignment=True)

    # Audio Runtime
    active_sample_ids: set[int] = Field(default_factory=set)
    """Pads that are currently active (playing)."""
    pressed_pads: list[bool] = Field(default_factory=lambda: [False] * NUM_SAMPLES)
    """Currently pressed pads."""

    # UI State
    file_dialog_pad_id: int | None = None
    """Current file dialog target pad ID or None if no file dialog is open."""

    @field_validator("active_sample_ids", "pressed_pads", mode="after")
    @classmethod
    def _validate_sample_ids(cls, value: Iterable[int]) -> Iterable[int]:
        for sid in value:
            validate_sample_id(sid)
        return value

    @field_validator("file_dialog_pad_id", mode="after")
    @classmethod
    def _validate_file_dialog_pad_id(cls, value: int | None) -> int | None:
        if value is not None:
            validate_sample_id(value)
        return value
