"""Actual owned filesystem tests; these payloads claim no codec or Native validity."""

import errno
import json
import os
import sys
from contextlib import contextmanager
from dataclasses import replace
from pathlib import Path
from typing import TYPE_CHECKING, Protocol, cast
from uuid import UUID, uuid4

import pytest
from pydantic import ValidationError

from flitzis_looper import recording_paths
from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.recording_paths import (
    RecordingNamespaceError,
    RecordingPaths,
    assert_recording_namespace,
    create_recording_namespace,
    hold_recording_namespace,
    publish_recording_no_replace,
)

if TYPE_CHECKING:
    from collections.abc import Iterator


class _UncheckedOptionsConstructor(Protocol):
    """Retain model_construct's arbitrary inputs for intentional invalid fixtures."""

    def __call__(self, **values: object) -> RecordingOptions: ...


_OWNER = (
    "# Owned recording namespace tests\n"
    "Created by src/tests/flitzis_looper/test_recording_paths.py only.\n"
    "UUID case directories contain disposable synthetic fixtures, retained until\n"
    "Root completes tests/review and applies the workspace storage policy.\n"
    "No Human audio, settings, models or installations belong to this block.\n"
)


@pytest.fixture
def owned_case() -> Path:
    workspace = Path(__file__).resolve().parents[4]
    block = workspace / "test-artifacts" / "recording-format-paths-20261020"
    block.mkdir(parents=True, exist_ok=True)
    owner = block / "OWNER.md"
    try:
        with owner.open("x", encoding="utf-8", newline="\n") as handle:
            handle.write(_OWNER)
    except FileExistsError:
        assert owner.read_text(encoding="utf-8") == _OWNER
    case = block / uuid4().hex
    case.mkdir()
    # No recursive cleanup: all test-created evidence remains in its owned case.
    return case


def _identity(path: Path) -> tuple[int, int]:
    metadata = path.stat(follow_symlinks=False)
    return metadata.st_dev, metadata.st_ino


def _encoded(paths: RecordingPaths) -> tuple[int, int]:
    with paths.encoded_path.open("xb") as handle:
        handle.write(b"synthetic closed encoded-fixture bytes; no codec claim")
        handle.flush()
        os.fsync(handle.fileno())
    return _identity(paths.encoded_path)


def test_publication_does_not_turn_a_successful_link_into_a_failed_postlink_read(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    identity = _encoded(paths)
    original = recording_paths._plain_metadata
    postlink_reads: list[Path] = []

    def fail_only_after_publication(path: Path, *, directory: bool) -> os.stat_result:
        if path == paths.final_path and path.exists():
            postlink_reads.append(path)
            message = "injected read error after completed link"
            raise OSError(message)
        return original(path, directory=directory)

    monkeypatch.setattr(recording_paths, "_plain_metadata", fail_only_after_publication)
    try:
        published = publish_recording_no_replace(paths, identity)
    finally:
        assert paths.final_path.is_file()
        assert _identity(paths.final_path) == identity
    assert published == paths.final_path
    assert postlink_reads == []


@pytest.mark.skipif(sys.platform != "win32", reason="actual Windows namespace pin release")
def test_postcommit_release_error_retains_explicit_published_outcome(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    identity = _encoded(paths)
    original = recording_paths._windows_pin

    @contextmanager
    def fail_after_actual_source_release(path: Path, *, directory: bool) -> Iterator[int]:
        with original(path, directory=directory) as descriptor:
            yield descriptor
        if path == paths.encoded_path:
            message = "injected custody release error after completed link"
            raise OSError(message)

    monkeypatch.setattr(recording_paths, "_windows_pin", fail_after_actual_source_release)
    with pytest.raises(OSError, match="custody release") as raised:
        publish_recording_no_replace(paths, identity)
    assert paths.final_path.is_file()
    assert _identity(paths.final_path) == identity
    assert getattr(raised.value, "published_path", None) == paths.final_path
    assert getattr(raised.value, "output_identity", None) == identity
    assert getattr(raised.value, "retirement_uncertain", None) is True


@pytest.mark.parametrize("format_name", ["wav", "flac", "mp3"])
def test_actual_nonreplace_hardlink_preserves_closed_source_identity_and_bytes(
    owned_case: Path, format_name: str
) -> None:
    options = RecordingOptions.model_validate({"format": format_name}, strict=True)
    paths = create_recording_namespace(owned_case / "record", options)
    identity = _encoded(paths)
    original = paths.encoded_path.read_bytes()

    published = publish_recording_no_replace(paths, identity)

    assert published == paths.final_path
    assert published.suffix == f".{format_name}"
    assert _identity(published) == identity
    assert _identity(paths.encoded_path) == identity
    assert published.read_bytes() == original
    assert paths.encoded_path.read_bytes() == original
    assert paths.metadata_path.is_file()
    assert paths.directory.is_dir()
    assert_recording_namespace(paths)
    metadata = json.loads(paths.metadata_path.read_text(encoding="utf-8"))
    assert metadata["schema"] == "flitzi-recording-owner-v1"
    assert metadata["attempt_id"] == paths.attempt_id
    assert metadata["options"] == options.model_dump(mode="json")
    assert metadata["final"] == paths.final_path.name


def test_existing_root_and_unknown_files_are_reused_without_claiming_or_modifying_them(
    owned_case: Path,
) -> None:
    root = owned_case / "record"
    root.mkdir()
    unknown = root / "unknown-existing.wav"
    unknown.write_bytes(b"unknown original must survive")
    identity = _identity(unknown)
    first = create_recording_namespace(root, RecordingOptions())
    second = create_recording_namespace(root, RecordingOptions(format="flac"))

    assert first.root_identity == second.root_identity
    assert first.directory != second.directory
    assert first.directory_identity != second.directory_identity
    assert first.metadata_identity != second.metadata_identity
    assert unknown.read_bytes() == b"unknown original must survive"
    assert _identity(unknown) == identity
    assert not (root / "owner.json").exists()
    assert not first.final_path.exists()
    assert not second.final_path.exists()
    assert_recording_namespace(first)
    assert_recording_namespace(second)


def test_existing_destination_collision_preserves_both_files_unchanged(owned_case: Path) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    source_identity = _encoded(paths)
    paths.final_path.write_bytes(b"existing unknown destination")
    destination_identity = _identity(paths.final_path)
    source = paths.encoded_path.read_bytes()

    with pytest.raises(FileExistsError):
        publish_recording_no_replace(paths, source_identity)

    assert paths.final_path.read_bytes() == b"existing unknown destination"
    assert _identity(paths.final_path) == destination_identity
    assert paths.encoded_path.read_bytes() == source
    assert _identity(paths.encoded_path) == source_identity


def test_replaced_encoded_file_cannot_use_the_original_closed_file_identity(
    owned_case: Path,
) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    original_identity = _encoded(paths)
    retained = paths.directory / "original-encoded.retained"
    paths.encoded_path.rename(retained)
    paths.encoded_path.write_bytes(b"replacement must never become published")
    replacement_identity = _identity(paths.encoded_path)
    assert replacement_identity != original_identity

    with pytest.raises(RecordingNamespaceError, match="encoded recording identity"):
        publish_recording_no_replace(paths, original_identity)

    assert not paths.final_path.exists()
    assert _identity(retained) == original_identity
    assert _identity(paths.encoded_path) == replacement_identity
    assert paths.encoded_path.read_bytes() == b"replacement must never become published"


@pytest.mark.parametrize("entry", ["root", "directory", "metadata"])
def test_namespace_aba_with_new_entries_cannot_recover_old_authority(
    owned_case: Path, entry: str
) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    original = paths.metadata_path.read_bytes()
    if entry == "root":
        paths.root.rename(owned_case / "record-before-aba")
        paths.root.mkdir()
        unknown = paths.root / "unknown.txt"
    elif entry == "directory":
        paths.directory.rename(paths.root / "attempt-before-aba")
        paths.directory.mkdir()
        unknown = paths.directory / "unknown.txt"
    else:
        paths.metadata_path.rename(paths.directory / "owner-before-aba.json")
        paths.metadata_path.write_bytes(original)
        unknown = paths.directory / "unknown.txt"
        assert _identity(paths.metadata_path) != paths.metadata_identity
    unknown.write_bytes(b"unknown replacement evidence")

    with pytest.raises(RecordingNamespaceError, match="identity changed"):
        assert_recording_namespace(paths)

    assert unknown.read_bytes() == b"unknown replacement evidence"
    assert not paths.final_path.exists()


def test_in_place_owner_metadata_change_is_not_hidden_by_same_file_identity(
    owned_case: Path,
) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    with paths.metadata_path.open("ab") as handle:
        handle.write(b"unexpected trailing metadata")
    assert _identity(paths.metadata_path) == paths.metadata_identity

    with pytest.raises(RecordingNamespaceError, match="owner metadata changed"):
        assert_recording_namespace(paths)

    assert paths.metadata_path.read_bytes().endswith(b"unexpected trailing metadata")


def test_relative_root_is_refused_before_creating_any_attempt(owned_case: Path) -> None:
    with pytest.raises(ValueError, match="absolute"):
        create_recording_namespace(Path("record"), RecordingOptions())
    assert not tuple(owned_case.iterdir())


@pytest.mark.parametrize(
    "bad",
    [
        {"format": "../unknown"},
        {"wav_encoding": "pcm32"},
        {"flac_bit_depth": True},
        {"flac_bit_depth": 16.0},
        {"flac_compression": "8"},
        {"mp3_bitrate_kbps": 320.0},
    ],
)
def test_invalid_constructed_options_fail_before_filesystem_mutation(
    owned_case: Path, bad: dict[str, object]
) -> None:
    construct = cast("_UncheckedOptionsConstructor", RecordingOptions.model_construct)
    options = construct(**bad)
    with pytest.raises(ValidationError):
        create_recording_namespace(owned_case / "record", options)
    assert not tuple(owned_case.iterdir())


def test_uuid_directory_collision_preserves_original_owner_and_unknown_files(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    original = paths.metadata_path.read_bytes()
    unknown = paths.directory / "unknown.bin"
    unknown.write_bytes(b"keep collision evidence")
    before = set(paths.root.iterdir())
    monkeypatch.setattr(recording_paths, "uuid4", lambda: UUID(hex=paths.attempt_id))

    with pytest.raises(FileExistsError):
        create_recording_namespace(paths.root, RecordingOptions(format="mp3"))

    assert set(paths.root.iterdir()) == before
    assert paths.metadata_path.read_bytes() == original
    assert unknown.read_bytes() == b"keep collision evidence"
    assert_recording_namespace(paths)


def test_unsupported_link_failure_is_visible_and_has_no_replace_fallback(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    identity = _encoded(paths)
    original = paths.encoded_path.read_bytes()

    def unsupported(*_args: object, **_kwargs: object) -> None:
        raise OSError(errno.ENOTSUP, "injected unsupported hard-link capability")

    monkeypatch.setattr(os, "link", unsupported)
    with pytest.raises(OSError, match="unsupported hard-link"):
        publish_recording_no_replace(paths, identity)

    assert not paths.final_path.exists()
    assert paths.encoded_path.read_bytes() == original
    assert _identity(paths.encoded_path) == identity


def _symlink_or_skip(link: Path, target: Path, *, directory: bool) -> None:
    try:
        link.symlink_to(target, target_is_directory=directory)
    except NotImplementedError:
        pytest.skip("this runtime cannot create an actual symlink")
    except OSError as error:
        if getattr(error, "winerror", None) == 1314 or error.errno in {errno.EPERM, errno.EACCES}:
            pytest.skip("this account cannot create an actual symlink")
        raise


def test_reparse_ancestor_is_refused_without_touching_its_unknown_target(owned_case: Path) -> None:
    target = owned_case / "ordinary-target"
    target.mkdir()
    unknown = target / "unknown.txt"
    unknown.write_bytes(b"unknown target")
    link = owned_case / "linked-parent"
    _symlink_or_skip(link, target, directory=True)

    with pytest.raises(RecordingNamespaceError, match="reparse"):
        create_recording_namespace(link / "record", RecordingOptions())

    assert unknown.read_bytes() == b"unknown target"
    assert not (target / "record").exists()


def test_reparse_encoded_file_cannot_publish_a_foreign_target(owned_case: Path) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    target = owned_case / "foreign-encoded.bin"
    target.write_bytes(b"unknown target must remain unchanged")
    identity = _identity(target)
    _symlink_or_skip(paths.encoded_path, target, directory=False)

    with pytest.raises(RecordingNamespaceError, match="reparse"):
        publish_recording_no_replace(paths, identity)

    assert not paths.final_path.exists()
    assert target.read_bytes() == b"unknown target must remain unchanged"
    assert _identity(target) == identity


def test_modified_dto_cannot_redirect_publication_to_an_unknown_leaf(owned_case: Path) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    unknown = paths.directory / "unknown.bin"
    unknown.write_bytes(b"unknown leaf")
    redirected = replace(paths, encoded_path=unknown)

    with pytest.raises(RecordingNamespaceError, match="attempt paths changed"):
        publish_recording_no_replace(redirected, _identity(unknown))

    assert unknown.read_bytes() == b"unknown leaf"
    assert not paths.final_path.exists()


@pytest.mark.skipif(sys.platform != "win32", reason="actual Windows sharing guards")
def test_actual_windows_pins_refuse_source_writes_and_namespace_moves_during_link(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    identity = _encoded(paths)
    original = paths.encoded_path.read_bytes()
    actual_link = os.link

    def observed_link(source: Path, destination: Path, *, follow_symlinks: bool = True) -> None:
        with pytest.raises(PermissionError, match=r"\[WinError (?:5|32)\]"):
            source.rename(paths.directory / "moved-source.bin")
        with pytest.raises(PermissionError, match=r"\[WinError (?:5|32)\]"):
            paths.directory.rename(paths.root / "moved-attempt")
        with pytest.raises(PermissionError, match=r"\[WinError (?:5|32)\]"):
            paths.root.rename(owned_case / "moved-root")
        with pytest.raises(PermissionError, match=r"\[Errno 13\]"), source.open("wb"):
            pytest.fail(
                "Windows allowed a foreign writer while the original encoded file was pinned"
            )
        actual_link(source, destination, follow_symlinks=follow_symlinks)

    monkeypatch.setattr(os, "link", observed_link)
    assert publish_recording_no_replace(paths, identity) == paths.final_path
    assert paths.final_path.read_bytes() == original
    assert _identity(paths.final_path) == identity


@pytest.mark.skipif(sys.platform != "win32", reason="actual Windows sharing guards")
@pytest.mark.parametrize("exit_mode", ["normal", "exception"])
def test_held_namespace_protects_original_entries_and_releases_on_every_exit(
    owned_case: Path, exit_mode: str
) -> None:
    paths = create_recording_namespace(owned_case / "record", RecordingOptions())
    original = paths.metadata_path.read_bytes()

    def held_phase() -> None:
        with hold_recording_namespace(paths):
            with paths.pcm_path.open("xb") as stage:
                stage.write(b"exclusive synthetic stage; no codec claim")
            with pytest.raises(PermissionError, match=r"\[WinError (?:5|32)\]"):
                paths.root.rename(owned_case / "blocked-root")
            with pytest.raises(PermissionError, match=r"\[WinError (?:5|32)\]"):
                paths.directory.rename(paths.root / "blocked-attempt")
            with pytest.raises(PermissionError, match=r"\[WinError (?:5|32)\]"):
                paths.metadata_path.rename(paths.directory / "blocked-owner.json")
            with (
                pytest.raises(PermissionError, match=r"\[Errno 13\]"),
                paths.metadata_path.open("wb"),
            ):
                pytest.fail("Windows allowed an owner-record writer while namespace pins were held")
            if exit_mode == "exception":
                message = "injected caller failure inside held namespace"
                raise RuntimeError(message)

    if exit_mode == "exception":
        with pytest.raises(RuntimeError, match="injected caller failure"):
            held_phase()
    else:
        held_phase()

    # A real writer and actual renames prove no pin leaked through either exit.
    with paths.metadata_path.open("r+b") as owner:
        owner.write(original)
    assert_recording_namespace(paths)
    released_attempt = paths.root / "released-attempt"
    paths.directory.rename(released_attempt)
    released_root = owned_case / "released-root"
    paths.root.rename(released_root)
    retained_owner = released_root / released_attempt.name / "owner.json"
    assert retained_owner.read_bytes() == original
    assert _identity(retained_owner) == paths.metadata_identity
