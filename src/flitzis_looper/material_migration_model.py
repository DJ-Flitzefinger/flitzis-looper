"""Durable migration evidence; native owners and acknowledgements stay transient."""

from typing import Annotated, Literal, Self

from pydantic import BaseModel, ConfigDict, Field, model_validator

type MigrationID = Annotated[str, Field(strict=True, pattern=r"^[0-9a-f]{32}$")]
type MigrationDigest = Annotated[str, Field(strict=True, pattern=r"^[0-9a-f]{64}$")]
type MigrationPhase = Literal[
    "captured",
    "material_verified",
    "references_prepared",
    "adoption_pending",
    "ack_confirmed",
    "config_committed",
    "failed",
    "unresolved",
]


class MaterialMigrationAlias(BaseModel):
    """Verified byte/descriptor lineage, never reconstructed source authority."""

    model_config = ConfigDict(frozen=True, extra="forbid")
    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    transaction_id: MigrationID
    material_id: MigrationID
    old_reference: str = Field(strict=True, min_length=1)
    new_reference: str = Field(strict=True, min_length=1)
    original_sha256: MigrationDigest
    original_bytes: int = Field(strict=True, gt=0)
    decoder_identity: MigrationDigest
    playback_identity: MigrationDigest
    cache_path: str = Field(strict=True, min_length=1)
    old_source_version: str = Field(strict=True, min_length=1)
    new_source_version: str = Field(strict=True, min_length=1)


class MigrationAssignment(BaseModel):
    """Capture one durable content identity without a journalled runtime permit."""

    model_config = ConfigDict(frozen=True, extra="forbid")
    sample_id: int = Field(strict=True, ge=0, le=215)
    instance_id: MigrationID
    old_reference: str = Field(strict=True, min_length=1)


class MaterialMigrationJournal(BaseModel):
    """Versioned, bounded current-project transaction and rollback intent."""

    model_config = ConfigDict(frozen=True, extra="forbid")
    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    transaction_id: MigrationID
    phase: MigrationPhase = "captured"
    config_reference: str | None = Field(default=None, strict=True, min_length=1)
    captured_revision: int = Field(strict=True, ge=0)
    intent_revision: int = Field(default=0, strict=True, ge=0)
    config_sha256: MigrationDigest | None = None
    snapshot_json: str = Field(strict=True, min_length=1, max_length=8 * 1024 * 1024)
    assignments: tuple[MigrationAssignment, ...] = Field(min_length=1, max_length=216)
    alias: MaterialMigrationAlias | None = None
    committed_revision: int | None = Field(default=None, strict=True, ge=0)
    committed_config_sha256: MigrationDigest | None = None
    error: str | None = Field(default=None, strict=True)

    @model_validator(mode="after")
    def validate_transaction(self) -> Self:
        if len({item.sample_id for item in self.assignments}) != len(self.assignments):
            message = "migration assignments must name distinct pad slots"
            raise ValueError(message)
        if self.alias is not None and self.alias.transaction_id != self.transaction_id:
            message = "migration alias belongs to another transaction"
            raise ValueError(message)
        if self.phase == "config_committed" and (
            self.committed_revision is None or self.committed_config_sha256 is None
        ):
            message = "committed migration must carry the actual config outcome"
            raise ValueError(message)
        return self

    def changed(self, **updates: object) -> MaterialMigrationJournal:
        """Validate the entire new record before changing a journal phase."""
        return MaterialMigrationJournal.model_validate(self.model_dump() | updates)
