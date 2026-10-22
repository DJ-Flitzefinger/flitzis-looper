"""Strict Pair evidence and its bounded append owner, without runtime authority."""

import hashlib
import json
from itertools import pairwise
from pathlib import Path
from typing import TYPE_CHECKING, Literal, Self

from pydantic import BaseModel, ConfigDict, Field, TypeAdapter, field_validator, model_validator

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.copy_snapshot import canonical_snapshot_json, musical_content_data
from flitzis_looper.material_migration_model import MigrationDigest, MigrationID
from flitzis_looper.models import ProjectState
from flitzis_looper.pair_snapshot import PairSlotData, PairSnapshot
from flitzis_looper_audio import MaterialMigrationJournalStore

if TYPE_CHECKING:
    from flitzis_looper.controller.persistence import ConfigCommitReceipt

JOURNAL_BYTES = 16 * 1024 * 1024
PROJECT_IMAGE_BYTES = 8 * 1024 * 1024
JOURNAL_RECORDS = 16
JOURNAL_TRANSACTIONS = 256
# Derive real runtime Field constraints from the one existing migration schema.
# Ordinary string annotations avoid deferred alias lookup during model creation.
_ID_PATTERN: str = TypeAdapter(MigrationID).json_schema()["pattern"]
_DIGEST_PATTERN: str = TypeAdapter(MigrationDigest).json_schema()["pattern"]

type PairJournalPhase = Literal[
    "captured",
    "publication_pending",
    "native_ack_observed",
    "config_committed",
    "settled",
    "cancelled",
]


def _unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            message = "journal JSON contains duplicate object keys"
            raise ValueError(message)
        result[key] = value
    return result


def _reject_constant(value: str) -> object:
    message = f"journal JSON contains a nonfinite number: {value}"
    raise ValueError(message)


def strict_json_object(text: str) -> dict[str, object]:
    """Decode one unambiguous finite JSON object without ignoring duplicate keys."""
    value = json.loads(text, object_pairs_hook=_unique_object, parse_constant=_reject_constant)
    if not isinstance(value, dict):
        message = "journal evidence must be a JSON object"
        raise TypeError(message)
    return value


def validated_project_image(text: str) -> ProjectState:
    """Require a strictly understood current or known historical project image."""
    if len(text.encode("utf-8")) > PROJECT_IMAGE_BYTES:
        message = "Pair project image exceeds bounded UTF8 size"
        raise ValueError(message)
    data = strict_json_object(text)
    fields = set(ProjectState.model_fields)
    computed = set(ProjectState.model_computed_fields)
    missing = fields - data.keys()
    historical_fields = {
        "global_timing_policy",
        "pad_source_beatmap_intent",
        "pad_source_sync_intent",
        "resident_pcm_budget_automatic",
    }
    if not missing <= historical_fields or data.keys() - fields - computed:
        message = "Pair project image is incomplete or contains unknown fields"
        raise ValueError(message)
    project = ProjectState.model_validate_json(text, strict=True)
    expected = project.model_dump(mode="json", exclude_computed_fields=True, warnings="error")
    for field in missing:
        expected.pop(field)
    if {key: value for key, value in data.items() if key not in computed} != expected:
        message = "Pair project image contains unrecognized or inconsistent nested metadata"
        raise ValueError(message)
    identities = [item.instance_id for item in project.pad_content if item is not None]
    if len(identities) != len(set(identities)):
        message = "Pair project image contains duplicate content identities"
        raise ValueError(message)
    return project


class PairArrangementAfterData(BaseModel):
    """Two complete resulting slots; Move is allowed to have an empty source."""

    model_config = ConfigDict(extra="forbid", strict=True, revalidate_instances="always")
    source_id: int = Field(ge=0, le=215)
    target_id: int = Field(ge=0, le=215)
    source: PairSlotData
    target: PairSlotData

    @model_validator(mode="after")
    def _distinct_slots(self) -> Self:
        if self.source_id == self.target_id:
            message = "Pair after image requires two distinct slots"
            raise ValueError(message)
        return self


class PairArrangementAfterImage(BaseModel):
    """Immutable canonical resulting musical seal, never an occupied-source capture."""

    model_config = ConfigDict(frozen=True, extra="forbid", revalidate_instances="always")
    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    payload_json: str = Field(strict=True, min_length=1)

    @field_validator("payload_json")
    @classmethod
    def _seal(cls, value: str) -> str:
        strict_json_object(value)
        data = PairArrangementAfterData.model_validate_json(value, strict=True)
        return canonical_snapshot_json(data.model_dump(mode="json", warnings="error"))

    def projected(self) -> PairArrangementAfterData:
        """Revalidate the seal and detach both complete child images."""
        validated = type(self).model_validate(self)
        return PairArrangementAfterData.model_validate_json(validated.payload_json, strict=True)

    @classmethod
    def from_before(cls, before: PairSnapshot) -> Self:
        """Compute exactly surviving lineage and independently empty Move defaults."""
        captured = PairSnapshot.model_validate(before).projected()
        source = (
            PairSlotData.model_validate(musical_content_data(ProjectState(), captured.source_id))
            if captured.operation == "move"
            else captured.target
        )
        data = PairArrangementAfterData(
            source_id=captured.source_id,
            target_id=captured.target_id,
            source=source,
            target=captured.source,
        )
        return cls(payload_json=canonical_snapshot_json(data.model_dump(mode="json")))


def _without_accepted_timing(data: dict[str, object]) -> dict[str, object]:
    analysis = data.get("analysis")
    if isinstance(analysis, dict):
        analysis = dict(analysis)
        analysis["accepted_timing"] = None
        if analysis == {
            "bpm": 0.0,
            "key": "",
            "beat_grid": {"beats": [], "downbeats": [], "bars": []},
            "accepted_timing": None,
        }:
            analysis = None
        data |= {"analysis": analysis}
    return data


def written_pair_image_matches(project: ProjectState, after: PairArrangementAfterImage) -> bool:
    """Compare complete intent while allowing the existing verified-save evidence export.

    Save normalizes source paths and exports fresh historical accepted timing.
    Neither a recorded export nor stem availability confers runtime eligibility.
    Every other musical field and durable lineage must retain the original seal.
    """
    image = after.projected()
    for sample_id, expected in ((image.source_id, image.source), (image.target_id, image.target)):
        actual_data = musical_content_data(project, sample_id)
        expected_data = expected.model_dump(mode="json", warnings="error")
        actual = PairSlotData.model_validate(actual_data).model_dump(mode="json", warnings="error")
        actual_path = actual.get("source_path")
        expected_path = expected_data.get("source_path")
        if actual_path is not None and expected_path is not None:
            if Path(str(actual_path)).absolute() != Path(str(expected_path)).absolute():
                return False
            actual["source_path"] = expected_path
        actual = _without_accepted_timing(actual)
        expected_data = _without_accepted_timing(expected_data)
        if actual != expected_data:
            return False
    return True


def written_project_intent(project: ProjectState) -> dict[str, object]:
    """Compare the complete saved intent independently of historical timing exports."""
    data = project.model_dump(mode="json", exclude_computed_fields=True, warnings="error")
    paths = ProjectPersistence._normalize_sample_paths_for_save(project.sample_paths)
    data["sample_paths"] = paths
    path_objects: list[object] = list(paths)
    key_lock_objects: list[object] = list(project.pad_key_lock)
    data["pad_key_lock"] = ProjectPersistence._normalize_pad_key_lock_for_save(
        path_objects,
        key_lock_objects,
    )
    analyses = data["sample_analysis"]
    intents = data["pad_timing_intent"]
    for sample_id, analysis in enumerate(analyses):
        if analysis is not None:
            analysis["accepted_timing"] = None
            if analysis == {
                "bpm": 0.0,
                "key": "",
                "beat_grid": {"beats": [], "downbeats": [], "bars": []},
                "accepted_timing": None,
            }:
                analyses[sample_id] = None
        if paths[sample_id] is None:
            intents[sample_id] = "legacy"
    return data


class PairArrangementJournal(BaseModel):
    """One strictly versioned Pair record with complete musical and Config evidence."""

    model_config = ConfigDict(
        frozen=True, extra="forbid", strict=True, revalidate_instances="always"
    )
    kind: Literal["pad_pair"] = "pad_pair"
    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    transaction_id: str = Field(strict=True, pattern=_ID_PATTERN)
    resume_of: str | None = Field(default=None, strict=True, pattern=_ID_PATTERN)
    config_reference: str = Field(strict=True, min_length=1, max_length=8192)
    captured_revision: int = Field(strict=True, ge=0)
    intent_revision: int = Field(strict=True, ge=0)
    captured_config_sha256: str | None = Field(strict=True, pattern=_DIGEST_PATTERN)
    before: PairSnapshot
    after: PairArrangementAfterImage
    project_image_json: str = Field(strict=True, min_length=1)
    image_role: Literal["before", "after"]
    phase: PairJournalPhase
    committed_revision: int | None = Field(default=None, strict=True, ge=0)
    committed_config_sha256: str | None = Field(default=None, strict=True, pattern=_DIGEST_PATTERN)
    error: str | None = Field(default=None, strict=True, max_length=2048)

    @field_validator("before", mode="before")
    @classmethod
    def _unambiguous_original_seal(cls, value: object) -> object:
        payload = (
            value.payload_json
            if isinstance(value, PairSnapshot)
            else (value.get("payload_json") if isinstance(value, dict) else None)
        )
        if isinstance(payload, str):
            strict_json_object(payload)
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
            message = "Pair journal exceeds Native bounded UTF8 record size"
            raise ValueError(message)
        return self

    def _validate_original_evidence(self) -> None:
        if not Path(self.config_reference).is_absolute() or self.resume_of == self.transaction_id:
            message = "Pair journal requires actual absolute Config binding and distinct lineage"
            raise ValueError(message)
        if self.intent_revision < self.captured_revision:
            message = "Pair intent revision cannot precede its captured revision"
            raise ValueError(message)
        if self.after != PairArrangementAfterImage.from_before(self.before):
            message = "Pair after seal changes original content, lineage or empty Move defaults"
            raise ValueError(message)

    def _validate_committed_image(self, project: ProjectState) -> None:
        message = "Pair committed record lacks the actual same-revision after Config outcome"
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
        if not written_pair_image_matches(project, self.after):
            raise ValueError(message)

    def _validate_before_image(self, project: ProjectState) -> None:
        if (
            self.image_role != "before"
            or self.committed_revision is not None
            or self.committed_config_sha256 is not None
        ):
            message = "Uncommitted Pair record cannot claim an after Config receipt"
            raise ValueError(message)
        captured = self.before.projected()
        for sample_id, expected in (
            (captured.source_id, captured.source),
            (captured.target_id, captured.target),
        ):
            if PairSlotData.model_validate(musical_content_data(project, sample_id)) != expected:
                message = "Pair before project image differs from the original participant seals"
                raise ValueError(message)
        if project.config_revision > self.intent_revision:
            message = "Pair before project image has a newer unrecorded revision"
            raise ValueError(message)

    def changed(self, **updates: object) -> Self:
        """Validate the full next record before staging an append."""
        return type(self).model_validate(self.model_dump() | updates)

    def committed(self, receipt: ConfigCommitReceipt) -> Self:
        """Record only actual written text, digest and the SAME committed intent revision."""
        return self.changed(
            phase="config_committed",
            image_role="after",
            project_image_json=receipt.written_text,
            intent_revision=receipt.revision,
            committed_revision=receipt.revision,
            committed_config_sha256=receipt.sha256,
            error=None,
        )


_ORIGINAL_FIELDS = (
    "kind",
    "schema_version",
    "transaction_id",
    "resume_of",
    "config_reference",
    "captured_revision",
    "captured_config_sha256",
    "before",
    "after",
)
_NEXT_PHASES = {
    "captured": {"captured", "publication_pending", "cancelled"},
    "publication_pending": {"publication_pending", "native_ack_observed", "cancelled"},
    "native_ack_observed": {"native_ack_observed", "config_committed"},
    "config_committed": {"config_committed", "settled"},
    "settled": {"settled"},
    "cancelled": {"cancelled"},
}


def validated_pair_history(encoded: list[str], transaction_id: str) -> list[PairArrangementJournal]:
    """Validate every full ordinal and its immutable original Pair transaction seals."""
    if not encoded or len(encoded) > JOURNAL_RECORDS:
        message = "Pair journal history is empty or exceeds Native phase capacity"
        raise ValueError(message)
    records = []
    for text in encoded:
        strict_json_object(text)
        records.append(PairArrangementJournal.model_validate_json(text, strict=True))
    first = records[0]
    if first.phase != "captured" or first.transaction_id != transaction_id:
        message = "Pair history does not begin with its directory-bound captured record"
        raise ValueError(message)
    for previous, current in pairwise(records):
        if (
            any(getattr(current, name) != getattr(first, name) for name in _ORIGINAL_FIELDS)
            or current.phase not in _NEXT_PHASES[previous.phase]
            or current.intent_revision < previous.intent_revision
            or _changed_committed_intent(previous, current)
        ):
            message = "Pair history changes original binding/seals or contains an invalid phase"
            raise ValueError(message)
    return records


def _changed_committed_intent(
    previous: PairArrangementJournal,
    current: PairArrangementJournal,
) -> bool:
    if previous.committed_revision is None:
        return False
    if current.phase == "config_committed":
        # Newer unrelated intent can be committed while the owning mirrors/finish retry.
        # At the same revision only the existing timing export/normalization may differ.
        return current.intent_revision == previous.intent_revision and (
            written_project_intent(validated_project_image(current.project_image_json))
            != written_project_intent(validated_project_image(previous.project_image_json))
        )
    return (
        current.committed_revision != previous.committed_revision
        or current.committed_config_sha256 != previous.committed_config_sha256
        or current.project_image_json != previous.project_image_json
    )


class PairJournalOwner:
    """Own exact intended bytes until one complete append is proven or remains fenced.

    Construction creates an empty Native journal, then the caller MUST persist
    ``append(initial)`` before preparation/publication. Keeping the owner in its
    coordinator before that call retains an initial failed or partial append.
    ``cancelled`` and ``settled`` may be appended only after the coordinator's
    actual live preclaim cancellation or every mirror and Native finish.
    """

    def __init__(
        self,
        samples: Path,
        initial: PairArrangementJournal,
        store: MaterialMigrationJournalStore | None = None,
    ) -> None:
        self.samples = samples
        self.initial = PairArrangementJournal.model_validate(initial)
        self.encoded: list[str] = []
        self.journal: PairArrangementJournal | None = None
        self.intended: str | None = None
        if store is None:
            if len(MaterialMigrationJournalStore.scan(str(samples))) >= JOURNAL_TRANSACTIONS:
                message = "Pair journal inventory capacity exhausted; transaction remains owned"
                raise ValueError(message)
            store = MaterialMigrationJournalStore(str(samples), initial.transaction_id)
        self.store = store

    @classmethod
    def open(cls, samples: Path, transaction_id: str) -> Self:
        """Reopen complete evidence with fresh file custody and no historical runtime owner."""
        store = MaterialMigrationJournalStore.open(str(samples), transaction_id)
        encoded = store.history()
        records = validated_pair_history(encoded, transaction_id)
        owner = cls(samples, records[0], store)
        owner.encoded = encoded
        owner.journal = records[-1]
        return owner

    def _adopt(self, store: MaterialMigrationJournalStore, encoded: list[str]) -> None:
        records = validated_pair_history(encoded, self.initial.transaction_id)
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
            message = "Pair append history differs from its exact owned bytes; evidence retained"
            raise ValueError(message)
        self.store = store
        return False

    def append(self, record: PairArrangementJournal) -> PairArrangementJournal:
        """Append once or reconcile a complete failed append; partial evidence stays owned."""
        validated = PairArrangementJournal.model_validate(record)
        text = validated.model_dump_json()
        if self.intended is not None and self.intended != text:
            message = "Previous Pair append outcome is unresolved; retain its exact intended bytes"
            raise ValueError(message)
        if self.intended is None and self.journal == validated:
            # Repeated retry errors neither spend ordinal capacity nor create new evidence.
            current = self.store.history()
            if current != self.encoded:
                message = "Pair retry history changed; evidence retained"
                raise ValueError(message)
            return validated
        validated_pair_history([*self.encoded, text], self.initial.transaction_id)
        if self.intended is not None and self._reopen_intended():
            return validated
        if self.store.history() != self.encoded:
            message = "Pair journal changed before append; evidence retained"
            raise ValueError(message)
        self.intended = text
        try:
            self.store.append(text)
        except OSError, RuntimeError, TypeError, ValueError:
            if not self._reopen_intended():
                raise
        else:
            if not self._reopen_intended():
                message = "Native Pair append returned without its complete intended record"
                raise ValueError(message)
        return validated

    def compact_into(self, successor: Self, persistence: ProjectPersistence) -> None:
        """Transfer the closed writer fence then compact only exact recognized history.

        The successor's actual Config must already be durably recorded. On any
        failure both owner objects and every unremoved record remain retained.
        """
        parent, child = self._durable_successor(successor)
        if self.store.history() != self.encoded or successor.store.history() != successor.encoded:
            message = "Pair compaction history changed; evidence retained"
            raise ValueError(message)
        parent_history = validated_pair_history(self.encoded, parent.transaction_id)
        child_history = validated_pair_history(successor.encoded, child.transaction_id)
        if (
            parent_history[-1] != parent
            or child_history[-1] != child
            or persistence.config_reference != child.config_reference
            or persistence.revision != child.committed_revision
            or persistence.config_path.read_bytes() != child.project_image_json.encode("utf-8")
        ):
            message = (
                "Pair successor no longer has the actual selected Config outcome; evidence retained"
            )
            raise ValueError(message)
        persistence.transfer_migration(parent.transaction_id, child.transaction_id)
        self.store.compact(self.encoded)

    def _durable_successor(
        self, successor: Self
    ) -> tuple[PairArrangementJournal, PairArrangementJournal]:
        parent = self.journal
        child = successor.journal
        message = "Pair compaction lacks a recognized complete durable successor"
        if parent is None or child is None:
            raise ValueError(message)
        if (
            self.intended is not None
            or successor.intended is not None
            or child.resume_of != parent.transaction_id
            or child.phase not in {"config_committed", "settled"}
        ):
            raise ValueError(message)
        if (
            child.config_reference != parent.config_reference
            or child.before != parent.before
            or child.after != parent.after
        ):
            raise ValueError(message)
        return parent, child
