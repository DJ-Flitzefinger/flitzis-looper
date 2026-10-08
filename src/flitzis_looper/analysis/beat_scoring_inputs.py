"""Strict caller-selected plan for private seal-bound temporal diagnostics."""

from typing import Annotated, Literal

from pydantic import Field

from flitzis_looper.analysis.beat_candidate_models import (  # noqa: TC001 - Pydantic resolves DTOs.
    CandidateSelection,
)
from flitzis_looper.analysis.reference_inputs_models import Digest, One, StrictInput


class ScoringPlan(StrictInput):
    """Select retained native candidates; omissions remain explicit missing inputs."""

    schema_version: One
    status: Literal["ready_for_temporal_scoring"]
    reference_seal_sha256: Digest
    candidates: Annotated[tuple[CandidateSelection, ...], Field(max_length=5)]
