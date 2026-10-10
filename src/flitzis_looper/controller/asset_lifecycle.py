import os
import re
import threading
from contextlib import contextmanager
from itertools import islice
from pathlib import Path
from typing import TYPE_CHECKING

from flitzis_looper.models import STEM_KINDS
from flitzis_looper.project_materials import original_asset, resolve_asset

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper.models import ProjectState
    from flitzis_looper_audio import AudioEngine, ProjectAssetLease


class AssetRetirementReservation:
    """Reserve finite cleanup slots before a source or separator intent changes."""

    def __init__(self, assets: ProjectAssetLifecycle, count: int) -> None:
        self._assets = assets
        self.remaining = count

    @contextmanager
    def activate(self) -> Iterator[None]:
        """Use this transaction's reserved slots without releasing unused capacity."""
        previous = getattr(self._assets._reservation_context, "current", None)
        self._assets._reservation_context.current = self
        try:
            yield
        finally:
            self._assets._reservation_context.current = previous

    def close(self) -> None:
        """Return unused reservation slots after publication or cancellation settles."""
        with self._assets._retirement_lock:
            self._assets._reserved -= self.remaining
            self.remaining = 0

    def split(self, count: int) -> AssetRetirementReservation:
        """Transfer reserved slots to a later ACK without enlarging the bound."""
        if count > self.remaining:
            message = "Retirement reservation has insufficient remaining capacity"
            raise RuntimeError(message)
        self.remaining -= count
        return AssetRetirementReservation(self._assets, count)


class ProjectAssetLifecycle:
    """Bind project assignments and Python jobs to native off-thread retirement.

    The native registry serializes admission and cleanup and retains native queued,
    bank, voice and job readers. Python supplies the additional saved-assignment
    and separator-job owners; releasing an assignment never deletes synchronously.
    """

    _MAX_PENDING_RETIREMENTS = 4096
    _MAX_REPORTED_ERRORS = 16
    _LEGACY_STEM_FILES = (*(f"{kind}.wav" for kind in STEM_KINDS), ".complete.json")

    def __init__(self, project: ProjectState, audio: AudioEngine) -> None:
        self._project = project
        self._audio = audio
        self._root = Path.cwd().absolute()
        self._assignments: dict[tuple[str, int], tuple[Path, ProjectAssetLease]] = {}
        self._pending: dict[tuple[Path, bool], list[ProjectAssetLease]] = {}
        self._retirement_lock = threading.Lock()
        self._retry_thread: threading.Thread | None = None
        self._pending_groups: list[tuple[Path, ProjectAssetLease]] = []
        self.retirement_errors: dict[Path, str] = {}
        self._reserved = 0
        self._reservation_context = threading.local()

    def reserve(self, count: int = 16) -> AssetRetirementReservation:
        """Fail before intent mutation when finite retirement capacity is exhausted."""
        with self._retirement_lock:
            if (
                count <= 0
                or self._pending_units_locked() + self._reserved + count
                > self._MAX_PENDING_RETIREMENTS
            ):
                message = "Project asset retirement backlog full; previous assignment is retained"
                raise RuntimeError(message)
            self._reserved += count
        return AssetRetirementReservation(self, count)

    def _pending_units_locked(self) -> int:
        return (
            len(self._pending)
            + sum(len(owners) for owners in self._pending.values())
            + len(self._pending_groups)
        )

    def _consume_capacity_locked(self, count: int) -> None:
        reservation = getattr(self._reservation_context, "current", None)
        if reservation is not None and reservation.remaining >= count:
            reservation.remaining -= count
            self._reserved -= count
        elif self._pending_units_locked() + self._reserved + count > self._MAX_PENDING_RETIREMENTS:
            message = "Project asset cleanup has no reserved retirement slot"
            raise RuntimeError(message)

    def reserve_pending(self, count: int) -> AssetRetirementReservation:
        """Transfer current capacity, or reserve before a standalone publication."""
        current = getattr(self._reservation_context, "current", None)
        return current.split(count) if current is not None else self.reserve(count)

    @contextmanager
    def admission(self) -> Iterator[None]:
        """Reserve before a synchronous mutation, reusing an enclosing transaction."""
        if getattr(self._reservation_context, "current", None) is not None:
            yield
            return
        reservation = self.reserve()
        try:
            with reservation.activate():
                yield
        finally:
            reservation.close()

    def sync_assignments(self) -> None:
        """Acquire every current assignment before retiring removed assignments."""
        current = self._current_assignments()
        retired: list[tuple[str, Path, ProjectAssetLease]] = []
        for key, path in current.items():
            previous = self._assignments.get(key)
            if previous is not None and previous[0] == path:
                continue
            lease = self.acquire(path)
            if previous is not None:
                retired.append((key[0], *previous))
            self._assignments[key] = (path, lease)
        for key in self._assignments.keys() - current.keys():
            path, lease = self._assignments.pop(key)
            retired.append((key[0], path, lease))
        current_originals = {path for key, path in current.items() if key[0] == "original"}
        for kind, path, lease in retired:
            if kind == "original" and path in current_originals:
                lease.release()
            else:
                self._retire_assignment(kind, path, lease)

    def prepare_original(
        self, source: str, delivered: ProjectAssetLease | None = None
    ) -> tuple[Path, ProjectAssetLease]:
        """Validate and acquire/acknowledge before the durable assignment changes."""
        path = original_asset(source, project_root=self._root).path
        lease = self.acquire(path) if delivered is None else delivered
        if delivered is not None:
            delivered.acknowledge(str(path))
        return path, lease

    def adopt_original(self, sample_id: int, prepared: tuple[Path, ProjectAssetLease]) -> None:
        """Install the reserved new owner before releasing the previous owner."""
        key = ("original", sample_id)
        previous = self._assignments.get(key)
        if previous is not None and previous[0] == prepared[0]:
            prepared[1].release()
            return
        self._assignments[key] = prepared
        if previous is not None:
            if any(
                k[0] == "original" and path == previous[0]
                for k, (path, _) in self._assignments.items()
            ):
                previous[1].release()
            else:
                self._retire_assignment("original", *previous)

    def acquire(self, path: Path) -> ProjectAssetLease:
        """Pin one checked project asset for a Python reader or separator job."""
        path = self._checked_path(path)
        with self._retirement_lock:
            lease = self._audio.acquire_project_asset_lease(str(path))
            targets = {path}
            if (
                path.parent == self._root / "samples" / "stems"
                and re.fullmatch(r"#[1-9][0-9]*", path.name) is not None
            ):
                targets.update(path / name for name in self._LEGACY_STEM_FILES)
            for key in tuple(self._pending):
                if key[0] in targets:
                    for previous in self._pending.pop(key):
                        previous.release()
            for target in targets:
                self.retirement_errors.pop(target, None)
            self._release_settled_groups_locked()
            return lease

    def retire(
        self,
        path: Path,
        *,
        recursive: bool = False,
        lease: ProjectAssetLease | None = None,
    ) -> None:
        """Schedule native contained cleanup after all assignment/read leases end."""
        with self._retirement_lock:
            key = (self._absolute_path(path), recursive)
            self._consume_capacity_locked(int(key not in self._pending) + int(lease is not None))
            owners = self._pending.setdefault(key, [])
            if lease is not None:
                owners.append(lease)
            self._queue_retirement(key)
            self._start_retry_worker_locked()

    def retry_retirements(self) -> None:
        """Retry at most eight admission failures per UI poll, retaining their owners."""
        with self._retirement_lock:
            for key in list(islice(self._pending, 8)):
                self._queue_retirement(key)
                if key in self._pending:
                    self._pending[key] = self._pending.pop(key)
            self._release_settled_groups_locked()

    def _release_settled_groups_locked(self) -> None:
        retained: list[tuple[Path, ProjectAssetLease]] = []
        for path, lease in self._pending_groups:
            if self._legacy_files_pending_locked(path):
                retained.append((path, lease))
            else:
                lease.release()
        self._pending_groups = retained

    def _legacy_files_pending_locked(self, path: Path) -> bool:
        return any(
            target.parent == path and target.name in self._LEGACY_STEM_FILES
            for target, _ in self._pending
        )

    def retire_unassigned_original(self, source: str) -> None:
        """Reconcile a stale ACKed Success without revoking any current assignment."""
        self.sync_assignments()
        path = self._owned_original(source)
        if path is not None:
            if any(
                key[0] == "original" and assigned_path == path
                for key, (assigned_path, _) in self._assignments.items()
            ):
                self.acknowledge_original(source)
            else:
                self.retire(path)

    def acknowledge_original(self, source: str | None) -> None:
        """Acknowledge a native delivery even when its durable path is unchanged."""
        path = self._owned_original(source)
        if path is not None and path.is_file():
            self.acquire(path).release()

    def _start_retry_worker_locked(self) -> None:
        if self._pending and self._retry_thread is None:
            self._retry_thread = threading.Thread(target=self._retry_worker, daemon=True)
            try:
                self._retry_thread.start()
            except RuntimeError as error:
                self._retry_thread = None
                for path, _ in self._pending:
                    self._record_error(path, f"Cleanup retry worker admission failed: {error}")

    def _retry_worker(self) -> None:
        pause = threading.Event()
        while True:
            pause.wait(0.1)
            self.retry_retirements()
            with self._retirement_lock:
                if not self._pending:
                    self._retry_thread = None
                    return

    def _queue_retirement(self, key: tuple[Path, bool]) -> None:
        path, recursive = key
        try:
            self._audio.retire_project_asset(str(self._checked_path(path)), recursive=recursive)
        except (OSError, RuntimeError, ValueError) as error:
            self._record_error(path, str(error))
            if self._retryable_admission(error):
                return
        else:
            self.retirement_errors.pop(path, None)
        for lease in self._pending.pop(key):
            lease.release()

    def _record_error(self, path: Path, message: str) -> None:
        if (
            path not in self.retirement_errors
            and len(self.retirement_errors) >= self._MAX_REPORTED_ERRORS
        ):
            self.retirement_errors.pop(next(iter(self.retirement_errors)))
        self.retirement_errors[path] = message

    @staticmethod
    def _retryable_admission(error: OSError | RuntimeError | ValueError) -> bool:
        # Native PyO3 admission reports io::Error as RuntimeError. Filesystem
        # sharing is retryable; invalid containment/ownership must preserve bytes
        # and release the token after reporting a terminal error.
        message = str(error)
        return (
            "retirement queue full" in message
            or any(f"(os error {code})" in message for code in (32, 33))
            or (isinstance(error, OSError) and getattr(error, "winerror", None) in {32, 33})
        )

    def release_saved_assignments(self) -> None:
        """Release process owners at shutdown without retiring persisted assignments."""
        for _, lease in self._assignments.values():
            lease.release()
        self._assignments.clear()

    def _current_assignments(self) -> dict[tuple[str, int], Path]:
        result: dict[tuple[str, int], Path] = {}
        for sample_id, source in enumerate(self._project.sample_paths):
            path = self._owned_original(source)
            if path is not None and path.is_file():
                result["original", sample_id] = path
        for sample_id, entry in enumerate(self._project.stem_cache):
            if entry is None:
                continue
            try:
                if not (self._root / "samples").is_dir():
                    continue
                path = self._checked_path(Path(entry.cache_dir))
                resolved = resolve_asset(path, project_root=self._root)
            except OSError, ValueError:
                continue
            original = self._owned_original(self._project.sample_paths[sample_id])
            material = (
                original_asset(original, project_root=self._root).material_id
                if original is not None else None
            )
            pad = (
                self._root / "samples" / "materials" / f"M{material}" / "stems"
                if material is not None else self._root / "samples" / "stems" / f"#{sample_id + 1}"
            )
            if resolved.kind != "stem_directory":
                continue
            if (path == pad and (entry.available or (path / ".complete.json").is_file())) or (
                path.parent == pad and re.fullmatch(r"\.ready-[0-9a-f]{32}", path.name) is not None
            ):
                result["stems", sample_id] = path
        return result

    def _owned_original(self, source: str | None) -> Path | None:
        if source is None:
            return None
        try:
            path = original_asset(source, project_root=self._root).path
        except OSError, ValueError:
            return None
        return path

    def _absolute_path(self, path: Path) -> Path:
        """Use one lexical key for relative and absolute retirement/reader paths."""
        if ".." in path.parts:
            message = "Project asset path contains traversal"
            raise ValueError(message)
        target = path if path.is_absolute() else self._root / path
        return Path(os.path.abspath(target))

    def _checked_path(self, path: Path) -> Path:
        return resolve_asset(path, project_root=self._root).path

    def _retire_assignment(self, kind: str, path: Path, lease: ProjectAssetLease) -> None:
        if kind == "original":
            self.retire(path, lease=lease)
        elif path.name.startswith(".ready-"):
            self.retire(path, recursive=True, lease=lease)
        else:
            # Legacy canonical containers can contain private/unknown files or new
            # generations. Retire only the declared files, never the pad container.
            for name in self._LEGACY_STEM_FILES:
                self.retire(path / name)
            # The container owner must stay alive if any of its exact-file requests
            # has not been admitted; release it only after every path is admitted.
            with self._retirement_lock:
                if self._legacy_files_pending_locked(path):
                    self._consume_capacity_locked(1)
                    self._pending_groups.append((path, lease))
                else:
                    lease.release()
