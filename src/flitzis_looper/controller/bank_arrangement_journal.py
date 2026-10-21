"""Strict whole-bank durable evidence, without Native claim or restart authority."""

import hashlib
from itertools import pairwise
from pathlib import Path
from typing import TYPE_CHECKING, Literal, Self, cast

from pydantic import BaseModel, ConfigDict, Field, TypeAdapter, field_validator, model_validator

from flitzis_looper.bank_snapshot import BankSnapshot, BankSnapshotData, project_with_bank_snapshot
from flitzis_looper.controller.pair_arrangement_journal import (
    JOURNAL_BYTES,
    JOURNAL_RECORDS,
    JOURNAL_TRANSACTIONS,
    strict_json_object,
    validated_project_image,
    written_project_intent,
)
from flitzis_looper.copy_snapshot import (
    CONTENT_FIELDS,
    CopySnapshot,
    CopySnapshotData,
    canonical_snapshot_json,
    musical_snapshot_projection,
)
from flitzis_looper.material_migration_model import MigrationDigest, MigrationID
from flitzis_looper_audio import MaterialMigrationJournalStore

if TYPE_CHECKING:
    from flitzis_looper.controller.persistence import ConfigCommitReceipt
    from flitzis_looper.models import ProjectState
    from flitzis_looper.pair_snapshot import PairSlotData

_ID_PATTERN: str = TypeAdapter(MigrationID).json_schema()["pattern"]
_DIGEST_PATTERN: str = TypeAdapter(MigrationDigest).json_schema()["pattern"]

type BankJournalPhase = Literal[
    "captured",
    "publication_pending",
    "native_ack_observed",
    "config_committed",
    "settled",
    "cancelled",
]
type BankImageRole = Literal["before", "after"]


def _project_rows(
    project: ProjectState, rows: tuple[tuple[int, PairSlotData], ...]
) -> ProjectState:
    data = project.model_dump(mode="python", exclude_computed_fields=True, warnings="error")
    for sample_id, image in rows:
        musical = musical_snapshot_projection(image)
        for name, field in CONTENT_FIELDS:
            data[field][sample_id] = musical[name]
    return validated_project_image(canonical_snapshot_json(data))


def _rows_intent(
    project: ProjectState, rows: tuple[tuple[int, PairSlotData], ...]
) -> dict[str, object]:
    data = written_project_intent(project)
    entries = cast("list[dict[str, object] | None]", data["stem_cache"])
    for sample_id, _ in rows:
        entry = entries[sample_id]
        if entry is not None:
            # Existing musical_content_data/CopySnapshot never copy current eligibility.
            entry["available"] = False
    return data


def _rows_match(project: ProjectState, rows: tuple[tuple[int, PairSlotData], ...]) -> bool:
    return _rows_intent(project, rows) == _rows_intent(_project_rows(project, rows), rows)


def written_bank_project_intent(project: ProjectState, snapshot: BankSnapshot) -> dict[str, object]:
    """Normalize full Config intent while keeping target stem eligibility unclaimed."""
    bank = BankSnapshot.model_validate(snapshot).projected()
    return _rows_intent(project, tuple((row.target_id, row.after) for row in bank.slots))


def written_bank_image_matches(
    project: ProjectState, snapshot: BankSnapshot, *, image_role: BankImageRole
) -> bool:
    """Compare all36 targets with only the established whole-Config save normalization."""
    if image_role not in {"before", "after"}:
        message = "Bank image role must be before or after"
        raise ValueError(message)
    bank = BankSnapshot.model_validate(snapshot).projected()
    rows = tuple((row.target_id, getattr(row, image_role)) for row in bank.slots)
    return _rows_match(project, rows)


def _resolved_inheritance_current(project: ProjectState, bank: BankSnapshotData) -> None:
    for row in bank.slots:
        if row.after.source_identity is not None:
            resolved = project.key_lock if row.after.key_lock is None else row.after.key_lock
            if resolved != row.resolved_key_lock:
                message = "Bank project changes fixed after-image Key Lock inheritance"
                raise ValueError(message)


def _fresh_after_ids(project: ProjectState, bank: BankSnapshotData) -> None:
    current = {content.instance_id for content in project.pad_content if content is not None}
    if any(
        row.after.source_identity is not None and row.after.source_identity.instance_id in current
        for row in bank.slots
    ):
        message = "Bank fixed after identity is already used in the before project"
        raise ValueError(message)


class BankArrangementJournal(BaseModel):
    """One immutable36-cell intent and exact full-project Config outcome evidence."""

    model_config = ConfigDict(
        frozen=True, extra="forbid", strict=True, revalidate_instances="always"
    )
    kind: Literal["pad_bank"] = "pad_bank"
    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    transaction_id: str = Field(strict=True, pattern=_ID_PATTERN)
    resume_of: str | None = Field(default=None, strict=True, pattern=_ID_PATTERN)
    config_reference: str = Field(strict=True, min_length=1, max_length=8192)
    captured_revision: int = Field(strict=True, ge=0)
    intent_revision: int = Field(strict=True, ge=0)
    captured_config_sha256: str | None = Field(strict=True, pattern=_DIGEST_PATTERN)
    bank: BankSnapshot
    captured_project_image_json: str = Field(strict=True, min_length=1)
    project_image_json: str = Field(strict=True, min_length=1)
    image_role: BankImageRole
    phase: BankJournalPhase
    committed_revision: int | None = Field(default=None, strict=True, ge=0)
    committed_config_sha256: str | None = Field(default=None, strict=True, pattern=_DIGEST_PATTERN)
    error: str | None = Field(default=None, strict=True, max_length=2048)

    @field_validator("bank", mode="before")
    @classmethod
    def _complete_bank_seal(cls, value: object) -> object:
        if isinstance(value, dict) and set(value) != {"schema_version", "payload_json"}:
            message = "Bank journal snapshot wrapper is incomplete or contains unknown fields"
            raise ValueError(message)
        payload = (
            value.payload_json
            if isinstance(value, BankSnapshot)
            else (value.get("payload_json") if isinstance(value, dict) else None)
        )
        if isinstance(payload, str):
            raw = strict_json_object(payload)
            typed = BankSnapshotData.model_validate_json(payload, strict=True)
            if raw != typed.model_dump(mode="json", warnings="error"):
                message = "Bank journal seal contains incomplete nested musical metadata"
                raise ValueError(message)
        return value

    @model_validator(mode="after")
    def _consistent_evidence(self) -> Self:
        self._validate_original()
        project = validated_project_image(self.project_image_json)
        if self.phase in {"config_committed", "settled"}:
            self._validate_committed(project)
        else:
            self._validate_before(project)
        if len(self.model_dump_json().encode("utf-8")) > JOURNAL_BYTES:
            message = "Bank journal exceeds Native bounded UTF8 record size"
            raise ValueError(message)
        return self

    def _validate_original(self) -> None:
        if (
            not Path(self.config_reference).is_absolute()
            or self.resume_of == self.transaction_id
            or self.intent_revision < self.captured_revision
        ):
            message = "Bank journal requires actual Config binding and monotone original revision"
            raise ValueError(message)
        captured = validated_project_image(self.captured_project_image_json)
        if captured.config_revision > self.captured_revision:
            message = "Bank captured project has a newer unrecorded revision"
            raise ValueError(message)
        # The original image alone uses admission's complete source/target witnesses.
        # Later post-ACK source edits must not replace this immutable historical seal.
        project_with_bank_snapshot(captured, self.bank)

    def _validate_before(self, project: ProjectState) -> None:
        if (
            self.image_role != "before"
            or self.committed_revision is not None
            or self.committed_config_sha256 is not None
            or project.config_revision > self.intent_revision
        ):
            message = "Bank before record cannot claim an after receipt or unrecorded revision"
            raise ValueError(message)
        if (
            self.phase == "captured"
            and self.resume_of is None
            and self.project_image_json != self.captured_project_image_json
        ):
            message = "Initial Bank record must retain its complete original project image"
            raise ValueError(message)
        if not written_bank_image_matches(project, self.bank, image_role="before"):
            message = "Bank before project changes its complete original target image"
            raise ValueError(message)
        bank = self.bank.projected()
        if self.phase == "captured":
            sources = tuple(
                (row.source_id, row.source)
                for row in bank.slots
                if row.source_id is not None and row.source is not None
            )
            if not _rows_match(project, sources):
                message = "Captured Bank record changes its complete original source witnesses"
                raise ValueError(message)
        _fresh_after_ids(project, bank)
        if self.phase != "cancelled":
            _resolved_inheritance_current(project, bank)

    def _validate_committed(self, project: ProjectState) -> None:
        if (
            self.image_role != "after"
            or self.committed_revision != self.intent_revision
            or project.config_revision != self.committed_revision
        ):
            message = "Bank committed record lacks its exact same-revision complete Config outcome"
            raise ValueError(message)
        if (
            self.committed_config_sha256 is None
            or hashlib.sha256(self.project_image_json.encode("utf-8")).hexdigest()
            != self.committed_config_sha256
            or not written_bank_image_matches(project, self.bank, image_role="after")
        ):
            message = "Bank committed record lacks its exact same-revision complete Config outcome"
            raise ValueError(message)
        _resolved_inheritance_current(project, self.bank.projected())

    def changed(self, **updates: object) -> Self:
        """Validate the complete next ordinal before any owned durable append."""
        return type(self).model_validate(self.model_dump() | updates)

    def committed(self, receipt: ConfigCommitReceipt) -> Self:
        """Record only actual written bytes at the SAME current intent revision."""
        return self.changed(
            phase="config_committed",
            image_role="after",
            project_image_json=receipt.written_text,
            intent_revision=receipt.revision,
            committed_revision=receipt.revision,
            committed_config_sha256=receipt.sha256,
            error=None,
        )


def bank_before_project(project: ProjectState, journal: BankArrangementJournal) -> ProjectState:
    """Retain latest source/outside intent while placing the complete sealed old bank."""
    journal = BankArrangementJournal.model_validate(journal)
    rows = tuple((row.target_id, row.before) for row in journal.bank.projected().slots)
    return _project_rows(project, rows)


def project_with_journal_bank(
    project: ProjectState, journal: BankArrangementJournal
) -> ProjectState:
    """Project fixed after intent over ALL before targets, or recognize ALL after targets.

    Newer source/outside intent is retained. A changed-cell mixture is refused, even
    when each individual target matches one of its own before/after images. This
    function grants no Native publication, ACK, writer or recovery permissions.
    """
    journal = BankArrangementJournal.model_validate(journal)
    bank = journal.bank.projected()
    _resolved_inheritance_current(project, bank)
    if written_bank_image_matches(project, journal.bank, image_role="after"):
        return validated_project_image(project.model_dump_json())
    if not written_bank_image_matches(project, journal.bank, image_role="before"):
        message = "Bank targets are neither the complete original nor the complete fixed after bank"
        raise ValueError(message)
    _fresh_after_ids(project, bank)
    return _project_rows(project, tuple((row.target_id, row.after) for row in bank.slots))


def bank_journal_snapshots(journal: BankArrangementJournal) -> tuple[CopySnapshot, ...]:
    """Supply occupied source/before/after seals to the existing independent lease custody.

    The caller acquires these before publication and before startup restore/cleanup,
    and retains them until actual cancellation or Native/config/mirror/journal
    settlement. Empty witnesses need Native scalar fences, never fake file leases.
    """
    journal = BankArrangementJournal.model_validate(journal)
    return tuple(
        CopySnapshot.from_data(
            CopySnapshotData.model_validate(image.model_dump(mode="python"), strict=True)
        )
        for row in journal.bank.projected().slots
        for image in (row.source, row.before, row.after)
        if image is not None and image.source_identity is not None
    )


_ORIGINAL_FIELDS = (
    "kind",
    "schema_version",
    "transaction_id",
    "resume_of",
    "config_reference",
    "captured_revision",
    "captured_config_sha256",
    "bank",
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


def _changed_commit(previous: BankArrangementJournal, current: BankArrangementJournal) -> bool:
    if previous.committed_revision is None:
        return False
    if current.phase == "native_ack_observed":
        return current.intent_revision <= previous.intent_revision
    if current.phase == "config_committed":
        return current.intent_revision != previous.intent_revision or (
            written_project_intent(validated_project_image(current.project_image_json))
            != written_project_intent(validated_project_image(previous.project_image_json))
        )
    return (
        current.committed_revision != previous.committed_revision
        or current.committed_config_sha256 != previous.committed_config_sha256
        or current.project_image_json != previous.project_image_json
    )


def _receipt_matches_recorded_intent(
    previous: BankArrangementJournal, current: BankArrangementJournal
) -> bool:
    if previous.phase != "native_ack_observed" or current.phase != "config_committed":
        return True
    before = validated_project_image(previous.project_image_json)
    after = _project_rows(
        before, tuple((row.target_id, row.after) for row in previous.bank.projected().slots)
    )
    after.config_revision = previous.intent_revision
    return current.intent_revision == previous.intent_revision and written_bank_project_intent(
        validated_project_image(current.project_image_json), previous.bank
    ) == written_bank_project_intent(after, previous.bank)


def validated_bank_history(encoded: list[str], transaction_id: str) -> list[BankArrangementJournal]:
    """Validate every bounded ordinal, immutable36-cell seal and exact phase/revision edge."""
    if not encoded or len(encoded) > JOURNAL_RECORDS:
        message = "Bank journal history is empty or exceeds Native phase capacity"
        raise ValueError(message)
    records = []
    for text in encoded:
        raw = strict_json_object(text)
        if set(raw) != set(BankArrangementJournal.model_fields):
            message = "Bank journal ordinal is incomplete or contains unknown fields"
            raise ValueError(message)
        records.append(BankArrangementJournal.model_validate_json(text, strict=True))
    first = records[0]
    if first.phase != "captured" or first.transaction_id != transaction_id:
        message = "Bank history does not begin with its directory-bound captured record"
        raise ValueError(message)
    for previous, current in pairwise(records):
        if (
            any(getattr(current, name) != getattr(first, name) for name in _ORIGINAL_FIELDS)
            or current.phase not in _NEXT_PHASES[previous.phase]
            or current.intent_revision < previous.intent_revision
            or _changed_commit(previous, current)
            or not _receipt_matches_recorded_intent(previous, current)
        ):
            message = "Bank history changes original seals or contains an invalid phase/revision"
            raise ValueError(message)
    return records


def validate_bank_successor(parent: BankArrangementJournal, child: BankArrangementJournal) -> None:
    """Validate one edge after both complete histories have been independently checked.

    Inventory selection, fork/cycle detection, config relocation and compaction
    remain the startup coordinator's responsibility; this grants no writer transfer.
    """
    parent = BankArrangementJournal.model_validate(parent)
    child = BankArrangementJournal.model_validate(child)
    shared = ("config_reference", "captured_config_sha256", "bank", "captured_project_image_json")
    if (
        parent.phase in {"settled", "cancelled"}
        or child.resume_of != parent.transaction_id
        or child.captured_revision < parent.intent_revision
        or any(getattr(child, name) != getattr(parent, name) for name in shared)
    ):
        message = "Bank successor changes immutable capture, Config or original lineage"
        raise ValueError(message)


class BankJournalOwner:
    """Own exact intended append bytes through uncertain outcomes in the existing store.

    Retain this owner before initial append. Terminal records follow proven Native
    cancellation/worker return or completed Native/config/mirror settlement; this
    class cannot prove those effects and never releases source/action pins itself.
    """

    def __init__(
        self,
        samples: Path,
        initial: BankArrangementJournal,
        store: MaterialMigrationJournalStore | None = None,
    ) -> None:
        self.samples = samples
        self.initial = BankArrangementJournal.model_validate(initial)
        validated_bank_history([self.initial.model_dump_json()], self.initial.transaction_id)
        self.encoded: list[str] = []
        self.journal: BankArrangementJournal | None = None
        self.intended: str | None = None
        if store is None:
            if len(MaterialMigrationJournalStore.scan(str(samples))) >= JOURNAL_TRANSACTIONS:
                message = "Bank journal inventory capacity exhausted; transaction remains owned"
                raise ValueError(message)
            store = MaterialMigrationJournalStore(str(samples), initial.transaction_id)
        self.store = store

    @classmethod
    def open(cls, samples: Path, transaction_id: str) -> Self:
        """Reopen complete durable history, obtaining no saved Native authority."""
        store = MaterialMigrationJournalStore.open(str(samples), transaction_id)
        encoded = store.history()
        records = validated_bank_history(encoded, transaction_id)
        owner = cls(samples, records[0], store)
        owner.encoded, owner.journal = encoded, records[-1]
        return owner

    def _reopen_intended(self) -> bool:
        store = MaterialMigrationJournalStore.open(str(self.samples), self.initial.transaction_id)
        current = store.history()
        if self.intended is not None and current == [*self.encoded, self.intended]:
            records = validated_bank_history(current, self.initial.transaction_id)
            self.store, self.encoded, self.journal = store, current, records[-1]
            self.intended = None
            return True
        if current != self.encoded:
            message = "Bank append history differs from its exact owned bytes; evidence retained"
            raise ValueError(message)
        self.store = store
        return False

    def append(self, record: BankArrangementJournal) -> BankArrangementJournal:
        """Append once or adopt exact completed bytes; partial evidence remains owned."""
        validated = BankArrangementJournal.model_validate(record)
        text = validated.model_dump_json()
        if self.intended is not None and self.intended != text:
            message = "Previous Bank append remains unresolved; retain its exact intended bytes"
            raise ValueError(message)
        if self.intended is None and self.journal == validated:
            if self.store.history() != self.encoded:
                message = "Bank retry history changed; evidence retained"
                raise ValueError(message)
            return validated
        validated_bank_history([*self.encoded, text], self.initial.transaction_id)
        if self.intended is not None and self._reopen_intended():
            return validated
        if self.store.history() != self.encoded:
            message = "Bank journal changed before append; evidence retained"
            raise ValueError(message)
        self.intended = text
        try:
            self.store.append(text)
        except OSError, RuntimeError, TypeError, ValueError:
            if not self._reopen_intended():
                raise
        else:
            if not self._reopen_intended():
                message = "Native Bank append returned without its complete intended record"
                raise ValueError(message)
        return validated
