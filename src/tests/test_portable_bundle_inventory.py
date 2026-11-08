"""Check bundle ownership inventories without packaging or running a runtime."""

import argparse
import hashlib
import importlib.util
import os
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from types import ModuleType


def test_checkout_directory_inventory_records_empty_and_hidden_trees(tmp_path: Path) -> None:
    script = Path(__file__).resolve().parents[2] / "scripts/portable/build_bundle.py"
    spec = importlib.util.spec_from_file_location("portable_bundle_inventory", script)
    assert spec is not None
    assert spec.loader is not None
    builder = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(builder)
    checkout = tmp_path / "checkout"
    for relative in (".git/objects/info", "config", "src/package"):
        (checkout / relative).mkdir(parents=True)
    source = checkout / "src/package/application.py"
    source.write_bytes(b"owned source")

    assert builder.directory_inventory(checkout) == [
        ".git",
        ".git/objects",
        ".git/objects/info",
        "config",
        "src",
        "src/package",
    ]
    assert builder.file_inventory(checkout) == {
        "src/package/application.py": hashlib.sha256(b"owned source").hexdigest()
    }
    assert not (checkout / "config/input").exists()


def _license_builder() -> ModuleType:
    script = Path(__file__).resolve().parents[2] / "scripts/portable/build_bundle.py"
    spec = importlib.util.spec_from_file_location("portable_license_inventory", script)
    assert spec is not None
    assert spec.loader is not None
    builder = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(builder)
    return builder


def test_tracked_licenses_and_provenance_are_copied_byte_exactly(tmp_path: Path) -> None:
    builder = _license_builder()
    staging = tmp_path / "staging"
    staging.mkdir()
    values = []
    for name in builder.LICENSE_NAMES:
        source = tmp_path / name
        source.write_bytes(b"external license fixture\r\n")
        values.append(f"{name}={source}")
    builder.copy_licenses(staging, values)
    for name, relative in builder.TRACKED_LICENSES.items():
        assert (staging / "licenses" / name).read_bytes() == (builder.REPO / relative).read_bytes()


def test_external_license_cannot_overwrite_tracked_provenance(tmp_path: Path) -> None:
    builder = _license_builder()
    staging = tmp_path / "staging"
    staging.mkdir()
    source = tmp_path / "external.txt"
    source.write_bytes(b"unrelated bytes")
    with pytest.raises(ValueError, match="must not overwrite"):
        builder.copy_licenses(staging, [f"ASIO-SDK-PROVENANCE.JSON={source}"])
    assert (staging / "licenses/asio-sdk-provenance.json").read_bytes() == (
        builder.REPO / builder.TRACKED_LICENSES["asio-sdk-provenance.json"]
    ).read_bytes()


@pytest.mark.skipif(
    os.name != "nt" or sys.version_info[:2] != (3, 14), reason="Windows Native bundle builder"
)
def test_overlay_changes_refuse_stale_native_source(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    builder = _license_builder()
    checkout = tmp_path / "source with spaces"
    checkout.mkdir()
    monkeypatch.setattr(builder, "REPO", checkout)
    monkeypatch.setenv("GIT_CONFIG_GLOBAL", os.devnull)
    monkeypatch.setenv("GIT_CONFIG_NOSYSTEM", "1")
    for name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"):
        monkeypatch.delenv(name, raising=False)
    hooks = tmp_path / "empty-hooks"
    hooks.mkdir()

    def git(*args: str) -> str:
        return subprocess.check_output(
            [
                "git",
                "-c",
                "init.templateDir=",
                "-c",
                f"core.hooksPath={hooks}",
                "-c",
                "user.name=Portable Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-C",
                str(checkout),
                *args,
            ],
            text=True,
        ).strip()

    git("init")
    patch = checkout / "scripts/native/vcpkg-overlay/libsamplerate/stereo-controls.patch"
    patch.parent.mkdir(parents=True)
    patch.write_text("original patch\n", encoding="utf-8")
    readme = checkout / "README.md"
    readme.write_text("unrelated documentation\n", encoding="utf-8")
    git("add", ".")
    git("commit", "--no-gpg-sign", "-m", "Fixture baseline")
    baseline = git("rev-parse", "HEAD")
    args = argparse.Namespace(native_source_commit=baseline, allow_dirty=True)
    assert builder.source_identity(args) == (baseline, False)
    readme.write_text("updated unrelated documentation\n", encoding="utf-8")
    assert builder.source_identity(args) == (baseline, True)
    extra = patch.with_name("new-input.patch")
    extra.write_text("new dependency input\n", encoding="utf-8")
    with pytest.raises(ValueError, match="Commit Native dependency overlay inputs"):
        builder.source_identity(args)
    extra.unlink()
    patch.write_text("changed patch\n", encoding="utf-8")
    with pytest.raises(ValueError, match="Native inputs changed"):
        builder.source_identity(args)
    git("add", ".")
    git("commit", "--no-gpg-sign", "-m", "Fixture overlay update")
    with pytest.raises(ValueError, match="Native inputs changed"):
        builder.source_identity(args)
    args.native_source_commit = git("rev-parse", "HEAD")
    assert builder.source_identity(args) == (args.native_source_commit, False)
