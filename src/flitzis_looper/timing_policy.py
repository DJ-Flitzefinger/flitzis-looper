"""Strict global timing intent and shared resolution, never audio ownership."""

from typing import TYPE_CHECKING, Literal

from pydantic import BaseModel, ConfigDict, Field

if TYPE_CHECKING:
    from flitzis_looper.source_sync_intent import PadSourceSyncIntent

MAX_TIMING_POLICY_REVISION = (1 << 64) - 1


class GlobalTimingPolicy(BaseModel):
    """Independent choices; None retains behavior of projects predating the controls."""

    model_config = ConfigDict(
        frozen=True, extra="forbid", strict=True, revalidate_instances="always"
    )

    schema_version: int = Field(strict=True, ge=1, le=1)
    sync: bool | None
    realtime_bpm: bool | None
    revision: int = Field(strict=True, ge=0, le=MAX_TIMING_POLICY_REVISION)

    @property
    def is_neutral(self) -> bool:
        """Identify only the unchanged legacy policy, not a later equal-looking choice."""
        return self.sync is None and self.realtime_bpm is None and self.revision == 0


def neutral_global_timing_policy() -> GlobalTimingPolicy:
    """Keep historical projects and their independent PAD requests behavior-compatible."""
    return GlobalTimingPolicy(schema_version=1, sync=None, realtime_bpm=None, revision=0)


def next_global_timing_policy(
    current: GlobalTimingPolicy,
    *,
    control: Literal["sync", "realtime_bpm"],
    enabled: bool,
) -> GlobalTimingPolicy:
    """Change exactly one choice with a checked fence, leaving content revisions alone."""
    current = GlobalTimingPolicy.model_validate(current)
    if type(enabled) is not bool:
        message = "Global timing policy requires a strict boolean."
        raise TypeError(message)
    if control not in {"sync", "realtime_bpm"}:
        message = "Unknown global timing control."
        raise ValueError(message)
    if getattr(current, control) is enabled:
        return current
    if current.revision == MAX_TIMING_POLICY_REVISION:
        message = "Global timing policy revision capacity exhausted."
        raise ValueError(message)
    data = current.model_dump()
    data[control] = enabled
    data["revision"] = current.revision + 1
    return GlobalTimingPolicy.model_validate(data)


def source_sync_preference(policy: GlobalTimingPolicy, pad: PadSourceSyncIntent) -> bool:
    """Resolve only untouched false/0 as inheritance; explicit true/0 is preserved."""
    if not pad.requested and pad.revision == 0:
        return policy.sync is True
    return pad.requested


def source_sync_requested(policy: GlobalTimingPolicy, pad: PadSourceSyncIntent) -> bool:
    """The global OFF gate suppresses every request without erasing its preference."""
    return policy.sync is not False and source_sync_preference(policy, pad)


def variable_source_timing(policy: GlobalTimingPolicy) -> bool:
    """A disabled tracker selects initial constant timing, never a frozen map segment."""
    return policy.realtime_bpm is not False
