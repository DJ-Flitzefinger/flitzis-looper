"""Durable per-content SYNC requests, separate from source-map preparation."""

from pydantic import BaseModel, ConfigDict, Field

MAX_SOURCE_SYNC_REVISION = (1 << 64) - 1


class PadSourceSyncIntent(BaseModel):
    """A request fence; persisted state never claims audible mapped adoption."""

    model_config = ConfigDict(
        frozen=True, extra="forbid", strict=True, revalidate_instances="always"
    )

    requested: bool
    revision: int = Field(strict=True, ge=0, le=MAX_SOURCE_SYNC_REVISION)


def neutral_source_sync_intent() -> PadSourceSyncIntent:
    """Supply neutral legacy intent only when its containing field is absent."""
    return PadSourceSyncIntent(requested=False, revision=0)


def next_source_sync_intent(
    current: PadSourceSyncIntent, *, requested: bool
) -> PadSourceSyncIntent:
    """Advance an explicit request without wrapping or changing map selection."""
    current = PadSourceSyncIntent.model_validate(current)
    if current.revision == MAX_SOURCE_SYNC_REVISION:
        message = "source SYNC request revision capacity exhausted"
        raise ValueError(message)
    return PadSourceSyncIntent(requested=requested, revision=current.revision + 1)
