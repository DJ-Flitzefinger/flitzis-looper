"""Inspect bounded durable intent without restoring a runtime ticket or ACK."""

import hashlib
import os
from dataclasses import dataclass, field
from pathlib import Path

from pydantic import ValidationError

import flitzis_looper_audio
from flitzis_looper.controller.bank_arrangement_journal import (
    BankArrangementJournal,
    validated_bank_history,
)
from flitzis_looper.controller.copy_arrangement_journal import (
    CopyArrangementJournal,
    validated_copy_history,
)
from flitzis_looper.controller.pair_arrangement_journal import (
    PairArrangementJournal,
    strict_json_object,
    validated_pair_history,
)
from flitzis_looper.material_migration_model import MaterialMigrationJournal
from flitzis_looper.models import ProjectState
from flitzis_looper_audio import MaterialMigrationJournalStore


@dataclass
class MigrationRecovery:
    """Pending journal evidence requires a fresh process-local fence and authority."""

    pending: list[MaterialMigrationJournal] = field(default_factory=list)
    settled: list[MaterialMigrationJournal] = field(default_factory=list)
    pair_journals: list[PairArrangementJournal] = field(default_factory=list)
    copy_journals: list[CopyArrangementJournal] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)
    intent: ProjectState | None = None
    bank_journals: list[BankArrangementJournal] = field(default_factory=list)

    @property
    def unfinished_arrangement_kinds(self) -> tuple[str, ...]:
        """Known owners requiring arrangement recovery, never Legacy alias cleanup."""
        return tuple(
            kind
            for kind, unfinished in (
                (
                    "pad_pair",
                    any(item.phase not in {"settled", "cancelled"} for item in self.pair_journals),
                ),
                (
                    "pad_copy_overwrite",
                    any(item.phase not in {"settled", "cancelled"} for item in self.copy_journals),
                ),
                (
                    "pad_bank",
                    any(item.phase not in {"settled", "cancelled"} for item in self.bank_journals),
                ),
            )
            if unfinished
        )


type TransactionJournal = (
    MaterialMigrationJournal
    | PairArrangementJournal
    | CopyArrangementJournal
    | BankArrangementJournal
)


def parse_transaction_journal(raw: str) -> TransactionJournal:
    """Route only known strict kinds; unknown durable evidence remains unresolved."""
    data = strict_json_object(raw)
    if "kind" not in data:
        return MaterialMigrationJournal.model_validate_json(raw)
    if (
        data.get("kind") == "pad_pair"
        and type(data.get("schema_version")) is int
        and data["schema_version"] == 1
    ):
        return PairArrangementJournal.model_validate_json(raw, strict=True)
    if (
        data.get("kind") == "pad_copy_overwrite"
        and type(data.get("schema_version")) is int
        and data["schema_version"] == 1
    ):
        return CopyArrangementJournal.model_validate_json(raw, strict=True)
    if (
        data.get("kind") == "pad_bank"
        and type(data.get("schema_version")) is int
        and data["schema_version"] == 1
    ):
        return BankArrangementJournal.model_validate_json(raw, strict=True)
    message = "Unknown migration transaction kind or schema version retained unresolved"
    raise ValueError(message)


def validated_transaction_history(
    store: MaterialMigrationJournalStore,
    transaction_id: str,
) -> tuple[list[str], list[TransactionJournal]]:
    """Read all sealed ordinals before accepting any selected transaction tail."""
    encoded = store.history()
    if not encoded:
        message = "Migration journal has no complete history"
        raise ValueError(message)
    journals = [parse_transaction_journal(text) for text in encoded]
    first = journals[0]
    if any(
        type(item) is not type(first)
        or item.schema_version != first.schema_version
        or item.transaction_id != transaction_id
        or item.config_reference != first.config_reference
        for item in journals
    ):
        message = "Migration history changes kind, version, Config or directory transaction binding"
        raise ValueError(message)
    if isinstance(first, PairArrangementJournal):
        validated_pair_history(encoded, transaction_id)
    elif isinstance(first, CopyArrangementJournal):
        validated_copy_history(encoded, transaction_id)
    elif isinstance(first, BankArrangementJournal):
        validated_bank_history(encoded, transaction_id)
    return encoded, journals


def _validated_record(record: dict[str, object], samples: Path) -> TransactionJournal:
    raw = record["record"]
    if not isinstance(raw, str):
        message = "Migration journal has no complete record"
        raise TypeError(message)
    journal = parse_transaction_journal(raw)
    if journal.transaction_id != record["transaction_id"]:
        message = "Migration journal transaction identity does not match its directory"
        raise ValueError(message)
    store = MaterialMigrationJournalStore.open(str(samples), journal.transaction_id)
    encoded, history = validated_transaction_history(store, journal.transaction_id)
    if encoded[-1] != raw or history[-1] != journal:
        message = "Migration history changed since scan; evidence retained unresolved"
        raise ValueError(message)
    return history[-1]


def _committed_current(journal: MaterialMigrationJournal, current: ProjectState | None) -> bool:
    if current is None or journal.committed_revision is None:
        return False
    return (
        current.material_migrations.get(journal.transaction_id) == journal.alias
        and current.config_revision >= journal.committed_revision
    )


def resolve_config_reference(samples: Path, reference: str) -> str:
    """Resolve location through Native custody, without granting journal authority."""
    resolver = getattr(flitzis_looper_audio, "resolve_migration_config_reference", None)
    if resolver is None:
        message = "Native config-location resolution is unavailable; migration data preserved"
        raise RuntimeError(message)
    resolved = resolver(str(samples), reference)
    if type(resolved) is not str:
        message = "Native config-location resolver returned no recognized location"
        raise TypeError(message)
    return os.path.normcase(os.path.abspath(resolved))


def config_reference_matches(samples: Path, current: str, historical: str | None) -> bool:
    if historical is None:
        return False
    return resolve_config_reference(samples, current) == resolve_config_reference(
        samples, historical
    )


def _record_for_config(raw: str, samples: Path, config_reference: str) -> dict[str, object] | None:
    record = strict_json_object(raw)
    if not isinstance(record, dict):
        message = "Migration scan record is not a recognized object"
        raise TypeError(message)
    journal_text = record.get("record")
    if not isinstance(journal_text, str):
        return record
    data = strict_json_object(journal_text)
    if not isinstance(data, dict):
        message = "Migration journal is not a recognized object"
        raise TypeError(message)
    binding = data.get("config_reference")
    if type(binding) is not str or not Path(binding).is_absolute():
        message = "Migration journal has no recognized project config binding"
        raise ValueError(message)
    if resolve_config_reference(samples, binding) != config_reference:
        # A known other config owns this intent even when its bytes match.
        # Inspect no foreign config and acquire none of its owners or fences.
        return None
    return record


def _settled(
    journal: MaterialMigrationJournal, current: ProjectState | None, digest: str | None
) -> bool:
    restored = current.material_migrations.get(journal.transaction_id) if current else None
    durable_alias = (
        (
            restored is not None
            and all(
                (content := current.pad_content[item.sample_id]) is None
                or content.instance_id != item.instance_id
                or current.sample_paths[item.sample_id] != item.old_reference
                for item in journal.assignments
            )
            and journal.alias is not None
            and restored == journal.alias
        )
        if current is not None
        else False
    )
    return (
        journal.phase == "failed"
        or durable_alias
        or (
            journal.phase == "config_committed"
            and (digest == journal.committed_config_sha256 or _committed_current(journal, current))
        )
    )


def _include_legacy_journal(
    result: MigrationRecovery,
    journal: MaterialMigrationJournal,
    current: ProjectState | None,
    digest: str | None,
) -> None:
    if _settled(journal, current, digest):
        result.settled.append(journal)
        return
    result.pending.append(journal)
    # Only the exact captured disk image admits recovery of unsaved current intent.
    # A different config remains current; no historical snapshot overwrites it.
    if journal.config_sha256 == digest:
        intent = ProjectState.model_validate_json(journal.snapshot_json)
        intent.config_revision = max(intent.config_revision, journal.intent_revision)
        if result.intent is None or intent.config_revision > result.intent.config_revision:
            result.intent = intent


def _include_transaction(
    result: MigrationRecovery,
    journal: TransactionJournal,
    current: ProjectState | None,
    digest: str | None,
) -> None:
    # Known arrangements never enter Legacy alias/artifact cleanup or its source router.
    if isinstance(journal, PairArrangementJournal):
        result.pair_journals.append(journal)
    elif isinstance(journal, CopyArrangementJournal):
        result.copy_journals.append(journal)
    elif isinstance(journal, BankArrangementJournal):
        result.bank_journals.append(journal)
    else:
        _include_legacy_journal(result, journal, current, digest)


def inspect_migration_recovery(samples: Path, config: Path) -> MigrationRecovery:
    """Keep incomplete/unknown journal data visible and leave every artifact untouched."""
    result = MigrationRecovery()
    if not samples.exists():
        return result
    try:
        config_reference = resolve_config_reference(samples, str(config.resolve()))
        records = MaterialMigrationJournalStore.scan(str(samples))
        located_config = Path(config_reference)
        config_bytes = located_config.read_bytes() if located_config.exists() else None
        digest = hashlib.sha256(config_bytes).hexdigest() if config_bytes is not None else None
        current = (
            ProjectState.model_validate_json(config_bytes) if config_bytes is not None else None
        )
    except (OSError, RuntimeError, TypeError, ValueError) as error:
        result.errors.append(str(error))
        return result
    for raw in records:
        try:
            record = _record_for_config(raw, samples, config_reference)
            if record is None:
                continue
            if record["error"] is not None or record["record"] is None:
                result.errors.append(f"Unresolved migration journal: {record['transaction_id']}")
                continue
            journal = _validated_record(record, samples)
            _include_transaction(result, journal, current, digest)
        except (KeyError, OSError, RuntimeError, TypeError, ValueError, ValidationError) as error:
            result.errors.append(str(error))
    unfinished_kinds = int(bool(result.pending)) + len(result.unfinished_arrangement_kinds)
    if unfinished_kinds > 1:
        result.errors.append(
            "Incompatible unfinished Material/Pair/Copy/Bank owners retain every history"
        )
    return result
