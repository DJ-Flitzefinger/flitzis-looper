"""Prepare private productive-device packets without opening an audio engine.

Only the explicit ``run`` command imports the live runner. ``prepare`` verifies
actual source bytes, freezes independent quarter assertions, and creates an
isolated project. It never promotes ordinary detector output to accepted timing.
"""

import argparse
import bisect
import hashlib
import importlib
import json
import math
import shutil
import sys
import uuid
import wave
from datetime import UTC, datetime
from pathlib import Path
from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, ValidationError, model_validator

from flitzis_looper.analysis.reference_inputs_validation import (
    private_path,
    read_json_bytes,
)
from flitzis_looper.constants import NUM_SAMPLES, SPEED_MAX, SPEED_MIN
from flitzis_looper.models import ProjectState

AUTHORED_FIXTURE_SHA256 = "96ffe98cf44215719b0b57d605d6dc586c9c4e763ad3d47d512d0ba787d204ef"


class SnapshotRequest(BaseModel):
    """One bounded observational command, with no audio-control operation."""

    model_config = ConfigDict(extra="forbid", strict=True)
    schema_version: Literal[1] = 1
    request_id: str = Field(pattern="^[0-9a-f]{32}$")
    label: str = Field(pattern="^[A-Za-z0-9][A-Za-z0-9_-]{0,79}$")
    pad_ids: list[int] = Field(min_length=1, max_length=6)
    requested_at_utc: str
    operation: Literal["observe_only"] = "observe_only"

    @model_validator(mode="after")
    def valid_pads(self) -> SnapshotRequest:
        """Reject repeated and out-of-range native pad requests."""
        if len(set(self.pad_ids)) != len(self.pad_ids) or any(
            item < 0 or item >= NUM_SAMPLES for item in self.pad_ids
        ):
            msg = "snapshot needs distinct native pad IDs"
            raise ValueError(msg)
        return self


def request_snapshot(workspace: Path, plan_path: Path, label: str, pad_ids: list[int]) -> Path:
    """Queue only an observation for a manually running packet app, without native import."""
    plan, _reference = load_plan(workspace, plan_path)
    request = SnapshotRequest(
        request_id=uuid.uuid4().hex,
        label=label,
        pad_ids=pad_ids,
        requested_at_utc=datetime.now(UTC).isoformat(),
    )
    if any(pad_id not in plan.pad_ids for pad_id in pad_ids):
        msg = "snapshot pads must be a subset of the prepared pads"
        raise ValueError(msg)
    path = Path(plan.project_directory) / "requests" / f"{request.request_id}.json"
    temporary = path.with_suffix(".tmp")
    write_json(temporary, request.model_dump(mode="json"))
    temporary.rename(path)
    return path


class QuarterReference(BaseModel):
    """Frozen independently authored source landmarks, separate from raw events."""

    model_config = ConfigDict(extra="forbid", strict=True)
    schema_version: Literal[1] = 1
    source_sha256: str = Field(pattern="^[0-9a-f]{64}$")
    source_sample_rate_hz: int = Field(gt=0)
    source_frame_count: int = Field(gt=0)
    feature_source_frames: list[int] = Field(min_length=3, max_length=100_000)
    quarter_counts: list[int] = Field(min_length=3, max_length=100_000)
    quarter_note_denominator: int = Field(gt=0)
    provenance: str = Field(min_length=1, max_length=4096)
    reference_kind: Literal["authored_quarter_pulse_fixture", "independent_human_reference"]
    origin_seconds: float
    origin_provenance: str = Field(min_length=1, max_length=4096)
    raw_match_halfwidth_seconds: float = Field(gt=0.0, le=0.1)
    timing_error_halfwidth_seconds: float = Field(ge=0.0, le=0.1)
    timing_error_provenance: str = Field(min_length=1, max_length=4096)

    @model_validator(mode="after")
    def coherent(self) -> QuarterReference:
        """Reject incomplete, ambiguous or unordered assertions before inference."""
        if len(self.feature_source_frames) != len(self.quarter_counts):
            msg = "quarter reference dimensions differ"
            raise ValueError(msg)
        if not math.isfinite(self.origin_seconds):
            msg = "quarter reference origin must be finite"
            raise ValueError(msg)
        if any(
            frame < 0 or frame >= self.source_frame_count for frame in self.feature_source_frames
        ):
            msg = "quarter reference feature outside original source"
            raise ValueError(msg)
        for first, second in zip(
            self.feature_source_frames, self.feature_source_frames[1:], strict=False
        ):
            if (second - first) / self.source_sample_rate_hz <= (
                2.0 * self.raw_match_halfwidth_seconds
            ):
                msg = "quarter reference matching windows overlap"
                raise ValueError(msg)
        if any(b <= a for a, b in zip(self.quarter_counts, self.quarter_counts[1:], strict=False)):
            msg = "quarter reference counts must increase"
            raise ValueError(msg)
        return self


class ProductiveRunPlan(BaseModel):
    """An opt-in private project and its frozen source/reference identities."""

    model_config = ConfigDict(extra="forbid", strict=True)
    schema_version: Literal[1] = 1
    evidence_kind: Literal["prepared_hardware_free_packet"] = "prepared_hardware_free_packet"
    project_directory: str
    project_config_path: str
    source_path: str
    source_sha256: str = Field(pattern="^[0-9a-f]{64}$")
    reference_path: str
    reference_sha256: str = Field(pattern="^[0-9a-f]{64}$")
    pad_ids: list[int] = Field(min_length=1, max_length=6)
    logical_loop_beats: float = Field(gt=0.0, le=256.0)
    source_speed: float = Field(ge=SPEED_MIN, le=SPEED_MAX)
    acceptance_policy_version: str = "g3c3-explicit-reference-raw-qm-v1"
    acceptance_provenance: str
    app_started: Literal[False] = False
    device_acceptance: Literal["pending"] = "pending"
    sustained_human_listening: Literal["pending"] = "pending"

    @model_validator(mode="after")
    def valid_pads(self) -> ProductiveRunPlan:
        """Require distinct bounded native pad IDs and a compatible beat length."""
        if len(set(self.pad_ids)) != len(self.pad_ids) or any(
            item < 0 or item >= NUM_SAMPLES for item in self.pad_ids
        ):
            msg = "packet needs distinct native pad IDs"
            raise ValueError(msg)
        if not math.isfinite(self.logical_loop_beats) or not math.isclose(
            self.logical_loop_beats * 16, round(self.logical_loop_beats * 16), abs_tol=1e-9
        ):
            msg = "loop beats must use 1/16-quarter granularity"
            raise ValueError(msg)
        return self


def file_sha256(path: Path) -> str:
    """Hash actual bytes using bounded memory."""
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_json(path: Path, data: object) -> None:
    """Create a new artifact without replacing historical evidence."""
    with path.open("x", encoding="utf-8") as stream:
        json.dump(data, stream, indent=2, ensure_ascii=False, allow_nan=False)
        stream.write("\n")


def authored_quarter_reference(source: Path) -> QuarterReference:
    """Verify the known private authored WAV completely, then freeze its quarters.

    The quarter interpretation is the retained explicit authored-fixture assertion.
    Byte repetition confirms its complete extent; no detector supplies these counts.
    """
    if file_sha256(source) != AUTHORED_FIXTURE_SHA256:
        msg = "source is not the retained authored quarter-pulse fixture"
        raise ValueError(msg)
    with wave.open(str(source), "rb") as wav:
        if (wav.getnchannels(), wav.getsampwidth(), wav.getframerate(), wav.getnframes()) != (
            1,
            3,
            48_000,
            28_800_000,
        ):
            msg = "authored fixture WAV dimensions changed"
            raise ValueError(msg)
        first = wav.readframes(24_000)
        if len(first) != 72_000:
            msg = "authored fixture first quarter is incomplete"
            raise ValueError(msg)
        for _index in range(1, 1200):
            if wav.readframes(24_000) != first:
                msg = "authored fixture quarter blocks differ"
                raise ValueError(msg)
        if wav.readframes(1):
            msg = "authored fixture contains an unexpected tail"
            raise ValueError(msg)
    if file_sha256(source) != AUTHORED_FIXTURE_SHA256:
        msg = "authored fixture changed during verification"
        raise ValueError(msg)
    return QuarterReference(
        source_sha256=AUTHORED_FIXTURE_SHA256,
        source_sample_rate_hz=48_000,
        source_frame_count=28_800_000,
        feature_source_frames=[index * 24_000 for index in range(1200)],
        quarter_counts=list(range(1200)),
        quarter_note_denominator=1,
        provenance=(
            "Retained independently specified synthetic quarter-pulse source; all 1200 "
            "original PCM24 quarter blocks verified byte-identical. Musical count is the "
            "authored fixture assertion, not detector index/BPM inference or music labels."
        ),
        reference_kind="authored_quarter_pulse_fixture",
        origin_seconds=0.0,
        origin_provenance="Authored quarter pulse 0 at actual original source frame zero",
        raw_match_halfwidth_seconds=0.05,
        timing_error_halfwidth_seconds=0.05,
        timing_error_provenance=(
            "Engineering raw-QM event matching bound against independently frozen source "
            "quarter attacks; actual mapped errors are retained and checked individually"
        ),
    )


def map_raw_quarters(
    metadata: dict[str, object], reference: QuarterReference
) -> tuple[str, dict[str, object]]:
    """Map fresh actual raw events by independent source-time proximity.

    Missing first/other raw events retain nonconsecutive independent counts. A raw
    index is never relabelled as a quarter count; unmatched/duplicate events fail.
    """
    if metadata.get("source_sha256") != reference.source_sha256:
        msg = "actual native source differs from frozen reference"
        raise ValueError(msg)
    loaded_rate = metadata.get("sample_rate_hz")
    loaded_count = metadata.get("frame_count")
    if type(loaded_rate) is not int or type(loaded_count) is not int:
        msg = "native loaded source dimensions are unavailable"
        raise TypeError(msg)
    if loaded_rate <= 0 or loaded_count <= 0:
        msg = "native loaded source dimensions are nonpositive"
        raise ValueError(msg)
    if (
        abs(
            loaded_count / loaded_rate
            - reference.source_frame_count / reference.source_sample_rate_hz
        )
        > 1.0 / loaded_rate
    ):
        msg = "native source extent differs from frozen reference"
        raise ValueError(msg)
    raw = metadata.get("beat_seconds")
    if not isinstance(raw, list) or len(raw) < 3:
        msg = "actual native raw beat sequence is unavailable"
        raise ValueError(msg)
    landmarks = [
        value / reference.source_sample_rate_hz for value in reference.feature_source_frames
    ]
    mapped: list[int] = []
    landmark_indices: list[int] = []
    errors: list[float] = []
    previous = -1
    for value in raw:
        if (
            not isinstance(value, (int, float))
            or isinstance(value, bool)
            or not math.isfinite(value)
        ):
            msg = "actual native raw event is invalid"
            raise ValueError(msg)
        insertion = bisect.bisect_left(landmarks, value)
        candidates = [
            index
            for index in (insertion - 1, insertion)
            if 0 <= index < len(landmarks)
            and abs(value - landmarks[index]) <= reference.raw_match_halfwidth_seconds
        ]
        if len(candidates) != 1 or candidates[0] <= previous:
            msg = "raw event lacks a unique unused independent source-quarter match"
            raise ValueError(msg)
        previous = candidates[0]
        error = value - landmarks[previous]
        if abs(error) > reference.timing_error_halfwidth_seconds:
            msg = "raw event exceeds independently supplied timing-error bound"
            raise ValueError(msg)
        landmark_indices.append(previous)
        mapped.append(reference.quarter_counts[previous])
        errors.append(error)
    hypotheses = [
        {
            "id": "independent-source-landmark-quarter-map-v1",
            "provenance": reference.provenance,
            "verification": "verified",
            "quarter_note_denominator": reference.quarter_note_denominator,
            "quarter_counts": mapped,
        }
    ]
    return json.dumps(hypotheses, allow_nan=False), {
        "raw_indices": list(range(len(mapped))),
        "independent_landmark_indices": landmark_indices,
        "quarter_counts": mapped,
        "raw_minus_independent_feature_seconds": errors,
        "maximum_absolute_raw_feature_error_seconds": max(map(abs, errors)),
        "reference_provenance": reference.provenance,
        "candidate_informed_musical_labels": False,
    }


def prepare_packet(
    workspace: Path,
    source: Path,
    output_directory: Path,
    *,
    reference: QuarterReference,
    pads: int = 6,
    loop_beats: float = 2.0,
    speed: float = 0.73,
) -> Path:
    """Create an isolated private source/config/plan, with no app/device actions."""
    source = private_path(workspace, source).resolve(strict=True)
    output_directory = private_path(workspace, output_directory)
    if not 1 <= pads <= 6:
        msg = "packet supports one to six pads"
        raise ValueError(msg)
    if file_sha256(source) != reference.source_sha256:
        msg = "source does not match frozen quarter reference"
        raise ValueError(msg)
    if output_directory.exists():
        msg = "packet destination already exists; preserve earlier evidence"
        raise ValueError(msg)
    project = ProjectState(multi_loop=True, bpm_lock=True, speed=speed, volume=0.1)
    for pad in range(pads):
        project.sample_paths[pad] = "samples/acceptance-source.wav"
        project.pad_loop_auto[pad] = False
        project.pad_loop_start_s[pad] = 0.0
        # Explicit intent may represent a shorter compatible loop than the UI's
        # auto-bar minimum. Fresh accepted timing later sets its exact duration.
        project.pad_loop_end_s[pad] = 1.0
    plan = ProductiveRunPlan(
        project_directory=str(output_directory),
        project_config_path=str(output_directory / "samples/flitzis_looper.config.json"),
        source_path=str(output_directory / "samples/acceptance-source.wav"),
        source_sha256=reference.source_sha256,
        reference_path=str(output_directory / "quarter-reference.json"),
        reference_sha256="0" * 64,
        pad_ids=list(range(pads)),
        logical_loop_beats=loop_beats,
        source_speed=speed,
        acceptance_provenance=(
            "Explicit opt-in G3c productive-device engineering run; independently frozen "
            "source-quarter mapping. Source timing acceptance is not device, listening, "
            "general musical/default-analyzer or B5 audible compensation acceptance."
        ),
    )
    (output_directory / "samples").mkdir(parents=True)
    shutil.copyfile(source, plan.source_path)
    if file_sha256(Path(plan.source_path)) != reference.source_sha256:
        msg = "isolated source copy differs; packet is incomplete"
        raise ValueError(msg)
    write_json(Path(plan.reference_path), reference.model_dump(mode="json"))
    plan.reference_sha256 = file_sha256(Path(plan.reference_path))
    write_json(Path(plan.project_config_path), project.model_dump(mode="json"))
    (output_directory / "requests").mkdir()
    (output_directory / "observations").mkdir()
    path = output_directory / "run-plan.json"
    write_json(path, plan.model_dump(mode="json"))
    return path


def load_plan(workspace: Path, path: Path) -> tuple[ProductiveRunPlan, QuarterReference]:
    """Verify actual immutable plan inputs before a human starts its app session."""
    path = private_path(workspace, path)
    plan = ProductiveRunPlan.model_validate_json(read_json_bytes(path), strict=True)
    if Path(plan.project_directory).resolve() != path.resolve().parent:
        msg = "run plan directory mismatch"
        raise ValueError(msg)
    for value in (plan.source_path, plan.reference_path, plan.project_config_path):
        if not private_path(workspace, value).is_relative_to(path.resolve().parent):
            msg = "run plan artifact escapes isolated project"
            raise ValueError(msg)
    if file_sha256(Path(plan.source_path)) != plan.source_sha256:
        msg = "isolated source bytes changed after preparation"
        raise ValueError(msg)
    if file_sha256(Path(plan.reference_path)) != plan.reference_sha256:
        msg = "independent reference bytes changed after preparation"
        raise ValueError(msg)
    reference = QuarterReference.model_validate_json(read_json_bytes(Path(plan.reference_path)))
    if reference.source_sha256 != plan.source_sha256:
        msg = "plan and independent reference source identity differ"
        raise ValueError(msg)
    return plan, reference


def main() -> None:
    """Prepare without hardware, or explicitly start the human-operated runner."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, required=True)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare = commands.add_parser("prepare")
    prepare.add_argument("--source", type=Path, required=True)
    references = prepare.add_mutually_exclusive_group(required=True)
    references.add_argument("--authored-quarter-fixture", action="store_true")
    references.add_argument("--reference", type=Path)
    prepare.add_argument("--output-dir", type=Path, required=True)
    prepare.add_argument("--pads", type=int, default=6)
    prepare.add_argument("--loop-beats", type=float, default=2.0)
    prepare.add_argument("--speed", type=float, default=0.73)
    run = commands.add_parser("run")
    run.add_argument("--plan", type=Path, required=True)
    request = commands.add_parser("request")
    request.add_argument("--plan", type=Path, required=True)
    request.add_argument("--label", required=True)
    request.add_argument("--pad-ids", type=int, nargs="+", required=True)
    args = parser.parse_args()
    try:
        if args.command == "prepare":
            reference = (
                authored_quarter_reference(private_path(args.workspace, args.source))
                if args.authored_quarter_fixture
                else QuarterReference.model_validate_json(
                    read_json_bytes(private_path(args.workspace, args.reference)), strict=True
                )
            )
            path = prepare_packet(
                args.workspace,
                args.source,
                args.output_dir,
                reference=reference,
                pads=args.pads,
                loop_beats=args.loop_beats,
                speed=args.speed,
            )
            sys.stdout.write(json.dumps({"prepared_plan": str(path), "app_started": False}) + "\n")
        elif args.command == "run":
            productive_run = importlib.import_module("flitzis_looper.analysis.productive_loop_run")
            productive_run.run_packet(args.workspace, args.plan)
        else:
            path = request_snapshot(args.workspace, args.plan, args.label, args.pad_ids)
            sys.stdout.write(json.dumps({"observational_request": str(path)}) + "\n")
    except (OSError, ValueError, ValidationError, wave.Error) as error:
        sys.stderr.write(f"productive packet rejected: {error}\n")
        raise SystemExit(2) from error


if __name__ == "__main__":
    main()
