"""Durable migration evidence; native owners and acknowledgements stay transient."""

from typing import Annotated, Literal, Self

from pydantic import BaseModel, ConfigDict, Field, model_validator

type MigrationID = Annotated[str, Field(strict=True, pattern=r"^[0-9a-f]{32}$")]
type MigrationDigest = Annotated[str, Field(strict=True, pattern=r"^[0-9a-f]{64}$")]
type ArtifactIdentity = tuple[
    Annotated[int, Field(strict=True, ge=0, le=2**64 - 1)],
    Annotated[int, Field(strict=True, ge=0, le=2**64 - 1)],
    Annotated[int, Field(strict=True, ge=0, le=2**64 - 1)],
]
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


def _ordinary_readable_root(parts: list[str]) -> bool:
    """Accept exactly the normalized two-part samples root spelling."""
    return (
        len(parts) == 2
        and parts[0] == "samples"
        and parts[1] not in {"", ".", ".."}
        and not any(character in parts[1] for character in "\\:")
    )


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
    resume_of: MigrationID | None = None
    material_creation: MaterialCreationReceipt | None = None

    @model_validator(mode="after")
    def validate_material_root(self) -> Self:
        if (
            self.material_creation is not None
            and self.new_reference.rsplit("/", 1)[0] != self.material_creation.root_reference
        ):
            message = "migration birth receipt belongs to another target root"
            raise ValueError(message)
        return self


class MigrationAssignment(BaseModel):
    """Capture one durable content identity without a journalled runtime permit."""

    model_config = ConfigDict(frozen=True, extra="forbid")
    sample_id: int = Field(strict=True, ge=0, le=215)
    instance_id: MigrationID
    old_reference: str = Field(strict=True, min_length=1)


class MigrationFileEvidence(BaseModel):
    """Exact sealed leaf identity and bytes; a filename grants no deletion permission."""

    model_config = ConfigDict(frozen=True, extra="forbid")
    name: str = Field(strict=True, min_length=1, max_length=255, pattern=r"^[^/\\:]+$")
    identity: ArtifactIdentity
    bytes: int = Field(strict=True, ge=0)
    sha256: MigrationDigest

    @model_validator(mode="after")
    def validate_leaf(self) -> Self:
        if self.name in {".", ".."}:
            message = "artifact evidence must name one ordinary leaf"
            raise ValueError(message)
        return self


class MaterialCreationReceipt(BaseModel):
    """Exact native birth evidence; fresh source ACK and inventory restore ownership."""

    model_config = ConfigDict(frozen=True, extra="forbid")
    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    root_reference: str = Field(strict=True, min_length=1, max_length=8192)
    root_identity: ArtifactIdentity
    binding: MigrationFileEvidence

    @model_validator(mode="after")
    def validate_binding(self) -> Self:
        parts = self.root_reference.split("/")
        if (
            not _ordinary_readable_root(parts)
            or self.binding.name != ".material.json"
            or self.binding.bytes == 0
        ):
            message = "material birth receipt must bind exactly one readable root and marker"
            raise ValueError(message)
        return self


class MigrationArtifactEvidence(BaseModel):
    """A bounded complete native receipt that must be freshly reverified after restart."""

    model_config = ConfigDict(frozen=True, extra="forbid")
    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    kind: Literal[
        "original", "stem_directory", "pcm_directory", "stem_pcm_directory", "stem_pair_descriptor"
    ]
    reference: str = Field(strict=True, min_length=1, max_length=8192)
    identity: ArtifactIdentity
    samples_identity: ArtifactIdentity
    files: tuple[MigrationFileEvidence, ...] = Field(min_length=1, max_length=6)
    material_creation: MaterialCreationReceipt | None = None

    @model_validator(mode="after")
    def validate_files(self) -> Self:
        if (
            not self.reference.startswith("samples/")
            or any(character in self.reference for character in "\\:")
            or any(part in {"", ".", ".."} for part in self.reference.split("/"))
        ):
            message = "artifact evidence requires a normalized contained samples reference"
            raise ValueError(message)
        names = {item.name for item in self.files}
        expected = {
            "pcm_directory": {"decoder.f32le", "playback.f32le", "manifest.json"},
            "stem_pcm_directory": {
                "vocals.f32le",
                "melody.f32le",
                "bass.f32le",
                "drums.f32le",
                "instrumental.f32le",
                "manifest.json",
            },
            "stem_directory": {
                "vocals.wav",
                "melody.wav",
                "bass.wav",
                "drums.wav",
                "instrumental.wav",
                ".complete.json",
            },
        }.get(self.kind)
        if self.kind in {"original", "stem_pair_descriptor"}:
            expected = {self.reference.rsplit("/", 1)[-1]}
        if len(names) != len(self.files) or (expected is not None and names != expected):
            message = "migration artifact receipt must describe exactly its recognized complete set"
            raise ValueError(message)
        if self.material_creation is not None and (
            self.kind != "original"
            or self.reference.rsplit("/", 1)[0] != self.material_creation.root_reference
        ):
            message = "material birth evidence must belong to its exact readable original"
            raise ValueError(message)
        return self


class MigrationArtifactRecord(BaseModel):
    """Rollback material versus newly created or reused target provenance."""

    model_config = ConfigDict(frozen=True, extra="forbid")
    role: Literal["rollback", "target"]
    created: bool = Field(strict=True)
    evidence: MigrationArtifactEvidence
    original_retired: bool = Field(default=False, strict=True)

    @model_validator(mode="after")
    def validate_terminal(self) -> Self:
        if self.original_retired and (
            self.evidence.kind != "original" or self.evidence.material_creation is None
        ):
            message = "original terminal receipt requires exact material birth evidence"
            raise ValueError(message)
        return self


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
    artifacts: tuple[MigrationArtifactRecord, ...] = Field(default=(), max_length=1024)
    resume_of: MigrationID | None = None
    cleanup_complete: bool = Field(default=False, strict=True)
    error: str | None = Field(default=None, strict=True)

    @model_validator(mode="after")
    def validate_transaction(self) -> Self:
        if len({item.sample_id for item in self.assignments}) != len(self.assignments):
            message = "migration assignments must name distinct pad slots"
            raise ValueError(message)
        if self.alias is not None and self.alias.transaction_id != self.transaction_id:
            message = "migration alias belongs to another transaction"
            raise ValueError(message)
        if self.resume_of == self.transaction_id or (
            self.alias is not None and self.alias.resume_of != self.resume_of
        ):
            message = "migration recovery lineage must bind a different parent transaction"
            raise ValueError(message)
        keys = {(item.role, item.evidence.reference) for item in self.artifacts}
        if len(keys) != len(self.artifacts):
            message = "migration artifact ledger must contain distinct role/reference records"
            raise ValueError(message)
        if any(item.created and item.role == "rollback" for item in self.artifacts):
            message = "rollback artifacts cannot inherit new target creation rights"
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
