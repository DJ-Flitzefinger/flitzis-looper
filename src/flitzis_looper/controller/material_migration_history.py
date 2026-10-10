"""Bounded metadata lineage; journal history never supplies runtime authority."""

from typing import TYPE_CHECKING

from flitzis_looper.material_migration_model import (
    MaterialMigrationAlias,
    MaterialMigrationJournal,
    MigrationArtifactRecord,
)

if TYPE_CHECKING:
    from collections.abc import Iterable

    from flitzis_looper.models import ProjectState
    from flitzis_looper_audio import MaterialMigrationJournalStore


def check_alias_capacity(current: ProjectState, parent: MaterialMigrationJournal | None) -> None:
    """Reject an unrelated full history before any native preparation or source claim."""
    replacing = parent is not None and parent.transaction_id in current.material_migrations
    if len(current.material_migrations) + 1 - replacing > 216:
        message = "migration alias capacity exhausted before native preparation"
        raise ValueError(message)


def journal_history(
    store: MaterialMigrationJournalStore, expected: MaterialMigrationJournal
) -> tuple[list[str], list[MaterialMigrationJournal]]:
    """Require all exact records to belong to the same selected project and transaction."""
    encoded = store.history()
    records = [MaterialMigrationJournal.model_validate_json(text) for text in encoded]
    if not records or any(
        item.transaction_id != expected.transaction_id
        or item.config_reference != expected.config_reference
        for item in records
    ):
        message = "journal history has an unknown project or transaction binding"
        raise ValueError(message)
    if records[-1] != expected:
        message = "journal changed before metadata reconciliation"
        raise ValueError(message)
    return encoded, records


def merge_artifacts(
    existing: tuple[MigrationArtifactRecord, ...], incoming: Iterable[MigrationArtifactRecord]
) -> tuple[MigrationArtifactRecord, ...]:
    """Duplicate references must retain exactly the same full immutable proof."""
    result = {(item.role, item.evidence.reference): item for item in existing}
    for item in incoming:
        key = item.role, item.evidence.reference
        previous = result.get(key)
        if previous is not None and previous != item:
            message = "artifact lineage changed for the same saved reference"
            raise ValueError(message)
        result[key] = item
    if len(result) > 1024:
        message = "migration artifact ledger capacity exceeded"
        raise ValueError(message)
    return tuple(result.values())


def rollback_artifacts(journal: MaterialMigrationJournal) -> tuple[MigrationArtifactRecord, ...]:
    """A successor rechecks earlier files and inherits no producer rollback rights."""
    return merge_artifacts(
        (),
        (
            MigrationArtifactRecord(role="rollback", created=False, evidence=item.evidence)
            for item in journal.artifacts
        ),
    )


def successor_aliases(
    current: ProjectState, alias: MaterialMigrationAlias, parent: MaterialMigrationJournal | None
) -> dict[str, MaterialMigrationAlias]:
    """Replace one recognized parent with one durable child within the original 216 bound."""
    aliases = dict(current.material_migrations)
    if parent is not None:
        previous = aliases.get(parent.transaction_id)
        if previous is not None and previous != parent.alias:
            message = "newer parent alias wins over captured recovery lineage"
            raise ValueError(message)
        aliases.pop(parent.transaction_id, None)
    aliases[alias.transaction_id] = alias
    if len(aliases) > 216:
        message = "migration alias capacity exhausted before adoption"
        raise ValueError(message)
    return aliases
