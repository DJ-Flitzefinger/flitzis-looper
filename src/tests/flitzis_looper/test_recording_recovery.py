"""Read-only discovery proofs; synthetic bytes confer no capture/codec authority."""

import errno
import io
import json
import os
import shutil
import subprocess
import sys
from contextlib import contextmanager
from dataclasses import FrozenInstanceError
from pathlib import Path
from typing import TYPE_CHECKING
from uuid import uuid4

import pytest
from pydantic import ValidationError

from flitzis_looper import recording_paths, recording_recovery
from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.recording_paths import (
    RecordingCustodyReleaseError,
    RecordingNamespaceError,
    RecordingPaths,
    create_recording_namespace,
)
from flitzis_looper.recording_recovery import (
    RecordingRecoveryObservation,
    iter_recording_recovery,
)

if TYPE_CHECKING:
    from collections.abc import Iterator

_OWNER = (
    "# Owned recording recovery discovery tests\n"
    "Created by src/tests/flitzis_looper/test_recording_recovery.py only.\n"
    "UUID cases contain synthetic metadata and deliberately unverified bytes.\n"
    "Retained until Root completes validation/review and workspace storage hygiene.\n"
    "No Human audio, settings, models, live capture or codec jobs belong here.\n"
)


@pytest.fixture
def owned_case() -> Path:
    workspace = Path(__file__).resolve().parents[4]
    block = workspace / "test-artifacts" / "recording-format-recovery-20261020"
    block.mkdir(parents=True, exist_ok=True)
    owner = block / "OWNER.md"
    try:
        with owner.open("x", encoding="utf-8", newline="\n") as handle:
            handle.write(_OWNER)
    except FileExistsError:
        assert owner.read_text(encoding="utf-8") == _OWNER
    case = block / uuid4().hex
    case.mkdir()
    return case


def _write_attribute(target: object, field: str, value: object) -> None:
    """Exercise frozen assignment through the object's own runtime guard."""
    setattr(target, field, value)


def _identity(path: Path) -> tuple[int, int]:
    metadata = path.stat(follow_symlinks=False)
    return metadata.st_dev, metadata.st_ino


def _attempt(owned_case: Path) -> RecordingPaths:
    return create_recording_namespace(owned_case / "record", RecordingOptions())


def _canonical(document: dict[str, object]) -> bytes:
    return (json.dumps(document, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")


def _owner_document(paths: RecordingPaths) -> dict[str, object]:
    document: dict[str, object] = json.loads(paths.metadata_path.read_bytes())
    return document


def _assert_no_authority(observation: RecordingRecoveryObservation) -> None:
    assert observation.original_provenance_authenticated is False
    assert observation.publication_verified is False
    assert observation.retirement_verified is False
    assert observation.custody == "unknown_after_restart"
    assert not hasattr(observation, "ticket")
    assert not hasattr(observation, "completion")
    assert not hasattr(observation, "publish")
    assert not hasattr(observation, "cleanup")


def _trace_scanner(
    monkeypatch: pytest.MonkeyPatch,
) -> tuple[list[Path], list[str], list[Path]]:
    actual = os.scandir
    opened: list[Path] = []
    visited: list[str] = []
    closed: list[Path] = []

    @contextmanager
    def tracked(root: Path) -> Iterator[Iterator[os.DirEntry[str]]]:
        opened.append(root)
        with actual(root) as scanner:

            def entries() -> Iterator[os.DirEntry[str]]:
                for entry in scanner:
                    visited.append(entry.name)
                    yield entry

            try:
                yield entries()
            finally:
                closed.append(root)

    monkeypatch.setattr(os, "scandir", tracked)
    return opened, visited, closed


def test_missing_root_is_not_created_and_observation_has_no_success_state(
    owned_case: Path,
) -> None:
    root = owned_case / "missing-record"
    assert list(iter_recording_recovery(root)) == []
    assert not root.exists()
    assert tuple(owned_case.iterdir()) == ()


def test_original_canonical_owner_is_only_a_frozen_declared_incomplete_snapshot(
    owned_case: Path,
) -> None:
    paths = _attempt(owned_case)
    original = paths.metadata_path.read_bytes()
    (observation,) = iter_recording_recovery(paths.root)

    assert observation.state == "incomplete"
    assert observation.publication == "absent"
    assert observation.declared_options == RecordingOptions()
    assert observation.root_identity == paths.root_identity
    assert observation.directory_identity == paths.directory_identity
    assert observation.metadata_identity == paths.metadata_identity
    assert observation.final_path == paths.final_path
    assert [(item.path, item.identity, item.size_bytes) for item in observation.files] == [
        (paths.metadata_path, paths.metadata_identity, len(original))
    ]
    _assert_no_authority(observation)
    with pytest.raises(FrozenInstanceError):
        _write_attribute(observation, "state", "candidate")
    assert observation.declared_options is not None
    with pytest.raises(ValidationError, match="frozen"):
        _write_attribute(observation.declared_options, "format", "flac")
    assert paths.metadata_path.read_bytes() == original
    assert not paths.final_path.exists()


@pytest.mark.parametrize("final_kind", ["absent", "hardlink", "copy", "final_only"])
def test_unverified_stage_and_final_relationships_never_become_a_completed_take(
    owned_case: Path, final_kind: str
) -> None:
    paths = _attempt(owned_case)
    payload = b"deliberately not an audio container; even equal bytes prove no codec"
    if final_kind != "final_only":
        paths.encoded_path.write_bytes(payload)
    if final_kind == "hardlink":
        os.link(paths.encoded_path, paths.final_path)
    elif final_kind in {"copy", "final_only"}:
        paths.final_path.write_bytes(payload)
    hint = paths.directory / "verified.json"
    hint.write_bytes(b'{"publication_ready":true,"forged":"not trusted or read"}')
    before = {path: (_identity(path), path.read_bytes()) for path in paths.directory.iterdir()}
    if paths.final_path.exists():
        before[paths.final_path] = (_identity(paths.final_path), payload)

    (observation,) = iter_recording_recovery(paths.root)

    expected = {
        "absent": ("candidate", "absent"),
        "hardlink": ("candidate", "same_file"),
        "copy": ("candidate", "conflict"),
        "final_only": ("incomplete", "unverified"),
    }
    assert (observation.state, observation.publication) == expected[final_kind]
    _assert_no_authority(observation)
    if final_kind == "hardlink":
        assert _identity(paths.final_path) == _identity(paths.encoded_path)
    elif final_kind == "copy":
        assert _identity(paths.final_path) != _identity(paths.encoded_path)
    assert any("contents were not read" in diagnostic for diagnostic in observation.diagnostics)
    for path, (identity, content) in before.items():
        assert _identity(path) == identity
        assert path.read_bytes() == content


def test_valid_canonical_in_place_change_is_declared_not_original_provenance(
    owned_case: Path,
) -> None:
    paths = _attempt(owned_case)
    original_identity = _identity(paths.metadata_path)
    document = _owner_document(paths)
    options = RecordingOptions(format="flac", flac_bit_depth=16, mp3_bitrate_kbps=128)
    document["options"] = options.model_dump(mode="json")
    document["final"] = paths.final_path.with_suffix(".flac").name
    paths.metadata_path.write_bytes(_canonical(document))
    assert _identity(paths.metadata_path) == original_identity

    (observation,) = iter_recording_recovery(paths.root)

    assert observation.state == "incomplete"
    assert observation.declared_options == options
    assert observation.final_path == paths.final_path.with_suffix(".flac")
    _assert_no_authority(observation)
    assert not paths.final_path.exists()
    assert observation.final_path is not None
    assert not observation.final_path.exists()


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("schema", "unknown-owner-v2"),
        ("attempt_id", "0" * 32),
        ("created_at_utc", "../../foreign"),
        ("root_identity", [0, 1]),
        ("directory_identity", [0, 1]),
        ("metadata_identity", [0, 1]),
        ("spool", "../foreign"),
        ("pcm", "other.pcm"),
        ("encoded", "different.partial"),
        ("decoded", "../decoded.pcm"),
        ("final", "existing-unknown.wav"),
        ("unrecognized", "preserved but invalid"),
        ("options", {"flac_bit_depth": 16.0}),
        ("options", {"flac_compression": True}),
        ("options", {"mp3_bitrate_kbps": "320"}),
        ("options", {"format": "mp3", "unknown": 1}),
    ],
)
def test_canonical_owner_must_agree_with_exact_schema_options_layout_and_observed_ids(
    owned_case: Path, field: str, value: object
) -> None:
    paths = _attempt(owned_case)
    document = _owner_document(paths)
    document[field] = value
    changed = _canonical(document)
    paths.metadata_path.write_bytes(changed)
    unknown = paths.directory / "unknown.bin"
    unknown.write_bytes(b"unknown data must survive invalid metadata")

    (observation,) = iter_recording_recovery(paths.root)

    assert observation.state == "invalid"
    assert observation.publication == "unverified"
    assert observation.declared_options is None
    assert observation.diagnostics
    _assert_no_authority(observation)
    assert paths.metadata_path.read_bytes() == changed
    assert unknown.read_bytes() == b"unknown data must survive invalid metadata"
    assert not paths.final_path.exists()


@pytest.mark.parametrize("shape", ["duplicate", "whitespace", "trailing", "utf8", "array", "deep"])
def test_malformed_or_noncanonical_json_is_retained_and_not_repaired(
    owned_case: Path, shape: str
) -> None:
    paths = _attempt(owned_case)
    original = paths.metadata_path.read_bytes()
    variants = {
        "duplicate": b'{"schema":"flitzi-recording-owner-v1",' + original[1:],
        "whitespace": b" " + original,
        "trailing": original + b"not-json",
        "utf8": b"\xff\xfe",
        "array": b"[]\n",
        "deep": b"[" * 2000 + b"0" + b"]" * 2000,
    }
    changed = variants[shape]
    paths.metadata_path.write_bytes(changed)

    (observation,) = iter_recording_recovery(paths.root)

    assert observation.state == "invalid"
    assert paths.metadata_path.read_bytes() == changed
    _assert_no_authority(observation)


def test_byte_copied_tree_and_replaced_owner_do_not_match_observed_file_identities(
    owned_case: Path,
) -> None:
    paths = _attempt(owned_case)
    owner = paths.metadata_path.read_bytes()
    copied_root = owned_case / "copied-record"
    shutil.copytree(paths.root, copied_root)
    (copied,) = iter_recording_recovery(copied_root)
    assert copied.state == "invalid"
    assert "FileIDs/layout" in copied.diagnostics[0]
    copied_owner = copied_root / paths.directory.name / "owner.json"
    assert copied_owner.read_bytes() == owner

    retained = paths.directory / "original-owner.retained"
    paths.metadata_path.rename(retained)
    paths.metadata_path.write_bytes(owner)
    assert _identity(paths.metadata_path) != paths.metadata_identity
    (replaced,) = iter_recording_recovery(paths.root)
    assert replaced.state == "invalid"
    assert retained.read_bytes() == owner
    assert paths.metadata_path.read_bytes() == owner


def test_missing_owner_is_observed_as_invalid_without_creating_or_repairing_it(
    owned_case: Path,
) -> None:
    paths = _attempt(owned_case)
    retained = paths.directory / "owner-retained.json"
    paths.metadata_path.rename(retained)
    original = retained.read_bytes()

    (observation,) = iter_recording_recovery(paths.root)

    assert observation.state == "invalid"
    assert "FileNotFoundError" in observation.diagnostics[0]
    assert not paths.metadata_path.exists()
    assert retained.read_bytes() == original
    _assert_no_authority(observation)


@pytest.mark.parametrize("leaf", ["float.capture", "input.pcm", "encoded.partial", "decoded.pcm"])
def test_known_leaf_with_wrong_type_is_invalid_without_descending_into_it(
    owned_case: Path, leaf: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = _attempt(owned_case)
    unexpected = paths.directory / leaf
    unexpected.mkdir()
    nested = unexpected / "unknown.bin"
    nested.write_bytes(b"never traversed or removed")
    opened, _visited, closed = _trace_scanner(monkeypatch)

    (observation,) = iter_recording_recovery(paths.root)

    assert observation.state == "invalid"
    assert opened == [paths.root]
    assert closed == [paths.root]
    assert nested.read_bytes() == b"never traversed or removed"


def test_unknown_names_are_skipped_without_recursive_enumeration_or_mutation(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = _attempt(owned_case)
    unknown = paths.root / ".incomplete-not-a-uuid"
    unknown.mkdir()
    nested_attempt = create_recording_namespace(unknown / "record", RecordingOptions())
    unknown_file = paths.root / "unknown.wav"
    unknown_file.write_bytes(b"unknown existing output")
    matching_file = paths.root / f".incomplete-{uuid4().hex}"
    matching_file.write_bytes(b"matching name but not a directory")
    original = nested_attempt.metadata_path.read_bytes()
    opened, _visited, closed = _trace_scanner(monkeypatch)

    observations = list(iter_recording_recovery(paths.root))

    assert len(observations) == 2
    assert {item.state for item in observations} == {"incomplete", "invalid"}
    assert opened == [paths.root]
    assert closed == [paths.root]
    assert unknown_file.read_bytes() == b"unknown existing output"
    assert matching_file.read_bytes() == b"matching name but not a directory"
    assert nested_attempt.metadata_path.read_bytes() == original


@pytest.mark.parametrize("oversized", [False, True])
def test_real_held_owner_reads_are_bounded_and_media_or_receipt_payloads_are_never_opened(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch, *, oversized: bool
) -> None:
    paths = _attempt(owned_case)
    paths.spool_path.write_bytes(b"untrusted spool bytes" * 1000)
    paths.encoded_path.write_bytes(b"unverified encoded bytes" * 1000)
    (paths.directory / "verified.json").write_bytes(b"unsigned receipt is not parsed")
    if oversized:
        paths.metadata_path.write_bytes(b"x" * 20000)
    actual_open = Path.open
    requests: list[int] = []

    class OwnerReadTrace(io.BufferedReader):
        def read(self, size: int | None = -1) -> bytes:
            assert size is not None
            requests.append(size)
            assert 0 < size <= 8193
            if sys.platform == "win32":
                with pytest.raises(PermissionError), actual_open(paths.metadata_path, "r+b"):
                    pytest.fail("metadata was not held against a real Windows writer")
            return super().read(size)

    def tracked_open(path: Path, *args: object, **kwargs: object) -> object:
        assert path == paths.metadata_path, f"unexpected payload open: {path}"
        assert args == ("rb",)
        assert not kwargs
        return OwnerReadTrace(io.FileIO(path, "rb"))

    monkeypatch.setattr(Path, "open", tracked_open)
    (observation,) = iter_recording_recovery(paths.root)

    assert requests == [8193]
    assert observation.state == ("invalid" if oversized else "candidate")
    if oversized:
        assert "byte limit" in observation.diagnostics[0]
    _assert_no_authority(observation)


@pytest.mark.skipif(sys.platform != "win32", reason="actual Windows root pin at scan admission")
def test_original_root_cannot_be_replaced_between_validation_and_scanner_open(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = _attempt(owned_case)
    actual_scandir = os.scandir
    rename_rejections: list[Path] = []

    @contextmanager
    def attempt_root_replacement(root: Path) -> Iterator[Iterator[os.DirEntry[str]]]:
        assert root == paths.root
        with pytest.raises(PermissionError):
            root.rename(owned_case / "displaced-original-record")
        rename_rejections.append(root)
        with actual_scandir(root) as scanner:
            yield scanner

    monkeypatch.setattr(os, "scandir", attempt_root_replacement)
    observations = list(iter_recording_recovery(paths.root))
    assert rename_rejections == [paths.root]
    assert len(observations) == 1
    assert observations[0].attempt_id == paths.attempt_id
    assert observations[0].state == "incomplete"
    assert not (owned_case / "displaced-original-record").exists()


def test_streaming_close_stops_enumeration_and_releases_the_actual_scanner(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    first = _attempt(owned_case)
    second = create_recording_namespace(first.root, RecordingOptions(format="mp3"))
    opened, visited, closed = _trace_scanner(monkeypatch)
    iterator = iter_recording_recovery(first.root)

    observation = next(iterator)
    assert observation.state == "incomplete"
    assert opened == [first.root]
    assert len(visited) == 1
    assert closed == []
    iterator.close()

    assert closed == [first.root]
    assert len(visited) == 1
    assert first.metadata_path.is_file()
    assert second.metadata_path.is_file()


def test_checkpoint_cancellation_propagates_after_entry_pins_close_and_closes_scanner(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = _attempt(owned_case)
    original = paths.metadata_path.read_bytes()
    opened, _visited, closed = _trace_scanner(monkeypatch)
    calls = 0

    def cancel_after_first_bounded_entry() -> None:
        nonlocal calls
        calls += 1
        if calls == 3:
            message = "cancel discovery before yielding the first observation"
            raise InterruptedError(message)

    with pytest.raises(InterruptedError, match="cancel discovery"):
        list(iter_recording_recovery(paths.root, checkpoint=cancel_after_first_bounded_entry))

    assert calls == 3
    assert opened == [paths.root]
    assert closed == [paths.root]
    with paths.metadata_path.open("r+b") as owner:
        owner.write(original)
    released = owned_case / "released-after-cancel"
    paths.root.rename(released)
    assert (released / paths.directory.name / "owner.json").read_bytes() == original


@pytest.mark.skipif(sys.platform != "win32", reason="actual Windows namespace custody")
def test_all_entry_pins_are_released_before_yield_while_scanner_remains_open(
    owned_case: Path,
) -> None:
    paths = _attempt(owned_case)
    paths.encoded_path.write_bytes(b"ordinary unverified stage")
    original = paths.metadata_path.read_bytes()
    iterator = iter_recording_recovery(paths.root)
    try:
        observation = next(iterator)
        assert observation.state == "candidate"
        # Each operation would fail under the existing actual deny-write/delete pins.
        with paths.metadata_path.open("r+b") as owner:
            owner.write(original)
        with paths.encoded_path.open("r+b") as stage:
            stage.write(b"ordinary unverified stage")
        moved_stage = paths.directory / "released-stage.retained"
        paths.encoded_path.rename(moved_stage)
        moved_attempt = paths.root / "released-attempt"
        paths.directory.rename(moved_attempt)
        assert (moved_attempt / moved_stage.name).read_bytes() == b"ordinary unverified stage"
    finally:
        iterator.close()
    paths.root.rename(owned_case / "released-root")


@pytest.mark.skipif(sys.platform != "win32", reason="actual Windows pin close error")
def test_actual_released_pin_custody_uncertainty_is_not_swallowed_as_invalid_metadata(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = _attempt(owned_case)
    original = recording_paths._windows_pin
    opened, _visited, closed = _trace_scanner(monkeypatch)

    @contextmanager
    def fail_after_real_owner_close(path: Path, *, directory: bool) -> Iterator[int]:
        with original(path, directory=directory) as descriptor:
            yield descriptor
        if path == paths.metadata_path:
            cause = OSError("injected actual owner-pin close uncertainty")
            raise RecordingCustodyReleaseError(path, cause)

    monkeypatch.setattr(recording_paths, "_windows_pin", fail_after_real_owner_close)
    with pytest.raises(RecordingCustodyReleaseError, match="close uncertainty") as raised:
        list(iter_recording_recovery(paths.root))

    assert raised.value.path == paths.metadata_path
    assert raised.value.retirement_uncertain is True
    assert opened == [paths.root]
    assert closed == [paths.root]
    # The injected report followed real release; no hidden pin may survive it.
    with paths.metadata_path.open("r+b") as owner:
        assert owner.read(1) == b"{"
    paths.root.rename(owned_case / "released-after-close-report")


def test_unexpected_inspection_exception_closes_scanner_and_actual_entry_pins(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = _attempt(owned_case)
    original = paths.metadata_path.read_bytes()
    opened, _visited, closed = _trace_scanner(monkeypatch)

    def fail_read(_path: Path, _identity: tuple[int, int]) -> bytes:
        message = "injected unexpected inspection failure while actual pins are held"
        raise RuntimeError(message)

    monkeypatch.setattr(recording_recovery, "_read_owner", fail_read)
    with pytest.raises(RuntimeError, match="unexpected inspection failure"):
        list(iter_recording_recovery(paths.root))

    assert opened == [paths.root]
    assert closed == [paths.root]
    with paths.metadata_path.open("r+b") as owner:
        owner.write(original)
    paths.root.rename(owned_case / "released-after-unexpected-error")


def _symlink_or_skip(link: Path, target: Path, *, directory: bool) -> None:
    try:
        link.symlink_to(target, target_is_directory=directory)
    except NotImplementedError:
        pytest.skip("this runtime cannot create an actual symlink")
    except OSError as error:
        if getattr(error, "winerror", None) == 1314 or error.errno in {errno.EPERM, errno.EACCES}:
            pytest.skip("this account cannot create an actual symlink")
        raise


def test_matching_reparse_attempt_is_invalid_and_its_unknown_target_is_untouched(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = owned_case / "record"
    root.mkdir()
    target = owned_case / "unknown-target"
    target.mkdir()
    unknown = target / "owner.json"
    unknown.write_bytes(b"not recorder ownership; do not follow or alter")
    link = root / f".incomplete-{uuid4().hex}"
    _symlink_or_skip(link, target, directory=True)
    opened, _visited, closed = _trace_scanner(monkeypatch)

    (observation,) = iter_recording_recovery(root)

    assert observation.state == "invalid"
    assert "reparse" in observation.diagnostics[0]
    assert opened == [root]
    assert closed == [root]
    assert unknown.read_bytes() == b"not recorder ownership; do not follow or alter"


def test_reparse_root_is_refused_before_scanning_its_target(owned_case: Path) -> None:
    target = owned_case / "unknown-root-target"
    target.mkdir()
    unknown = target / "unknown.wav"
    unknown.write_bytes(b"unknown bytes remain")
    link = owned_case / "linked-record"
    _symlink_or_skip(link, target, directory=True)

    with pytest.raises(RecordingNamespaceError, match="reparse"):
        list(iter_recording_recovery(link))

    assert unknown.read_bytes() == b"unknown bytes remain"
    assert list(target.iterdir()) == [unknown]


def test_discovery_does_not_start_processes_publish_delete_or_repair(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = _attempt(owned_case)
    paths.spool_path.write_bytes(b"incomplete spool left untouched")
    paths.encoded_path.write_bytes(b"unverified stage left untouched")
    paths.final_path.write_bytes(b"unknown conflicting output left untouched")
    original = {path: path.read_bytes() for path in (*paths.directory.iterdir(), paths.final_path)}

    def forbidden(*_args: object, **_kwargs: object) -> None:
        pytest.fail("discovery attempted a process, publication or filesystem mutation")

    for operation in ("link", "unlink", "remove", "rename", "replace", "mkdir"):
        monkeypatch.setattr(os, operation, forbidden)
    monkeypatch.setattr(subprocess, "Popen", forbidden)

    (observation,) = iter_recording_recovery(paths.root)

    assert observation.state == "candidate"
    assert observation.publication == "conflict"
    _assert_no_authority(observation)
    for path, payload in original.items():
        assert path.read_bytes() == payload


@pytest.mark.parametrize("root", [Path("relative-record"), Path("record/../other")])
def test_relative_or_parent_traversal_root_is_refused_without_scanning(root: Path) -> None:
    with pytest.raises(ValueError, match="absolute"):
        list(iter_recording_recovery(root))
