"""Check bundle ownership inventories without packaging or running a runtime."""

import hashlib
import importlib.util
from pathlib import Path


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
