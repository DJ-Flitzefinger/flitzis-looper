"""Sealed musical Copy data, without copied current Native authority or custody."""

import json
import math
from typing import Self

from pydantic import BaseModel, ConfigDict, Field, field_validator

from flitzis_looper.accepted_timing import PersistedAcceptedTiming
from flitzis_looper.constants import (
    NUM_SAMPLES,
    PAD_EQ_DB_MAX,
    PAD_EQ_DB_MIN,
    PAD_GAIN_DB_MAX,
    PAD_GAIN_DB_MIN,
    PAD_LOOP_BARS_GRANULARITY,
    PAD_LOOP_BARS_MIN,
)
from flitzis_looper.key_intent import PadKeyIntent
from flitzis_looper.models import (
    BeatGrid,
    PadContentIdentity,
    ProjectState,
    SampleAnalysis,
    StemCacheEntry,
    StemFileSet,
    StemMixMode,
    TimingIntent,
    validate_sample_id,
)
from flitzis_looper.stem_intent import PadStemIntent

CONTENT_FIELDS = (
    ("source_path", "sample_paths"),
    ("source_identity", "pad_content"),
    ("duration", "sample_durations"),
    ("analysis", "sample_analysis"),
    ("stem_cache", "stem_cache"),
    ("stem_mix_mode", "pad_stem_mix_mode"),
    ("stem_intent", "pad_stem_intent"),
    ("key_lock", "pad_key_lock"),
    ("manual_bpm", "manual_bpm"),
    ("timing_intent", "pad_timing_intent"),
    ("key_intent", "pad_key_intent"),
    ("gain_db", "pad_gain_db"),
    ("eq_low_db", "pad_eq_low_db"),
    ("eq_mid_db", "pad_eq_mid_db"),
    ("eq_high_db", "pad_eq_high_db"),
    ("loop_start_s", "pad_loop_start_s"),
    ("loop_end_s", "pad_loop_end_s"),
    ("loop_auto", "pad_loop_auto"),
    ("loop_bars", "pad_loop_bars"),
    ("grid_offset_samples", "pad_grid_offset_samples"),
    ("grid_anchor_s", "pad_grid_anchor_s"),
)


def canonical_snapshot_json(value: object) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)


def _model_data(value: object) -> object:
    if isinstance(value, BaseModel):
        return value.model_dump(mode="python", warnings="error")
    return value


def _reject_unknown_model_fields(value: object, model: type[BaseModel]) -> None:
    if isinstance(value, dict) and set(value) - set(model.model_fields):
        message = f"Copy {model.__name__} contains unsupported fields"
        raise ValueError(message)


class MusicalSnapshotData(BaseModel):
    """One complete musical schema shared by occupied Copy and pair slot images."""

    model_config = ConfigDict(extra="forbid", strict=True, revalidate_instances="always")

    source_path: str | None = Field(min_length=1)
    source_identity: PadContentIdentity | None
    duration: float | None = Field(ge=0, allow_inf_nan=False)
    analysis: SampleAnalysis | None
    stem_cache: StemCacheEntry | None
    stem_mix_mode: StemMixMode
    stem_intent: PadStemIntent
    key_lock: bool | None
    manual_bpm: float | None = Field(gt=0, allow_inf_nan=False)
    timing_intent: TimingIntent
    key_intent: PadKeyIntent
    gain_db: float = Field(ge=PAD_GAIN_DB_MIN, le=PAD_GAIN_DB_MAX, allow_inf_nan=False)
    eq_low_db: float = Field(ge=PAD_EQ_DB_MIN, le=PAD_EQ_DB_MAX, allow_inf_nan=False)
    eq_mid_db: float = Field(ge=PAD_EQ_DB_MIN, le=PAD_EQ_DB_MAX, allow_inf_nan=False)
    eq_high_db: float = Field(ge=PAD_EQ_DB_MIN, le=PAD_EQ_DB_MAX, allow_inf_nan=False)
    loop_start_s: float = Field(ge=0, allow_inf_nan=False)
    loop_end_s: float | None = Field(ge=0, allow_inf_nan=False)
    loop_auto: bool
    loop_bars: float = Field(ge=PAD_LOOP_BARS_MIN, allow_inf_nan=False)
    grid_offset_samples: int
    grid_anchor_s: float | None = Field(ge=0, allow_inf_nan=False)

    @field_validator("stem_intent", "key_intent", mode="before")
    @classmethod
    def _detach_intent_models(cls, value: object) -> object:
        if isinstance(value, (PadContentIdentity, PadStemIntent, PadKeyIntent)):
            return _model_data(value)
        return value

    @field_validator("source_identity", mode="before")
    @classmethod
    def _validate_source_identity(cls, value: object) -> object:
        # Strict JSON gives fixed FileID tuples their canonical array encoding.
        # Return a newly validated typed receipt so Python revalidation keeps tuples.
        data = _model_data(value)
        if isinstance(data, dict):
            return PadContentIdentity.model_validate_json(
                canonical_snapshot_json(data), strict=True
            )
        return data

    @field_validator("analysis", mode="before")
    @classmethod
    def _validate_complete_analysis(cls, value: object) -> object:
        data = _model_data(value)
        _reject_unknown_model_fields(data, SampleAnalysis)
        if isinstance(data, dict):
            data = dict(data)
            grid = _model_data(data.get("beat_grid"))
            _reject_unknown_model_fields(grid, BeatGrid)
            if "beat_grid" in data:
                data["beat_grid"] = grid
            accepted = _model_data(data.get("accepted_timing"))
            if accepted is not None:
                # Snapshot capture must reject evidence that legacy loading would discard.
                data["accepted_timing"] = PersistedAcceptedTiming.model_validate(
                    accepted, strict=True
                ).model_dump(mode="python", warnings="error")
        return data

    @field_validator("stem_cache", mode="before")
    @classmethod
    def _validate_stem_metadata(cls, value: object) -> object:
        data = _model_data(value)
        _reject_unknown_model_fields(data, StemCacheEntry)
        if isinstance(data, dict):
            data = dict(data)
            stems = _model_data(data.get("stems"))
            _reject_unknown_model_fields(stems, StemFileSet)
            if "stems" in data:
                data["stems"] = stems
        if isinstance(data, dict) and data.get("available", False) is not False:
            message = "Copy stem metadata cannot claim target playback availability"
            raise ValueError(message)
        return data

    @field_validator("loop_bars")
    @classmethod
    def _validate_loop_bar_granularity(cls, value: float) -> float:
        steps = value / PAD_LOOP_BARS_GRANULARITY
        if not math.isclose(steps, round(steps), abs_tol=1e-9):
            message = f"Copy loop bars must use {PAD_LOOP_BARS_GRANULARITY}-bar granularity"
            raise ValueError(message)
        return float(round(steps) * PAD_LOOP_BARS_GRANULARITY)


class CopySnapshotData(MusicalSnapshotData):
    """Fresh occupied musical projection; mutable children never belong to the seal."""

    source_path: str = Field(min_length=1)
    source_identity: PadContentIdentity


class CopySnapshot(BaseModel):
    """Immutable canonical JSON seal with independently validated typed projections."""

    model_config = ConfigDict(frozen=True, extra="forbid", revalidate_instances="always")

    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    payload_json: str = Field(strict=True, min_length=1)

    @field_validator("payload_json")
    @classmethod
    def _validate_and_seal_payload(cls, value: str) -> str:
        projected = CopySnapshotData.model_validate_json(value, strict=True)
        return canonical_snapshot_json(projected.model_dump(mode="json", warnings="error"))

    def projected(self) -> CopySnapshotData:
        """Return fresh typed data after checking even unchecked wrapper instances."""
        validated = type(self).model_validate(self)
        return CopySnapshotData.model_validate_json(validated.payload_json, strict=True)

    @classmethod
    def from_data(cls, data: CopySnapshotData) -> Self:
        """Seal typed data after independently revalidating its full JSON projection."""
        return cls(
            payload_json=canonical_snapshot_json(data.model_dump(mode="json", warnings="error"))
        )


def copy_after_snapshot(source: CopySnapshot, identity: PadContentIdentity) -> CopySnapshot:
    """Derive the fixed fresh C seal by changing only captured A's durable identity."""
    captured = CopySnapshot.model_validate(source).projected()
    target = PadContentIdentity.model_validate_json(
        canonical_snapshot_json(identity.model_dump(mode="json", warnings="error")),
        strict=True,
    )
    original = captured.source_identity
    if (
        target.instance_id == original.instance_id
        or target.material_id != original.material_id
        or target.material_creation != original.material_creation
        or target.copied_from != original.instance_id
    ):
        message = "Copy successor must be fresh and retain captured material, birth and lineage"
        raise ValueError(message)
    data = captured.model_dump(mode="python", warnings="error")
    data["source_identity"] = target
    return CopySnapshot.from_data(CopySnapshotData.model_validate(data, strict=True))


def strict_snapshot_sample_id(sample_id: int) -> int:
    if type(sample_id) is not int:
        message = "Copy sample_id must be an integer"
        raise ValueError(message)
    return validate_sample_id(sample_id)


def musical_content_data(project: ProjectState, sample_id: int) -> dict[str, object]:
    """Read the one complete musical field table without claiming runtime availability."""
    sample_id = strict_snapshot_sample_id(sample_id)
    data: dict[str, object] = {}
    for name, project_field in CONTENT_FIELDS:
        table = getattr(project, project_field)
        if not isinstance(table, list) or len(table) != NUM_SAMPLES:
            message = f"Copy {project_field} must contain exactly {NUM_SAMPLES} slots"
            raise ValueError(message)
        data[name] = _model_data(table[sample_id])
    stems = data["stem_cache"]
    if isinstance(stems, dict):
        data["stem_cache"] = stems | {"available": False}
    return data


def capture_copy_snapshot(project: ProjectState, source_id: int) -> CopySnapshot:
    """Capture musical content without reading artifacts or retaining current authority.

    Args:
        project: Current durable assignments, not a Native authority envelope.
        source_id: Occupied source slot with an established content identity.

    Returns:
        An independent seal preserving source metadata and all musical settings.
    """
    source_id = strict_snapshot_sample_id(source_id)
    data = musical_content_data(project, source_id)
    if data["source_path"] is None or data["source_identity"] is None:
        message = "Copy source must contain loaded content with a durable identity"
        raise ValueError(message)
    return CopySnapshot.from_data(CopySnapshotData.model_validate(data, strict=True))


def _validate_target_identity(
    project: ProjectState, captured: PadContentIdentity, target_identity: PadContentIdentity
) -> PadContentIdentity:
    identity = PadContentIdentity.model_validate_json(
        canonical_snapshot_json(_model_data(target_identity)), strict=True
    )
    if identity.instance_id == captured.instance_id or any(
        content is not None and content.instance_id == identity.instance_id
        for content in project.pad_content
    ):
        message = "Copy target must have a fresh content instance identity"
        raise ValueError(message)
    if (
        identity.material_id != captured.material_id
        or identity.material_creation != captured.material_creation
    ):
        message = "Copy target must retain the captured immutable material and birth evidence"
        raise ValueError(message)
    if identity.copied_from != captured.instance_id:
        message = "Copy target lineage must identify the captured source instance"
        raise ValueError(message)
    return identity


def project_with_copy_snapshot(
    project: ProjectState,
    target_id: int,
    snapshot: CopySnapshot,
    *,
    target_identity: PadContentIdentity,
) -> ProjectState:
    """Prepare a complete project candidate without changing the current project.

    Args:
        project: Project whose other slots and global settings must be preserved.
        target_id: Slot receiving the captured musical content after a separate transaction.
        snapshot: Sealed musical data; it provides no current Native permissions.
        target_identity: Fresh independent identity retaining material and source lineage.

    Returns:
        A fully revalidated project candidate with target stem availability unclaimed.
    """
    target_id = strict_snapshot_sample_id(target_id)
    captured = CopySnapshot.model_validate(snapshot).projected()
    identity = _validate_target_identity(project, captured.source_identity, target_identity)
    musical_content_data(project, target_id)
    data = project.model_dump(mode="python", exclude_computed_fields=True, warnings="error")
    projection = captured.model_dump(mode="python", warnings="error")
    projection["source_identity"] = identity.model_dump(mode="python", warnings="error")
    for name, project_field in CONTENT_FIELDS:
        data[project_field][target_id] = projection[name]
    return ProjectState.model_validate_json(canonical_snapshot_json(data), strict=True)
