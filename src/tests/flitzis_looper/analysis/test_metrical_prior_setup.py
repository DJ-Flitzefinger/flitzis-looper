"""Immutable profile tokens, streamed inventory and atomic pointer changes."""

import hashlib
import json
from threading import Event
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import metrical_prior_setup as setup

if TYPE_CHECKING:
    from pathlib import Path


def _fixture_profile(
    root: Path, monkeypatch: pytest.MonkeyPatch, marker: str
) -> setup.PriorProfileToken:
    installed = root / f"installation-{marker * 32}"
    installed.mkdir(parents=True)
    files = {
        "python/python.exe": b"actual-base",
        "python/python312.dll": b"base-dll",
        "node/node.exe": b"node",
        "prepare-features.py": b"e4",
        "prepare-features-streamed.py": b"301",
        "infer-prior.cjs": b"classifier",
        "numeric-profile.json": json.dumps({"numeric_profile_id": setup.PROFILE_ID}).encode(),
        "python-packages/owned.py": marker.encode(),
        "upstream/deeptemp-k16-3-tfjs.zip": b"archive",
        "upstream/deeptemp-k16-3-tfjs/model.json": b"graph",
        "upstream/deeptemp-k16-3-tfjs/group1-shard1of1.bin": b"shard",
    }
    hashes = {name: hashlib.sha256(data).hexdigest() for name, data in files.items()}
    for name, data in files.items():
        path = installed / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    for constant, name in (
        ("PYTHON_SHA256", "python/python.exe"),
        ("PYTHON_DLL_SHA256", "python/python312.dll"),
        ("NODE_SHA256", "node/node.exe"),
        ("E4_SHA256", "prepare-features.py"),
        ("FRONTEND_SHA256", "prepare-features-streamed.py"),
        ("INFERENCE_SHA256", "infer-prior.cjs"),
        ("NUMERIC_PROFILE_SHA256", "numeric-profile.json"),
    ):
        monkeypatch.setattr(setup, constant, hashes[name])
    monkeypatch.setattr(
        setup,
        "MODEL_HASHES",
        {name: hashes[name] for name in files if name.startswith("upstream/")},
    )
    index = installed / "files.jsonl"
    rows = [
        {"path": name, "bytes": len(files[name]), "sha256": hashes[name]} for name in sorted(files)
    ]
    index.write_text(
        "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in rows), encoding="utf-8"
    )
    manifest = {
        "schema_version": 1,
        "profile_id": setup.PROFILE_ID,
        "numeric_profile_sha256": hashes["numeric-profile.json"],
        "python": {
            "path": "python/python.exe",
            "sha256": hashes["python/python.exe"],
            "version": "3.12.13",
        },
        "node": {"path": "node/node.exe", "sha256": hashes["node/node.exe"], "version": "v25.5.0"},
        "frontend": {
            "path": "prepare-features-streamed.py",
            "sha256": hashes["prepare-features-streamed.py"],
        },
        "inference": {"path": "infer-prior.cjs", "sha256": hashes["infer-prior.cjs"]},
        "inventory": {
            "path": "files.jsonl",
            "sha256": setup.digest(index),
            "count": len(rows),
            "total_bytes": sum(len(value) for value in files.values()),
        },
    }
    setup.write_json_new(installed / "manifest.json", manifest)
    return setup.PriorProfileToken(
        installed, setup.digest(installed / "manifest.json"), hashes["numeric-profile.json"]
    )


def test_pointer_change_keeps_a_previously_captured_profile_token(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    first = _fixture_profile(tmp_path, monkeypatch, "a")
    setup.activate_profile(tmp_path, first.installation_root)
    captured = setup.token_from_wire(json.loads(setup.probe_metrical_prior_profile(str(tmp_path))))
    second = _fixture_profile(tmp_path, monkeypatch, "b")
    setup.activate_profile(tmp_path, second.installation_root)
    assert captured == first
    assert setup.verify_profile(captured)["inventory"]["count"] == 11
    assert (
        setup.token_from_wire(json.loads(setup.probe_metrical_prior_profile(str(tmp_path))))
        == second
    )
    assert first.installation_root.is_dir()


@pytest.mark.parametrize(
    "extra", ["python-packages/unlisted.py", "python-packages/__pycache__/owned.cpython-312.pyc"]
)
def test_unlisted_or_mutable_import_files_make_profile_unavailable(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, extra: str
) -> None:
    token = _fixture_profile(tmp_path, monkeypatch, "a")
    path = token.installation_root / extra
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"unverified")
    with pytest.raises(ValueError, match=r"unlisted|mutable"):
        setup.verify_profile(token)


def test_corrupt_inventory_or_source_never_changes_current_pointer(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    token = _fixture_profile(tmp_path, monkeypatch, "a")
    setup.activate_profile(tmp_path, token.installation_root)
    original = (tmp_path / "current.json").read_bytes()
    (token.installation_root / "python-packages/owned.py").write_bytes(b"changed")
    with pytest.raises(ValueError, match="file changed"):
        setup.activate_profile(tmp_path, token.installation_root)
    assert (tmp_path / "current.json").read_bytes() == original


def test_inventory_verification_observes_original_cancellation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    token = _fixture_profile(tmp_path, monkeypatch, "a")
    cancel = Event()
    cancel.set()
    with pytest.raises(RuntimeError, match="cancelled"):
        setup.verify_profile(token, cancel)
