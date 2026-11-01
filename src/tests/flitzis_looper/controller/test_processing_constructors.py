"""Generated host constructors must agree with the held dataclass declarations."""

import ast
import inspect
import json
from dataclasses import fields, make_dataclass
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import contracts
from flitzis_looper.controller import processing_code, stem_artifacts, stem_generation
from flitzis_looper_audio import SealedProcessingFiles
from tests.flitzis_looper.controller.test_processing_code import _guarded_identity, _load

if TYPE_CHECKING:
    from types import ModuleType


def _record(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, options: str = "", body: str = ""
) -> tuple[Path, ModuleType]:
    path = tmp_path / "owned_constructor.py"
    path.write_text(
        "from dataclasses import dataclass\n"
        f"@dataclass({options})\nclass Record:\n    value: int = 1\n{body}",
        encoding="utf-8",
    )
    module = _load(path, "owned_constructor", monkeypatch)
    assert _guarded_identity((path,)) is not None
    return path, module


@pytest.mark.parametrize(
    "change",
    ["binding", "body", "default", "default_type", "qualname", "origin", "descriptor"],
)
def test_generated_constructor_binding_body_and_defaults_are_not_metadata_only(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, change: str
) -> None:
    path, module = _record(tmp_path, monkeypatch, options="slots=True")
    constructor = module.Record.__init__
    if change in {"binding", "body"}:
        namespace = {}
        exec("def alternate(self, value=1):\n    self.value = -value\n", namespace)
        alternate = namespace["alternate"]
        if change == "binding":
            alternate.__qualname__ = constructor.__qualname__
            monkeypatch.setattr(module.Record, "__init__", alternate)
        else:
            monkeypatch.setattr(constructor, "__code__", alternate.__code__)
        assert module.Record().value == -1
    elif change in {"default", "default_type"}:
        monkeypatch.setattr(constructor, "__defaults__", (True if change == "default_type" else 2,))
        assert fields(module.Record)[0].default == 1
        if change == "default_type":
            assert type(module.Record().value) is bool
        else:
            assert module.Record().value == 2
    elif change == "qualname":
        monkeypatch.setattr(constructor, "__qualname__", "Other.__init__")
    elif change == "origin":
        monkeypatch.setattr(
            constructor, "__code__", constructor.__code__.replace(co_filename=__file__)
        )
    else:
        monkeypatch.setattr(module.Record, "__init__", staticmethod(constructor))
    assert _guarded_identity((path,)) is None


def test_generated_keyword_defaults_must_match_source_not_just_field_metadata(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path, module = _record(tmp_path, monkeypatch, options="kw_only=True, frozen=True, slots=True")
    monkeypatch.setattr(module.Record.__init__, "__kwdefaults__", {"value": 2})
    assert fields(module.Record)[0].default == 1
    assert module.Record().value == 2
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("keyword_only", [False, True])
@pytest.mark.parametrize("nested", [False, True])
def test_generated_defaults_cannot_substitute_equal_builtin_subclasses(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, keyword_only: bool, nested: bool
) -> None:
    path, module = _record(tmp_path, monkeypatch, options=f"kw_only={keyword_only}")
    if nested:
        path.write_text(path.read_text().replace("value: int = 1", "value: tuple = (1,)"))
        module = _load(path, "owned_constructor", monkeypatch)
        assert _guarded_identity((path,)) is not None

    class OtherInt(int):
        pass

    replacement = (OtherInt(1),) if nested else OtherInt(1)
    if keyword_only:
        monkeypatch.setattr(module.Record.__init__, "__kwdefaults__", {"value": replacement})
    else:
        monkeypatch.setattr(module.Record.__init__, "__defaults__", (replacement,))
    assert module.Record().value is replacement
    assert _guarded_identity((path,)) is None


def test_source_bound_custom_default_is_not_executed_by_reference_generation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = tmp_path / "owned_custom_default.py"
    path.write_text(
        "from dataclasses import dataclass\nclass OtherInt(int):\n    pass\n"
        "default = OtherInt(1)\n@dataclass\nclass Record:\n    value: int = default\n",
        encoding="utf-8",
    )
    module = _load(path, "owned_custom_default", monkeypatch)
    assert type(module.Record().value) is module.OtherInt
    assert _guarded_identity((path,)) is None


def test_frozen_generated_constructor_binds_the_canonical_object_cell(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path, module = _record(tmp_path, monkeypatch, options="frozen=True, slots=True")
    constructor = module.Record.__init__
    assert constructor.__code__.co_freevars == ("__dataclass_builtins_object__",)
    cell = constructor.__closure__[0]
    canonical = cell.cell_contents
    assign = canonical.__setattr__
    other_object = SimpleNamespace(
        __setattr__=lambda instance, name, value: assign(instance, name, -value)
    )

    try:
        cell.cell_contents = other_object
        assert module.Record().value == -1
        assert _guarded_identity((path,)) is None
    finally:
        cell.cell_contents = canonical
    assert _guarded_identity((path,)) is not None


@pytest.mark.parametrize("change", ["skip", "inject", "delete", "body"])
def test_generated_constructor_cannot_skip_or_invent_source_post_init(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, change: str
) -> None:
    body = (
        "    def __post_init__(self):\n"
        "        if self.value <= 0:\n            raise ValueError('positive')\n"
    )
    path, module = _record(
        tmp_path, monkeypatch, options="slots=True", body="" if change == "inject" else body
    )
    if change == "skip":
        reference = make_dataclass("Record", [("value", int, 1)], slots=True)
        monkeypatch.setattr(module.Record.__init__, "__code__", reference.__init__.__code__)
        assert module.Record(-1).value == -1
    elif change == "inject":
        monkeypatch.setattr(module.Record, "__post_init__", lambda _: None, raising=False)
    elif change == "delete":
        monkeypatch.delattr(module.Record, "__post_init__")
    else:
        monkeypatch.setattr(module.Record, "__post_init__", lambda _: None)
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("init", [True, False])
def test_source_declared_constructor_keeps_its_body_check(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, init: bool
) -> None:
    path, module = _record(
        tmp_path,
        monkeypatch,
        options=f"init={init}",
        body="    def __init__(self, value=1):\n        self.value = value + 1\n",
    )
    assert module.Record().value == 2
    monkeypatch.setattr(module.Record.__init__, "__defaults__", (2,))
    assert _guarded_identity((path,)) is None


def test_disabled_generated_constructor_requires_exact_object_init(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path, module = _record(tmp_path, monkeypatch, options="init=False")
    assert inspect.getattr_static(module.Record, "__init__") is object.__init__
    monkeypatch.setattr(module.Record, "__init__", lambda _: None)
    assert _guarded_identity((path,)) is None


def test_inherited_constructor_hook_remains_outside_the_simple_dataclass_policy(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = tmp_path / "owned_inherited_constructor.py"
    path.write_text(
        "from dataclasses import dataclass\nclass Base:\n"
        "    def __post_init__(self):\n        raise RuntimeError('inherited hook')\n"
        "@dataclass\nclass Record(Base):\n    value: int = 1\n",
        encoding="utf-8",
    )
    module = _load(path, "owned_inherited_constructor", monkeypatch)
    with pytest.raises(RuntimeError, match="inherited hook"):
        module.Record()
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("change", ["init", "metadata", "kw_only"])
def test_generated_constructor_refuses_unproved_mutated_field_policy(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, change: str
) -> None:
    path, module = _record(tmp_path, monkeypatch)
    field = fields(module.Record)[0]
    replacement = {"custom": True} if change == "metadata" else 0 if change == "kw_only" else False
    monkeypatch.setattr(field, change, replacement)
    assert _guarded_identity((path,)) is None


def test_generated_constructor_check_does_not_invoke_host_constructor_or_hook(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path, module = _record(
        tmp_path,
        monkeypatch,
        options="frozen=True, slots=True",
        body="    def __post_init__(self):\n        raise RuntimeError('must not execute')\n",
    )
    assert _guarded_identity((path,)) is not None
    with pytest.raises(RuntimeError, match="must not execute"):
        module.Record()


def test_retained_generated_class_alias_does_not_adopt_reloaded_provider_defaults(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    provider, previous = _record(tmp_path, monkeypatch, options="frozen=True, slots=True")
    consumer = tmp_path / "owned_constructor_consumer.py"
    consumer.write_text(
        "from owned_constructor import Record\ndef construct():\n    return Record()\n",
        encoding="utf-8",
    )
    retained = _load(consumer, "owned_constructor_consumer", monkeypatch)
    assert _guarded_identity((provider, consumer)) is not None
    provider.write_text(provider.read_text().replace("value: int = 1", "value: int = 2"))
    current = _load(provider, "owned_constructor", monkeypatch)
    assert retained.Record is previous.Record is not current.Record
    assert retained.construct().value == 1
    assert current.Record().value == 2
    assert _guarded_identity((provider,)) is not None
    assert _guarded_identity((provider, consumer)) is None


@pytest.mark.parametrize(
    ("module", "name"),
    [
        (stem_artifacts, "_Source"),
        (contracts, "WorkerLimits"),
        (stem_generation, "StemGenerationRequest"),
    ],
)
def test_actual_source_limits_and_request_generated_constructor_components(
    monkeypatch: pytest.MonkeyPatch, module: ModuleType, name: str
) -> None:
    path = Path(module.__file__).resolve()
    held = SealedProcessingFiles([str(path)])
    manifest = processing_code._manifest(path)
    declaration = next(
        item for item in manifest[0].body if isinstance(item, ast.ClassDef) and item.name == name
    )
    actual = vars(module)[name]
    before = processing_code._check_class(
        actual, declaration, manifest[1], vars(module), {path: manifest}
    )
    assert json.dumps(before)
    constructor = actual.__init__
    defaults = tuple(item for item in constructor.__defaults__ or ())
    if defaults:
        monkeypatch.setattr(constructor, "__defaults__", (*defaults[:-1], "changed construction"))
    else:
        monkeypatch.setattr(constructor, "__qualname__", "Other.__init__")
    with pytest.raises(ValueError, match="constructor"):
        processing_code._check_class(
            actual, declaration, manifest[1], vars(module), {path: manifest}
        )
    held.verify()


def test_actual_worker_limits_cannot_skip_the_source_bounds_check(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    path = Path(contracts.__file__).resolve()
    assert _guarded_identity((path,)) is not None
    with pytest.raises(ValueError, match="finite and positive"):
        contracts.WorkerLimits(max_pcm_bytes=-1)
    reference = make_dataclass(
        "WorkerLimits",
        [(item.name, object, item.default) for item in fields(contracts.WorkerLimits)],
        frozen=True,
        slots=True,
    )
    monkeypatch.setattr(contracts.WorkerLimits.__init__, "__code__", reference.__init__.__code__)
    assert contracts.WorkerLimits(max_pcm_bytes=-1).max_pcm_bytes == -1
    assert _guarded_identity((path,)) is None
