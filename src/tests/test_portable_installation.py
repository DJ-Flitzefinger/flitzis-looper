"""Exercise the real installer with small packages; never start GUI/audio."""

import ctypes
import hashlib
import json
import os
import shutil
import subprocess
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING
from zipfile import ZipFile, ZipInfo

import pytest

if TYPE_CHECKING:
    from collections.abc import Iterator

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Windows portable installation")
REPO = Path(__file__).resolve().parents[2]
COMMIT = "a" * 40
LAUNCH = """
param([string] $Root, [switch] $Check, [string] $VersionDirectory)
$ErrorActionPreference = 'Stop'
if (-not $Check) { exit 90 }
if (-not $VersionDirectory) { throw 'Staging must name its actual candidate version' }
$version = Join-Path (Join-Path $Root 'versions') $VersionDirectory
$releasePath = Join-Path $version 'release-manifest.json'
$release = Get-Content -LiteralPath $releasePath -Raw | ConvertFrom-Json
if ($release.release_id -ne '@VERSION@') { throw 'Wrong launch implementation' }
$required = Join-Path $version '@VERSION@-required.txt'
if (-not (Test-Path -LiteralPath $required)) { throw 'New launcher required' }
Set-Content -LiteralPath (Join-Path $Root 'cache/last-preflight.txt') -Value $release.release_id
exit @EXIT@
"""


@contextmanager
def exclusive_file(path: Path) -> Iterator[None]:
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateFileW.argtypes = [
        ctypes.c_wchar_p,
        ctypes.c_uint,
        ctypes.c_uint,
        ctypes.c_void_p,
        ctypes.c_uint,
        ctypes.c_uint,
        ctypes.c_void_p,
    ]
    kernel.CreateFileW.restype = ctypes.c_void_p
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    handle = kernel.CreateFileW(str(path), 0x80000000, 0, None, 3, 0, None)
    assert handle != ctypes.c_void_p(-1).value, ctypes.get_last_error()
    try:
        yield
    finally:
        assert kernel.CloseHandle(handle)


@dataclass
class Installation:
    installer: Path
    channel: Path
    target: Path

    def publish(self, version: str, *, failure: str = "") -> Path:
        package = self.channel.parent / (version + ".zip")
        files = {
            "release-manifest.json": json.dumps({"release_id": version, "git_commit": COMMIT}),
            "launch.ps1": LAUNCH.replace("@VERSION@", version).replace(
                "@EXIT@", "17" if failure == "preflight" else "0"
            ),
            "dispatch.ps1": (REPO / "scripts/portable/dispatch.ps1").read_text(),
            "START-LOOPER.bat": "@echo off\r\nexit /b 90\r\n",
            "python/python.exe": "immutable-runtime-" + version,
            "python/keynet.onnx": "model",
            "bootstrap.py": "not executed by the installer fixture",
            "runtime/ffmpeg/ffmpeg.exe": "ffmpeg fixture",
            "runtime/ffmpeg/ffprobe.exe": "ffprobe fixture",
            version + "-required.txt": "version-specific launch contract",
        }
        with ZipFile(package, "w") as archive:
            for name, content in files.items():
                archive.writestr(name, content)
            if failure == "escape":
                archive.writestr("../escaped.txt", "must not be extracted")
            if failure == "symlink":
                link = ZipInfo("link")
                link.create_system = 3
                link.external_attr = 0o120777 << 16
                archive.writestr(link, "outside")
            if failure == "windows-alias":
                archive.writestr("python/python.exe.", "alias")
        self.channel.write_text(
            json.dumps({
                "schema_version": 1,
                "release_id": version,
                "git_commit": COMMIT,
                "package": package.name,
                "package_size": package.stat().st_size,
                "package_sha256": "0" * 64
                if failure == "hash"
                else hashlib.sha256(package.read_bytes()).hexdigest(),
            }),
            encoding="utf-8",
        )
        return package

    def run(self) -> subprocess.CompletedProcess[str]:
        command = f'call "{self.installer}" "{self.target}" --no-pause'
        processor = os.environ.get("COMSPEC", "cmd.exe")
        environment = os.environ.copy()
        environment.pop("LOOPER_PORTABLE_CHANNEL", None)
        return subprocess.run(
            f'"{processor}" /d /s /c "{command}"',
            cwd=self.installer.parent,
            env=environment,
            capture_output=True,
            text=True,
            errors="replace",
            timeout=30,
            check=False,
        )

    def pointer(self) -> dict[str, object]:
        return json.loads((self.target / "current.json").read_text())


@pytest.fixture
def installation(tmp_path: Path) -> Installation:
    channel = tmp_path / "bundle source" / "portable-manifest.json"
    channel.parent.mkdir()
    caller = tmp_path / "Test ü ä with spaces"
    caller.mkdir()
    installer = caller / "INSTALL-LOOPER-PORTABLE.bat"
    content = (
        (REPO / installer.name)
        .read_text(encoding="utf-8-sig")
        .replace(
            "$DefaultChannel = '@PORTABLE_CHANNEL@'",
            "$DefaultChannel = '" + channel.as_uri().replace("'", "''") + "'",
        )
    )
    installer.write_bytes(content.replace("\n", "\r\n").encode("utf-8"))
    return Installation(installer, channel, caller / "Flitzis-Looper")


def require_success(result: subprocess.CompletedProcess[str]) -> None:
    assert result.returncode == 0, result.stdout + result.stderr


def test_install_repeat_and_update_keep_old_locked_runtime_and_user_data(
    installation: Installation,
) -> None:
    first_package = installation.publish("first")
    require_success(installation.run())
    original = installation.pointer()
    old_runtime = (
        installation.target / "versions" / str(original["directory"]) / "python/python.exe"
    )
    data = installation.target / "data/user-settings.json"
    model = installation.target / "models/my-model.bin"
    data.write_text("Human settings", encoding="utf-8")
    model.write_bytes(b"Human model")
    starter = (installation.target / "launch.ps1").read_bytes()
    first_package.rename(first_package.with_suffix(".unavailable"))
    require_success(installation.run())  # unchanged bundle must not be fetched
    assert installation.pointer() == original
    installation.publish("second")
    with exclusive_file(old_runtime):
        require_success(installation.run())
    assert installation.pointer()["release_id"] == "second"
    assert old_runtime.read_text() == "immutable-runtime-first"
    assert data.read_text() == "Human settings"
    assert model.read_bytes() == b"Human model"
    assert (installation.target / "launch.ps1").read_bytes() == starter
    # Use the stable dispatcher after activation, proving it invokes the new launcher.
    result = subprocess.run(
        [
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(installation.target / "launch.ps1"),
            "-Root",
            str(installation.target),
            "-Check",
        ],
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )
    require_success(result)
    assert (installation.target / "cache/last-preflight.txt").read_text().strip() == "second"


@pytest.mark.parametrize("failure", ["hash", "escape", "symlink", "windows-alias", "preflight"])
def test_failed_update_preserves_accepted_pointer_runtime_and_data(
    installation: Installation, failure: str
) -> None:
    installation.publish("first")
    require_success(installation.run())
    original = installation.pointer()
    data = installation.target / "data/project.bin"
    data.write_bytes(b"saved project")
    installation.publish("broken", failure=failure)
    result = installation.run()
    assert result.returncode != 0, result.stdout + result.stderr
    assert installation.pointer() == original
    assert data.read_bytes() == b"saved project"
    assert not list(installation.target.rglob("escaped.txt"))
    assert not (installation.target / "versions/escaped.txt").exists()


def test_installer_rejects_unowned_nonempty_target(installation: Installation) -> None:
    installation.publish("first")
    installation.target.mkdir()
    sentinel = installation.target / "human.txt"
    sentinel.write_text("owned by Human")
    assert installation.run().returncode != 0
    assert sentinel.read_text() == "owned by Human"
    assert not (installation.target / "portable-root.json").exists()


def test_installer_rejects_reparse_target_without_writing_through_it(
    installation: Installation,
) -> None:
    installation.publish("first")
    outside = installation.target.parent / "other owned folder"
    outside.mkdir()
    command = f'mklink /J "{installation.target}" "{outside}"'
    subprocess.run(f'cmd.exe /d /s /c "{command}"', check=True, capture_output=True)
    assert installation.run().returncode != 0
    assert list(outside.iterdir()) == []


def test_concurrent_installer_fails_without_changing_current_version(
    installation: Installation,
) -> None:
    installation.publish("first")
    require_success(installation.run())
    original = installation.pointer()
    installation.publish("second")
    with exclusive_file(installation.target / ".install.lock"):
        assert installation.run().returncode != 0
    assert installation.pointer() == original


def test_portable_launch_rejects_escaping_current_pointer(tmp_path: Path) -> None:
    shutil.copyfile(REPO / "scripts/portable/dispatch.ps1", tmp_path / "launch.ps1")
    (tmp_path / "current.json").write_text(
        json.dumps({"schema_version": 1, "directory": "../outside"})
    )
    result = subprocess.run(
        [
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(tmp_path / "launch.ps1"),
            "-Root",
            str(tmp_path),
            "-Check",
        ],
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )
    assert result.returncode != 0
