"""Explicit routing for separately verified historical and fresh native profiles."""

from typing import TYPE_CHECKING

from flitzis_looper.analysis.beat_candidates import available_historical_profiles
from flitzis_looper.analysis.beat_candidates import load_candidate as load_historical_candidate
from flitzis_looper.analysis.beat_native_lineage import (
    available_fresh_profiles,
    load_fresh_candidate,
)

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.analysis.beat_candidate_models import NativeCandidate
    from flitzis_looper.analysis.reference_inputs_models import ReferenceTrack


def available_candidate_profiles() -> list[dict[str, str]]:
    """List all supported attempts; fresh profiles follow their preserved predecessors."""
    return available_historical_profiles() + available_fresh_profiles()


def load_candidate(workspace: Path, track: ReferenceTrack, profile_id: str) -> NativeCandidate:
    """Read a fixed trusted profile; arbitrary caller receipts are never registered."""
    fresh = load_fresh_candidate(workspace, track, profile_id)
    if fresh is not None:
        return fresh
    return load_historical_candidate(workspace, track, profile_id)
