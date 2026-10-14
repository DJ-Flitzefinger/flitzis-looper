"""Strict occupied Copy evidence and bounded append custody, without runtime rights."""

import hashlib
from itertools import pairwise
from pathlib import Path
from typing import TYPE_CHECKING, Literal, Self

from pydantic import BaseModel, ConfigDict, Field, TypeAdapter, field_validator, model_validator

from flitzis_looper.controller.pair_arrangement_journal import (
    JOURNAL_BYTES,
    JOURNAL_RECORDS,
    JOURNAL_TRANSACTIONS,
    _without_accepted_timing,
    strict_json_object,
    validated_project_image,
    written_project_intent,
)
from flitzis_looper.copy_snapshot import (
    CopySnapshot,
    CopySnapshotData,
    canonical_snapshot_json,
    capture_copy_snapshot,
    musical_content_data,
    project_with_copy_snapshot,
)
from flitzis_looper.material_migration_model import MigrationDigest, MigrationID
from flitzis_looper.models import PadContentIdentity, ProjectState
from flitzis_looper_audio import MaterialMigrationJournalStore

if TYPE_CHECKING:
    from flitzis_looper.controller.persistence import ConfigCommitReceipt, ProjectPersistence

_ID_PATTERN: str = TypeAdapter(MigrationID).json_schema()["pattern"]
_DIGEST_PATTERN: str = TypeAdapter(MigrationDigest).json_schema()["pattern"]

type CopyJournalPhase = Literal[
    "captured",
    "publication_pending",
    "native_ack_observed",
    "config_committed",
    "settled",
    "cancelled",
]


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


def written_copy_image_matches(project: ProjectState, after: CopySnapshot, target_id: int) -> bool:
    """Compare one complete seal, allowing only established Config save normalization.

    Relative/absolute source spelling and historical accepted-timing export do
    not change musical intent or confer current Native playback acceptance.
    """
    expected = (
        CopySnapshot.model_validate(after).projected().model_dump(mode="json", warnings="error")
    )
    actual_data = musical_content_data(project, target_id)
    if actual_data["source_path"] is None or actual_data["source_identity"] is None:
        return False
    actual = CopySnapshotData.model_validate(actual_data, strict=True).model_dump(
        mode="json", warnings="error"
    )
    if Path(str(actual["source_path"])).absolute() != Path(str(expected["source_path"])).absolute():
        return False
    actual["source_path"] = expected["source_path"]
    return _without_accepted_timing(actual) == _without_accepted_timing(expected)


class CopyArrangementJournal(BaseModel):
    """One strict Copy record retaining original A/B and the deterministic fixed C."""

    model_config = ConfigDict(
        frozen=True, extra="forbid", strict=True, revalidate_instances="always"
    )
    kind: Literal["pad_copy_overwrite"] = "pad_copy_overwrite"
    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    transaction_id: str = Field(strict=True, pattern=_ID_PATTERN)
    resume_of: str | None = Field(default=None, strict=True, pattern=_ID_PATTERN)
    config_reference: str = Field(strict=True, min_length=1, max_length=8192)
    captured_revision: int = Field(strict=True, ge=0)
    intent_revision: int = Field(strict=True, ge=0)
    captured_config_sha256: str | None = Field(strict=True, pattern=_DIGEST_PATTERN)
    source_id: int = Field(strict=True, ge=0, le=215)
    target_id: int = Field(strict=True, ge=0, le=215)
    source: CopySnapshot
    predecessor: CopySnapshot
    after: CopySnapshot
    resolved_key_lock: bool = Field(strict=True)
    captured_project_image_json: str = Field(strict=True, min_length=1)
    project_image_json: str = Field(strict=True, min_length=1)
    image_role: Literal["before", "after"]
    phase: CopyJournalPhase
    committed_revision: int | None = Field(default=None, strict=True, ge=0)
    committed_config_sha256: str | None = Field(default=None, strict=True, pattern=_DIGEST_PATTERN)
    error: str | None = Field(default=None, strict=True, max_length=2048)

    @field_validator("source", "predecessor", "after", mode="before")
    @classmethod
    def _unambiguous_snapshot(cls, value: object) -> object:
        if isinstance(value, dict) and set(value) != {"schema_version", "payload_json"}:
            message = "Copy journal snapshot wrapper is incomplete or contains unknown fields"
            raise ValueError(message)
        payload = (
            value.payload_json
            if isinstance(value, CopySnapshot)
            else (value.get("payload_json") if isinstance(value, dict) else None)
        )
        if isinstance(payload, str):
            data = strict_json_object(payload)
            projected = CopySnapshotData.model_validate_json(payload, strict=True)
            if data != projected.model_dump(mode="json", warnings="error"):
                message = "Copy journal snapshot contains incomplete nested musical metadata"
                raise ValueError(message)
        return value

    @model_validator(mode="after")
    def _consistent_evidence(self) -> Self:
        self._validate_original_evidence()
        project = validated_project_image(self.project_image_json)
        if self.phase in {"config_committed", "settled"}:
            self._validate_committed_image(project)
        else:
            self._validate_before_image(project)
        if len(self.model_dump_json().encode("utf-8")) > JOURNAL_BYTES:
            message = "Copy journal exceeds Native bounded UTF8 record size"
            raise ValueError(message)
        return self

    @property
    def target_identity(self) -> PadContentIdentity:
        """Return detached fixed C lineage, never a runtime acceptance token."""
        return self.after.projected().source_identity

    def _validate_original_evidence(self) -> None:
        if (
            not Path(self.config_reference).is_absolute()
            or self.resume_of == self.transaction_id
            or self.source_id == self.target_id
            or self.intent_revision < self.captured_revision
        ):
            message = "Copy journal requires distinct slots, actual Config binding and lineage"
            raise ValueError(message)
        captured = validated_project_image(self.captured_project_image_json)
        if (
            captured.config_revision > self.captured_revision
            or capture_copy_snapshot(captured, self.source_id) != self.source
            or capture_copy_snapshot(captured, self.target_id) != self.predecessor
        ):
            message = "Copy captured project differs from its original occupied A/B seals"
            raise ValueError(message)
        source = self.source.projected()
        predecessor = self.predecessor.projected().source_identity
        if source.source_identity.instance_id == predecessor.instance_id:
            message = "Copy predecessor and surviving source must have distinct content identities"
            raise ValueError(message)
        resolved = captured.key_lock if source.key_lock is None else source.key_lock
        if self.resolved_key_lock != resolved:
            message = "Copy resolved Key Lock differs from its original captured inheritance"
            raise ValueError(message)
        if self.after != copy_after_snapshot(self.source, self.target_identity):
            message = "Copy after seal changes captured musical content or durable lineage"
            raise ValueError(message)
        # The established Copy projection checks C freshness against every original
        # project assignment while preserving A and all unrelated content/settings.
        project_with_copy_snapshot(
            captured, self.target_id, self.source, target_identity=self.target_identity
        )

    def _validate_before_image(self, project: ProjectState) -> None:
        if (
            self.image_role != "before"
            or self.committed_revision is not None
            or self.committed_config_sha256 is not None
        ):
            message = "Uncommitted Copy record cannot claim an after Config receipt"
            raise ValueError(message)
        if (
            self.phase == "captured"
            and self.resume_of is None
            and self.project_image_json != self.captured_project_image_json
        ):
            message = "Initial captured Copy record must retain the complete original project image"
            raise ValueError(message)
        if not written_copy_image_matches(project, self.predecessor, self.target_id):
            message = "Copy before project changes the exact original target predecessor"
            raise ValueError(message)
        if self.phase == "captured" and (
            not written_copy_image_matches(project, self.source, self.source_id)
        ):
            message = "Captured Copy image changes its original surviving source"
            raise ValueError(message)
        if project.config_revision > self.intent_revision:
            message = "Copy before project image has a newer unrecorded revision"
            raise ValueError(message)
        if self.phase != "cancelled":
            self._validate_resolved_key_lock(project)
        project_with_copy_snapshot(
            project, self.target_id, self.source, target_identity=self.target_identity
        )

    def _validate_committed_image(self, project: ProjectState) -> None:
        message = "Copy committed record lacks its actual same-revision fixed-C Config outcome"
        if (
            self.image_role != "after"
            or self.committed_revision != self.intent_revision
            or self.committed_config_sha256 is None
        ):
            raise ValueError(message)
        if (
            project.config_revision != self.committed_revision
            or hashlib.sha256(self.project_image_json.encode("utf-8")).hexdigest()
            != self.committed_config_sha256
        ):
            raise ValueError(message)
        if not written_copy_image_matches(project, self.after, self.target_id):
            raise ValueError(message)
        self._validate_resolved_key_lock(project)

    def _validate_resolved_key_lock(self, project: ProjectState) -> None:
        override = self.after.projected().key_lock
        resolved = project.key_lock if override is None else override
        if resolved != self.resolved_key_lock:
            message = "Copy current project changes fixed C's resolved Key Lock inheritance"
            raise ValueError(message)

    def changed(self, **updates: object) -> Self:
        """Validate the complete next record before attempting an owned append."""
        return type(self).model_validate(self.model_dump() | updates)

    def committed(self, receipt: ConfigCommitReceipt) -> Self:
        """Record actual written bytes, digest and the SAME committed intent revision."""
        return self.changed(
            phase="config_committed",
            image_role="after",
            project_image_json=receipt.written_text,
            intent_revision=receipt.revision,
            committed_revision=receipt.revision,
            committed_config_sha256=receipt.sha256,
            error=None,
        )


def project_with_journal_copy(
    project: ProjectState, journal: CopyArrangementJournal
) -> ProjectState:
    """Project fixed C over exact B, or recognize C without copying from newer A.

    This is detached intent only. The caller must independently own current
    Native publication/acceptance and all required original/stem leases.
    """
    journal = CopyArrangementJournal.model_validate(journal)
    if written_copy_image_matches(project, journal.after, journal.target_id):
        return validated_project_image(project.model_dump_json())
    if not written_copy_image_matches(project, journal.predecessor, journal.target_id):
        message = "Copy target is neither the original predecessor nor the fixed successor"
        raise ValueError(message)
    return project_with_copy_snapshot(
        project, journal.target_id, journal.source, target_identity=journal.target_identity
    )


_ORIGINAL_FIELDS = (
    "kind",
    "schema_version",
    "transaction_id",
    "resume_of",
    "config_reference",
    "captured_revision",
    "captured_config_sha256",
    "source_id",
    "target_id",
    "source",
    "predecessor",
    "after",
    "resolved_key_lock",
    "captured_project_image_json",
)
_NEXT_PHASES = {
    "captured": {"captured", "publication_pending", "cancelled"},
    "publication_pending": {"publication_pending", "native_ack_observed", "cancelled"},
    "native_ack_observed": {"native_ack_observed", "config_committed"},
    "config_committed": {"config_committed", "native_ack_observed", "settled"},
    "settled": {"settled"},
    "cancelled": {"cancelled"},
}


def validated_copy_history(encoded: list[str], transaction_id: str) -> list[CopyArrangementJournal]:
    """Validate every complete ordinal, immutable A/B/C seal and phase transition."""
    if not encoded or len(encoded) > JOURNAL_RECORDS:
        message = "Copy journal history is empty or exceeds Native phase capacity"
        raise ValueError(message)
    records = []
    for text in encoded:
        data = strict_json_object(text)
        if set(data) != set(CopyArrangementJournal.model_fields):
            message = "Copy journal ordinal is incomplete or contains unknown fields"
            raise ValueError(message)
        records.append(CopyArrangementJournal.model_validate_json(text, strict=True))
    first = records[0]
    if first.phase != "captured" or first.transaction_id != transaction_id:
        message = "Copy history does not begin with its directory-bound captured record"
        raise ValueError(message)
    for previous, current in pairwise(records):
        if (
            any(getattr(current, name) != getattr(first, name) for name in _ORIGINAL_FIELDS)
            or current.phase not in _NEXT_PHASES[previous.phase]
            or current.intent_revision < previous.intent_revision
            or _changed_committed_intent(previous, current)
        ):
            message = "Copy history changes original binding/seals or contains an invalid phase"
            raise ValueError(message)
    return records


def _changed_committed_intent(
    previous: CopyArrangementJournal,
    current: CopyArrangementJournal,
) -> bool:
    if previous.committed_revision is None:
        return False
    if current.phase == "native_ack_observed":
        # A fresh full pre-C image must precede replacement for newer surviving
        # source/unrelated intent. It cannot erase the receipt at the same revision.
        return current.intent_revision <= previous.intent_revision
    if current.phase == "config_committed":
        # Every newer replacement needs its own prior complete pre-C ordinal.
        return current.intent_revision != previous.intent_revision or (
            written_project_intent(validated_project_image(current.project_image_json))
            != written_project_intent(validated_project_image(previous.project_image_json))
        )
    return (
        current.committed_revision != previous.committed_revision
        or current.committed_config_sha256 != previous.committed_config_sha256
        or current.project_image_json != previous.project_image_json
    )


class CopyJournalOwner:
    """Retain exact intended append bytes through uncertain Native journal outcomes.

    The coordinator retains this owner before its initial ``append`` and must
    persist captured evidence before preparation. Terminal append follows only
    actual preclaim cancellation/worker return or complete durable/native finish.
    """

    def __init__(
        self,
        samples: Path,
        initial: CopyArrangementJournal,
        store: MaterialMigrationJournalStore | None = None,
    ) -> None:
        self.samples = samples
        self.initial = CopyArrangementJournal.model_validate(initial)
        self.encoded: list[str] = []
        self.journal: CopyArrangementJournal | None = None
        self.intended: str | None = None
        if store is None:
            if len(MaterialMigrationJournalStore.scan(str(samples))) >= JOURNAL_TRANSACTIONS:
                message = "Copy journal inventory capacity exhausted; transaction remains owned"
                raise ValueError(message)
            store = MaterialMigrationJournalStore(str(samples), initial.transaction_id)
        self.store = store

    @classmethod
    def open(cls, samples: Path, transaction_id: str) -> Self:
        """Reopen complete history with fresh file custody and no saved runtime authority."""
        store = MaterialMigrationJournalStore.open(str(samples), transaction_id)
        encoded = store.history()
        records = validated_copy_history(encoded, transaction_id)
        owner = cls(samples, records[0], store)
        owner.encoded = encoded
        owner.journal = records[-1]
        return owner

    def _adopt(self, store: MaterialMigrationJournalStore, encoded: list[str]) -> None:
        records = validated_copy_history(encoded, self.initial.transaction_id)
        self.store = store
        self.encoded = encoded
        self.journal = records[-1]
        self.intended = None

    def _reopen_intended(self) -> bool:
        store = MaterialMigrationJournalStore.open(str(self.samples), self.initial.transaction_id)
        current = store.history()
        if self.intended is not None and current == [*self.encoded, self.intended]:
            self._adopt(store, current)
            return True
        if current != self.encoded:
            message = "Copy append history differs from its exact owned bytes; evidence retained"
            raise ValueError(message)
        self.store = store
        return False

    def append(self, record: CopyArrangementJournal) -> CopyArrangementJournal:
        """Append once or adopt exact completed bytes; partial evidence stays owned."""
        validated = CopyArrangementJournal.model_validate(record)
        text = validated.model_dump_json()
        if self.intended is not None and self.intended != text:
            message = "Previous Copy append outcome is unresolved; retain its exact intended bytes"
            raise ValueError(message)
        if self.intended is None and self.journal == validated:
            if self.store.history() != self.encoded:
                message = "Copy retry history changed; evidence retained"
                raise ValueError(message)
            return validated
        validated_copy_history([*self.encoded, text], self.initial.transaction_id)
        if self.intended is not None and self._reopen_intended():
            return validated
        if self.store.history() != self.encoded:
            message = "Copy journal changed before append; evidence retained"
            raise ValueError(message)
        self.intended = text
        try:
            self.store.append(text)
        except OSError, RuntimeError, TypeError, ValueError:
            if not self._reopen_intended():
                raise
        else:
            if not self._reopen_intended():
                message = "Native Copy append returned without its complete intended record"
                raise ValueError(message)
        return validated

    def compact_into(self, successor: Self, persistence: ProjectPersistence) -> None:
        """Transfer the closed writer only to a complete exact durable successor."""
        parent, child = self._durable_successor(successor)
        if self.store.history() != self.encoded or successor.store.history() != successor.encoded:
            message = "Copy compaction history changed; evidence retained"
            raise ValueError(message)
        parent_history = validated_copy_history(self.encoded, parent.transaction_id)
        child_history = validated_copy_history(successor.encoded, child.transaction_id)
        if (
            parent_history[-1] != parent
            or child_history[-1] != child
            or persistence.config_reference != child.config_reference
            or persistence.revision != child.committed_revision
            or persistence.config_path.read_bytes() != child.project_image_json.encode("utf-8")
        ):
            message = "Copy successor lacks the actual selected Config outcome; evidence retained"
            raise ValueError(message)
        persistence.transfer_migration(parent.transaction_id, child.transaction_id)
        self.store.compact(self.encoded)

    def _durable_successor(
        self, successor: Self
    ) -> tuple[CopyArrangementJournal, CopyArrangementJournal]:
        parent, child = self.journal, successor.journal
        message = "Copy compaction lacks a recognized complete durable successor"
        if parent is None or child is None:
            raise ValueError(message)
        if (
            self.intended is not None
            or successor.intended is not None
            or parent.phase in {"settled", "cancelled"}
        ):
            raise ValueError(message)
        if (
            child.resume_of != parent.transaction_id
            or child.phase not in {"config_committed", "settled"}
            or child.captured_revision < parent.intent_revision
        ):
            raise ValueError(message)
        shared = (
            "config_reference",
            "captured_config_sha256",
            "source_id",
            "target_id",
            "source",
            "predecessor",
            "after",
            "resolved_key_lock",
            "captured_project_image_json",
        )
        if any(getattr(child, name) != getattr(parent, name) for name in shared):
            raise ValueError(message)
        return parent, child
