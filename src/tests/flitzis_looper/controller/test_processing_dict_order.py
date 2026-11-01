"""Held source dictionary order remains observable in guarded execution."""

from typing import TYPE_CHECKING

import pytest

from tests.flitzis_looper.controller.test_processing_code import _guarded_identity, _load

if TYPE_CHECKING:
    from pathlib import Path


_VALUES = [
    ("{'left': 1, 'right': 2}", {"right": 2, "left": 1}, "value"),
    ("({'left': 1, 'right': 2},)", ({"right": 2, "left": 1},), "value[0]"),
    ("[{'left': 1, 'right': 2}]", [{"right": 2, "left": 1}], "value[0]"),
    (
        "{'nested': {'left': 1, 'right': 2}}",
        {"nested": {"right": 2, "left": 1}},
        "value['nested']",
    ),
]


@pytest.mark.parametrize("keyword", [False, True])
@pytest.mark.parametrize(("expression", "replacement", "selection"), _VALUES)
def test_source_defaults_preserve_dictionary_order(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    keyword: bool,
    expression: str,
    replacement: object,
    selection: str,
) -> None:
    path = tmp_path / "owned_dict_default.py"
    path.write_text(
        f"def first({'*, ' if keyword else ''}value={expression}):\n"
        f"    return next(iter({selection}))\n",
        encoding="utf-8",
    )
    source = path.read_bytes()
    module = _load(path, "owned_dict_default", monkeypatch)
    function = module.first
    code = function.__code__
    defaults, keywords = function.__defaults__, function.__kwdefaults__
    original = keywords["value"] if keyword else defaults[0]
    assert original == replacement
    before = _guarded_identity((path,))
    assert before is not None
    assert function() == "left"
    if keyword:
        monkeypatch.setattr(function, "__kwdefaults__", {"value": replacement})
    else:
        monkeypatch.setattr(function, "__defaults__", (replacement,))
    assert function() == "right"
    assert function.__code__ is code
    assert path.read_bytes() == source
    assert _guarded_identity((path,)) is None
    monkeypatch.setattr(function, "__defaults__", defaults)
    monkeypatch.setattr(function, "__kwdefaults__", keywords)
    assert _guarded_identity((path,)) == before


@pytest.mark.parametrize("constructor", [False, True])
@pytest.mark.parametrize("keyword", [False, True])
def test_source_methods_and_constructors_preserve_dictionary_order(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, constructor: bool, keyword: bool
) -> None:
    path = tmp_path / "owned_dict_method.py"
    name = "__init__" if constructor else "first"
    result = "self.result =" if constructor else "return"
    path.write_text(
        f"class Reader:\n    def {name}(self, {'*, ' if keyword else ''}"
        "value={'left': 1, 'right': 2}):\n"
        f"        {result} next(iter(value))\n",
        encoding="utf-8",
    )
    source = path.read_bytes()
    module = _load(path, "owned_dict_method", monkeypatch)
    function = getattr(module.Reader, name)
    code = function.__code__
    before = module.Reader().result if constructor else module.Reader().first()
    assert before == "left"
    assert _guarded_identity((path,)) is not None
    replacement = {"right": 2, "left": 1}
    if keyword:
        monkeypatch.setattr(function, "__kwdefaults__", {"value": replacement})
    else:
        monkeypatch.setattr(function, "__defaults__", (replacement,))
    after = module.Reader().result if constructor else module.Reader().first()
    assert after == "right"
    assert function.__code__ is code
    assert path.read_bytes() == source
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize(("expression", "replacement", "selection"), _VALUES)
def test_source_constants_preserve_nested_dictionary_order(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    expression: str,
    replacement: object,
    selection: str,
) -> None:
    path = tmp_path / "owned_dict_constant.py"
    path.write_text(
        f"VALUE = {expression}\ndef first():\n"
        f"    value = VALUE\n    return next(iter({selection}))\n",
        encoding="utf-8",
    )
    source = path.read_bytes()
    module = _load(path, "owned_dict_constant", monkeypatch)
    code = module.first.__code__
    assert replacement == module.VALUE
    assert module.first() == "left"
    assert _guarded_identity((path,)) is not None
    monkeypatch.setattr(module, "VALUE", replacement)
    assert module.first() == "right"
    assert module.first.__code__ is code
    assert path.read_bytes() == source
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("keyword", [False, True])
def test_retained_callable_defaults_preserve_inner_dictionary_order(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, keyword: bool
) -> None:
    provider = tmp_path / "owned_dict_provider.py"
    provider.write_text(
        "def first(value={'left': 1, 'right': 2}):\n    return next(iter(value))\n",
        encoding="utf-8",
    )
    previous = _load(provider, "owned_dict_provider", monkeypatch)
    consumer = tmp_path / "owned_dict_consumer.py"
    consumer.write_text(
        "from owned_dict_provider import first\n"
        f"def consume({'*, ' if keyword else ''}worker=first):\n    return worker()\n",
        encoding="utf-8",
    )
    retained = _load(consumer, "owned_dict_consumer", monkeypatch)
    source = provider.read_bytes(), consumer.read_bytes()
    code = previous.first.__code__
    assert retained.consume() == "left"
    assert _guarded_identity((provider, consumer)) is not None
    current = _load(provider, "owned_dict_provider", monkeypatch)
    retained.__dict__["first"] = current.first
    monkeypatch.setattr(previous.first, "__defaults__", ({"right": 2, "left": 1},))
    assert retained.consume() == "right"
    assert previous.first.__code__ is code
    assert (provider.read_bytes(), consumer.read_bytes()) == source
    assert _guarded_identity((provider,)) is not None
    assert _guarded_identity((provider, consumer)) is None


@pytest.mark.parametrize("keyword", [False, True])
def test_generated_constructor_defaults_preserve_nested_dictionary_order(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, keyword: bool
) -> None:
    path = tmp_path / "owned_dict_generated.py"
    path.write_text(
        "from dataclasses import dataclass\n"
        f"@dataclass(kw_only={keyword}, frozen=True, slots=True)\nclass Reader:\n"
        "    value: tuple = ({'left': 1, 'right': 2},)\n",
        encoding="utf-8",
    )
    source = path.read_bytes()
    module = _load(path, "owned_dict_generated", monkeypatch)
    function = module.Reader.__init__
    code = function.__code__
    before = _guarded_identity((path,))
    assert before is not None
    assert next(iter(module.Reader().value[0])) == "left"
    replacement = ({"right": 2, "left": 1},)
    if keyword:
        monkeypatch.setattr(function, "__kwdefaults__", {"value": replacement})
    else:
        monkeypatch.setattr(function, "__defaults__", (replacement,))
    assert next(iter(module.Reader().value[0])) == "right"
    assert function.__code__ is code
    assert path.read_bytes() == source
    assert _guarded_identity((path,)) is None


@pytest.mark.parametrize("keyword", [False, True])
def test_equal_order_replacement_default_retains_source_identity(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, keyword: bool
) -> None:
    path = tmp_path / "owned_equal_dict.py"
    path.write_text(
        f"def first({'*, ' if keyword else ''}value={{'right': 2, 'left': 1}}):\n"
        "    return next(iter(value))\n",
        encoding="utf-8",
    )
    module = _load(path, "owned_equal_dict", monkeypatch)
    before = _guarded_identity((path,))
    assert before is not None
    replacement = {"right": 2, "left": 1}
    if keyword:
        monkeypatch.setattr(module.first, "__kwdefaults__", {"value": replacement})
    else:
        monkeypatch.setattr(module.first, "__defaults__", (replacement,))
    assert module.first() == "right"
    assert _guarded_identity((path,)) == before
