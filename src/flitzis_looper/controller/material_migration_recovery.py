"""Inspect bounded durable intent without restoring a runtime ticket or ACK."""

import hashlib
import json
import os
from dataclasses import dataclass, field
from pathlib import Path

from pydantic import ValidationError

from flitzis_looper.material_migration_model import MaterialMigrationJournal
from flitzis_looper.models import ProjectState
from flitzis_looper_audio import MaterialMigrationJournalStore


@dataclass
class MigrationRecovery:
    """Pending journal evidence requires a fresh process-local fence and authority."""

    pending: list[MaterialMigrationJournal] = field(default_factory=list)
    settled: list[MaterialMigrationJournal] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)
    intent: ProjectState | None = None


def _validated_record(record: dict[str, object]) -> MaterialMigrationJournal:
    raw = record["record"]
    if not isinstance(raw, str):
        message = "Migration journal has no complete record"
        raise TypeError(message)
    journal = MaterialMigrationJournal.model_validate_json(raw)
    if journal.transaction_id != record["transaction_id"]:
        message = "Migration journal transaction identity does not match its directory"
        raise ValueError(message)
    return journal


def _committed_current(journal: MaterialMigrationJournal, current: ProjectState | None) -> bool:
    if current is None or journal.committed_revision is None:
        return False
    return (
        current.material_migrations.get(journal.transaction_id) == journal.alias
        and current.config_revision >= journal.committed_revision
    )


def _record_for_config(raw: str, config_reference: str) -> dict[str, object] | None:
    record = json.loads(raw)
    if not isinstance(record, dict):
        message = "Migration scan record is not a recognized object"
        raise TypeError(message)
    journal_text = record.get("record")
    if not isinstance(journal_text, str):
        return record
    data = json.loads(journal_text)
    if not isinstance(data, dict):
        message = "Migration journal is not a recognized object"
        raise TypeError(message)
    binding = data.get("config_reference")
    if type(binding) is not str or not Path(binding).is_absolute():
        message = "Migration journal has no recognized project config binding"
        raise ValueError(message)
    if os.path.normcase(os.path.abspath(binding)) != config_reference:
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


def inspect_migration_recovery(samples: Path, config: Path) -> MigrationRecovery:
    """Keep incomplete/unknown journal data visible and leave every artifact untouched."""
    result = MigrationRecovery()
    if not samples.exists():
        return result
    try:
        config_reference = os.path.normcase(str(config.resolve()))
        records = MaterialMigrationJournalStore.scan(str(samples))
        config_bytes = config.read_bytes() if config.exists() else None
        digest = hashlib.sha256(config_bytes).hexdigest() if config_bytes is not None else None
        current = (
            ProjectState.model_validate_json(config_bytes) if config_bytes is not None else None
        )
    except (OSError, RuntimeError, ValueError) as error:
        result.errors.append(str(error))
        return result
    for raw in records:
        try:
            record = _record_for_config(raw, config_reference)
            if record is None:
                continue
            if record["error"] is not None or record["record"] is None:
                result.errors.append(f"Unresolved migration journal: {record['transaction_id']}")
                continue
            journal = _validated_record(record)
            if _settled(journal, current, digest):
                result.settled.append(journal)
                continue
            result.pending.append(journal)
            # Only the exact captured disk image admits recovery of unsaved current intent.
            # A different config remains current; no historical snapshot overwrites it.
            if journal.config_sha256 == digest:
                intent = ProjectState.model_validate_json(journal.snapshot_json)
                intent.config_revision = max(intent.config_revision, journal.intent_revision)
                if result.intent is None or intent.config_revision > result.intent.config_revision:
                    result.intent = intent
        except (KeyError, TypeError, ValueError, ValidationError) as error:
            result.errors.append(str(error))
    return result
