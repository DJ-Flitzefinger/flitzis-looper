"""Bounded background artifact proof and cross-project retirement reconciliation."""

import hashlib
import json
import os
from concurrent.futures import Future, ThreadPoolExecutor
from dataclasses import dataclass
from itertools import islice
from pathlib import Path
from typing import TYPE_CHECKING, Literal

from flitzis_looper.controller.material_migration_history import (
    journal_history,
    merge_artifacts,
    rollback_artifacts,
)
from flitzis_looper.material_migration_model import (
    MaterialMigrationJournal,
    MigrationArtifactEvidence,
    MigrationArtifactRecord,
)
from flitzis_looper.models import ProjectState
from flitzis_looper.project_materials import original_asset, resolve_asset
from flitzis_looper_audio import (
    MaterialMigrationJournalStore,
    MigrationArtifactLease,
    MigrationProjectGuard,
)

if TYPE_CHECKING:
    from collections.abc import Iterable

    from flitzis_looper_audio import MigrationInventoryLease


def _config_identity(metadata: os.stat_result) -> tuple[int, ...]:
    """Bind the opened file and current path; reading may change access time."""
    return (
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_mode,
        metadata.st_size,
        metadata.st_mtime_ns,
        getattr(metadata, "st_birthtime_ns", 0),
        getattr(metadata, "st_file_attributes", 0),
        getattr(metadata, "st_reparse_tag", 0),
    )


@dataclass(frozen=True)
class ArtifactRequest:
    """One related old or target object whose complete identity is captured off-thread."""

    reference: str
    role: Literal["rollback", "target"]
    created: bool = False
    required: bool = True


@dataclass
class ArtifactCapture:
    """Complete evidence and real pins retained until settlement or retirement."""

    records: tuple[MigrationArtifactRecord, ...]
    leases: list[MigrationArtifactLease]

    def release(self) -> None:
        """Drop only this capture's runtime pins, never infer a deletion permission."""
        for lease in self.leases:
            lease.release()
        self.leases.clear()


@dataclass
class CleanupResult:
    """Queueing checked retirement is distinct from actual final-reader deletion."""

    queued: tuple[str, ...] = ()
    protected: tuple[str, ...] = ()
    errors: tuple[str, ...] = ()
    pending: tuple[str, ...] = ()
    inventory: MigrationInventoryLease | None = None
    leases: tuple[MigrationArtifactLease, ...] = ()
    journal: MaterialMigrationJournal | None = None


class MigrationCleanupQueue:
    """Serial checked retirement remains independent of the source/adoption controller."""

    def __init__(self, samples: Path) -> None:
        self._samples = samples
        self._queue: list[MaterialMigrationJournal] = []
        self._deferred: list[MaterialMigrationJournal] = []
        self._journal: MaterialMigrationJournal | None = None
        self._future: Future[CleanupResult] | None = None
        self._pending: CleanupResult | None = None
        self.error: str | None = None

    @property
    def busy(self) -> bool:
        return self._future is not None or self._pending is not None

    @property
    def preparing(self) -> bool:
        """A physical last-reader wait does not prevent the next material's adoption."""
        return self._future is not None

    def add(self, journal: MaterialMigrationJournal) -> None:
        """Queue recognized receipts without assuming their cleanup outcome."""
        if not journal.cleanup_complete and (journal.artifacts or journal.alias is not None):
            self._queue = [
                item for item in self._queue if item.transaction_id != journal.transaction_id
            ]
            self._deferred = [
                item for item in self._deferred if item.transaction_id != journal.transaction_id
            ]
            self._queue.append(journal)

    def retry(self) -> None:
        """Recheck the finite protected set when known references or readers change."""
        self._queue.extend(self._deferred)
        self._deferred.clear()
        self.error = None

    def poll(
        self, reconciler: MigrationArtifactReconciler, current: ProjectState, *, admit: bool
    ) -> None:
        """Advance outcomes; a queued physical deletion is never a completion receipt."""
        if self._pending is not None:
            if not reconciler.settled(self._pending):
                return
            self._finish(self._pending)
            self._pending = None
        if self._future is not None:
            if not self._future.done():
                return
            self._collect()
        if not self.busy and admit and self._queue:
            self._journal = self._queue.pop(0)
            try:
                self._future = reconciler.retire(self._journal, current)
            except (OSError, RuntimeError, ValueError) as error:
                self._defer(str(error))

    def _collect(self) -> None:
        future = self._future
        if future is None:
            return
        self._future = None
        try:
            result = future.result()
            if result.pending:
                self._pending = result
            else:
                self._finish(result)
        except (OSError, RuntimeError, ValueError) as error:
            self._defer(str(error))

    def _defer(self, message: str) -> None:
        self.error = message
        if self._journal is not None:
            self._deferred.append(self._journal)
        self._journal = None

    def _finish(self, result: CleanupResult) -> None:
        journal = result.journal or self._journal
        if journal is None:
            return
        # A freshly upgraded ledger is durable even when its files stay protected.
        # Retry must carry that actual record instead of the pre-upgrade snapshot.
        self._journal = journal
        if result.errors or result.protected:
            self._defer("; ".join(result.errors) or "cleanup protected by saved references")
            return
        try:
            store = MaterialMigrationJournalStore.open(str(self._samples), journal.transaction_id)
            store.append(journal.changed(cleanup_complete=True).model_dump_json())
        except (OSError, RuntimeError, ValueError) as error:
            self._defer(str(error))
        else:
            self._journal = None


class MigrationArtifactReconciler:
    """Use existing native leases/retirement after complete known-project inventory."""

    def __init__(self, samples: Path, config: Path) -> None:
        self._samples = samples
        self._config = os.path.normcase(str(config.resolve()))
        self._guard = MigrationProjectGuard(str(samples), self._config)
        self._worker = ThreadPoolExecutor(max_workers=1, thread_name_prefix="material-recovery")
        self._pending: (
            Future[ArtifactCapture]
            | Future[CleanupResult]
            | Future[MaterialMigrationJournal]
            | None
        ) = None

    def capture(self, requests: Iterable[ArtifactRequest]) -> Future[ArtifactCapture]:
        """Hash complete sealed objects in one bounded worker rather than UI polling."""
        self._check_available()
        captured = tuple(requests)
        if not captured or len(captured) > 1024:
            message = "migration artifact capture exceeds its bounded request set"
            raise ValueError(message)
        result = self._worker.submit(self._capture, captured)
        self._pending = result
        return result

    def retire(
        self, journal: MaterialMigrationJournal, current: ProjectState
    ) -> Future[CleanupResult]:
        """Reopen exact receipts and queue only proven obsolete rollback objects."""
        self._check_available()
        image = ProjectState.model_validate(current.model_dump())
        result = self._worker.submit(self._retire, journal, image)
        self._pending = result
        return result

    def transfer_history(
        self,
        parent: MaterialMigrationJournal,
        child: MaterialMigrationJournal,
        current: ProjectState,
    ) -> Future[MaterialMigrationJournal]:
        """Transfer exact recognized evidence before compacting one obsolete parent."""
        self._check_available()
        image = ProjectState.model_validate(current.model_dump())
        result = self._worker.submit(self._transfer_history, parent, child, image)
        self._pending = result
        return result

    def _transfer_history(
        self,
        parent: MaterialMigrationJournal,
        child: MaterialMigrationJournal,
        current: ProjectState,
    ) -> MaterialMigrationJournal:
        inventory = self._guard.lock_inventory()
        try:
            _, errors = self._known_configs(inventory.records())
            if errors:
                raise ValueError("; ".join(errors))
            return self._transfer_recognized_history(parent, child, current)
        finally:
            inventory.release()

    def _transfer_recognized_history(
        self,
        parent: MaterialMigrationJournal,
        child: MaterialMigrationJournal,
        current: ProjectState,
    ) -> MaterialMigrationJournal:
        if child.phase != "config_committed" or child.resume_of != parent.transaction_id:
            message = "history transfer requires a durably committed actual successor"
            raise ValueError(message)
        self._check_current_alias(child, current)
        parent_store = MaterialMigrationJournalStore.open(str(self._samples), parent.transaction_id)
        encoded, records = journal_history(parent_store, parent)
        parent = self._upgrade_ledger(parent, records)
        child_store = MaterialMigrationJournalStore.open(str(self._samples), child.transaction_id)
        _, history = journal_history(child_store, child)
        updated = history[-1].changed(
            artifacts=merge_artifacts(child.artifacts, rollback_artifacts(parent))
        )
        if updated != child:
            child_store.append(updated.model_dump_json())
        # The exact current config binds the successor before any old metadata is
        # removed. The full artifact lineage is now durable even while readers live.
        self._check_current_alias(updated, current)
        parent_store.compact(encoded)
        return updated

    def _check_current_alias(
        self, journal: MaterialMigrationJournal, current: ProjectState
    ) -> None:
        saved = self._read_config(Path(self._config))
        if journal.alias is None or journal.committed_revision is None:
            message = "history has no committed alias binding"
            raise ValueError(message)
        if (
            journal.config_reference != self._config
            or saved.material_migrations.get(journal.transaction_id) != journal.alias
            or current.material_migrations.get(journal.transaction_id) != journal.alias
            or saved.config_revision < journal.committed_revision
        ):
            message = "current selected project does not bind the committed migration alias"
            raise ValueError(message)

    def _upgrade_ledger(
        self, journal: MaterialMigrationJournal, history: list[MaterialMigrationJournal]
    ) -> MaterialMigrationJournal:
        """Published P2a history identifies candidates; only fresh native proof owns bytes."""
        if journal.artifacts:
            return journal
        references, stem_versions = self._old_upgrade_inputs(journal, history)
        if journal.alias is not None:
            references.add(journal.alias.old_reference)
        if journal.alias is not None:
            references.update(self._legacy_pcm({journal.alias.original_sha256}))
        requests = tuple(
            ArtifactRequest(reference, "rollback")
            for reference in references
            if resolve_asset(Path(reference), project_root=self._samples.parent).path.exists()
        )
        capture = self._capture(requests)
        try:
            self._check_upgraded_stems(capture.records, stem_versions)
            if journal.alias is not None:
                for artifact in capture.records:
                    if artifact.evidence.kind == "original" and (
                        artifact.evidence.files[0].sha256 != journal.alias.original_sha256
                        or artifact.evidence.files[0].bytes != journal.alias.original_bytes
                    ):
                        message = "old original changed before fresh P2a ledger upgrade; preserved"
                        raise ValueError(message)
            return journal.changed(artifacts=capture.records)
        finally:
            capture.release()

    def _old_upgrade_inputs(
        self, journal: MaterialMigrationJournal, history: list[MaterialMigrationJournal]
    ) -> tuple[set[str], dict[str, str]]:
        references = {item.old_reference for item in journal.assignments}
        if journal.alias is not None:
            references.discard(journal.alias.new_reference)
        versions: dict[str, str] = {}
        for record in history:
            image = ProjectState.model_validate_json(record.snapshot_json)
            for assignment in record.assignments:
                entry = image.stem_cache[assignment.sample_id]
                if entry is None:
                    continue
                if journal.alias is not None:
                    if entry.source_version == journal.alias.new_source_version:
                        # Published target provenance is unknown; never infer rollback rights.
                        continue
                    if entry.source_version != journal.alias.old_source_version:
                        message = "old stem lineage changed before fresh P2a ledger upgrade"
                        raise ValueError(message)
                asset = resolve_asset(entry.cache_dir, project_root=self._samples.parent)
                if asset.kind != "stem_directory":
                    message = "old stem lineage does not name a typed stem directory"
                    raise ValueError(message)
                reference = "samples/" + asset.path.relative_to(self._samples).as_posix()
                if reference in versions and versions[reference] != entry.source_version:
                    message = "old stem lineage is ambiguous across migration history"
                    raise ValueError(message)
                versions[reference] = entry.source_version
                references.add(reference)
        return references, versions

    def _check_upgraded_stems(
        self, records: tuple[MigrationArtifactRecord, ...], versions: dict[str, str]
    ) -> None:
        for record in records:
            evidence = record.evidence
            if evidence.kind != "stem_directory":
                continue
            path = (
                resolve_asset(Path(evidence.reference), project_root=self._samples.parent).path
                / ".complete.json"
            )
            with path.open("rb") as reader:
                encoded = reader.read(65537)
            marker = next(item for item in evidence.files if item.name == ".complete.json")
            if (
                len(encoded) > 65536
                or hashlib.sha256(encoded).hexdigest() != marker.sha256
                or json.loads(encoded).get("source_version") != versions[evidence.reference]
            ):
                message = "old stem marker changed before fresh P2a ledger upgrade; preserved"
                raise ValueError(message)

    def _check_available(self) -> None:
        if self._pending is not None and not self._pending.done():
            message = "material reconciliation worker already owns a bounded request"
            raise RuntimeError(message)

    def _capture(self, requests: tuple[ArtifactRequest, ...]) -> ArtifactCapture:
        result = ArtifactCapture((), [])
        records: list[MigrationArtifactRecord] = []
        seen: set[tuple[str, str]] = set()
        try:
            for request in requests:
                try:
                    lease = MigrationArtifactLease.capture(str(self._samples), request.reference)
                except OSError, RuntimeError, ValueError:
                    if request.required:
                        raise
                    continue
                result.leases.append(lease)
                evidence = MigrationArtifactEvidence.model_validate_json(lease.receipt_json())
                key = request.role, evidence.reference
                if key in seen:
                    lease.release()
                    result.leases.pop()
                    continue
                seen.add(key)
                records.append(
                    MigrationArtifactRecord(
                        role=request.role, created=request.created, evidence=evidence
                    )
                )
            original_digests = {
                item.evidence.files[0].sha256
                for item in records
                if item.role == "rollback" and item.evidence.kind == "original"
            }
            for reference in self._legacy_pcm(original_digests):
                if ("rollback", reference) in seen:
                    continue
                seen.add(("rollback", reference))
                try:
                    lease = MigrationArtifactLease.capture(str(self._samples), reference)
                except OSError, RuntimeError, ValueError:
                    # A corrupt warm candidate remains protected; ordinary preparation
                    # may still decode the verified original without repairing it.
                    continue
                result.leases.append(lease)
                evidence = MigrationArtifactEvidence.model_validate_json(lease.receipt_json())
                records.append(
                    MigrationArtifactRecord(role="rollback", created=False, evidence=evidence)
                )
        except OSError, RuntimeError, ValueError:
            result.release()
            raise
        result.records = tuple(records)
        return result

    def _legacy_pcm(self, digests: set[str]) -> list[str]:
        """Discover bounded complete legacy candidates; native capture verifies every byte."""
        root = self._samples / ".pcm-cache" / "v1"
        if not digests or not root.exists():
            return []
        metadata = root.stat(follow_symlinks=False)
        if (
            root.is_symlink()
            or getattr(metadata, "st_file_attributes", 0) & 0x400
            or not root.is_dir()
        ):
            message = "legacy PCM inventory is not an ordinary directory"
            raise ValueError(message)
        entries = list(islice(root.iterdir(), 257))
        if len(entries) > 256:
            message = "legacy PCM inventory capacity exceeded"
            raise ValueError(message)
        result: list[str] = []
        for entry in entries:
            if not entry.name.startswith(".ready-"):
                continue
            try:
                metadata = entry.stat(follow_symlinks=False)
                if (
                    entry.is_symlink()
                    or getattr(metadata, "st_file_attributes", 0) & 0x400
                    or not entry.is_dir()
                ):
                    continue
                manifest = entry / "manifest.json"
                with manifest.open("rb") as reader:
                    encoded = reader.read(65537)
                if len(encoded) > 65536:
                    continue
                parsed = json.loads(encoded)
                digest = (
                    parsed
                    .get("descriptor", {})
                    .get("decoder", {})
                    .get("original", {})
                    .get("sha256")
                )
                if digest in digests:
                    result.append("samples/" + entry.relative_to(self._samples).as_posix())
            except OSError, RuntimeError, ValueError, AttributeError:
                # Unknown, partial or corrupt generations remain untouched.
                continue
        return result

    def _known_configs(self, records: list[str]) -> tuple[set[Path], list[str]]:
        configs = {Path(self._config)}
        errors: list[str] = []
        for raw in records:
            record = json.loads(raw)
            if not isinstance(record, dict) or record.get("error") is not None:
                errors.append("unknown or unqueryable migration project owner")
                continue
            reference = record.get("config_reference")
            if type(reference) is not str or not Path(reference).is_absolute():
                errors.append("migration project inventory has no actual config binding")
                continue
            path = Path(reference)
            if not path.is_relative_to(self._samples.parent):
                errors.append("migration project binding leaves the guarded project root")
                continue
            configs.add(path)
            if (
                record.get("instance_id") != self._guard.instance_id
                and record.get("live") is not False
            ):
                errors.append("another live or unqueryable process retains migration data")
        # Older projects have no registry. Recognize ordinary top-level saved configs
        # as well; malformed candidates block cleanup rather than being ignored.
        entries = list(islice(self._samples.iterdir(), 1025))
        if len(entries) > 1024 or len(configs) > 256:
            errors.append("migration project reference inventory capacity exceeded")
            return configs, errors
        for entry in entries:
            if entry.name == "config.json" or entry.name.endswith(".config.json"):
                configs.add(entry)
        if len(configs) > 256:
            errors.append("migration project reference inventory capacity exceeded")
        return configs, errors

    @staticmethod
    def _read_config(config: Path) -> ProjectState:
        before = config.stat(follow_symlinks=False)
        if (
            config.is_symlink()
            or getattr(before, "st_file_attributes", 0) & 0x400
            or not config.is_file()
        ):
            message = f"missing or non-ordinary known project config: {config}"
            raise ValueError(message)
        if before.st_size > 16 * 1024 * 1024:
            message = "project config exceeds bounded inventory size"
            raise ValueError(message)
        with config.open("rb") as reader:
            opened = os.fstat(reader.fileno())
            if _config_identity(before) != _config_identity(opened):
                message = "known project config changed before reference inventory"
                raise ValueError(message)
            content = reader.read(16 * 1024 * 1024 + 1)
            finished = os.fstat(reader.fileno())
        after = config.stat(follow_symlinks=False)
        if (
            _config_identity(before) != _config_identity(finished)
            or _config_identity(before) != _config_identity(after)
            or before.st_ctime_ns != after.st_ctime_ns
            or opened.st_ctime_ns != finished.st_ctime_ns
            or len(content) > 16 * 1024 * 1024
        ):
            message = "known project config changed during reference inventory"
            raise ValueError(message)
        return ProjectState.model_validate_json(content)

    def _references(
        self, current: ProjectState, records: list[str]
    ) -> tuple[set[Path], set[str], list[str]]:
        configs, errors = self._known_configs(records)
        paths: set[Path] = set()
        legacy_digests: set[str] = set()
        images = [current]
        for config in configs:
            try:
                images.append(self._read_config(config))
            except (OSError, RuntimeError, ValueError) as error:
                errors.append(str(error))
        for image in images:
            self._original_references(image, paths, legacy_digests, errors)
            self._stem_references(image, paths, errors)
        return paths, legacy_digests, errors

    def _original_references(
        self, image: ProjectState, paths: set[Path], digests: set[str], errors: list[str]
    ) -> None:
        for reference in image.sample_paths:
            if reference is None:
                continue
            try:
                asset = original_asset(reference, project_root=self._samples.parent)
                paths.add(asset.path)
                if asset.material_id is None:
                    lease = MigrationArtifactLease.capture(str(self._samples), reference)
                    try:
                        evidence = MigrationArtifactEvidence.model_validate_json(
                            lease.receipt_json()
                        )
                        digests.add(evidence.files[0].sha256)
                    finally:
                        lease.release()
            except (OSError, RuntimeError, ValueError) as error:
                errors.append(str(error))

    def _stem_references(self, image: ProjectState, paths: set[Path], errors: list[str]) -> None:
        for entry in image.stem_cache:
            if entry is not None:
                try:
                    paths.add(
                        resolve_asset(Path(entry.cache_dir), project_root=self._samples.parent).path
                    )
                    if entry.pair is not None:
                        paths.update(
                            resolve_asset(reference, project_root=self._samples.parent).path
                            for reference in (
                                entry.pair.wav_generation,
                                entry.pair.pcm_generation,
                                entry.pair.descriptor_reference,
                            )
                        )
                except (OSError, RuntimeError, ValueError) as error:
                    errors.append(str(error))

    def _retire(self, journal: MaterialMigrationJournal, current: ProjectState) -> CleanupResult:
        if not journal.artifacts and journal.alias is not None:
            self._check_current_alias(journal, current)
            store = MaterialMigrationJournalStore.open(str(self._samples), journal.transaction_id)
            _, history = journal_history(store, journal)
            journal = self._upgrade_ledger(journal, history)
            store.append(journal.model_dump_json())
        inventory: MigrationInventoryLease | None = None
        try:
            inventory = self._guard.lock_inventory()
            paths, legacy_digests, errors = self._references(current, inventory.records())
        except (OSError, RuntimeError, ValueError) as error:
            if inventory is not None:
                inventory.release()
            return CleanupResult(errors=(str(error),), journal=journal)
        if errors:
            inventory.release()
            return CleanupResult(errors=tuple(errors), journal=journal)
        queued: list[str] = []
        protected: list[str] = []
        leases: list[MigrationArtifactLease] = []
        for record in journal.artifacts:
            try:
                status, lease = self._retire_record(
                    record, journal, paths, legacy_digests, inventory
                )
                if status == "protected":
                    protected.append(record.evidence.reference)
                elif status == "queued":
                    queued.append(record.evidence.reference)
                    if lease is not None:
                        leases.append(lease)
            except (OSError, RuntimeError, ValueError) as error:
                errors.append(str(error))
        # Every actual queue entry owns the native lock until its physical terminal.
        # Dropping Python state or shutting down cannot open registration early.
        inventory.release()
        return CleanupResult(
            tuple(queued),
            tuple(protected),
            tuple(errors),
            tuple(queued),
            leases=tuple(leases),
            journal=journal,
        )

    def _retire_record(
        self,
        record: MigrationArtifactRecord,
        journal: MaterialMigrationJournal,
        paths: set[Path],
        legacy_digests: set[str],
        inventory: MigrationInventoryLease,
    ) -> tuple[Literal["queued", "protected", "skip"], MigrationArtifactLease | None]:
        evidence = record.evidence
        if record.role != "rollback" and not (journal.phase == "failed" and record.created):
            return "skip", None
        path = resolve_asset(Path(evidence.reference), project_root=self._samples.parent).path
        if path in paths or any(reference.is_relative_to(path) for reference in paths):
            return "protected", None
        if not path.exists():
            return "skip", None
        if evidence.kind == "pcm_directory" and legacy_digests:
            return "protected", None
        lease = MigrationArtifactLease.reopen(str(self._samples), evidence.model_dump_json())
        try:
            lease.retire(inventory)
        finally:
            lease.release()
        return "queued", lease

    def settled(self, result: CleanupResult) -> bool:
        """Observe actual queue outcomes; changed objects settle as preserved errors."""
        statuses = tuple(lease.retirement_status() for lease in result.leases)
        if any(status == "pending" for status in statuses):
            return False
        errors = tuple(
            lease.retirement_error() or "retirement did not finish with a recognized outcome"
            for lease, status in zip(result.leases, statuses, strict=True)
            if status != "complete"
        )
        result.errors += errors
        result.leases = ()
        if result.inventory is not None:
            result.inventory.release()
        return True

    def shut_down(self) -> None:
        """Keep physical worker leases until its real return, then close the process guard."""
        self._worker.shutdown(wait=True, cancel_futures=False)
        self._guard.release()
