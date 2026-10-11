"""Actual stale-cache and loaded host-code/default/binding policy regressions."""

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
from textwrap import dedent
from types import ModuleType

import pytest

from flitzis_looper.controller.processing_code import (
    fresh_python_source_environment,
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


def test_missing_parent_source_roles_cannot_claim_execution_identity() -> None:
    assert loaded_code_identity(()) is None


def test_actual_slice_code_constant_matches_current_source_and_rejects_stale_code(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "owned_slice_transform.py"
    source.write_text("def transform(payload):\n    return payload[24:40]\n", encoding="utf-8")
    _load(source, "flitzis_owned_slice", monkeypatch)
    assert _guarded_identity((source,)) is not None
    source.write_text("def transform(payload):\n    return payload[25:40]\n", encoding="utf-8")
    assert _guarded_identity((source,)) is None


def test_actual_timestamp_valid_old_pyc_is_ignored_by_new_owned_empty_child_cache(
    tmp_path: Path,
) -> None:
    source = tmp_path / "owned_cache_producer.py"
    old = "VALUE = 1\ndef transform(default=1):\n    return default + VALUE\n"
    source.write_text(old, encoding="utf-8")
    original = source.stat()
    inherited = tmp_path / "preexisting-python-cache"
    inherited.mkdir()
    env = dict(os.environ) | {"PYTHONPYCACHEPREFIX": str(inherited)}
    env.pop("PYTHONDONTWRITEBYTECODE", None)

    def child(child_env: dict[str, str], *, disabled_writes: bool = False) -> dict[str, object]:
        args = [sys.executable, *(["-B"] if disabled_writes else []), "-c", _CHILD_IMPORT]
        result = subprocess.run(
            args, cwd=tmp_path, env=child_env, capture_output=True, text=True, check=True
        )
        result_body: object = json.loads(result.stdout)
        assert isinstance(result_body, dict)
        return result_body

    seeded = child(env)
    cached = Path(str(seeded["cached"]))
    assert cached.is_relative_to(inherited)
    assert cached.is_file()
    before_cache = cached.stat()
    cache_digest = hashlib.sha256(cached.read_bytes()).hexdigest()
    source.write_text(old.replace("= 1", "= 2").replace("default=1", "default=2"), encoding="utf-8")
    os.utime(source, ns=(original.st_atime_ns, original.st_mtime_ns))
    assert source.stat().st_size == original.st_size
    assert source.stat().st_mtime_ns == original.st_mtime_ns
    legacy = child(env | {"PYTHONDONTWRITEBYTECODE": "1"}, disabled_writes=True)
    assert legacy["value"] == 1
    assert legacy["defaults"] == [1]
    assert legacy["result"] == 2
    assert legacy["dont_write"] is True
    guard = SealedProcessingFiles([str(source)])
    with fresh_python_source_environment(env, tmp_path) as current_env:
        prefix = Path(current_env["PYTHONPYCACHEPREFIX"])
        born = prefix.stat()
        assert not any(prefix.iterdir())
        current = child(current_env)
        assert current["value"] == 2
        assert current["defaults"] == [2]
        assert current["result"] == 4
        assert current["dont_write"] is True
        assert Path(str(current["prefix"])) == prefix
        assert not Path(str(current["cached"])).exists()
        assert not any(prefix.iterdir())
        guard.verify()
        _retain(
            tmp_path,
            {
                "scope": (
                    "actual owned child import; preexisting timestamp-valid stale pyc; "
                    "guarded current source; private fresh empty namespace; "
                    "no model/NN/audio/device"
                ),
                "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                "source_bytes": source.stat().st_size,
                "source_preserved_mtime_ns": source.stat().st_mtime_ns,
                "old_pyc_sha256": cache_digest,
                "old_pyc_file_id": [before_cache.st_dev, before_cache.st_ino],
                "seeded": seeded,
                "legacy_B_still_old": legacy,
                "fresh_current": current,
                "fresh_namespace_file_id": [born.st_dev, born.st_ino],
                "guard_witnesses": guard.witnesses(),
            },
        )
    after_cache = cached.stat()
    assert (after_cache.st_dev, after_cache.st_ino) == (before_cache.st_dev, before_cache.st_ino)
    assert hashlib.sha256(cached.read_bytes()).hexdigest() == cache_digest


@pytest.mark.parametrize(
    "change", ["body", "global", "default", "kwdefault", "binding", "method_binding", "property"]
)
def test_parent_loaded_transform_matches_source_then_rejects_old_code_defaults_or_binding(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, change: str
) -> None:
    path = tmp_path / "owned_parent_transform.py"
    path.write_text(_SOURCE, encoding="utf-8")
    module = _load(path, "owned_parent_transform", monkeypatch)
    before = _guarded_identity((path,))
    assert before is not None
    if change in {"body", "global", "default", "kwdefault", "property"}:
        replacements = {
            "body": ("value + LIMIT", "value - LIMIT"),
            "global": ("LIMIT = 1", "LIMIT = 2"),
            "default": ("def transform(value=1", "def transform(value=2"),
            "kwdefault": (
                "def transform(value=1, *, multiplier=1",
                "def transform(value=1, *, multiplier=2",
            ),
            "property": ("return LIMIT", "return -LIMIT"),
        }
        first, second = replacements[change]
        path.write_text(_SOURCE.replace(first, second, 1), encoding="utf-8")
    elif change == "binding":
        module.__dict__["transform"] = module.alternate
    else:
        module.Reader.apply = module.Reader.alternate
    assert _guarded_identity((path,)) is None
    _retain(
        tmp_path,
        {
            "scope": (
                "already-loaded own parent source/functions/classmethods/properties/defaults/"
                "constants; actual Native held current source"
            ),
            "change": change,
            "original_identity": before,
            "current_source_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "rejected_UNKNOWN": True,
        },
    )


@pytest.mark.parametrize("change", ["body", "default", "global", "class"])
def test_actual_stale_imported_callable_or_class_alias_is_checked_after_definition_reload(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, change: str
) -> None:
    provider = tmp_path / "owned_provider.py"
    consumer = tmp_path / "owned_consumer.py"
    source = dedent("""\
        LIMIT = 1
        def transform(value=1):
            return value + LIMIT
        class Reader:
            @property
            def limit(self):
                return LIMIT
    """)
    provider.write_text(source, encoding="utf-8")
    _load(provider, "owned_provider", monkeypatch)
    consumer.write_text(
        "from owned_provider import Reader, transform\n"
        "def consume():\n    return Reader().limit + transform()\n",
        encoding="utf-8",
    )
    old_consumer = _load(consumer, "owned_consumer", monkeypatch)
    before = _guarded_identity((provider, consumer))
    assert before is not None
    replacements = {
        "body": ("value + LIMIT", "value - LIMIT"),
        "default": ("def transform(value=1", "def transform(value=2"),
        "global": ("LIMIT = 1", "LIMIT = 2"),
        "class": ("return LIMIT", "return -LIMIT"),
    }
    first, second = replacements[change]
    provider.write_text(source.replace(first, second, 1), encoding="utf-8")
    current_provider = _load(provider, "owned_provider", monkeypatch)
    assert old_consumer.Reader is not current_provider.Reader
    assert old_consumer.transform is not current_provider.transform
    assert _guarded_identity((provider, consumer)) is None
    _retain(
        tmp_path,
        {
            "scope": (
                "actual old imported alias retained across own provider reload; current defining "
                "namespace passes but actual old invoked alias is rejected"
            ),
            "change": change,
            "original_identity": before,
            "provider_sha256": hashlib.sha256(provider.read_bytes()).hexdigest(),
            "consumer_sha256": hashlib.sha256(consumer.read_bytes()).hexdigest(),
            "rejected_UNKNOWN": True,
        },
    )


def test_parent_unknown_runtime_closure_does_not_claim_current_source_execution(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = tmp_path / "owned_closure.py"
    path.write_text(
        dedent("""\
        def make():
            value = 1
            def captured():
                return value
            return captured
        active = make()
        def consume():
            return active()
    """),
        encoding="utf-8",
    )
    module = _load(path, "owned_closure", monkeypatch)
    assert module.active.__closure__[0].cell_contents == 1
    module.active.__closure__[0].cell_contents = 2
    assert module.consume() == 2
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("change", ["frozen", "slots"])
def test_loaded_stdlib_dataclass_generation_policy_is_bound_to_current_source(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, change: str
) -> None:
    path = tmp_path / "owned_record.py"
    source = (
        "from dataclasses import dataclass\n@dataclass(frozen=False, slots=True)\n"
        "class Record:\n    value: int = 1\n"
    )
    path.write_text(source, encoding="utf-8")
    _load(path, "owned_record", monkeypatch)
    assert _guarded_identity((path,)) is not None
    path.write_text(
        source.replace("frozen=False", "frozen=True")
        if change == "frozen"
        else source.replace("slots=True", "slots=False"),
        encoding="utf-8",
    )
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("change", ["body", "default", "kwdefault"])
def test_stale_imported_contextmanager_checks_its_guarded_underlying_function(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, change: str
) -> None:
    provider = tmp_path / "owned_context_provider.py"
    consumer = tmp_path / "owned_context_consumer.py"
    source = (
        "from contextlib import contextmanager\n"
        "@contextmanager\ndef cache(value=1, *, multiplier=1):\n"
        "    yield value * multiplier\n"
    )
    provider.write_text(source, encoding="utf-8")
    _load(provider, "owned_context_provider", monkeypatch)
    consumer.write_text(
        "from owned_context_provider import cache\n"
        "def consume():\n    with cache() as value:\n        return value\n",
        encoding="utf-8",
    )
    previous = _load(consumer, "owned_context_consumer", monkeypatch)
    before = _guarded_identity((provider, consumer))
    assert before is not None
    first, second = {
        "body": ("yield value * multiplier", "yield value + multiplier"),
        "default": ("cache(value=1", "cache(value=2"),
        "kwdefault": ("multiplier=1", "multiplier=2"),
    }[change]
    provider.write_text(source.replace(first, second), encoding="utf-8")
    current = _load(provider, "owned_context_provider", monkeypatch)
    assert previous.cache is not current.cache
    assert previous.cache.__wrapped__ is not current.cache.__wrapped__
    assert _guarded_identity((provider,)) is not None
    assert _guarded_identity((provider, consumer)) is None
    _retain(
        tmp_path,
        {
            "scope": "actual retained stdlib contextmanager import wrapper",
            "change": change,
            "original_identity": before,
            "rejected_UNKNOWN": True,
        },
    )


@pytest.mark.parametrize("binding", ["positional", "keyword", "constructor"])
@pytest.mark.parametrize("change", ["default", "kwdefault"])
def test_retained_callable_default_checks_its_own_current_defaults(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, binding: str, change: str
) -> None:
    provider = tmp_path / "owned_default_provider.py"
    consumer = tmp_path / "owned_default_consumer.py"
    source = "def transform(value=1, *, multiplier=1):\n    return value * multiplier\n"
    provider.write_text(source, encoding="utf-8")
    _load(provider, "owned_default_provider", monkeypatch)
    body = {
        "positional": "def consume(worker=transform):\n    return worker()\n",
        "keyword": "def consume(*, worker=transform):\n    return worker()\n",
        "constructor": (
            "class Consumer:\n    def __init__(self, worker=transform):\n"
            "        self.worker = worker\n"
        ),
    }[binding]
    consumer.write_text("from owned_default_provider import transform\n" + body, encoding="utf-8")
    previous = _load(consumer, "owned_default_consumer", monkeypatch)
    before = _guarded_identity((provider, consumer))
    assert before is not None
    first, second = (
        ("value=1", "value=2") if change == "default" else ("multiplier=1", "multiplier=2")
    )
    provider.write_text(source.replace(first, second), encoding="utf-8")
    current = _load(provider, "owned_default_provider", monkeypatch)
    previous.__dict__["transform"] = current.transform
    actual = previous.Consumer.__init__ if binding == "constructor" else previous.consume
    retained = actual.__kwdefaults__["worker"] if binding == "keyword" else actual.__defaults__[0]
    assert retained is not current.transform
    assert retained.__code__.co_code == current.transform.__code__.co_code
    assert _guarded_identity((provider,)) is not None
    assert _guarded_identity((provider, consumer)) is None
    _retain(
        tmp_path,
        {
            "scope": "actual retained function as constructor/positional/keyword default",
            "binding": binding,
            "change": change,
            "original_identity": before,
            "rejected_UNKNOWN": True,
        },
    )
