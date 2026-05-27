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

from flitzis_looper.audio_gain import legacy_gain_value_to_db
from flitzis_looper.constants import (
    DEFAULT_DEMUCS_OVERLAP,
    DEFAULT_DEMUCS_SHIFTS,
    DEFAULT_KEY_LOCK_DELAY_MIN_SAMPLES,
    DEFAULT_KEY_LOCK_DELAY_RANGE_SAMPLES,
    DEFAULT_KEY_LOCK_HEAD_COUNT,
    DEFAULT_KEY_LOCK_INTERPOLATION,
    DEFAULT_KEY_LOCK_OUTPUT_GAIN,
    DEFAULT_KEY_LOCK_QUALITY,
    DEFAULT_KEY_LOCK_SMOOTHING_STEP,
    DEFAULT_KEY_LOCK_WINDOW,
    MAX_DEMUCS_OVERLAP,
    MAX_DEMUCS_SHIFTS,
    MAX_KEY_LOCK_DELAY_MIN_SAMPLES,
    MAX_KEY_LOCK_DELAY_RANGE_SAMPLES,
    MAX_KEY_LOCK_DELAY_TOTAL_SAMPLES,
    MAX_KEY_LOCK_HEAD_COUNT,
    MAX_KEY_LOCK_OUTPUT_GAIN,
    MAX_KEY_LOCK_SMOOTHING_STEP,
    MIN_DEMUCS_OVERLAP,
    MIN_DEMUCS_SHIFTS,
    MIN_KEY_LOCK_DELAY_MIN_SAMPLES,
    MIN_KEY_LOCK_DELAY_RANGE_SAMPLES,
    MIN_KEY_LOCK_HEAD_COUNT,
    MIN_KEY_LOCK_OUTPUT_GAIN,
    MIN_KEY_LOCK_SMOOTHING_STEP,
    NUM_BANKS,
    NUM_SAMPLES,
    PAD_EQ_DB_MAX,
    PAD_EQ_DB_MIN,
    PAD_GAIN_DB_DEFAULT,
    PAD_GAIN_DB_MAX,
    PAD_GAIN_DB_MIN,
    PAD_LOOP_BARS_DEFAULT,
    PAD_LOOP_BARS_GRANULARITY,
    PAD_LOOP_BARS_MIN,
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
type KeyLockQuality = Literal["performance", "balanced", "high", "very_high"]
type KeyLockInterpolation = Literal["linear", "cubic"]
type KeyLockWindow = Literal["triangle", "hann"]

TRIGGER_QUANTIZATION_STEPS: tuple[TriggerQuantizationStep, ...] = (
    "1_64",
    "1_32",
    "1_16",
)
TRIGGER_QUANTIZATION_STEP_LABELS: dict[TriggerQuantizationStep, str] = {
    "1_64": "1/64",
    "1_32": "1/32",
    "1_16": "1/16",
}
DEFAULT_TRIGGER_QUANTIZATION_STEP: TriggerQuantizationStep = "1_64"
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
KEY_LOCK_QUALITIES: tuple[KeyLockQuality, ...] = (
    "performance",
    "balanced",
    "high",
    "very_high",
)
KEY_LOCK_QUALITY_LABELS: dict[KeyLockQuality, str] = {
    "performance": "Performance",
    "balanced": "Balanced",
    "high": "High",
    "very_high": "Very High",
}
KEY_LOCK_INTERPOLATIONS: tuple[KeyLockInterpolation, ...] = ("linear", "cubic")
KEY_LOCK_INTERPOLATION_LABELS: dict[KeyLockInterpolation, str] = {
    "linear": "Linear",
    "cubic": "Cubic",
}
KEY_LOCK_WINDOWS: tuple[KeyLockWindow, ...] = ("triangle", "hann")
KEY_LOCK_WINDOW_LABELS: dict[KeyLockWindow, str] = {
    "triangle": "Triangle",
    "hann": "Hann",
}
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


def _default_pad_gain_db() -> list[float]:
    return [PAD_GAIN_DB_DEFAULT] * NUM_SAMPLES


def _default_pad_eq() -> list[float]:
    return [0.0] * NUM_SAMPLES


def _default_pad_loop_start_s() -> list[float]:
    return [0.0] * NUM_SAMPLES


def _default_pad_loop_end_s() -> list[float | None]:
    return [None] * NUM_SAMPLES


def _default_pad_loop_auto() -> list[bool]:
    return [False] * NUM_SAMPLES


def _default_pad_loop_bars() -> list[float]:
    return [PAD_LOOP_BARS_DEFAULT] * NUM_SAMPLES


def _default_pad_grid_offset_samples() -> list[int]:
    return [0] * NUM_SAMPLES


def _migrate_legacy_pad_gain_field(data: dict[str, object]) -> None:
    if "pad_gain_db" not in data and "pad_gain" in data:
        legacy_pad_gain = data.pop("pad_gain")
        if isinstance(legacy_pad_gain, list):
            data["pad_gain_db"] = [legacy_gain_value_to_db(gain) for gain in legacy_pad_gain]
    else:
        data.pop("pad_gain", None)


def _migrate_legacy_trigger_quantization_fields(data: dict[str, object]) -> None:
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

    trigger_step = data.get("trigger_quantization_step")
    if isinstance(trigger_step, str):
        step = LEGACY_TRIGGER_QUANTIZATION_TO_STEP.get(trigger_step)
        if step is not None:
            data["trigger_quantization_step"] = step


def _migrate_legacy_key_lock_quality_fields(data: dict[str, object]) -> None:
    key_lock_parameter_fields = {
        "key_lock_delay_min_samples",
        "key_lock_delay_range_samples",
        "key_lock_head_count",
        "key_lock_interpolation",
        "key_lock_window",
        "key_lock_smoothing_step",
        "key_lock_output_gain",
    }
    legacy_quality = data.get("key_lock_quality")
    if not isinstance(legacy_quality, str) or key_lock_parameter_fields.intersection(data):
        return

    legacy_presets: dict[str, dict[str, object]] = {
        "performance": {
            "key_lock_delay_min_samples": 48.0,
            "key_lock_delay_range_samples": 1024.0,
            "key_lock_head_count": 2,
            "key_lock_interpolation": "linear",
            "key_lock_window": "triangle",
            "key_lock_smoothing_step": 0.08,
            "key_lock_output_gain": 1.0,
        },
        "balanced": {
            "key_lock_delay_min_samples": 64.0,
            "key_lock_delay_range_samples": 1280.0,
            "key_lock_head_count": 2,
            "key_lock_interpolation": "linear",
            "key_lock_window": "hann",
            "key_lock_smoothing_step": 0.06,
            "key_lock_output_gain": 1.0,
        },
        "high": {
            "key_lock_delay_min_samples": DEFAULT_KEY_LOCK_DELAY_MIN_SAMPLES,
            "key_lock_delay_range_samples": DEFAULT_KEY_LOCK_DELAY_RANGE_SAMPLES,
            "key_lock_head_count": DEFAULT_KEY_LOCK_HEAD_COUNT,
            "key_lock_interpolation": DEFAULT_KEY_LOCK_INTERPOLATION,
            "key_lock_window": DEFAULT_KEY_LOCK_WINDOW,
            "key_lock_smoothing_step": DEFAULT_KEY_LOCK_SMOOTHING_STEP,
            "key_lock_output_gain": DEFAULT_KEY_LOCK_OUTPUT_GAIN,
        },
        "very_high": {
            "key_lock_delay_min_samples": 96.0,
            "key_lock_delay_range_samples": 1792.0,
            "key_lock_head_count": 4,
            "key_lock_interpolation": "cubic",
            "key_lock_window": "hann",
            "key_lock_smoothing_step": 0.035,
            "key_lock_output_gain": 1.0,
        },
    }
    legacy_settings = legacy_presets.get(legacy_quality)
    if legacy_settings is not None:
        data.update(legacy_settings)


class ProjectState(BaseModel):
    """Persistent state. Saved to disk."""

    model_config = ConfigDict(validate_assignment=True)

    @model_validator(mode="before")
    @classmethod
    def _migrate_legacy_project_fields(cls, value: object) -> object:
        if not isinstance(value, dict):
            return value

        data = dict(value)

        _migrate_legacy_pad_gain_field(data)
        _migrate_legacy_trigger_quantization_fields(data)
        _migrate_legacy_key_lock_quality_fields(data)

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

    pad_gain_db: list[float] = Field(default_factory=_default_pad_gain_db)
    """Per-pad Gain/Trim in dB."""

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

    pad_loop_bars: list[float] = Field(default_factory=_default_pad_loop_bars)
    """Per-pad bar count used when auto-loop is enabled."""

    pad_grid_offset_samples: list[int] = Field(default_factory=_default_pad_grid_offset_samples)
    """Per-pad sample offset applied to the musical grid anchor."""

    # Global Audio Settings
    multi_loop: bool = False
    """Allow to play multiple loops at the same time."""
    key_lock: bool = False
    """Key lock state."""
    key_lock_quality: KeyLockQuality = DEFAULT_KEY_LOCK_QUALITY
    """Legacy global Key Lock DSP quality preset."""
    key_lock_delay_min_samples: float = Field(
        default=DEFAULT_KEY_LOCK_DELAY_MIN_SAMPLES,
        ge=MIN_KEY_LOCK_DELAY_MIN_SAMPLES,
        le=MAX_KEY_LOCK_DELAY_MIN_SAMPLES,
    )
    """Minimum delay used by the Key Lock pitch-compensation delay heads."""
    key_lock_delay_range_samples: float = Field(
        default=DEFAULT_KEY_LOCK_DELAY_RANGE_SAMPLES,
        ge=MIN_KEY_LOCK_DELAY_RANGE_SAMPLES,
        le=MAX_KEY_LOCK_DELAY_RANGE_SAMPLES,
    )
    """Delay modulation range used by the Key Lock pitch-compensation delay heads."""
    key_lock_head_count: int = Field(
        default=DEFAULT_KEY_LOCK_HEAD_COUNT,
        ge=MIN_KEY_LOCK_HEAD_COUNT,
        le=MAX_KEY_LOCK_HEAD_COUNT,
    )
    """Number of Key Lock delay heads mixed per channel."""
    key_lock_interpolation: KeyLockInterpolation = DEFAULT_KEY_LOCK_INTERPOLATION
    """Delay-line interpolation used by Key Lock."""
    key_lock_window: KeyLockWindow = DEFAULT_KEY_LOCK_WINDOW
    """Crossfade window used by Key Lock delay heads."""
    key_lock_smoothing_step: float = Field(
        default=DEFAULT_KEY_LOCK_SMOOTHING_STEP,
        ge=MIN_KEY_LOCK_SMOOTHING_STEP,
        le=MAX_KEY_LOCK_SMOOTHING_STEP,
    )
    """Maximum per-callback tempo-ratio smoothing step used by Key Lock."""
    key_lock_output_gain: float = Field(
        default=DEFAULT_KEY_LOCK_OUTPUT_GAIN,
        ge=MIN_KEY_LOCK_OUTPUT_GAIN,
        le=MAX_KEY_LOCK_OUTPUT_GAIN,
    )
    """Linear output gain applied after Key Lock pitch compensation."""
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

    @field_validator("pad_gain_db", mode="after")
    @classmethod
    def _validate_pad_gain_db(cls, value: list[float]) -> list[float]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_gain_db must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        for gain_db in value:
            if not math.isfinite(gain_db) or not PAD_GAIN_DB_MIN <= gain_db <= PAD_GAIN_DB_MAX:
                msg = (
                    f"pad_gain_db values must be finite and in "
                    f"{PAD_GAIN_DB_MIN}..={PAD_GAIN_DB_MAX}, got {gain_db}"
                )
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
    def _validate_pad_loop_bars(cls, value: list[float]) -> list[float]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_loop_bars must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        normalized: list[float] = []
        for bars in value:
            if not math.isfinite(bars) or bars < PAD_LOOP_BARS_MIN:
                msg = f"pad_loop_bars values must be finite and >= {PAD_LOOP_BARS_MIN}, got {bars}"
                raise ValueError(msg)
            steps = bars / PAD_LOOP_BARS_GRANULARITY
            if not math.isclose(steps, round(steps), abs_tol=1e-9):
                msg = (
                    "pad_loop_bars values must use "
                    f"{PAD_LOOP_BARS_GRANULARITY}-bar granularity, got {bars}"
                )
                raise ValueError(msg)
            normalized.append(float(round(steps) * PAD_LOOP_BARS_GRANULARITY))
        return normalized

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

    @field_validator(
        "key_lock_delay_min_samples",
        "key_lock_delay_range_samples",
        "key_lock_smoothing_step",
        "key_lock_output_gain",
        mode="after",
    )
    @classmethod
    def _validate_key_lock_finite_float(cls, value: float) -> float:
        if not math.isfinite(value):
            msg = "key lock numeric settings must be finite"
            raise ValueError(msg)
        return value

    @model_validator(mode="after")
    def _validate_key_lock_delay_total(self) -> ProjectState:
        if (
            self.key_lock_delay_min_samples + self.key_lock_delay_range_samples
            > MAX_KEY_LOCK_DELAY_TOTAL_SAMPLES
        ):
            msg = (
                "key_lock_delay_min_samples + key_lock_delay_range_samples "
                f"must be <= {MAX_KEY_LOCK_DELAY_TOTAL_SAMPLES}"
            )
            raise ValueError(msg)
        return self

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

    global_stop_engaged: bool = False
    """Whether the bottom-bar START/STOP control has stopped a remembered active set."""

    global_stop_restore_sample_ids: set[int] = Field(default_factory=set)
    """Session-only pad ids remembered by the bottom-bar START/STOP control."""

    global_stop_momentary_mute_active: bool = False
    """Whether the START/STOP control's middle-hold output mute is currently active."""

    global_start_stop_left_pressed: bool = False
    """Whether START/STOP has already handled the current left mouse hold."""

    pressed_pads: list[bool] = Field(default_factory=lambda: [False] * NUM_SAMPLES)
    """Currently pressed pads."""

    pad_peak: list[float] = Field(default_factory=lambda: [0.0] * NUM_SAMPLES)
    """Best-effort per-pad peak level (0.0..=1.0)."""

    pad_peak_updated_at: list[float] = Field(default_factory=lambda: [0.0] * NUM_SAMPLES)
    """Monotonic timestamp of last pad peak update (seconds)."""

    pad_clip_hold_until: list[float] = Field(default_factory=lambda: [0.0] * NUM_SAMPLES)
    """Monotonic timestamp until which the per-pad clip indicator stays active."""

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

    pad_stem_last_custom_mask: list[int] = Field(default_factory=_default_pad_stem_last_custom_mask)
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

    waveform_pause_hold_pad_id: int | None = None
    """Pad paused by the waveform editor's transient right-button Pause hold."""

    waveform_editor_maximized: bool = False
    """Whether the waveform editor is using its transient maximized window state."""

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

    global_bpm_edit_active: bool = False
    """Whether the right-side global BPM display is currently in manual edit mode."""

    global_bpm_edit_text: str = ""
    """Temporary sanitized text buffer for the right-side global BPM edit field."""

    global_bpm_edit_focus_requested: bool = False
    """Whether the global BPM edit field should receive keyboard focus on the next frame."""

    tap_bpm_pad_id: int | None = None
    """Current Tap BPM target pad. Resets tap timestamps when changed."""

    tap_bpm_timestamps: list[float] = Field(default_factory=list)
    """Accepted timestamps for the current explicit Tap BPM measurement series."""

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

    @field_validator("pad_clip_hold_until", mode="after")
    @classmethod
    def _validate_pad_clip_hold_until(cls, value: list[float]) -> list[float]:
        if len(value) != NUM_SAMPLES:
            msg = f"pad_clip_hold_until must have length {NUM_SAMPLES}, got {len(value)}"
            raise ValueError(msg)
        for ts in value:
            if not math.isfinite(ts) or ts < 0.0:
                msg = f"pad_clip_hold_until values must be finite and >= 0.0, got {ts}"
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
        "global_stop_restore_sample_ids",
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
        "waveform_pause_hold_pad_id",
        mode="after",
    )
    @classmethod
    def _validate_optional_sample_id(cls, value: int | None) -> int | None:
        if value is not None:
            validate_sample_id(value)
        return value
