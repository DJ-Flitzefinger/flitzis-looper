"""Actual stale-cache and loaded host-code/default/binding policy regressions."""

import json
import sys
from contextvars import ContextVar
from pathlib import Path
from textwrap import dedent
from threading import Event
from types import ModuleType

import pytest

from flitzis_looper import offline_runtime, stem_cancellation
from flitzis_looper.analysis import contracts as analysis_contracts
from flitzis_looper.analysis import linux_process, windows_job
from flitzis_looper.analysis import process as analysis_process
from flitzis_looper.controller import stem_process
from flitzis_looper.controller.processing_code import (
    loaded_code_identity,
)
from flitzis_looper_audio import SealedProcessingFiles

_SOURCE = dedent("""\
    LIMIT = 1

    def transform(value=1, *, multiplier=1):
        return (value + LIMIT) * multiplier

    def alternate(value=1, *, multiplier=1):
        return (value + LIMIT) * multiplier

    class Reader:
        def apply(self, value=1):
            return value + LIMIT

        def alternate(self, value=1):
            return value + LIMIT

        @property
        def limit(self):
            return LIMIT
""")

_CHILD_IMPORT = dedent("""\
    import json
    import sys
    import owned_cache_producer as producer
    print(json.dumps({
        "value": producer.VALUE,
        "defaults": producer.transform.__defaults__,
        "result": producer.transform(),
        "source": producer.__file__,
        "cached": producer.__cached__,
        "prefix": sys.pycache_prefix,
        "dont_write": sys.dont_write_bytecode,
    }))
""")


def _load(path: Path, name: str, monkeypatch: pytest.MonkeyPatch) -> ModuleType:
    module = ModuleType(name)
    module.__dict__.update({"__file__": str(path), "__package__": ""})
    monkeypatch.setitem(sys.modules, name, module)
    exec(compile(path.read_bytes(), str(path), "exec", dont_inherit=True), module.__dict__)
    return module


def _guarded_identity(paths: tuple[Path, ...]) -> str | None:
    guard = SealedProcessingFiles([str(path) for path in paths])
    result = loaded_code_identity(paths)
    guard.verify()
    return result


def _retain(path: Path, values: dict[str, object]) -> None:
    output = path.parent / (path.name + "-python-code-lineage.json")
    output.write_text(json.dumps(values, indent=2), encoding="utf-8")


def _owned_process_roles(
    directory: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[tuple[Path, ...], dict[str, ModuleType]]:
    """Load exact own copies of the actually executed process helper declarations."""
    directory.mkdir()
    paths = []
    modules = {}
    for source in (
        offline_runtime,
        analysis_contracts,
        windows_job,
        linux_process,
        stem_cancellation,
        analysis_process,
        stem_process,
    ):
        assert source.__file__ is not None
        path = directory / Path(source.__file__).name
        path.write_bytes(Path(source.__file__).read_bytes())
        paths.append(path)
        modules[path.name] = _load(path, source.__name__, monkeypatch)
    return tuple(paths), modules


def test_actual_process_helper_roles_are_known_and_equal_relocation_keeps_code_identity(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    first, _modules = _owned_process_roles(tmp_path / "first", monkeypatch)
    before = _guarded_identity(first)
    assert before is not None
    relocated, _modules = _owned_process_roles(tmp_path / "relocated", monkeypatch)
    assert [path.read_bytes() for path in first] == [path.read_bytes() for path in relocated]
    assert _guarded_identity(relocated) == before


def test_active_cancel_events_context_paths_and_admission_slot_do_not_change_code_identity(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths, modules = _owned_process_roles(tmp_path / "carriers", monkeypatch)
    before = _guarded_identity(paths)
    assert before is not None
    process = modules["process.py"]
    scope = modules["stem_process.py"]
    cancel = Event()
    assert process._PROCESS_SLOT.acquire(blocking=False)
    try:
        with scope.stem_command_scope(cancel, directory=tmp_path / "own-job-a"):
            assert _guarded_identity(paths) == before
            cancel.set()
            assert _guarded_identity(paths) == before
        with scope.stem_command_scope(Event(), directory=tmp_path / "own-job-b"):
            assert _guarded_identity(paths) == before
    finally:
        process._PROCESS_SLOT.release()
    assert _guarded_identity(paths) == before


@pytest.mark.parametrize("role", ["process", "windows_job", "linux_process", "stem_cancellation"])
def test_changed_actual_process_helper_source_rejects_loaded_code_until_own_reload(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, role: str
) -> None:
    directory = tmp_path / "changed"
    paths, modules = _owned_process_roles(directory, monkeypatch)
    before = _guarded_identity(paths)
    assert before is not None
    path = directory / f"{role}.py"
    replacements = {
        "process": ("_READ_CHUNK_BYTES = 16384", "_READ_CHUNK_BYTES = 8192"),
        "windows_job": ("_MAX_JOB_PROCESSES = 64", "_MAX_JOB_PROCESSES = 32"),
        "linux_process": ("time.monotonic() + 5.0", "time.monotonic() + 4.0"),
        "stem_cancellation": ("TOOL_POLL_SECONDS = 0.02", "TOOL_POLL_SECONDS = 0.03"),
    }
    first, second = replacements[role]
    source = path.read_text(encoding="utf-8")
    assert first in source
    path.write_text(source.replace(first, second), encoding="utf-8", newline="\n")
    assert _guarded_identity(paths) is None
    for current in paths:
        original = modules[current.name]
        _load(current, original.__name__, monkeypatch)
    after = _guarded_identity(paths)
    assert after is not None
    assert after != before


@pytest.mark.parametrize(
    "binding", ["command", "job", "linux", "context", "work_context", "resource_slot", "layout"]
)
def test_changed_actual_process_helper_binding_cannot_claim_known_loaded_code(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, binding: str
) -> None:
    paths, modules = _owned_process_roles(tmp_path / "binding", monkeypatch)
    assert _guarded_identity(paths) is not None
    if binding == "command":
        modules["stem_process.py"].__dict__["run_owned_command"] = lambda *_args, **_kwargs: None
    elif binding == "job":
        modules["process.py"].__dict__["WindowsJob"] = object
    elif binding == "linux":
        modules["process.py"].__dict__["LinuxProcess"] = object
    elif binding == "context":
        modules["stem_process.py"].__dict__["_COMMAND_SCOPE"] = ContextVar(
            "different-channel", default=None
        )
    elif binding == "work_context":
        modules["process.py"].__dict__["_WORK_OWNER"] = ContextVar("foreign-work", default=None)
    elif binding == "resource_slot":
        modules["process.py"].__dict__["_PROCESS_SLOT"] = object()
    else:
        modules["windows_job.py"]._BasicLimits.flags = 0
    assert _guarded_identity(paths) is None


def test_missing_parent_source_roles_cannot_claim_execution_identity() -> None:
    assert loaded_code_identity(()) is None
