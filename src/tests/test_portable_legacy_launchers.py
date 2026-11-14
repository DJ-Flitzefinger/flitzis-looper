"""Verify historical launchers without granting ownership of runtime files."""

import hashlib
import importlib.util
from pathlib import Path

import pytest


@pytest.mark.parametrize(
    ("dispatcher", "git_checkout"),
    [
        (b"First\nSecond\n", b"First\r\nSecond\r\n"),
        (b"First\r\nSecond\n", None),
        (b"First\r\nSecond\r\n", None),
        (b"First\0\nSecond\n", None),
    ],
)
def test_legacy_launchers_use_only_original_source_blobs(
    monkeypatch: pytest.MonkeyPatch, dispatcher: bytes, git_checkout: bytes | None
) -> None:
    source = Path("cache/s-01234567")
    commit = "a" * 40
    blobs = {
        "scripts/portable/build_bundle.py": b"STARTER = ('fixture ' 'starter\\r\\n')\n",
        "scripts/portable/dispatch.ps1": dispatcher,
    }

    def read(arguments: list[str]) -> bytes:
        assert arguments[:4] == ["git", "-C", str(source), "show"]
        actual_commit, path = arguments[4].split(":", 1)
        assert actual_commit == commit
        return blobs[path]

    utility = Path(__file__).resolve().parents[2] / "scripts/portable/legacy_launchers.py"
    specification = importlib.util.spec_from_file_location("portable_legacy_launchers", utility)
    assert specification is not None
    assert specification.loader is not None
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    monkeypatch.setattr(module.subprocess, "check_output", read)

    expected = {
        "START-LOOPER.bat": hashlib.sha256(b"fixture starter\r\n").hexdigest(),
        "dispatch.ps1": hashlib.sha256(dispatcher).hexdigest(),
    }
    if git_checkout is not None:
        expected["dispatch.ps1.crlf"] = hashlib.sha256(git_checkout).hexdigest()
    assert module.legacy_launchers("git", source, commit) == expected
    blobs["scripts/portable/build_bundle.py"] += b"STARTER = 'unknown replacement'\n"
    with pytest.raises(ValueError, match="Unrecognized legacy launcher declaration"):
        module.legacy_launchers("git", source, commit)
