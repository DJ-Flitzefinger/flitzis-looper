"""Check bundle ownership inventories without packaging or running a runtime."""

import argparse
import base64
import hashlib
import importlib.metadata
import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING
from zipfile import ZipFile

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


@pytest.mark.parametrize("defect", [None, "abi", "hash", "size", "repeated-record"])
def test_native_wheel_checks_exact_extension_record(tmp_path: Path, defect: str | None) -> None:
    builder = _license_builder()
    wheel = tmp_path / "flitzis_looper-0.1.0-cp314-cp314-win_amd64.whl"
    binary = b"extension fixture"
    tag = "cp313-cp313-win_amd64" if defect == "abi" else builder.WHEEL_TAG
    checksum = base64.urlsafe_b64encode(hashlib.sha256(binary).digest()).decode().rstrip("=")
    if defect == "hash":
        checksum = "wrong"
    length = len(binary) + (defect == "size")
    row = f"{builder.NATIVE_ENTRY},sha256={checksum},{length}\n"
    with ZipFile(wheel, "x") as archive:
        metadata = "flitzis_looper-0.1.0.dist-info"
        archive.writestr(f"{metadata}/METADATA", "Name: flitzis-looper\nVersion: 0.1.0\n")
        archive.writestr(f"{metadata}/WHEEL", f"Root-Is-Purelib: false\nTag: {tag}\n")
        archive.writestr(f"{metadata}/RECORD", row * (2 if defect == "repeated-record" else 1))
        archive.writestr(builder.NATIVE_ENTRY, binary)

    if defect is None:
        assert builder.read_native_wheel(wheel) == (
            binary,
            hashlib.sha256(wheel.read_bytes()).hexdigest(),
        )
    else:
        with pytest.raises(ValueError, match=r"metadata|RECORD"):
            builder.read_native_wheel(wheel)


def test_candidate_archive_contains_tracked_bytes_and_empty_directories(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    builder = _license_builder()
    source = tmp_path / "source"
    source.mkdir()
    (source / "Tracked File.py").write_bytes(b"exact bytes\r\n")
    (source / ".git").write_text("gitdir: local private path", encoding="utf-8")
    (source / "private.txt").write_text("not tracked", encoding="utf-8")
    monkeypatch.setattr(builder, "REPO", source)
    monkeypatch.setattr(builder, "git", lambda *args: "100644 object 0\tTracked File.py\0")
    stage = tmp_path / "stage"
    stage.mkdir()
    checkout = stage / "flitzis-looper"
    builder.complete_bundle_checkout(checkout, dirty=False)
    (checkout / "config").mkdir()
    output = tmp_path / "output"
    output.mkdir()
    release = {"release_id": "candidate", "git_commit": "a" * 40, "source_kind": "candidate"}
    builder.package_runtime(stage, output, release)

    manifest = json.loads((output / "portable-manifest.json").read_text(encoding="utf-8"))
    assert manifest["source_kind"] == "candidate"
    assert not (output / "INSTALL-LOOPER-PORTABLE.bat").exists()
    with ZipFile(output / manifest["package"]) as archive:
        assert set(archive.namelist()) == {
            "flitzis-looper/",
            "flitzis-looper/Tracked File.py",
            "flitzis-looper/config/",
            "release-manifest.json",
        }
        assert archive.read("flitzis-looper/Tracked File.py") == b"exact bytes\r\n"


def test_vendored_wheel_must_match_source_even_at_same_version(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    builder = _license_builder()
    relative = builder.VENDORED_SOURCE / "inference.py"
    source = tmp_path / "source"
    tracked = source / relative
    tracked.parent.mkdir(parents=True)
    tracked.write_bytes(b"current vendor source")
    monkeypatch.setattr(builder, "REPO", source)
    monkeypatch.setattr(builder, "git", lambda *args: relative.as_posix() + "\0")
    site = tmp_path / "site"
    metadata = site / "flitzis_bs_roformer-0.1.0.dist-info"
    metadata.mkdir(parents=True)
    package_file = site / "flitzis_bs_roformer/inference.py"
    package_file.parent.mkdir()
    package_file.write_bytes(b"stale vendor source")
    checksum = (
        base64.urlsafe_b64encode(hashlib.sha256(tracked.read_bytes()).digest()).decode().rstrip("=")
    )
    record = f"flitzis_bs_roformer/inference.py,sha256={checksum},{tracked.stat().st_size}\n"
    (metadata / "RECORD").write_text(record, encoding="utf-8")
    distribution = importlib.metadata.PathDistribution(metadata)
    with pytest.raises(ValueError, match=r"differs from tracked source: inference\.py"):
        builder.check_vendored_source(distribution)

    package_file.write_bytes(tracked.read_bytes())
    builder.check_vendored_source(distribution)


@pytest.mark.skipif(os.name != "nt", reason="Windows Portable starter")
@pytest.mark.parametrize(
    ("kind", "owner", "check", "expected"),
    [
        ("candidate", True, False, "Run START-LOOPER.bat --check"),
        ("candidate", True, True, "True True True False"),
        ("candidate", False, True, "owner does not match"),
        ("main", True, True, "owner does not match"),
        ("main", False, True, "True False False True"),
    ],
)
def test_candidate_dispatch_uses_build_preflight_only(
    tmp_path: Path, kind: str, *, owner: bool, check: bool, expected: str
) -> None:
    script = Path(__file__).resolve().parents[2] / "scripts/portable/dispatch.ps1"
    (tmp_path / ".install.lock").touch()
    if owner:
        (tmp_path / "portable-root.json").write_text(
            json.dumps({"schema_version": 1, "app": "flitzis-looper-candidate"}),
            encoding="utf-8",
        )
    (tmp_path / "current.json").write_text(
        json.dumps({"schema_version": 2, "git_commit": "a" * 40, "source_kind": kind}),
        encoding="utf-8",
    )
    runtime = tmp_path / "Runtime"
    runtime.mkdir()
    (runtime / "launch.ps1").write_text(
        """param([string] $Root, [switch] $Check, [switch] $InstallerOwned,
[switch] $BuildRuntimeCheck, [switch] $StartLockHeld)
$switches = @($Check.IsPresent, $InstallerOwned.IsPresent,
    $BuildRuntimeCheck.IsPresent, $StartLockHeld.IsPresent)
Write-Output ($switches -join ' ')
""",
        encoding="utf-8",
    )
    command = [
        "powershell.exe",
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        str(script),
        "-Root",
        str(tmp_path),
    ]
    if check:
        command += ["-Check"]
    result = subprocess.run(command, capture_output=True, text=True, check=False, timeout=20)
    if expected.startswith(("Run ", "owner ")):
        assert result.returncode == 1
        assert expected in result.stdout
    else:
        assert result.returncode == 0, result.stdout + result.stderr
        assert result.stdout.strip() == expected
