"""Actual fresh child origins versus earlier parent selection and path shadows."""

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
from types import ModuleType
from typing import TYPE_CHECKING

from flitzis_looper.controller.processing_code import fresh_python_source_environment
from flitzis_looper_audio import SealedProcessingFiles

if TYPE_CHECKING:
    import pytest


def _package(root: Path, value: int) -> Path:
    package = root / "owned_origin_producer"
    package.mkdir(parents=True)
    (package / "__init__.py").write_bytes(b"")
    source = package / "transform.py"
    source.write_text(f"VALUE = {value}\n", encoding="utf-8")
    return source


def _child_origin(cwd: Path, env: dict[str, str]) -> dict[str, object]:
    script = (
        "import hashlib,json,os,sys;from pathlib import Path;"
        "from owned_origin_producer import transform;"
        "p=Path(transform.__file__);"
        "print(json.dumps(dict(value=transform.VALUE,path=str(p),"
        "sha=hashlib.sha256(p.read_bytes()).hexdigest(),"
        "pid=os.getpid(),safe_path=sys.flags.safe_path)))"
    )
    result = subprocess.run(
        [sys.executable, "-c", script], cwd=cwd, env=env, capture_output=True, text=True, check=True
    )
    parsed: dict[str, object] = json.loads(result.stdout)
    return parsed


def test_actual_child_uses_held_roots_before_cwd_and_inherited_pythonpath(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    held_root = tmp_path / "held-root"
    source = _package(held_root, 7)
    cwd = tmp_path / "poison-cwd"
    cwd_source = _package(cwd, 999)
    inherited_root = tmp_path / "poison-inherited-path"
    inherited_source = _package(inherited_root, 555)
    # The already-loaded parent selection alone cannot attest a fresh child.
    parent = ModuleType("owned_origin_producer.transform")
    parent.__file__ = str(source)
    exec(compile(source.read_bytes(), str(source), "exec"), parent.__dict__)
    monkeypatch.setitem(sys.modules, parent.__name__, parent)
    env = dict(os.environ) | {"PYTHONPATH": str(inherited_root)}
    env.pop("PYTHONSAFEPATH", None)
    wrong_result = _child_origin(cwd, env)
    assert wrong_result["value"] == 999
    wrong_path = wrong_result["path"]
    assert isinstance(wrong_path, str)
    assert Path(wrong_path).samefile(cwd_source)
    guard = SealedProcessingFiles([str(source.parent / "__init__.py"), str(source)])
    with fresh_python_source_environment(env, tmp_path, source_roots=(held_root,)) as child_env:
        result = _child_origin(cwd, child_env)
        assert result["value"] == 7
        actual_path = result["path"]
        assert isinstance(actual_path, str)
        assert Path(actual_path).samefile(source)
        assert result["sha"] == hashlib.sha256(source.read_bytes()).hexdigest()
        assert result["pid"] != os.getpid()
        assert result["safe_path"] is True
        guard.verify()
    assert cwd_source.read_text(encoding="utf-8") == "VALUE = 999\n"
    assert inherited_source.read_text(encoding="utf-8") == "VALUE = 555\n"
    (tmp_path / "actual-origin-lineage.json").write_text(
        json.dumps(
            {
                "earlier_parent": str(source),
                "unbound_child": wrong_result,
                "actual_held_child": result,
            },
            indent=2,
        ),
        encoding="utf-8",
    )
