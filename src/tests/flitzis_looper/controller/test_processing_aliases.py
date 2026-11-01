"""Mutable default owners retain their source-declared sharing relationships."""

import sys
from types import FunctionType, ModuleType
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller import processing_code
from tests.flitzis_looper.controller.test_processing_code import _guarded_identity, _load

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path


_SIGNATURES = {
    "positional": "first={first}, second={second}",
    "keyword": "*, first={first}, second={second}",
    "cross": "first={first}, *, second={second}",
}


def _replace_defaults(function: FunctionType, kind: str, first: object, second: object) -> None:
    if kind == "keyword":
        function.__kwdefaults__ = {"first": first, "second": second}
    elif kind == "cross":
        function.__defaults__ = (first,)
        function.__kwdefaults__ = {"second": second}
    else:
        function.__defaults__ = (first, second)


def _owners(expression: str, *, shared: bool) -> tuple[object, object]:
    factory = {"[]": list, "{}": dict, "{1}": lambda: {1}}[expression]
    first = factory()
    return first, first if shared else factory()


@pytest.mark.parametrize("kind", _SIGNATURES)
@pytest.mark.parametrize("expression", ["[]", "{}", "{1}"])
@pytest.mark.parametrize("shared", [False, True])
def test_source_default_owner_merge_and_split(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, kind: str, expression: str, *, shared: bool
) -> None:
    path = tmp_path / "owned_alias_defaults.py"
    value = "OWNER" if shared else expression
    signature = _SIGNATURES[kind].format(first=value, second=value)
    path.write_text(
        f"OWNER = {expression}\ndef same({signature}):\n    return first is second\n",
        encoding="utf-8",
    )
    source = path.read_bytes()
    module = _load(path, "owned_alias_defaults", monkeypatch)
    function = module.same
    code = function.__code__
    defaults, keywords = function.__defaults__, function.__kwdefaults__
    before = _guarded_identity((path,))
    assert before is not None
    assert function() is shared
    first, second = _owners(expression, shared=not shared)
    _replace_defaults(function, kind, first, second)
    assert function() is not shared
    assert function.__code__ is code
    assert path.read_bytes() == source
    assert _guarded_identity((path,)) is None
    function.__defaults__, function.__kwdefaults__ = defaults, keywords
    assert _guarded_identity((path,)) == before
    first, second = _owners(expression, shared=shared)
    _replace_defaults(function, kind, first, second)
    assert function() is shared
    assert _guarded_identity((path,)) == before


@pytest.mark.parametrize(
    ("expression", "selection", "replacement"),
    [
        ("[[], []]", "value[0] is value[1]", lambda item: [item, item]),
        ("([], [])", "value[0] is value[1]", lambda item: (item, item)),
        (
            "{'first': [], 'second': []}",
            "value['first'] is value['second']",
            lambda item: {"first": item, "second": item},
        ),
    ],
)
@pytest.mark.parametrize("constant", [False, True])
def test_nested_default_and_constant_owner_topology(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    expression: str,
    selection: str,
    replacement: Callable[[list[object]], object],
    *,
    constant: bool,
) -> None:
    path = tmp_path / "owned_nested_alias.py"
    source = (
        f"VALUE = {expression}\ndef same():\n    value = VALUE\n    return {selection}\n"
        if constant
        else f"def same(value={expression}):\n    return {selection}\n"
    )
    path.write_text(source, encoding="utf-8")
    held = path.read_bytes()
    module = _load(path, "owned_nested_alias", monkeypatch)
    code = module.same.__code__
    assert _guarded_identity((path,)) is not None
    assert module.same() is False
    changed = replacement([])
    if constant:
        monkeypatch.setattr(module, "VALUE", changed)
    else:
        monkeypatch.setattr(module.same, "__defaults__", (changed,))
    assert module.same() is True
    assert module.same.__code__ is code
    assert path.read_bytes() == held
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("constructor", [False, True])
def test_source_method_and_constructor_owner_topology(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, constructor: bool
) -> None:
    path = tmp_path / "owned_alias_method.py"
    name = "__init__" if constructor else "same"
    result = "self.result =" if constructor else "return"
    path.write_text(
        f"class Reader:\n    def {name}(self, first=[], *, second=[]):\n"
        f"        {result} first is second\n",
        encoding="utf-8",
    )
    held = path.read_bytes()
    module = _load(path, "owned_alias_method", monkeypatch)
    function = getattr(module.Reader, name)
    code = function.__code__
    assert _guarded_identity((path,)) is not None
    before = module.Reader().result if constructor else module.Reader().same()
    assert before is False
    shared = []
    _replace_defaults(function, "cross", shared, shared)
    after = module.Reader().result if constructor else module.Reader().same()
    assert after is True
    assert function.__code__ is code
    assert path.read_bytes() == held
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("keyword", [False, True])
@pytest.mark.parametrize("shared", [False, True])
def test_generated_constructor_owner_topology(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, keyword: bool, shared: bool
) -> None:
    path = tmp_path / "owned_alias_generated.py"
    value = "OWNER" if shared else "([],)"
    path.write_text(
        "from dataclasses import dataclass\nOWNER = ([],)\n"
        f"@dataclass(kw_only={keyword}, frozen=True, slots=True)\nclass Reader:\n"
        f"    first: tuple = {value}\n    second: tuple = {value}\n",
        encoding="utf-8",
    )
    source = path.read_bytes()
    module = _load(path, "owned_alias_generated", monkeypatch)
    function = module.Reader.__init__
    code = function.__code__
    before = _guarded_identity((path,))
    assert before is not None
    original = module.Reader()
    assert (original.first[0] is original.second[0]) is shared
    first, second = _owners("[]", shared=not shared)
    _replace_defaults(function, "keyword" if keyword else "positional", (first,), (second,))
    changed = module.Reader()
    assert (changed.first[0] is changed.second[0]) is not shared
    assert function.__code__ is code
    assert path.read_bytes() == source
    assert _guarded_identity((path,)) is None
    first, second = _owners("[]", shared=shared)
    _replace_defaults(function, "keyword" if keyword else "positional", (first,), (second,))
    assert _guarded_identity((path,)) == before


def test_retained_callable_default_owner_topology(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    provider = tmp_path / "owned_alias_provider.py"
    provider.write_text(
        "def same(first=[], second=[]):\n    return first is second\n", encoding="utf-8"
    )
    previous = _load(provider, "owned_alias_provider", monkeypatch)
    consumer = tmp_path / "owned_alias_consumer.py"
    consumer.write_text(
        "from owned_alias_provider import same\ndef consume(worker=same):\n    return worker()\n",
        encoding="utf-8",
    )
    retained = _load(consumer, "owned_alias_consumer", monkeypatch)
    source = provider.read_bytes(), consumer.read_bytes()
    code = previous.same.__code__
    assert retained.consume() is False
    assert _guarded_identity((provider, consumer)) is not None
    current = _load(provider, "owned_alias_provider", monkeypatch)
    retained.__dict__["same"] = current.same
    shared = []
    monkeypatch.setattr(previous.same, "__defaults__", (shared, shared))
    assert retained.consume() is True
    assert previous.same.__code__ is code
    assert (provider.read_bytes(), consumer.read_bytes()) == source
    assert _guarded_identity((provider,)) is not None
    assert _guarded_identity((provider, consumer)) is None


def test_slice_members_retain_mutable_owner_topology(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    provider = ModuleType("owned_alias_slice_provider")
    provider.parts = slice([], [])
    monkeypatch.setitem(sys.modules, provider.__name__, provider)
    path = tmp_path / "owned_alias_slice.py"
    path.write_text(
        "from owned_alias_slice_provider import parts\n"
        "def same(value=parts):\n    return value.start is value.stop\n",
        encoding="utf-8",
    )
    module = _load(path, "owned_alias_slice", monkeypatch)
    held, code = path.read_bytes(), module.same.__code__
    assert _guarded_identity((path,)) is not None
    assert module.same() is False
    shared = []
    monkeypatch.setattr(module.same, "__defaults__", (slice(shared, shared),))
    assert module.same() is True
    assert module.same.__code__ is code
    assert path.read_bytes() == held
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("kind", ["cycle", "depth", "nodes", "dict", "dag"])
def test_unbounded_owner_graph_is_unknown(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, kind: str
) -> None:
    path = tmp_path / "owned_alias_bounds.py"
    path.write_text("def transform(value=[]):\n    return value\n", encoding="utf-8")
    module = _load(path, "owned_alias_bounds", monkeypatch)
    assert _guarded_identity((path,)) is not None
    value = []
    if kind == "cycle":
        value.append(value)
    elif kind == "depth":
        for _ in range(65):
            value = [value]
    elif kind == "dag":
        for _ in range(12):
            value = [value, value]
    elif kind == "dict":
        value = dict.fromkeys(range(4097))
    else:
        value = [0] * 4097
    monkeypatch.setattr(module.transform, "__defaults__", (value,))
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("kind", ["cycle", "depth", "nodes", "dict", "dag"])
def test_generated_field_graph_is_bounded_before_recursive_value_encoding(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, kind: str
) -> None:
    provider = ModuleType("owned_alias_generated_provider")
    provider.value = ([],)
    monkeypatch.setitem(sys.modules, provider.__name__, provider)
    path = tmp_path / "owned_alias_generated_bounds.py"
    path.write_text(
        "from dataclasses import dataclass\n"
        "from owned_alias_generated_provider import value\n"
        "@dataclass(frozen=True)\nclass Reader:\n    field: tuple = value\n",
        encoding="utf-8",
    )
    _load(path, "owned_alias_generated_bounds", monkeypatch)
    assert _guarded_identity((path,)) is not None
    value = provider.value[0]
    if kind == "cycle":
        value.append(value)
    elif kind == "depth":
        for _ in range(65):
            value.append([])
            value = value[0]
    elif kind == "dag":
        subtree = []
        for _ in range(12):
            subtree = [subtree, subtree]
        value.append(subtree)
    elif kind == "dict":
        value.append(dict.fromkeys(range(4097)))
    else:
        value.extend([0] * 4097)

    # Isolate preflight ordering: neither recursive encoder may see this graph.
    def unexpected_value(_value: object) -> object:
        pytest.fail("unbounded generated field reached its recursive value encoder")

    monkeypatch.setattr(processing_code, "_value", unexpected_value)
    monkeypatch.setattr(processing_code, "_constructor_value", unexpected_value)
    assert _guarded_identity((path,)) is None
