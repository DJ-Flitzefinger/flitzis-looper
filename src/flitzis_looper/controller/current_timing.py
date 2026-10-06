"""Immutable control projections of current native timing, never ticket history."""

import math
from dataclasses import dataclass
from enum import Enum


class UnresolvedTiming(Enum):
    """Distinguish a missing lookup from a deliberately unavailable snapshot."""

    VALUE = "unresolved"


UNRESOLVED_TIMING = UnresolvedTiming.VALUE


@dataclass(frozen=True)
class CurrentAcceptedIdentity:
    """Complete current source/adoption identity returned by the native owner."""

    source_id: str
    source_generation: int
    accepted_request_id: int
    publication_epoch: int
    source_sha256: str
    source_provenance: str
    pcm_sha256: str
    frame_count: int
    source_zero_seconds: float
    mono_revision: str
    raw_revision: str
    acceptance_policy_version: str
    acceptance_provenance: str


@dataclass(frozen=True)
class CurrentPadTiming:
    """One coherent source period/origin snapshot with optional accepted ownership.

    BPM is a presentation projection. Musical consumers use period_seconds
    directly; only the native current resolver can supply accepted_revision.
    """

    period_seconds: float
    origin_seconds: float
    sample_rate_hz: int | None
    accepted_revision: str | None = None
    origin_provenance: str | None = None
    accepted_identity: CurrentAcceptedIdentity | None = None

    @property
    def bpm(self) -> float:
        """Return the compatibility BPM for display and intentional BPM edits."""
        return 60.0 / self.period_seconds

    @property
    def source_duration_seconds(self) -> float | None:
        """Return the accepted actual native full extent, independent of saved duration."""
        if self.accepted_identity is None or self.sample_rate_hz is None:
            return None
        return self.accepted_identity.frame_count / self.sample_rate_hz


def current_accepted_timing(metadata: dict[str, object], *, sample_id: int) -> CurrentPadTiming:
    """Project the native current record without refitting or narrowing its values.

    The native method has already checked source ownership, Automatic intent and
    callback acknowledgement. A malformed contract is an error, not permission
    to silently publish an older automatic estimate.
    """
    if metadata.get("pad_id") != sample_id:
        msg = "current timing pad identity mismatch"
        raise RuntimeError(msg)
    period = _number(metadata, "period_seconds_per_quarter")
    if period <= 0.0 or not math.isfinite(60.0 / period):
        msg = "current timing period is out of range"
        raise RuntimeError(msg)
    identity = CurrentAcceptedIdentity(
        source_id=_text(metadata, "source_id"),
        source_generation=_integer(metadata, "source_generation"),
        accepted_request_id=_integer(metadata, "accepted_request_id"),
        publication_epoch=_integer(metadata, "publication_epoch"),
        source_sha256=_text(metadata, "source_sha256"),
        source_provenance=_text(metadata, "source_provenance"),
        pcm_sha256=_text(metadata, "pcm_sha256"),
        frame_count=_integer(metadata, "frame_count"),
        source_zero_seconds=_number(metadata, "source_zero_seconds"),
        mono_revision=_text(metadata, "mono_revision"),
        raw_revision=_text(metadata, "raw_revision"),
        acceptance_policy_version=_text(metadata, "acceptance_policy_version"),
        acceptance_provenance=_text(metadata, "acceptance_provenance"),
    )
    return CurrentPadTiming(
        period_seconds=period,
        origin_seconds=_number(metadata, "origin_seconds"),
        sample_rate_hz=_integer(metadata, "sample_rate_hz"),
        accepted_revision=_text(metadata, "revision"),
        origin_provenance=_text(metadata, "origin_provenance"),
        accepted_identity=identity,
    )


def _text(metadata: dict[str, object], key: str) -> str:
    value = metadata.get(key)
    if isinstance(value, str) and value:
        return value
    msg = f"current timing {key} is invalid"
    raise RuntimeError(msg)


def _integer(metadata: dict[str, object], key: str) -> int:
    value = metadata.get(key)
    if isinstance(value, int) and not isinstance(value, bool) and value > 0:
        return value
    msg = f"current timing {key} is invalid"
    raise RuntimeError(msg)


def _number(metadata: dict[str, object], key: str) -> float:
    value = metadata.get(key)
    if isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value):
        return float(value)
    msg = f"current timing {key} is invalid"
    raise RuntimeError(msg)
