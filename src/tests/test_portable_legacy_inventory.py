"""Prove recognized legacy leaves without claiming unrelated Human files."""

import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
from typing import TYPE_CHECKING
from zipfile import ZipFile

import pytest

if TYPE_CHECKING:
    from types import ModuleType

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Windows file ownership attributes")
REPO = Path(__file__).resolve().parents[2]
SOURCE = Path("cache/s-01234567")


def _write(root: Path, relative: str, content: bytes) -> None:
    target = root / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(content)


def _record(contents: dict[str, bytes], self_path: str) -> bytes:
    rows = []
    for name, content in contents.items():
        checksum = base64.urlsafe_b64encode(hashlib.sha256(content).digest()).rstrip(b"=")
        rows.append(f"{name},sha256={checksum.decode()},{len(content)}\n")
    rows.append(self_path + ",,\n")
    return "".join(rows).encode()


def _native_distribution(root: Path, tracked: dict[str, bytes], native: bytes) -> None:
    distribution = "flitzis_looper-0.1.0.dist-info"
    metadata = {
        "METADATA": b"Name: flitzis-looper\nVersion: 0.1.0\n",
        "WHEEL": b"Wheel-Version: 1.0\nTag: cp314-cp314-win_amd64\n",
        "licenses/LICENSE.txt": tracked["LICENSE.txt"],
    }
    contents = {
        "flitzis_looper_audio/looper.pyd": native,
        "flitzis_looper_audio/__init__.py": tracked["src/flitzis_looper_audio/__init__.py"],
    }
    contents.update({distribution + "/" + name: content for name, content in metadata.items()})
    for name, content in contents.items():
        _write(root / "legacy/app", name, content)
    _write(
        root / "legacy/app", distribution + "/RECORD", _record(contents, distribution + "/RECORD")
    )
    wheel = root / "cache/wheels-01234567/flitzis_looper-0.1.0-cp314-cp314-win_amd64.whl"
    wheel.parent.mkdir(parents=True)
    with ZipFile(wheel, "w") as archive:
        for name, content in metadata.items():
            archive.writestr(distribution + "/" + name, content)
        archive.writestr("flitzis_looper_audio/looper.pyd", b"different new Native build")


@pytest.fixture
def legacy_proof(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> tuple[ModuleType, Path]:
    specification = importlib.util.spec_from_file_location(
        "portable_legacy_inventory",
        REPO / "scripts/portable/legacy_inventory.py",
    )
    assert specification is not None
    assert specification.loader is not None
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    candidate, legacy = tmp_path / "candidate", tmp_path / "legacy"

    tracked = {
        "scripts/portable/build_bundle.py": (
            b"DLL_NAMES = {'rubberband.dll'}\nLICENSE_NAMES = {'rubberband.txt'}\n"
            b"STARTER = 'fixture starter\\r\\n'\n"
        ),
        "src/flitzis_looper/__init__.py": b"# tracked source\n",
        "src/flitzis_looper_audio/__init__.py": b"# tracked Native wrapper\n",
        "assets/models/keynet.onnx": b"tracked keynet",
        "scripts/portable/sitecustomize.py": b"# private path setup\n",
        "LICENSE.txt": b"tracked project license",
        "vendor/bs_roformer/NOTICE.md": b"tracked vendor notice",
    }
    for name in ["bootstrap.py", "launch.ps1", "dispatch.ps1", "common.ps1", "build-path.ps1"]:
        content = f"tracked helper {name}".encode()
        tracked[f"scripts/portable/{name}"] = content
        _write(legacy, name, content)
    for name in ["python/python.exe", "runtime/ffmpeg/ffmpeg.exe"]:
        _write(candidate, name, b"pinned private tool")
        _write(legacy, name, b"pinned private tool")
    for name in ["src/flitzis_looper/__init__.py", "src/flitzis_looper_audio/__init__.py"]:
        _write(legacy, "app/" + name.removeprefix("src/"), tracked[name])
    _write(legacy, "python/keynet.onnx", tracked["assets/models/keynet.onnx"])
    _write(
        legacy,
        "python/Lib/site-packages/sitecustomize.py",
        tracked["scripts/portable/sitecustomize.py"],
    )
    _write(legacy, "START-LOOPER.bat", b"fixture starter\r\n")
    _write(legacy, "app/flitzis_looper_audio/rubberband.dll", b"owned Native DLL")
    _write(tmp_path / SOURCE, "src/flitzis_looper_audio/rubberband.dll", b"owned Native DLL")
    native = b"accepted Native extension"
    _write(legacy, "app/flitzis_looper_audio/looper.pyd", native)
    _native_distribution(tmp_path, tracked, native)
    _write(legacy, "licenses/rubberband.txt", b"owned license")
    _write(candidate, "licenses/rubberband.txt", b"owned license")
    _write(legacy, "licenses/flitzis-looper.txt", tracked["LICENSE.txt"])
    _write(legacy, "licenses/bs-roformer-NOTICE.md", tracked["vendor/bs_roformer/NOTICE.md"])
    metadata = b"Name: dep-one\nVersion: 1.0\n"
    dependency = b"# installed dependency\n"
    distribution = "dep_one-1.0.dist-info"
    rows = []
    for name, content in [(distribution + "/METADATA", metadata), ("dep_one.py", dependency)]:
        _write(legacy, "python/Lib/site-packages/" + name, content)
        _write(candidate, "python/Lib/site-packages/" + name, content)
        checksum = base64.urlsafe_b64encode(hashlib.sha256(content).digest()).rstrip(b"=")
        rows.append(f"{name},sha256={checksum.decode()},{len(content)}\n")
    rows.append(distribution + "/RECORD,,\n")
    for directory in [legacy, candidate]:
        _write(
            directory,
            "python/Lib/site-packages/" + distribution + "/RECORD",
            "".join(rows).encode(),
        )
    _write(
        legacy,
        "release-manifest.json",
        json.dumps({
            "schema_version": 1,
            "git_commit": "a" * 40,
            "source_dirty": False,
            "python_version": "3.14.2",
            "native_sha256": hashlib.sha256(native).hexdigest(),
            "native_source_commit": "a" * 40,
            "runtime_dependencies": {"dep-one": "1.0"},
        }).encode(),
    )

    def read_git(arguments: list[str]) -> bytes:
        command = arguments[3:]
        if command[0] == "show":
            return tracked[command[1].split(":", 1)[1]]
        return b"\0".join(name.encode() for name in tracked if name.startswith("src/"))

    monkeypatch.setattr(module.subprocess, "check_output", read_git)
    return module, tmp_path


def test_legacy_known_inventory_is_complete(legacy_proof: tuple[ModuleType, Path]) -> None:
    module, root = legacy_proof
    result = module.inventory("git", root / SOURCE, root / "candidate", root / "legacy", "a" * 40)
    actual = {
        path.relative_to(root / "legacy").as_posix()
        for path in (root / "legacy").rglob("*")
        if path.is_file()
    }
    assert set(result) == actual - {"release-manifest.json"}


@pytest.mark.parametrize(
    "relative",
    [
        "human.wav",
        "python/python.exe",
        "python/Lib/site-packages/dep_one.py",
        "app/flitzis_looper_audio/rubberband.dll",
        "licenses/rubberband.txt",
        "licenses/flitzis-looper.txt",
        "licenses/bs-roformer-NOTICE.md",
        "app/flitzis_looper-0.1.0.dist-info/licenses/LICENSE.txt",
    ],
)
def test_legacy_unknown_or_changed_leaf_stops_and_preserves(
    legacy_proof: tuple[ModuleType, Path],
    relative: str,
) -> None:
    module, root = legacy_proof
    leaf = root / "legacy" / relative
    leaf.write_bytes(b"Human bytes")
    with pytest.raises(ValueError, match=r"Unknown legacy file|Changed legacy file"):
        module.inventory("git", root / SOURCE, root / "candidate", root / "legacy", "a" * 40)
    assert leaf.read_bytes() == b"Human bytes"


@pytest.mark.parametrize("native", [False, True])
def test_forged_legacy_record_cannot_claim_a_human_leaf(
    legacy_proof: tuple[ModuleType, Path],
    *,
    native: bool,
) -> None:
    module, root = legacy_proof
    base = root / "legacy" / ("app" if native else "python/Lib/site-packages")
    distribution = "flitzis_looper-0.1.0.dist-info" if native else "dep_one-1.0.dist-info"
    record = base / distribution / "RECORD"
    leaf = base / "human.wav"
    leaf.write_bytes(b"Human bytes")
    checksum = base64.urlsafe_b64encode(hashlib.sha256(leaf.read_bytes()).digest()).rstrip(b"=")
    with record.open("a", encoding="utf-8") as stream:
        stream.write(f"human.wav,sha256={checksum.decode()},11\n")
    forged = record.read_bytes()
    with pytest.raises(
        ValueError,
        match=r"Changed legacy RECORD|Unauthenticated Native RECORD leaf",
    ):
        module.inventory("git", root / SOURCE, root / "candidate", root / "legacy", "a" * 40)
    assert record.read_bytes() == forged
    assert leaf.read_bytes() == b"Human bytes"


def test_record_direct_url_omission_does_not_grant_extra_leaf_authority(
    legacy_proof: tuple[ModuleType, Path],
) -> None:
    module, root = legacy_proof
    record = root / "legacy/python/Lib/site-packages/dep_one-1.0.dist-info/RECORD"
    with record.open("a", encoding="utf-8") as stream:
        stream.write("dep_one-1.0.dist-info/direct_url.json,sha256=omitted,1\n")
    result = module.inventory("git", root / SOURCE, root / "candidate", root / "legacy", "a" * 40)
    assert "python/Lib/site-packages/dep_one-1.0.dist-info/RECORD" in result
    assert not any("direct_url.json" in name for name in result)


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("native_source_commit", "b" * 40),
        ("native_sha256", "not-a-sha"),
    ],
)
def test_legacy_native_identity_must_match_exact_old_source(
    legacy_proof: tuple[ModuleType, Path],
    field: str,
    value: str,
) -> None:
    module, root = legacy_proof
    manifest = root / "legacy/release-manifest.json"
    contents = json.loads(manifest.read_text())
    contents[field] = value
    manifest.write_text(json.dumps(contents))
    with pytest.raises(ValueError, match="Legacy Native source/checksum identity"):
        module.inventory("git", root / SOURCE, root / "candidate", root / "legacy", "a" * 40)


def test_missing_candidate_dll_proof_preserves_known_old_leaf(
    legacy_proof: tuple[ModuleType, Path],
) -> None:
    module, root = legacy_proof
    (root / SOURCE / "src/flitzis_looper_audio/rubberband.dll").unlink()
    preserved = root / "legacy/app/flitzis_looper_audio/rubberband.dll"
    original = preserved.read_bytes()
    with pytest.raises(ValueError, match="Candidate content proof unavailable") as error:
        module.inventory("git", root / SOURCE, root / "candidate", root / "legacy", "a" * 40)
    assert str(preserved) in str(error.value)
    assert preserved.read_bytes() == original
