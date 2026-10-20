"""Bounded read-only discovery of unsigned recording-attempt metadata.

Present canonical metadata and FileIDs establish self-consistency only. In-place
metadata changes cannot be authenticated after restart. Observations never grant
an original Native ticket, content verification, publication or retirement.
"""

import json
import os
import re
from contextlib import ExitStack
from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal, cast

from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.recording_paths import (
    RecordingCustodyReleaseError,
    RecordingNamespaceError,
    RecordingPaths,
    _absolute,
    _assert_layout,
    _identity,
    _owner_bytes,
    _pin,
    _pin_ancestors,
    _plain_metadata,
)

if TYPE_CHECKING:
    from collections.abc import Callable, Generator
    from pathlib import Path

    from flitzis_looper.recording_paths import FileIdentity

type RecordingRecoveryState = Literal["invalid", "incomplete", "candidate"]
type RecordingRecoveryPublication = Literal["absent", "same_file", "conflict", "unverified"]

_OWNER_MAX_BYTES = 8192
_ATTEMPT_NAME = re.compile(r"\.incomplete-([0-9a-f]{32})")
_STAGE_NAMES = ("float.capture", "input.pcm", "encoded.partial", "decoded.pcm", "verified.json")


@dataclass(frozen=True, kw_only=True, slots=True)
class RecordingRecoveryFile:
    """One observed ordinary file; its bytes and original custody are unverified."""

    path: Path
    identity: FileIdentity
    size_bytes: int


@dataclass(frozen=True, kw_only=True, slots=True)
class RecordingRecoveryObservation:
    """A released snapshot, without any capture or filesystem action capability.

    ``candidate`` means an encoded stage exists, including possibly partial data.
    ``same_file`` is an observed encoded/final FileID match, never verified audio.
    Declared options are unsigned even when every canonical owner field agrees.
    Custody, original provenance and successful retirement remain unknown after
    restart. Other platforms retain the existing pre/post identity checks rather
    than the Windows held-handle guarantee. The filesystem can change after yield.
    """

    root: Path
    directory: Path
    attempt_id: str
    state: RecordingRecoveryState
    publication: RecordingRecoveryPublication
    declared_options: RecordingOptions | None = None
    created_at_utc: str | None = None
    root_identity: FileIdentity | None = None
    directory_identity: FileIdentity | None = None
    metadata_identity: FileIdentity | None = None
    final_path: Path | None = None
    files: tuple[RecordingRecoveryFile, ...] = ()
    diagnostics: tuple[str, ...] = ()
    original_provenance_authenticated: Literal[False] = False
    publication_verified: Literal[False] = False
    retirement_verified: Literal[False] = False
    custody: Literal["unknown_after_restart"] = "unknown_after_restart"


def _read_owner(path: Path, identity: FileIdentity) -> bytes:
    with path.open("rb") as owner:
        if _identity(os.fstat(owner.fileno())) != identity:
            message = "recording owner identity changed before bounded read"
            raise RecordingNamespaceError(message)
        encoded = owner.read(_OWNER_MAX_BYTES + 1)
    if len(encoded) > _OWNER_MAX_BYTES:
        message = "recording owner metadata exceeds the discovery byte limit"
        raise RecordingNamespaceError(message)
    return encoded


def _declared_paths(
    root: Path,
    attempt_id: str,
    encoded: bytes,
    identities: tuple[FileIdentity, FileIdentity, FileIdentity],
) -> RecordingPaths:
    document: object = json.loads(encoded)
    if not isinstance(document, dict):
        message = "recording owner metadata must be an object"
        raise RecordingNamespaceError(message)
    owner = cast("dict[str, object]", document)
    created = owner.get("created_at_utc")
    if not isinstance(created, str):
        message = "recording owner creation identity must be a string"
        raise RecordingNamespaceError(message)
    options = RecordingOptions.model_validate(owner.get("options"), strict=True)
    directory = root / f".incomplete-{attempt_id}"
    paths = RecordingPaths(
        attempt_id=attempt_id,
        created_at_utc=created,
        options=options,
        root=root,
        directory=directory,
        spool_path=directory / "float.capture",
        pcm_path=directory / "input.pcm",
        encoded_path=directory / "encoded.partial",
        decoded_path=directory / "decoded.pcm",
        metadata_path=directory / "owner.json",
        final_path=root / f"recording-{created}-{attempt_id}.{options.format}",
        root_identity=identities[0],
        directory_identity=identities[1],
        metadata_identity=identities[2],
    )
    _assert_layout(paths)
    # Exact bytes also reject duplicate/extra keys, missing default choices,
    # noncanonical numbers, redirected leaves and claimed/observed ID mismatch.
    if encoded != _owner_bytes(paths):
        message = "recording owner is not canonical and consistent with observed FileIDs/layout"
        raise RecordingNamespaceError(message)
    return paths


def _observe_file(stack: ExitStack, path: Path) -> RecordingRecoveryFile | None:
    try:
        identity = _pin(stack, path, directory=False)
    except FileNotFoundError:
        return None
    metadata = _plain_metadata(path, directory=False)
    if _identity(metadata) != identity:
        message = "recording file identity changed during discovery"
        raise RecordingNamespaceError(message)
    return RecordingRecoveryFile(path=path, identity=identity, size_bytes=metadata.st_size)


def _publication(
    encoded: RecordingRecoveryFile | None,
    final: RecordingRecoveryFile | None,
) -> tuple[RecordingRecoveryPublication, tuple[str, ...]]:
    if final is None:
        return "absent", ()
    if encoded is None:
        return "unverified", ("final entry exists without an observed encoded stage",)
    if encoded.identity == final.identity:
        return "same_file", ("encoded/final FileIDs match; current bytes remain unverified",)
    return "conflict", ("encoded/final FileIDs differ; both entries are preserved",)


def _consistent_observation(root: Path, attempt_id: str) -> RecordingRecoveryObservation:
    directory = root / f".incomplete-{attempt_id}"
    with ExitStack() as stack:
        root_identity = _pin_ancestors(stack, root)
        directory_identity = _pin(stack, directory, directory=True)
        metadata_path = directory / "owner.json"
        metadata_identity = _pin(stack, metadata_path, directory=False)
        paths = _declared_paths(
            root,
            attempt_id,
            _read_owner(metadata_path, metadata_identity),
            (root_identity, directory_identity, metadata_identity),
        )
        files: list[RecordingRecoveryFile] = []
        for path in (metadata_path, *(directory / name for name in _STAGE_NAMES), paths.final_path):
            observed = _observe_file(stack, path)
            if observed is not None:
                files.append(observed)
        encoded = next((item for item in files if item.path == paths.encoded_path), None)
        final = next((item for item in files if item.path == paths.final_path), None)
        publication, diagnostics = _publication(encoded, final)
        if encoded is not None:
            diagnostics += ("encoded.partial is present; partial or complete bytes are unverified",)
        if any(item.path.name == "verified.json" for item in files):
            diagnostics += (
                "unsigned verified.json is a candidate hint; its contents were not read",
            )
        return RecordingRecoveryObservation(
            root=root,
            directory=directory,
            attempt_id=attempt_id,
            state="candidate" if encoded is not None else "incomplete",
            publication=publication,
            declared_options=paths.options,
            created_at_utc=paths.created_at_utc,
            root_identity=root_identity,
            directory_identity=directory_identity,
            metadata_identity=metadata_identity,
            final_path=paths.final_path,
            files=tuple(files),
            diagnostics=diagnostics,
        )


def _observe_attempt(root: Path, attempt_id: str) -> RecordingRecoveryObservation:
    try:
        return _consistent_observation(root, attempt_id)
    except RecordingCustodyReleaseError:
        # A real failed close is not an ordinary invalid-metadata observation.
        raise
    except (OSError, ValueError, RecursionError) as error:
        return RecordingRecoveryObservation(
            root=root,
            directory=root / f".incomplete-{attempt_id}",
            attempt_id=attempt_id,
            state="invalid",
            publication="unverified",
            diagnostics=(f"{type(error).__name__}: {error}",),
        )


def iter_recording_recovery(
    root: Path, *, checkpoint: Callable[[], None] | None = None
) -> Generator[RecordingRecoveryObservation]:
    """Stream released, unverified immediate recording-attempt observations.

    A missing root produces no entries and is never created. Unknown names are
    skipped without traversal; matching malformed/reparse entries yield invalid
    observations. At most 8193 owner bytes and a fixed set of ordinary file
    metadata are inspected per attempt. Media and verified-receipt contents are
    never read. No sorting, whole-directory list or whole-media hash is needed.

    ``checkpoint`` runs before scanning and around each bounded entry; its
    exceptions propagate unchanged. Exhaust, close, or use ``contextlib.closing``
    when abandoning iteration so the scanner closes promptly. Entry pins always
    close before yield. Root/ancestor pins remain with the scanner until it is
    closed, preventing root replacement between validation and enumeration.
    Root errors and actual custody-release errors propagate.
    """
    _absolute(root)
    if checkpoint is not None:
        checkpoint()
    with ExitStack() as stack:
        try:
            _pin_ancestors(stack, root)
        except FileNotFoundError:
            return
        scanner = os.scandir(root)
        with scanner as entries:
            for entry in entries:
                if checkpoint is not None:
                    checkpoint()
                match = _ATTEMPT_NAME.fullmatch(entry.name)
                if match is None:
                    continue
                observation = _observe_attempt(root, match.group(1))
                if checkpoint is not None:
                    checkpoint()
                yield observation
    if checkpoint is not None:
        checkpoint()
