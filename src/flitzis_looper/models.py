from collections.abc import Iterable  # noqa: TC003
from typing import Annotated

from pydantic import AfterValidator, BaseModel, ConfigDict, Field, field_validator

from flitzis_looper.constants import (
    NUM_BANKS,
    NUM_SAMPLES,
    SPEED_MAX,
    SPEED_MIN,
    VOLUME_MAX,
    VOLUME_MIN,
)


def _default_sample_paths() -> list[str | None]:
    return [None] * NUM_SAMPLES


def validate_sample_id(sample_id: int) -> int:
    if not 0 <= sample_id < NUM_SAMPLES:
        msg = f"sample_id must be >= 0 and < {NUM_SAMPLES}, got {sample_id}"
        raise ValueError(msg)
    return sample_id


class BeatGrid(BaseModel):
    beats: list[float]
    downbeats: list[float]


class SampleAnalysis(BaseModel):
    bpm: float
    key: str
    beat_grid: BeatGrid


def _default_sample_analysis() -> list[SampleAnalysis | None]:
    return [None] * NUM_SAMPLES


def _default_manual_bpm() -> list[float | None]:
    return [None] * NUM_SAMPLES


def _default_manual_key() -> list[str | None]:
    return [None] * NUM_SAMPLES


class ProjectState(BaseModel):
    """Persistent state. Saved to disk."""

    model_config = ConfigDict(validate_assignment=True)

    sample_paths: list[str | None] = Field(default_factory=_default_sample_paths)
    """Maps pad IDs to file paths."""

    sample_analysis: list[SampleAnalysis | None] = Field(default_factory=_default_sample_analysis)
    """Per-pad audio analysis results (BPM/key/beat grid) or None when unknown."""

    manual_bpm: list[float | None] = Field(default_factory=_default_manual_bpm)
    """Optional per-pad BPM override. When set, used for effective BPM display."""

    manual_key: list[str | None] = Field(default_factory=_default_manual_key)
    """Optional per-pad key override. When set, used for effective key display."""

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

    loading_sample_ids: set[int] = Field(default_factory=set)
    """Pads that are currently being loaded asynchronously."""

    pending_sample_paths: dict[int, str] = Field(default_factory=dict)
    """Paths for pads with an in-flight async load."""

    sample_load_progress: dict[int, float] = Field(default_factory=dict)
    """Best-effort async load progress (0.0..=1.0)."""

    sample_load_stage: dict[int, str] = Field(default_factory=dict)
    """Human-readable async load stage per pad (e.g. "Loading (decoding)")."""

    sample_load_errors: dict[int, str] = Field(default_factory=dict)
    """Last async load error message per pad."""

    analyzing_sample_ids: set[int] = Field(default_factory=set)
    """Pads that are currently running audio analysis in the background."""

    sample_analysis_progress: dict[int, float] = Field(default_factory=dict)
    """Best-effort analysis progress (0.0..=1.0)."""

    sample_analysis_stage: dict[int, str] = Field(default_factory=dict)
    """Human-readable analysis stage per pad."""

    sample_analysis_errors: dict[int, str] = Field(default_factory=dict)
    """Last analysis error message per pad."""

    # UI State
    file_dialog_pad_id: int | None = None
    """Current file dialog target pad ID or None if no file dialog is open."""

    tap_bpm_pad_id: int | None = None
    """Current Tap BPM target pad. Resets tap timestamps when changed."""

    tap_bpm_timestamps: list[float] = Field(default_factory=list)
    """Recent Tap BPM timestamps in monotonic seconds."""

    bpm_lock_anchor_pad_id: int | None = None
    """Pad id used to anchor BPM lock master BPM calculation."""

    bpm_lock_anchor_bpm: float | None = None
    """Pad BPM value captured when BPM lock is enabled."""

    master_bpm: float | None = None
    """Current master BPM when BPM lock is enabled."""

    @field_validator(
        "active_sample_ids",
        "pressed_pads",
        "loading_sample_ids",
        "analyzing_sample_ids",
        mode="after",
    )
    @classmethod
    def _validate_sample_ids(cls, value: Iterable[int]) -> Iterable[int]:
        for sid in value:
            validate_sample_id(sid)
        return value

    @field_validator(
        "pending_sample_paths",
        "sample_load_progress",
        "sample_load_stage",
        "sample_load_errors",
        "sample_analysis_progress",
        "sample_analysis_stage",
        "sample_analysis_errors",
        mode="after",
    )
    @classmethod
    def _validate_sample_id_keys(cls, value: dict[int, object]) -> dict[int, object]:
        for sid in value:
            validate_sample_id(sid)
        return value

    @field_validator(
        "file_dialog_pad_id",
        "tap_bpm_pad_id",
        "bpm_lock_anchor_pad_id",
        mode="after",
    )
    @classmethod
    def _validate_optional_sample_id(cls, value: int | None) -> int | None:
        if value is not None:
            validate_sample_id(value)
        return value
