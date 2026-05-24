import math
from typing import TYPE_CHECKING, Annotated, Literal

from pydantic import (
    AfterValidator,
    BaseModel,
    ConfigDict,
    Field,
    field_validator,
    model_validator,
)

from flitzis_looper.constants import (
    DEFAULT_DEMUCS_OVERLAP,
    DEFAULT_DEMUCS_SHIFTS,
    MAX_DEMUCS_OVERLAP,
    MAX_DEMUCS_SHIFTS,
    MIN_DEMUCS_OVERLAP,
    MIN_DEMUCS_SHIFTS,
    NUM_BANKS,
    NUM_SAMPLES,
    PAD_EQ_DB_MAX,
    PAD_EQ_DB_MIN,
    PAD_GAIN_MAX,
    PAD_GAIN_MIN,
    SPEED_MAX,
    SPEED_MIN,
    VOLUME_MAX,
    VOLUME_MIN,
)

if TYPE_CHECKING:
    from collections.abc import Iterable

type TriggerQuantizationMode = Literal[
    "immediate",
    "enabled",
    "next_beat",
    "next_bar",
    "1_16",
    "1_32",
    "1_64",
]
type TriggerQuantizationStep = Literal[
    "1_64",
    "1_32",
    "1_16",
]
type StemMixMode = Literal["full_mix", "all_stems"]
type StemMaskDisplayMode = Literal["custom", "instrumental", "all"]
type StemKind = Literal["vocals", "melody", "bass", "drums", "instrumental"]
type StemGridIndicatorState = Literal["available", "generating", "blocked", "error"]

TRIGGER_QUANTIZATION_STEPS: tuple[TriggerQuantizationStep, ...] = (
    "1_16",
    "1_32",
    "1_64",
)
TRIGGER_QUANTIZATION_STEP_LABELS: dict[TriggerQuantizationStep, str] = {
    "1_64": "1/64",
    "1_32": "1/32",
    "1_16": "1/16",
}
DEFAULT_TRIGGER_QUANTIZATION_STEP: TriggerQuantizationStep = "1_16"
LEGACY_TRIGGER_QUANTIZATION_TO_STEP: dict[str, TriggerQuantizationStep] = {
    "next_beat": "1_16",
    "next-beat": "1_16",
    "beat": "1_16",
    "next_bar": "1_16",
    "next-bar": "1_16",
    "bar": "1_16",
    "1_8": "1_16",
    "1/8": "1_16",
    "1_4": "1_16",
    "1/4": "1_16",
    "1_2": "1_16",
    "1/2": "1_16",
    "1_bar": "1_16",
    "1-bar": "1_16",
    "bar_1": "1_16",
}

STEM_KINDS: tuple[StemKind, ...] = ("vocals", "melody", "bass", "drums", "instrumental")
STEM_MIX_MODES: tuple[StemMixMode, ...] = ("full_mix", "all_stems")
STEM_MASK_DISPLAY_MODES: tuple[StemMaskDisplayMode, ...] = ("custom", "instrumental", "all")
STEM_MASK_VOCALS = 1 << 0
STEM_MASK_MELODY = 1 << 1
STEM_MASK_BASS = 1 << 2
STEM_MASK_DRUMS = 1 << 3
STEM_MASK_INSTRUMENTAL = 1 << 4
STEM_COMPONENT_MASK = STEM_MASK_VOCALS | STEM_MASK_MELODY | STEM_MASK_BASS | STEM_MASK_DRUMS
STEM_INSTRUMENTAL_PRESET_MASK = STEM_MASK_DRUMS | STEM_MASK_MELODY | STEM_MASK_BASS


def _default_sample_paths() -> list[str | None]:
    return [None] * NUM_SAMPLES


def _default_sample_durations() -> list[float | None]:
    return [None] * NUM_SAMPLES


def validate_sample_id(sample_id: int) -> int:
    if not 0 <= sample_id < NUM_SAMPLES:
        msg = f"sample_id must be >= 0 and < {NUM_SAMPLES}, got {sample_id}"
        raise ValueError(msg)
    return sample_id


class BeatGrid(BaseModel):
    beats: list[float]
    downbeats: list[float]
    bars: list[float]


class SampleAnalysis(BaseModel):
    bpm: float
    key: str
    beat_grid: BeatGrid


class StemFileSet(BaseModel):
    vocals: str | None = None
    melody: str | None = None
    bass: str | None = None
    drums: str | None = None
    instrumental: str | None = None

    def path_for(self, kind: StemKind) -> str | None:
        """Return the cache path for a known stem kind."""
        match kind:
            case "vocals":
                return self.vocals
            case "melody":
                return self.melody
            case "bass":
                return self.bass
            case "drums":
                return self.drums
            case "instrumental":
                return self.instrumental

    def with_kind(self, kind: StemKind, path: str | None) -> StemFileSet:
        """Return a copy with one stem path changed."""
        return self.model_copy(update={kind: path})


class StemCacheEntry(BaseModel):
    """Project-local cache metadata for one pad source version."""

    source_version: str = Field(min_length=1)
    """Deterministic project-local source version token."""

    cache_dir: str = Field(min_length=1)
    """Project-relative cache directory for this stem set."""

    stems: StemFileSet = Field(default_factory=StemFileSet)
    """Expected per-stem artifact paths."""

    available: bool = False
    """Whether all expected cache artifacts are present and eligible for playback."""


def _default_sample_analysis() -> list[SampleAnalysis | None]:
    return [None] * NUM_SAMPLES


def _default_stem_cache() -> list[StemCacheEntry | None]:
    return [None] * NUM_SAMPLES


def _default_pad_stem_mix_mode() -> list[StemMixMode]:
    return ["full_mix"] * NUM_SAMPLES


def _default_manual_bpm() -> list[float | None]:
    return [None] * NUM_SAMPLES


def _default_manual_key() -> list[str | None]:
    return [None] * NUM_SAMPLES


def _default_pad_gain() -> list[float]:
    return [1.0] * NUM_SAMPLES


def _default_pad_eq() -> list[float]:
    return [0.0] * NUM_SAMPLES


def _default_pad_loop_start_s() -> list[float]:
    return [0.0] * NUM_SAMPLES


def _default_pad_loop_end_s() -> list[float | None]:
    return [None] * NUM_SAMPLES


def _default_pad_loop_auto() -> list[bool]:
    return [False] * NUM_SAMPLES


def _default_pad_loop_bars() -> list[int]:
    return [4] * NUM_SAMPLES


def _default_pad_grid_offset_samples() -> list[int]:
    return [0] * NUM_SAMPLES


class ProjectState(BaseModel):
    """Persistent state. Saved to disk."""

    model_config = ConfigDict(validate_assignment=True)

    @model_validator(mode="before")
    @classmethod
    def _migrate_legacy_trigger_quantization(cls, value: object) -> object:
        if not isinstance(value, dict):
            return value

        data = dict(value)
        legacy = data.pop("trigger_quantization", None)
        if isinstance(legacy, str) and "trigger_quantization_enabled" not in data:
            data["trigger_quantization_enabled"] = legacy not in {
                "immediate",
                "disabled",
                "off",
            }

        if isinstance(legacy, str) and "trigger_quantization_step" not in data:
            step = LEGACY_TRIGGER_QUANTIZATION_TO_STEP.get(legacy)
            if step is not None:
                data["trigger_quantization_step"] = step
            elif data.get("trigger_quantization_enabled") is True:
                data["trigger_quantization_step"] = legacy

        if isinstance(data.get("trigger_quantization_step"), str):
            step = LEGACY_TRIGGER_QUANTIZATION_TO_STEP.get(data["trigger_quantization_step"])
            if step is not None:
                data["trigger_quantization_step"] = step

        return data

    sample_paths: list[str | None] = Field(default_factory=_default_sample_paths)
    """Maps pad IDs to file paths."""

    sample_durations: list[float | None] = Field(default_factory=_default_sample_durations)
    """Maps pad IDs to sample durations (in seconds)."""

    sample_analysis: list[SampleAnalysis | None] = Field(default_factory=_default_sample_analysis)
    """Per-pad audio analysis results (BPM/key/beat grid) or None when unknown."""

    stem_cache: list[StemCacheEntry | None] = Field(default_factory=_default_stem_cache)
    """Per-pad project-local stem cache metadata or None when unavailable."""

    pad_stem_mix_mode: list[StemMixMode] = Field(default_factory=_default_pad_stem_mix_mode)
    """Per-pad durable stem mix preference."""

    manual_bpm: list[float | None] = Field(default_factory=_default_manual_bpm)
    """Optional per-pad BPM override. When set, used for effective BPM display."""

    manual_key: list[str | None] = Field(default_factory=_default_manual_key)
    """Optional per-pad key override. When set, used for effective key display."""

    pad_gain: list[float] = Field(default_factory=_default_pad_gain)
    """Per-pad linear gain scalar (0.0..=1.0)."""

    pad_eq_low_db: list[float] = Field(default_factory=_default_pad_eq)
    """Per-pad EQ low band gain in dB."""

    pad_eq_mid_db: list[float] = Field(default_factory=_default_pad_eq)
    """Per-pad EQ mid band gain in dB."""

    pad_eq_high_db: list[float] = Field(default_factory=_default_pad_eq)
    """Per-pad EQ high band gain in dB."""

    pad_loop_start_s: list[float] = Field(default_factory=_default_pad_loop_start_s)
    """Per-pad loop region start time in seconds."""

    pad_loop_end_s: list[float | None] = Field(default_factory=_default_pad_loop_end_s)
    """Per-pad loop region end time in seconds, or None for full sample."""

    pad_loop_auto: list[bool] = Field(default_factory=_default_pad_loop_auto)
    """Per-pad auto-loop enabled state."""

    pad_loop_bars: list[int] = Field(default_factory=_default_pad_loop_bars)
    """Per-pad bar count used when auto-loop is enabled."""

    pad_grid_offset_samples: list[int] = Field(default_factory=_default_pad_grid_offset_samples)
    """Per-pad sample offset applied to the musical grid anchor."""

    # Global Audio Settings
    multi_loop: bool = False
    """Allow to play multiple loops at the same time."""
    key_lock: bool = False
    """Key lock state."""
    bpm_lock: bool = False
    """BPM lock state."""
    trigger_quantization_enabled: bool = False
    """Whether global pad trigger quantization is enabled."""
    trigger_quantization_step: TriggerQuantizationStep = DEFAULT_TRIGGER_QUANTIZATION_STEP
    """Global pad trigger quantization grid step."""
    input_mapping_enabled: bool = False
    """Enable performer MIDI/keyboard input mappings."""
    demucs_shifts: int = Field(
        default=DEFAULT_DEMUCS_SHIFTS,
        ge=MIN_DEMUCS_SHIFTS,
        le=MAX_DEMUCS_SHIFTS,
    )
    """Global Demucs stem-generation shifts setting."""
    demucs_overlap: float = Field(
        default=DEFAULT_DEMUCS_OVERLAP,
        ge=MIN_DEMUCS_OVERLAP,
        le=MAX_DEMUCS_OVERLAP,
    )
    """Global Demucs stem-generation overlap setting."""
    volume: float = Field(default=1.0, ge=VOLUME_MIN, le=VOLUME_MAX)
    """Global volume."""
    speed: float = Field(default=1.0, ge=SPEED_MIN, le=SPEED_MAX)
    """Global speed."""

    # UI State
    selected_pad: Annotated[int, AfterValidator(validate_sample_id)] = 0
    """The currently selected pad ID."""
    selected_bank: int = Field(default=0, ge=0, lt=NUM_BANKS)
    """Currently selected pad bank."""
    sidebar_left_expanded: bool = True
    """Left sidebar expanded/collapsed state."""
    sidebar_right_expanded: bool = True
    """Right sidebar expanded/collapsed state."""

    @field_validator("pad_gain", mode="after")
    @classmethod
    def _validate_pad_gain(cls, value: list[float]) -> list[float]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_gain must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        for gain in value:
            if not PAD_GAIN_MIN <= gain <= PAD_GAIN_MAX:
                msg = f"pad_gain values must be in {PAD_GAIN_MIN}..={PAD_GAIN_MAX}, got {gain}"
                raise ValueError(msg)
        return value

    @field_validator(
        "pad_eq_low_db",
        "pad_eq_mid_db",
        "pad_eq_high_db",
        mode="after",
    )
    @classmethod
    def _validate_pad_eq(cls, value: list[float]) -> list[float]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad EQ arrays must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        for db in value:
            if not PAD_EQ_DB_MIN <= db <= PAD_EQ_DB_MAX:
                msg = f"pad EQ dB values must be in {PAD_EQ_DB_MIN}..={PAD_EQ_DB_MAX}, got {db}"
                raise ValueError(msg)
        return value

    @field_validator("pad_loop_start_s", mode="after")
    @classmethod
    def _validate_pad_loop_start_s(cls, value: list[float]) -> list[float]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_loop_start_s must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        for start_s in value:
            if not math.isfinite(start_s) or start_s < 0.0:
                msg = f"pad_loop_start_s values must be finite and >= 0.0, got {start_s}"
                raise ValueError(msg)
        return value

    @field_validator("pad_loop_end_s", mode="after")
    @classmethod
    def _validate_pad_loop_end_s(cls, value: list[float | None]) -> list[float | None]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_loop_end_s must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        for end_s in value:
            if end_s is None:
                continue
            if not math.isfinite(end_s) or end_s < 0.0:
                msg = f"pad_loop_end_s values must be None or finite and >= 0.0, got {end_s}"
                raise ValueError(msg)
        return value

    @field_validator("pad_loop_auto", mode="after")
    @classmethod
    def _validate_pad_loop_auto(cls, value: list[bool]) -> list[bool]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_loop_auto must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        return value

    @field_validator("pad_loop_bars", mode="after")
    @classmethod
    def _validate_pad_loop_bars(cls, value: list[int]) -> list[int]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_loop_bars must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        for bars in value:
            if bars < 1:
                msg = f"pad_loop_bars values must be >= 1, got {bars}"
                raise ValueError(msg)
        return value

    @field_validator("stem_cache", mode="after")
    @classmethod
    def _validate_stem_cache(
        cls, value: list[StemCacheEntry | None]
    ) -> list[StemCacheEntry | None]:
        if len(value) != NUM_SAMPLES:
            msg = f"stem_cache must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        return value

    @field_validator("pad_stem_mix_mode", mode="after")
    @classmethod
    def _validate_pad_stem_mix_mode(cls, value: list[StemMixMode]) -> list[StemMixMode]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_stem_mix_mode must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        return value

    @field_validator("demucs_overlap", mode="after")
    @classmethod
    def _validate_demucs_overlap(cls, value: float) -> float:
        if not math.isfinite(value):
            msg = "demucs_overlap must be finite"
            raise ValueError(msg)
        return value

    @field_validator("pad_grid_offset_samples", mode="after")
    @classmethod
    def _validate_pad_grid_offset_samples(cls, value: list[int]) -> list[int]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_grid_offset_samples must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        return value


def _default_pad_playhead_s() -> list[float | None]:
    return [None] * NUM_SAMPLES


def _default_pad_stem_enabled_mask() -> list[int]:
    return [STEM_COMPONENT_MASK] * NUM_SAMPLES


def _default_pad_stem_last_custom_mask() -> list[int]:
    return [STEM_COMPONENT_MASK] * NUM_SAMPLES


def _default_pad_stem_mask_display_mode() -> list[StemMaskDisplayMode]:
    return ["all"] * NUM_SAMPLES


class SessionState(BaseModel):
    """Runtime/UI state. Recreated on app launch."""

    model_config = ConfigDict(validate_assignment=True)

    # Audio Runtime
    active_sample_ids: set[int] = Field(default_factory=set)
    """Pads that are currently active (playing)."""

    paused_sample_ids: set[int] = Field(default_factory=set)
    """Pads that are currently paused (active but temporarily silenced)."""

    pressed_pads: list[bool] = Field(default_factory=lambda: [False] * NUM_SAMPLES)
    """Currently pressed pads."""

    pad_peak: list[float] = Field(default_factory=lambda: [0.0] * NUM_SAMPLES)
    """Best-effort per-pad peak level (0.0..=1.0)."""

    pad_peak_updated_at: list[float] = Field(default_factory=lambda: [0.0] * NUM_SAMPLES)
    """Monotonic timestamp of last pad peak update (seconds)."""

    pad_playhead_s: list[float | None] = Field(default_factory=_default_pad_playhead_s)
    """Best-effort per-pad playback position in seconds."""

    pad_playhead_updated_at: list[float] = Field(default_factory=lambda: [0.0] * NUM_SAMPLES)
    """Monotonic timestamp of last playhead update (seconds)."""

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

    stem_generating_sample_ids: set[int] = Field(default_factory=set)
    """Pads that are currently running offline stem generation in the background."""

    stem_generation_source_versions: dict[int, str] = Field(default_factory=dict)
    """Source-version token captured when each stem generation task was scheduled."""

    stem_generation_progress: dict[int, float] = Field(default_factory=dict)
    """Best-effort stem generation progress (0.0..=1.0)."""

    stem_generation_stage: dict[int, str] = Field(default_factory=dict)
    """Human-readable stem generation stage per pad."""

    stem_generation_errors: dict[int, str] = Field(default_factory=dict)
    """Last stem generation error message per pad."""

    stem_generation_diagnostics: dict[int, str] = Field(default_factory=dict)
    """Last non-error stem generation diagnostic per pad."""

    pad_stem_enabled_mask: list[int] = Field(default_factory=_default_pad_stem_enabled_mask)
    """Session-only per-pad component-stem mask used when all-stems mode is active."""

    pad_stem_last_custom_mask: list[int] = Field(
        default_factory=_default_pad_stem_last_custom_mask
    )
    """Session-only remembered component mask restored when a stem preset is toggled off."""

    pad_stem_mask_display_mode: list[StemMaskDisplayMode] = Field(
        default_factory=_default_pad_stem_mask_display_mode
    )
    """Session-only display mode for bottom-bar stem mask buttons."""

    # UI State
    file_dialog_pad_id: int | None = None
    """Current file dialog target pad ID or None if no file dialog is open."""

    waveform_editor_open: bool = False
    """Whether the waveform editor window is open."""

    waveform_editor_pad_id: int | None = None
    """Pad id currently being edited in the waveform editor."""

    settings_open: bool = False
    """Whether the Settings overlay is open."""

    input_learn_active: bool = False
    """Whether input mapping Learn mode is waiting for an input/action."""

    input_learn_pending_source: str | None = None
    """Pending Learn input source (`midi` or `keyboard`) captured before action selection."""

    input_learn_pending_binding_key: str | None = None
    """Stable pending input binding key captured by Learn mode."""

    input_mapping_error: str | None = None
    """Last non-fatal input mapping runtime error for UI diagnostics."""

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

    @field_validator("pad_peak", mode="after")
    @classmethod
    def _validate_pad_peak(cls, value: list[float]) -> list[float]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_peak must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        for peak in value:
            if not 0.0 <= peak <= 1.0:
                msg = f"pad_peak values must be in 0.0..=1.0, got {peak}"
                raise ValueError(msg)
        return value

    @field_validator("pad_peak_updated_at", mode="after")
    @classmethod
    def _validate_pad_peak_updated_at(cls, value: list[float]) -> list[float]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_peak_updated_at must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        for ts in value:
            if not math.isfinite(ts) or ts < 0.0:
                msg = f"pad_peak_updated_at values must be finite and >= 0.0, got {ts}"
                raise ValueError(msg)
        return value

    @field_validator("pad_playhead_s", mode="after")
    @classmethod
    def _validate_pad_playhead_s(cls, value: list[float | None]) -> list[float | None]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_playhead_s must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        for pos_s in value:
            if pos_s is None:
                continue
            if not math.isfinite(pos_s) or pos_s < 0.0:
                msg = f"pad_playhead_s values must be None or finite and >= 0.0, got {pos_s}"
                raise ValueError(msg)
        return value

    @field_validator("pad_playhead_updated_at", mode="after")
    @classmethod
    def _validate_pad_playhead_updated_at(cls, value: list[float]) -> list[float]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_playhead_updated_at must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        for ts in value:
            if not math.isfinite(ts) or ts < 0.0:
                msg = f"pad_playhead_updated_at values must be finite and >= 0.0, got {ts}"
                raise ValueError(msg)
        return value

    @field_validator(
        "active_sample_ids",
        "paused_sample_ids",
        "pressed_pads",
        "loading_sample_ids",
        "analyzing_sample_ids",
        "stem_generating_sample_ids",
        mode="after",
    )
    @classmethod
    def _validate_sample_ids(cls, value: Iterable[int]) -> Iterable[int]:
        for sid in value:
            validate_sample_id(sid)
        return value

    @field_validator("pad_stem_enabled_mask", "pad_stem_last_custom_mask", mode="after")
    @classmethod
    def _validate_pad_stem_enabled_mask(cls, value: list[int]) -> list[int]:
        if len(value) != NUM_SAMPLES:
            msg = f"stem mask arrays must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        for mask in value:
            if mask < 0 or mask & ~STEM_COMPONENT_MASK:
                msg = f"stem mask values must be component masks, got {mask}"
                raise ValueError(msg)
        return value

    @field_validator("pad_stem_mask_display_mode", mode="after")
    @classmethod
    def _validate_pad_stem_mask_display_mode(
        cls, value: list[StemMaskDisplayMode]
    ) -> list[StemMaskDisplayMode]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_stem_mask_display_mode must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        return value

    @field_validator(
        "pending_sample_paths",
        "sample_load_progress",
        "sample_load_stage",
        "sample_load_errors",
        "sample_analysis_progress",
        "sample_analysis_stage",
        "sample_analysis_errors",
        "stem_generation_source_versions",
        "stem_generation_progress",
        "stem_generation_stage",
        "stem_generation_errors",
        "stem_generation_diagnostics",
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
        "waveform_editor_pad_id",
        mode="after",
    )
    @classmethod
    def _validate_optional_sample_id(cls, value: int | None) -> int | None:
        if value is not None:
            validate_sample_id(value)
        return value
