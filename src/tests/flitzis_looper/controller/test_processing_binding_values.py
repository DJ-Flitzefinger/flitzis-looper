"""Held source defaults must retain their exact supported builtin types and values."""

from types import FunctionType
from typing import TYPE_CHECKING

import pytest

from tests.flitzis_looper.controller.test_processing_code import _guarded_identity, _load

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize(
    ("expression", "replacement", "body"),
    [
        ("slice(1)", slice(*(True,)), "value.stop is True"),
        ("frozenset({1})", frozenset({True}), "next(iter(value)) is True"),
    ],
)
def test_named_slice_and_frozen_collection_defaults_preserve_nested_types(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    expression: str,
    replacement: object,
    body: str,
) -> None:
    path = tmp_path / "owned_named_types.py"
    path.write_text(
        f"default = {expression}\ndef classify(value=default):\n    return {body}\n",
        encoding="utf-8",
    )
    module = _load(path, "owned_named_types", monkeypatch)
    assert module.classify() is False
    assert _guarded_identity((path,)) is not None
    monkeypatch.setattr(module.classify, "__defaults__", (replacement,))
    assert module.classify() is True
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("keyword", [False, True])
@pytest.mark.parametrize(
    ("expression", "replacement", "body"),
    [
        ("1", True, "value is True"),
        ("(1,)", (True,), "value[0] is True"),
        ("[1]", [True], "value[0] is True"),
        ("{'item': 1}", {"item": True}, "value['item'] is True"),
        ("{1: 'item'}", {True: "item"}, "next(iter(value)) is True"),
        ("{1}", {True}, "next(iter(value)) is True"),
        ("({'item': [1]},)", ({"item": [True]},), "value[0]['item'][0] is True"),
    ],
)
def test_equal_default_values_cannot_erase_nested_builtin_types(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    keyword: bool,
    expression: str,
    replacement: object,
    body: str,
) -> None:
    path = tmp_path / "owned_default_types.py"
    path.write_text(
        f"def classify({'*, ' if keyword else ''}value={expression}):\n    return {body}\n",
        encoding="utf-8",
    )
    source = path.read_bytes()
    module = _load(path, "owned_default_types", monkeypatch)
    function = module.classify
    code = function.__code__
    assert function() is False
    assert _guarded_identity((path,)) is not None
    if keyword:
        monkeypatch.setattr(function, "__kwdefaults__", {"value": replacement})
    else:
        monkeypatch.setattr(function, "__defaults__", (replacement,))
    assert function() is True
    assert function.__code__ is code
    assert path.read_bytes() == source
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("kind", ["method", "constructor"])
@pytest.mark.parametrize("keyword", [False, True])
def test_source_declared_methods_and_constructors_preserve_default_type(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, kind: str, keyword: bool
) -> None:
    path = tmp_path / "owned_method_types.py"
    name = "classify" if kind == "method" else "__init__"
    body = "return value is True" if kind == "method" else "self.result = value is True"
    path.write_text(
        f"class Reader:\n    def {name}(self, {'*, ' if keyword else ''}value=1):\n"
        f"        {body}\n",
        encoding="utf-8",
    )
    module = _load(path, "owned_method_types", monkeypatch)
    function = getattr(module.Reader, name)
    before = module.Reader().classify() if kind == "method" else module.Reader().result
    assert before is False
    assert _guarded_identity((path,)) is not None
    if keyword:
        monkeypatch.setattr(function, "__kwdefaults__", {"value": True})
    else:
        monkeypatch.setattr(function, "__defaults__", (True,))
    after = module.Reader().classify() if kind == "method" else module.Reader().result
    assert after is True
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("expression", ["1", "(1,)", "{'item': 1}"])
def test_source_constants_reject_equal_values_with_different_nested_types(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, expression: str
) -> None:
    path = tmp_path / "owned_constant_types.py"
    path.write_text(f"LIMIT = {expression}\ndef current():\n    return LIMIT\n", encoding="utf-8")
    module = _load(path, "owned_constant_types", monkeypatch)
    assert _guarded_identity((path,)) is not None
    replacement = {"1": True, "(1,)": (True,), "{'item': 1}": {"item": True}}[expression]
    monkeypatch.setattr(module, "LIMIT", replacement)
    assert module.current() is replacement
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize(
    "expression", ["1", "'item'", "1.0", "b'item'", "(1,)", "[1]", "{1}", "{'item': 1}"]
)
def test_equal_builtin_subclasses_cannot_supply_default_evidence(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, expression: str
) -> None:
    path = tmp_path / "owned_subclass_types.py"
    path.write_text(
        f"default = {expression}\n"
        "def classify(value=default):\n    return type(value) is type(default)\n",
        encoding="utf-8",
    )
    module = _load(path, "owned_subclass_types", monkeypatch)
    assert module.classify() is True
    assert _guarded_identity((path,)) is not None
    original = module.default
    # Reusing the builtin name also checks collection evidence's old name-only tag.
    substitute = type(type(original).__name__, (type(original),), {})(original)
    assert substitute == original
    monkeypatch.setattr(module.classify, "__defaults__", (substitute,))
    assert module.classify() is False
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("constructor", [False, True])
@pytest.mark.parametrize(("expression", "base"), [("1", int), ("(1,)", tuple)])
def test_builtin_type_gates_do_not_use_metaclass_equality_or_hash(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    constructor: bool,
    expression: str,
    base: type,
) -> None:
    path = tmp_path / "owned_metaclass_types.py"
    source = (
        "from dataclasses import dataclass\n@dataclass\nclass Reader:\n"
        f"    value: object = {expression}\n"
        if constructor
        else f"def classify(value={expression}):\n    return value\n"
    )
    path.write_text(source, encoding="utf-8")
    module = _load(path, "owned_metaclass_types", monkeypatch)
    function = module.Reader.__init__ if constructor else module.classify
    original = module.Reader().value if constructor else module.classify()
    assert type(original) is base
    assert _guarded_identity((path,)) is not None
    calls = []

    class DeceptiveType(type):
        def __hash__(cls) -> int:
            calls.append("type-hash")
            return hash(base)

        def __eq__(cls, other: object) -> bool:
            calls.append("type-equality")
            return other is base

    substitute = DeceptiveType(base.__name__, (base,), {})(original)
    monkeypatch.setattr(function, "__defaults__", (substitute,))
    actual = module.Reader().value if constructor else module.classify()
    assert type(actual) is not base
    assert actual == original
    calls.clear()
    assert _guarded_identity((path,)) is None
    assert calls == []


def test_rejected_subclass_evidence_never_calls_custom_equality(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = tmp_path / "owned_equality_hook.py"
    path.write_text("def classify(value=1):\n    return value is True\n", encoding="utf-8")
    module = _load(path, "owned_equality_hook", monkeypatch)
    assert _guarded_identity((path,)) is not None
    calls = []

    class OtherInt(int):
        __hash__ = int.__hash__

        def __eq__(self, other: object) -> bool:
            calls.append(other)
            return True

    monkeypatch.setattr(module.classify, "__defaults__", (OtherInt(1),))
    assert _guarded_identity((path,)) is None
    assert calls == []


@pytest.mark.parametrize("kind", ["positional", "keyword", "keyword_name"])
def test_default_containers_and_keyword_names_reject_subclasses_before_hooks(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, kind: str
) -> None:
    path = tmp_path / "owned_default_container.py"
    path.write_text("def classify(value=1, *, multiplier=1):\n    return value * multiplier\n")
    module = _load(path, "owned_default_container", monkeypatch)
    assert _guarded_identity((path,)) is not None
    calls = []

    class OtherTuple(tuple):
        __slots__ = ()

        def __len__(self) -> int:
            calls.append("tuple-len")
            return 1

    def observed_keys(value: dict) -> object:
        calls.append("dict-keys")
        return dict.keys(value)

    dictionary_type = type("dict", (dict,), {"keys": observed_keys})
    string_type = type("str", (str,), {})

    if kind == "positional":
        monkeypatch.setattr(module.classify, "__defaults__", OtherTuple((1,)))
    elif kind == "keyword":
        monkeypatch.setattr(module.classify, "__kwdefaults__", dictionary_type(multiplier=1))
    else:
        monkeypatch.setattr(module.classify, "__kwdefaults__", {string_type("multiplier"): 1})
    assert _guarded_identity((path,)) is None
    assert calls == []


@pytest.mark.parametrize("keyword", [False, True])
def test_retained_callable_defaults_reject_equal_but_type_changed_inner_defaults(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, keyword: bool
) -> None:
    provider = tmp_path / "owned_type_provider.py"
    provider.write_text("def classify(value=1):\n    return value is True\n", encoding="utf-8")
    previous = _load(provider, "owned_type_provider", monkeypatch)
    consumer = tmp_path / "owned_type_consumer.py"
    consumer.write_text(
        "from owned_type_provider import classify\n"
        f"def consume({'*, ' if keyword else ''}worker=classify):\n    return worker()\n",
        encoding="utf-8",
    )
    retained = _load(consumer, "owned_type_consumer", monkeypatch)
    assert retained.consume() is False
    assert _guarded_identity((provider, consumer)) is not None
    current = _load(provider, "owned_type_provider", monkeypatch)
    retained.__dict__["classify"] = current.classify
    assert isinstance(previous.classify, FunctionType)
    monkeypatch.setattr(previous.classify, "__defaults__", (True,))
    assert retained.consume() is True
    assert _guarded_identity((provider,)) is not None
    assert _guarded_identity((provider, consumer)) is None
