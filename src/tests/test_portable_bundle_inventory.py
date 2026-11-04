"""Check bundle ownership inventories without packaging or running a runtime."""

import hashlib
import importlib.util
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
