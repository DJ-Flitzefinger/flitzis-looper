"""Real Windows file custody; no Native, byte-authentication or recording acceptance."""

import os
import sys
from contextlib import contextmanager
from pathlib import Path
from typing import TYPE_CHECKING
from uuid import uuid4

import pytest

from flitzis_looper import recording_paths
from flitzis_looper.recording_paths import (
    RecordingCustodyReleaseError,
    RecordingNamespaceError,
    hold_live_recording_spool,
)

if TYPE_CHECKING:
    from collections.abc import Iterator

_OWNER = (
    "Owned synthetic live recording spool custody cases. No Human files or Native\n"
    "authority. Keep positive/negative evidence through reviewed delivery; Root\n"
    "applies the normal storage cleanup boundary.\n"
)
_WINDOWS = pytest.mark.skipif(sys.platform != "win32", reason="actual Windows sharing")


@pytest.fixture
def owned_case() -> Path:
    block = (
        Path(__file__).resolve().parents[4]
        / "test-artifacts"
        / "recording-format-20261020"
        / "live-spool-tests"
    )
    block.mkdir(parents=True, exist_ok=True)
    owner = block / "OWNER.md"
    try:
        with owner.open("x", encoding="utf-8", newline="\n") as stream:
            stream.write(_OWNER)
    except FileExistsError:
        assert owner.read_text(encoding="utf-8") == _OWNER
    case = block / uuid4().hex
    case.mkdir()
    return case


def identity(path: Path) -> tuple[int, int]:
    metadata = path.stat(follow_symlinks=False)
    return metadata.st_dev, metadata.st_ino


@_WINDOWS
@pytest.mark.parametrize("raise_inside", [False, True])
def test_live_witness_allows_original_writer_and_retains_entry_after_writer_close(
    owned_case: Path, *, raise_inside: bool
) -> None:
    path = owned_case / "float.capture"
    renamed = owned_case / "closed.capture"
    writer = path.open("xb")
    writer.write(b"original prefix")
    writer.flush()
    original = identity(path)

    def run() -> None:
        with hold_live_recording_spool(path) as held:
            assert held == original
            writer.write(b"; actual original writer continues")
            writer.flush()
            writer.close()
            assert path.read_bytes() == b"original prefix; actual original writer continues"
            with pytest.raises(PermissionError):
                path.rename(renamed)
            with pytest.raises(PermissionError):
                path.unlink()
            with pytest.raises(PermissionError):
                owned_case.rename(owned_case.with_name(owned_case.name + "-blocked"))
            if raise_inside:
                message = "actual caller error while original file witness is held"
                raise RuntimeError(message)

    try:
        if raise_inside:
            with pytest.raises(RuntimeError, match="actual caller error"):
                run()
        else:
            run()
    finally:
        writer.close()
    path.rename(renamed)
    assert identity(renamed) == original
    with renamed.open("r+b") as released_writer:
        released_writer.write(b"released")
    released_case = owned_case.with_name(owned_case.name + "-released")
    owned_case.rename(released_case)
    assert (released_case / renamed.name).read_bytes().startswith(b"released")


@_WINDOWS
def test_closed_file_pin_retains_its_existing_deny_write_policy(owned_case: Path) -> None:
    path = owned_case / "closed.capture"
    path.write_bytes(b"closed original")
    with recording_paths._windows_pin(path, directory=False) as descriptor:
        assert os.fstat(descriptor).st_ino == identity(path)[1]
        assert not os.get_inheritable(descriptor)
        with pytest.raises(PermissionError), path.open("r+b"):
            pytest.fail("strict closed-file pin unexpectedly shared writes")
        with pytest.raises(PermissionError):
            path.rename(owned_case / "blocked")
    with path.open("r+b") as stream:
        stream.write(b"released")


@_WINDOWS
def test_actual_replacement_between_path_stat_and_read_pin_is_rejected(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = owned_case / "float.capture"
    original_path = owned_case / "original.keep"
    path.write_bytes(b"original creator identity")
    original = identity(path)
    original_open = recording_paths._windows_read_handle

    @contextmanager
    def swapped(candidate: Path, *, directory: bool, sharing: int) -> Iterator[int]:
        if candidate == path and not directory:
            candidate.rename(original_path)
            candidate.write_bytes(b"foreign replacement")
        with original_open(candidate, directory=directory, sharing=sharing) as descriptor:
            yield descriptor

    monkeypatch.setattr(recording_paths, "_windows_read_handle", swapped)
    with (
        pytest.raises(RecordingNamespaceError, match="changed before"),
        hold_live_recording_spool(path),
    ):
        pytest.fail("foreign replacement received the original identity permission")
    assert identity(original_path) == original
    assert original_path.read_bytes() == b"original creator identity"
    assert path.read_bytes() == b"foreign replacement"
    path.rename(owned_case / "replacement-released")
    owned_case.rename(owned_case.with_name(owned_case.name + "-released"))


@_WINDOWS
def test_actual_close_error_remains_explicit_even_if_os_already_closed_the_handle(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = owned_case / "float.capture"
    path.write_bytes(b"original live file")
    original = identity(path)
    original_close = os.close
    target_closes = []

    def close_then_fail(descriptor: int) -> None:
        metadata = os.fstat(descriptor)
        target = (metadata.st_dev, metadata.st_ino) == original
        original_close(descriptor)
        if target:
            target_closes.append(descriptor)
            message = "injected uncertain close result after actual original close"
            raise OSError(message)

    monkeypatch.setattr(os, "close", close_then_fail)
    with (
        pytest.raises(RecordingCustodyReleaseError, match="uncertain close"),
        hold_live_recording_spool(path) as held,
    ):
        assert held == original
    assert len(target_closes) == 1
    path.rename(owned_case / "actually-released")
    owned_case.rename(owned_case.with_name(owned_case.name + "-released"))


@pytest.mark.parametrize("invalid", [Path("relative.capture"), Path("../outside.capture")])
def test_live_witness_rejects_nonabsolute_paths(invalid: Path) -> None:
    with pytest.raises(ValueError, match="absolute path"), hold_live_recording_spool(invalid):
        pytest.fail("nonabsolute file acquired custody")


@_WINDOWS
def test_live_witness_rejects_directory_and_releases_ancestor_pins(owned_case: Path) -> None:
    with (
        pytest.raises(RecordingNamespaceError, match="unsupported file type"),
        hold_live_recording_spool(owned_case),
    ):
        pytest.fail("directory acquired live-spool custody")
    owned_case.rename(owned_case.with_name(owned_case.name + "-released"))


def test_live_witness_has_no_silent_nonwindows_fallback(
    owned_case: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(sys, "platform", "linux")
    with (
        pytest.raises(RecordingNamespaceError, match="Windows sharing contract"),
        hold_live_recording_spool(owned_case / "float.capture"),
    ):
        pytest.fail("unsupported platform fabricated Windows file custody")
