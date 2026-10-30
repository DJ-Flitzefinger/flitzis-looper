"""Bounded Python execution policy for the guarded offline transform seam.

Fresh child imports use an exclusively born, empty private cache namespace.
Parent checks compile held host source without executing or importing modules.
External libraries and OS images retain their separate version/image policy.
"""

import ast
import builtins
import ctypes
import hashlib
import inspect
import json
import operator
import os
import subprocess
import sys
import tempfile
from contextlib import contextmanager
from contextvars import Context, ContextVar
from ctypes import wintypes
from dataclasses import MISSING, dataclass, fields, is_dataclass
from pathlib import Path
from threading import Semaphore
from types import CodeType, FunctionType, ModuleType
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator


_HOST_TARGETS = {
    "contracts.py": ((), ("WorkerLimits",)),
    # Explicit acquisition is setup-only; inference executes the pinned verifier.
    "bs_roformer_assets.py": (("verify_model_assets",), ()),
    "encoder_provenance.py": (
        (
            "supported_encoder_images",
            "encoder_contract",
            "_file_witness",
            "verified_encoder_output",
            "_read_encoder_receipt",
        ),
        (),
    ),
    "bs_input_provenance.py": (
        ("bs_input_contract", "verified_input_output", "_read_input_receipt"),
        (),
    ),
}
_HOST_CONSTANT_TARGETS = {"contracts.py": ()}


@contextmanager
def fresh_python_source_environment(
    env: dict[str, str], attempt: Path, *, source_roots: tuple[Path, ...] = ()
) -> Iterator[dict[str, str]]:
    """Override inherited caches with this attempt's newly born private empty namespace.

    This is an owned-attempt namespace policy, not protection against arbitrary
    hostile writers. No existing namespace is adopted and imports write no pyc.
    The identity and emptiness checks also detect ordinary accidental reuse.
    """
    with tempfile.TemporaryDirectory(prefix=".python-source-", dir=attempt) as name:
        cache = Path(name)
        before = cache.stat()
        if any(cache.iterdir()):
            msg = "new Python execution cache must be empty"
            raise RuntimeError(msg)
        child = env | {"PYTHONPYCACHEPREFIX": str(cache), "PYTHONDONTWRITEBYTECODE": "1"}
        if source_roots:
            roots = tuple(str(root.resolve(strict=True)) for root in source_roots)
            if any(not Path(root).is_dir() for root in roots):
                msg = "guarded Python source root is unavailable"
                raise ValueError(msg)
            child["PYTHONPATH"] = os.pathsep.join((*roots, env.get("PYTHONPATH", "")))
            child["PYTHONSAFEPATH"] = "1"
        yield child
        after = cache.stat()
        if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino) or any(cache.iterdir()):
            msg = "private Python execution cache changed during the producer attempt"
            raise RuntimeError(msg)


def loaded_code_identity(paths: tuple[Path, ...]) -> str | None:
    """Compare already loaded guarded host code/defaults/constants with held source.

    Only explicitly passed host transform files are considered. Unsupported
    executable bindings return UNKNOWN rather than claiming source execution.
    """
    if not paths or len(set(paths)) != len(paths):
        return None
    try:
        manifests = {path.resolve(strict=True): _manifest(path) for path in paths}
        evidence: list[object] = []
        for index, path in enumerate(paths):
            source = path.resolve(strict=True)
            modules = _loaded_at(source)
            if not modules:
                return None
            evidence.extend(
                (
                    index,
                    _check_namespace(module.__dict__, manifests[source], manifests),
                )
                for module in modules
            )
        encoded = json.dumps(evidence, sort_keys=True, separators=(",", ":")).encode()
        return hashlib.sha256(encoded).hexdigest()
    except (
        ArithmeticError,
        AttributeError,
        KeyError,
        OSError,
        RuntimeError,
        SyntaxError,
        TypeError,
        ValueError,
    ):
        return None


def _loaded_at(path: Path) -> list[ModuleType]:
    modules = []
    for module in tuple(sys.modules.values()):
        if not isinstance(module, ModuleType):
            continue
        filename = module.__dict__.get("__file__")
        if isinstance(filename, str) and Path(filename).resolve() == path:
            modules.append(module)
    return modules


def _manifest(path: Path) -> tuple[ast.Module, dict[str, CodeType]]:
    source = path.read_bytes()
    tree = ast.parse(source, filename=str(path))
    code = compile(tree, str(path), "exec", dont_inherit=True, optimize=sys.flags.optimize)
    index: dict[str, CodeType] = {}
    pending = [code]
    while pending:
        actual = pending.pop()
        index[actual.co_qualname] = actual
        pending.extend(item for item in actual.co_consts if isinstance(item, CodeType))
        if len(index) > 4096:
            msg = "host transform code exceeds bounded declaration capacity"
            raise ValueError(msg)
    return tree, index


def _check_namespace(
    namespace: dict[str, object],
    manifest: tuple[ast.Module, dict[str, CodeType]],
    manifests: dict[Path, tuple[ast.Module, dict[str, CodeType]]],
) -> list[object]:
    tree, codes = manifest
    evidence: list[object] = []
    targets = _HOST_TARGETS.get(Path(str(namespace["__file__"])).name)
    for statement in tree.body:
        if isinstance(statement, ast.FunctionDef | ast.AsyncFunctionDef) and (
            targets is None or statement.name in targets[0]
        ):
            evidence.append(
                _check_function(
                    namespace[statement.name],
                    statement,
                    statement.name,
                    codes,
                    namespace,
                    manifests,
                )
            )
        elif isinstance(statement, ast.ClassDef) and (
            targets is None or statement.name in targets[1]
        ):
            evidence.append(
                _check_class(namespace[statement.name], statement, codes, namespace, manifests)
            )
    evidence.append(_check_constants(tree.body, namespace))
    return evidence


def _check_constants(body: list[ast.stmt], namespace: dict[str, object]) -> dict[str, object]:
    evidence: dict[str, object] = {}
    selected = _HOST_CONSTANT_TARGETS.get(Path(str(namespace["__file__"])).name)
    for statement in body:
        binding = _assignment(statement)
        if binding is None:
            continue
        name, expression = binding
        if name.lstrip("_").isupper() and (selected is None or name in selected):
            runtime = _runtime_constant(name, expression, namespace)
            if runtime is not None:
                evidence[name] = runtime
                continue
            expected = _expression(expression, namespace)
            actual = namespace[name]
            if _value(actual) != _value(expected):
                msg = "loaded host transform constant differs from held source"
                raise ValueError(msg)
            evidence[name] = _value(actual)
    return evidence


def _runtime_constant(
    name: str, expression: ast.expr, namespace: dict[str, object]
) -> tuple[object, ...] | None:
    """Check exact declared carriers without measuring mutable cancellation/admission state."""
    role = Path(str(namespace["__file__"])).name, name
    if role not in {("process.py", "_PROCESS_SLOT"), ("stem_process.py", "_COMMAND_SCOPE")}:
        return None
    if not isinstance(expression, ast.Call) or not isinstance(expression.func, ast.Name):
        msg = "runtime carrier differs from its supported constructor declaration"
        raise TypeError(msg)
    actual = namespace[name]
    arguments = tuple(_expression(item, namespace) for item in expression.args)
    keywords = {item.arg: _expression(item.value, namespace) for item in expression.keywords}
    if role == ("process.py", "_PROCESS_SLOT"):
        if (
            expression.func.id != "Semaphore"
            or namespace.get("Semaphore") is not Semaphore
            or arguments != (1,)
            or keywords
            or type(actual) is not Semaphore
        ):
            msg = "analysis admission slot differs from the supported stdlib binding"
            raise ValueError(msg)
        if (
            getattr(actual.acquire, "__func__", None) is not Semaphore.acquire
            or getattr(actual.release, "__func__", None) is not Semaphore.release
        ):
            msg = "analysis admission slot methods differ from the supported stdlib binding"
            raise ValueError(msg)
        return "runtime-carrier", "threading.Semaphore", 1
    if (
        expression.func.id != "ContextVar"
        or namespace.get("ContextVar") is not ContextVar
        or arguments != ("stem-command-scope",)
        or keywords != {"default": None}
        or type(actual) is not ContextVar
    ):
        msg = "stem cancellation context differs from the supported stdlib binding"
        raise ValueError(msg)
    if actual.name != arguments[0] or Context().run(actual.get) is not None:
        msg = "stem cancellation context name/default differs from its declaration"
        raise ValueError(msg)
    return "runtime-carrier", "contextvars.ContextVar", arguments[0], None


def _assignment(statement: ast.stmt) -> tuple[str, ast.expr] | None:
    if isinstance(statement, ast.Assign) and len(statement.targets) == 1:
        target = statement.targets[0]
        if isinstance(target, ast.Name):
            return target.id, statement.value
    if (
        isinstance(statement, ast.AnnAssign)
        and isinstance(statement.target, ast.Name)
        and statement.value is not None
    ):
        return statement.target.id, statement.value
    return None


def _check_class(
    value: object,
    declaration: ast.ClassDef,
    codes: dict[str, CodeType],
    namespace: dict[str, object],
    manifests: dict[Path, tuple[ast.Module, dict[str, CodeType]]],
) -> list[object]:
    value = _class_binding(value, declaration, namespace)
    evidence: list[object] = [declaration.name]
    evidence.extend(
        _class_method(value, member, declaration.name, codes, namespace, manifests)
        for member in declaration.body
        if isinstance(member, ast.FunctionDef | ast.AsyncFunctionDef)
    )
    if is_dataclass(value):
        evidence.append(_dataclass_policy(value, declaration, namespace))
        evidence.extend(_dataclass_fields(value, declaration, namespace))
    if issubclass(value, ctypes.Structure):
        evidence.append(_ctypes_layout(value, declaration, namespace))
    return evidence


def _class_binding(value: object, declaration: ast.ClassDef, namespace: dict[str, object]) -> type:
    if not isinstance(value, type):
        msg = "guarded host class binding is unavailable"
        raise TypeError(msg)
    if value.__qualname__ != declaration.name:
        msg = "loaded host class does not match the declared binding"
        raise ValueError(msg)
    module = sys.modules.get(value.__module__)
    if (
        not isinstance(module, ModuleType)
        or Path(str(module.__dict__.get("__file__"))).resolve()
        != Path(str(namespace["__file__"])).resolve()
    ):
        msg = "loaded host class origin differs from the declared source"
        raise ValueError(msg)
    if declaration.decorator_list and (
        not is_dataclass(value)
        or any(_decorator_name(item) != "dataclass" for item in declaration.decorator_list)
    ):
        msg = "host class has unsupported decorator policy"
        raise ValueError(msg)
    expected_bases = tuple(_class_base(base, namespace) for base in declaration.bases) or (object,)
    if value.__bases__ != expected_bases:
        msg = "loaded host class bases differ from held source"
        raise ValueError(msg)
    return value


def _class_base(expression: ast.expr, namespace: dict[str, object]) -> object:
    if isinstance(expression, ast.Subscript):
        if not isinstance(expression.value, ast.Attribute) or not isinstance(
            expression.value.value, ast.Name
        ):
            msg = "host generic base requires a supported stdlib attribute declaration"
            raise TypeError(msg)
        if (
            expression.value.value.id != "subprocess"
            or expression.value.attr != "Popen"
            or namespace.get("subprocess") is not subprocess
            or _expression(expression.slice, namespace) is not bytes
        ):
            msg = "host generic base differs from supported stdlib Popen[bytes]"
            raise ValueError(msg)
        return subprocess.Popen
    return _expression(expression, namespace)


def _ctypes_layout(
    value: type, declaration: ast.ClassDef, namespace: dict[str, object]
) -> list[object]:
    declarations = [
        binding[1]
        for member in declaration.body
        if (binding := _assignment(member)) is not None and binding[0] == "_fields_"
    ]
    if len(declarations) != 1 or any(
        inspect.getattr_static(value, name, None) is not None
        for name in ("_pack_", "_align_", "_anonymous_")
    ):
        msg = "host ctypes layout requires unsupported packing or declarations"
        raise ValueError(msg)
    expected = _ctypes_fields(declarations[0], namespace)
    if tuple(inspect.getattr_static(value, "_fields_")) != expected:
        msg = "loaded host ctypes fields differ from held source"
        raise ValueError(msg)
    held = type("_HeldWindowsLayout", (ctypes.Structure,), {"_fields_": expected})
    evidence: list[object] = [ctypes.sizeof(value), ctypes.alignment(value)]
    if evidence != [ctypes.sizeof(held), ctypes.alignment(held)]:
        msg = "loaded host ctypes layout size differs from held declarations"
        raise ValueError(msg)
    for name, field_type in expected:
        actual = inspect.getattr_static(value, name)
        declared = inspect.getattr_static(held, name)
        if (actual.offset, actual.size) != (declared.offset, declared.size):
            msg = "loaded host ctypes descriptor differs from held declarations"
            raise ValueError(msg)
        evidence.append((name, _ctypes_type_identity(field_type), actual.offset, actual.size))
    return evidence


def _ctypes_fields(
    expression: ast.expr, namespace: dict[str, object]
) -> tuple[tuple[str, type], ...]:
    if isinstance(expression, ast.Tuple):
        entries = [(item, namespace) for item in expression.elts]
    elif (
        isinstance(expression, ast.Call)
        and isinstance(expression.func, ast.Name)
        and expression.func.id == "tuple"
    ):
        if (
            namespace.get("tuple", tuple) is not tuple
            or expression.keywords
            or len(expression.args) != 1
            or not isinstance(expression.args[0], ast.GeneratorExp)
        ):
            msg = "host ctypes fields require one supported stdlib literal comprehension"
            raise ValueError(msg)
        entries = _ctypes_field_generator(expression.args[0], namespace)
    else:
        msg = "host ctypes fields require supported direct tuple declarations"
        raise ValueError(msg)
    fields = []
    for item, scope in entries:
        if not isinstance(item, ast.Tuple) or len(item.elts) != 2:
            msg = "host ctypes field requires one literal name and one measured type"
            raise ValueError(msg)
        name = _expression(item.elts[0], scope)
        if not isinstance(name, str):
            msg = "host ctypes field name must be a literal string"
            raise TypeError(msg)
        fields.append((name, _ctypes_field_type(item.elts[1], scope)))
    if not 1 <= len(fields) <= 64 or len({name for name, _kind in fields}) != len(fields):
        msg = "host ctypes fields exceed the supported distinct layout bound"
        raise ValueError(msg)
    return tuple(fields)


def _ctypes_field_generator(
    expression: ast.GeneratorExp, namespace: dict[str, object]
) -> list[tuple[ast.expr, dict[str, object]]]:
    if len(expression.generators) != 1:
        msg = "host ctypes fields require one bounded literal comprehension"
        raise ValueError(msg)
    generator = expression.generators[0]
    values = _expression(generator.iter, namespace)
    if (
        not isinstance(generator.target, ast.Name)
        or generator.ifs
        or generator.is_async
        or not isinstance(values, tuple)
    ):
        msg = "host ctypes fields require bounded literal names"
        raise ValueError(msg)
    if not 1 <= len(values) <= 64 or any(not isinstance(value, str) for value in values):
        msg = "host ctypes fields require bounded literal names"
        raise ValueError(msg)
    return [(expression.elt, namespace | {generator.target.id: value}) for value in values]


def _ctypes_field_type(expression: ast.expr, namespace: dict[str, object]) -> type:
    if isinstance(expression, ast.BinOp) and isinstance(expression.op, ast.Mult):
        count = _expression(expression.right, namespace)
        if type(count) is not int or not 1 <= count <= 64:
            msg = "host ctypes array exceeds the supported process membership bound"
            raise ValueError(msg)
        kind = cast("type[ctypes.Structure]", _ctypes_field_type(expression.left, namespace))
        return kind * count
    value = _expression(expression, namespace)
    if not isinstance(value, type) or not (
        issubclass(value, ctypes.Structure) or isinstance(getattr(value, "_type_", None), str)
    ):
        msg = "host ctypes field requires a measured structure or stdlib scalar type"
        raise TypeError(msg)
    return value


def _ctypes_type_identity(value: type) -> object:
    if issubclass(value, ctypes.Array):
        return (
            "array",
            _ctypes_type_identity(inspect.getattr_static(value, "_type_")),
            inspect.getattr_static(value, "_length_"),
        )
    measured = cast("type[ctypes.Structure]", value)
    return value.__qualname__, ctypes.sizeof(measured), ctypes.alignment(measured)


def _class_method(
    value: type,
    member: ast.FunctionDef | ast.AsyncFunctionDef,
    class_name: str,
    codes: dict[str, CodeType],
    namespace: dict[str, object],
    manifests: dict[Path, tuple[ast.Module, dict[str, CodeType]]],
) -> list[object]:
    actual = inspect.getattr_static(value, member.name)
    decorators = [_decorator_name(item) for item in member.decorator_list]
    expected_kind = (
        property
        if "property" in decorators
        else staticmethod
        if "staticmethod" in decorators
        else classmethod
        if "classmethod" in decorators
        else FunctionType
    )
    if not isinstance(actual, expected_kind):
        msg = "loaded host method decorator differs from held source"
        raise TypeError(msg)
    if isinstance(actual, property):
        actual = actual.fget
    elif isinstance(actual, staticmethod | classmethod):
        actual = actual.__func__
    return _check_function(
        actual, member, class_name + "." + member.name, codes, namespace, manifests
    )


def _dataclass_fields(
    value: type, declaration: ast.ClassDef, namespace: dict[str, object]
) -> list[object]:
    evidence: list[object] = []
    declared = [
        (member.target.id, member)
        for member in declaration.body
        if isinstance(member, ast.AnnAssign) and isinstance(member.target, ast.Name)
    ]
    actual_fields = fields(value)
    if [field.name for field in actual_fields] != [name for name, _member in declared]:
        msg = "loaded host dataclass fields differ from held source"
        raise ValueError(msg)
    for field, (_name, member) in zip(actual_fields, declared, strict=True):
        expected = MISSING if member.value is None else _expression(member.value, namespace)
        if field.default is not expected and _value(field.default) != _value(expected):
            msg = "loaded host dataclass default differs from held source"
            raise ValueError(msg)
        evidence.append((field.name, "required" if expected is MISSING else _value(expected)))
    return evidence


def _dataclass_policy(
    value: type, declaration: ast.ClassDef, namespace: dict[str, object]
) -> dict[str, object]:
    decorators = [
        item for item in declaration.decorator_list if _decorator_name(item) == "dataclass"
    ]
    if len(decorators) != 1 or namespace.get("dataclass") is not dataclass:
        msg = "host dataclass has unsupported decorator lineage"
        raise ValueError(msg)
    decorator = decorators[0]
    options = (
        {keyword.arg: _expression(keyword.value, namespace) for keyword in decorator.keywords}
        if isinstance(decorator, ast.Call)
        else {}
    )
    supported = {"init", "repr", "eq", "order", "unsafe_hash", "frozen", "slots", "kw_only"}
    if any(name not in supported for name in options) or (
        isinstance(decorator, ast.Call) and decorator.args
    ):
        msg = "host dataclass requires unsupported generation options"
        raise ValueError(msg)
    params = inspect.getattr_static(value, "__dataclass_params__")
    evidence = {}
    for name, default in (
        ("init", True),
        ("repr", True),
        ("eq", True),
        ("order", False),
        ("unsafe_hash", False),
        ("frozen", False),
    ):
        expected = options.get(name, default)
        if getattr(params, name) != expected:
            msg = "loaded host dataclass generation options differ from held source"
            raise ValueError(msg)
        evidence[name] = expected
    slots = "__slots__" in vars(value)
    if slots != options.get("slots", False):
        msg = "loaded host dataclass slots differ from held source"
        raise ValueError(msg)
    evidence["slots"] = slots
    for field in fields(value):
        if field.kw_only != options.get("kw_only", False) or field.default_factory is not MISSING:
            msg = "host dataclass field requires unsupported generation policy"
            raise ValueError(msg)
    return evidence


def _check_function(
    value: object,
    declaration: ast.FunctionDef | ast.AsyncFunctionDef,
    qualname: str,
    codes: dict[str, CodeType],
    namespace: dict[str, object],
    manifests: dict[Path, tuple[ast.Module, dict[str, CodeType]]],
) -> list[object]:
    if not isinstance(value, FunctionType):
        msg = "guarded host callable binding is unavailable"
        raise TypeError(msg)
    _function_decorators(value, declaration, namespace)
    actual = inspect.unwrap(value)
    if not isinstance(actual, FunctionType):
        msg = "guarded host function binding is unavailable"
        raise TypeError(msg)
    if actual.__qualname__ != qualname:
        msg = "loaded host callable does not match the declared binding"
        raise ValueError(msg)
    evidence = _function_body(actual, declaration, codes, namespace, manifests)
    evidence.append(_imported_callables(actual, manifests))
    return evidence


def _function_body(
    actual: FunctionType,
    declaration: ast.FunctionDef | ast.AsyncFunctionDef,
    codes: dict[str, CodeType],
    namespace: dict[str, object],
    manifests: dict[Path, tuple[ast.Module, dict[str, CodeType]]],
    *,
    depth: int = 0,
) -> list[object]:
    if depth > 8:
        msg = "guarded callable defaults exceed bounded source policy"
        raise ValueError(msg)
    qualname = actual.__qualname__
    expected = codes[qualname]
    if _code(actual.__code__) != _code(expected):
        msg = "loaded host function differs from held source"
        raise ValueError(msg)
    if Path(actual.__code__.co_filename).resolve() != Path(expected.co_filename).resolve():
        msg = "loaded host function origin differs from the declared source"
        raise ValueError(msg)
    defaults = tuple(_expression(item, namespace) for item in declaration.args.defaults)
    if not _same_bindings(actual.__defaults__ or (), defaults, manifests, depth=depth):
        msg = "loaded host function defaults differ from held source"
        raise ValueError(msg)
    expected_keywords = {
        argument.arg: _expression(default, namespace)
        for argument, default in zip(
            declaration.args.kwonlyargs, declaration.args.kw_defaults, strict=True
        )
        if default is not None
    }
    if not _same_keyword_bindings(
        actual.__kwdefaults__ or {}, expected_keywords, manifests, depth=depth
    ):
        msg = "loaded host keyword defaults differ from held source"
        raise ValueError(msg)
    if actual.__closure__ and not (
        actual.__code__.co_freevars == ("__class__",)
        and len(actual.__closure__) == 1
        and actual.__closure__[0].cell_contents is namespace.get(qualname.split(".")[0])
    ):
        msg = "guarded host function has unsupported captured runtime cells"
        raise ValueError(msg)
    constants = _check_constants(
        manifests[Path(actual.__code__.co_filename).resolve()][0].body, actual.__globals__
    )
    return [
        actual.__qualname__,
        _code(actual.__code__),
        [_binding(item, manifests, depth=depth) for item in defaults],
        {key: _binding(item, manifests, depth=depth) for key, item in expected_keywords.items()},
        constants,
    ]


def _function_decorators(
    value: FunctionType,
    declaration: ast.FunctionDef | ast.AsyncFunctionDef,
    namespace: dict[str, object],
) -> None:
    decorators = [_decorator_name(item) for item in declaration.decorator_list]
    if any(
        name not in {"contextmanager", "property", "staticmethod", "classmethod"}
        for name in decorators
    ):
        msg = "host callable has unsupported decorator policy"
        raise ValueError(msg)
    if "contextmanager" in decorators:
        supported = contextmanager(_stdlib_context_body)
        if not isinstance(supported, FunctionType):
            msg = "supported stdlib contextmanager wrapper is unavailable"
            raise TypeError(msg)
        if (
            decorators != ["contextmanager"]
            or namespace.get("contextmanager") is not contextmanager
        ):
            msg = "host contextmanager has unsupported decorator binding"
            raise ValueError(msg)
        if (
            _code(value.__code__) != _code(supported.__code__)
            or value.__code__.co_filename != supported.__code__.co_filename
            or not value.__closure__
            or len(value.__closure__) != 1
            or value.__closure__[0].cell_contents is not vars(value).get("__wrapped__")
        ):
            msg = "host contextmanager wrapper differs from the supported stdlib policy"
            raise ValueError(msg)
    elif inspect.unwrap(value) is not value:
        msg = "host callable has an unexpected runtime wrapper"
        raise ValueError(msg)


def _stdlib_context_body() -> Iterator[None]:
    yield None


def _decorator_name(value: ast.expr) -> str:
    target = value.func if isinstance(value, ast.Call) else value
    if isinstance(target, ast.Name):
        return target.id
    msg = "host decorator requires unmeasured execution"
    raise ValueError(msg)


def _same_bindings(
    actual: tuple[object, ...],
    expected: tuple[object, ...],
    manifests: dict[Path, tuple[ast.Module, dict[str, CodeType]]],
    *,
    depth: int = 0,
) -> bool:
    return len(actual) == len(expected) and all(
        _binding(first, manifests, depth=depth) == _binding(second, manifests, depth=depth)
        for first, second in zip(actual, expected, strict=True)
    )


def _same_keyword_bindings(
    actual: dict[str, object],
    expected: dict[str, object],
    manifests: dict[Path, tuple[ast.Module, dict[str, CodeType]]],
    *,
    depth: int = 0,
) -> bool:
    return actual.keys() == expected.keys() and all(
        _binding(actual[name], manifests, depth=depth) == _binding(value, manifests, depth=depth)
        for name, value in expected.items()
    )


def _imported_callables(
    function: FunctionType, manifests: dict[Path, tuple[ast.Module, dict[str, CodeType]]]
) -> list[object]:
    evidence: list[object] = []
    guarded_imports = _guarded_import_names(function, manifests)
    for name in function.__code__.co_names:
        value = function.__globals__.get(name)
        if name in guarded_imports and not isinstance(value, FunctionType | type):
            msg = "invoked guarded host binding no longer names its measured callable"
            raise ValueError(msg)
        if isinstance(value, FunctionType):
            actual = inspect.unwrap(value)
            origin = Path(actual.__code__.co_filename).resolve()
            if origin in manifests:
                evidence.append((name, _guarded_function(value, manifests)))
            elif value.__module__.split(".", 1)[0] not in sys.stdlib_module_names:
                msg = "invoked host callable is outside the guarded producer roles"
                raise ValueError(msg)
        elif isinstance(value, type):
            module = sys.modules.get(value.__module__)
            filename = module.__dict__.get("__file__") if isinstance(module, ModuleType) else None
            if isinstance(filename, str) and (origin := Path(filename).resolve()) in manifests:
                classes = [
                    item
                    for item in manifests[origin][0].body
                    if isinstance(item, ast.ClassDef) and item.name == value.__qualname__
                ]
                if len(classes) != 1:
                    msg = "imported guarded host class declaration is unavailable"
                    raise ValueError(msg)
                evidence.append((
                    name,
                    _check_class(
                        value, classes[0], manifests[origin][1], module.__dict__, manifests
                    ),
                ))
            elif name in guarded_imports or value.__module__.startswith("flitzis_looper."):
                msg = "invoked host class is outside the guarded producer roles"
                raise ValueError(msg)
    return evidence


def _guarded_import_names(
    function: FunctionType, manifests: dict[Path, tuple[ast.Module, dict[str, CodeType]]]
) -> set[str]:
    """Fence class/callable imports whose actual defining module is a held host role."""
    tree = manifests[Path(function.__code__.co_filename).resolve()][0]
    guarded = set()
    for statement in tree.body:
        if not isinstance(statement, ast.ImportFrom) or statement.module is None:
            continue
        module = sys.modules.get(statement.module)
        filename = module.__dict__.get("__file__") if isinstance(module, ModuleType) else None
        if not isinstance(filename, str) or Path(filename).resolve() not in manifests:
            continue
        for alias in statement.names:
            if isinstance(module.__dict__.get(alias.name), FunctionType | type):
                guarded.add(alias.asname or alias.name)
    return guarded


def _function_declaration(tree: ast.Module, name: str) -> ast.FunctionDef | ast.AsyncFunctionDef:
    body = tree.body
    for component in name.split("."):
        candidates = [
            item
            for item in body
            if isinstance(item, ast.ClassDef | ast.FunctionDef | ast.AsyncFunctionDef)
            and item.name == component
        ]
        if len(candidates) != 1:
            msg = "imported host callable has no direct current source declaration"
            raise ValueError(msg)
        declaration = candidates[0]
        body = declaration.body
    if not isinstance(declaration, ast.FunctionDef | ast.AsyncFunctionDef):
        msg = "host callable source declaration is unavailable"
        raise TypeError(msg)
    return declaration


def _binding(
    value: object,
    manifests: dict[Path, tuple[ast.Module, dict[str, CodeType]]],
    *,
    depth: int = 0,
) -> object:
    if isinstance(value, FunctionType):
        return ("function", _guarded_function(value, manifests, depth=depth + 1))
    return _value(value)


def _guarded_function(
    value: FunctionType,
    manifests: dict[Path, tuple[ast.Module, dict[str, CodeType]]],
    *,
    depth: int = 0,
) -> list[object]:
    actual = inspect.unwrap(value)
    if not isinstance(actual, FunctionType):
        msg = "guarded callable wrapper has no supported function body"
        raise TypeError(msg)
    origin = Path(actual.__code__.co_filename).resolve()
    if origin not in manifests:
        msg = "unguarded callable default has no current source proof"
        raise ValueError(msg)
    tree, codes = manifests[origin]
    declaration = _function_declaration(tree, actual.__qualname__)
    _function_decorators(value, declaration, actual.__globals__)
    return _function_body(actual, declaration, codes, actual.__globals__, manifests, depth=depth)


def _expression(value: ast.expr, namespace: dict[str, object]) -> object:
    if isinstance(value, ast.Constant):
        return value.value
    if isinstance(value, ast.Name):
        return namespace[value.id] if value.id in namespace else vars(builtins)[value.id]
    if isinstance(value, ast.Tuple | ast.List | ast.Set | ast.Dict):
        return _literal_collection(value, namespace)
    if isinstance(value, ast.Attribute):
        return _ctypes_attribute(value, namespace)
    return _literal_operation(value, namespace)


def _ctypes_attribute(expression: ast.Attribute, namespace: dict[str, object]) -> object:
    if isinstance(expression.value, ast.Name):
        module = namespace.get(expression.value.id)
        supported = (
            {"Structure", "c_int64", "c_uint64", "c_size_t"}
            if module is ctypes
            else {"DWORD", "LONG"}
            if module is wintypes
            else set()
        )
        if expression.attr in supported:
            return getattr(module, expression.attr)
    if (
        expression.attr == "value"
        and isinstance(expression.value, ast.Call)
        and isinstance(expression.value.func, ast.Attribute)
        and isinstance(expression.value.func.value, ast.Name)
    ):
        valid_module = namespace.get(expression.value.func.value.id) is ctypes
        valid_constructor = expression.value.func.attr == "c_void_p"
        valid_arguments = len(expression.value.args) == 1 and not expression.value.keywords
        if not (valid_module and valid_constructor and valid_arguments):
            msg = "host ctypes invalid-handle constructor differs from held declarations"
            raise ValueError(msg)
        if _expression(expression.value.args[0], namespace) == -1:
            return ctypes.c_void_p(-1).value
    msg = "host attribute requires an unsupported runtime binding"
    raise ValueError(msg)


def _literal_collection(
    value: ast.Tuple | ast.List | ast.Set | ast.Dict, namespace: dict[str, object]
) -> object:
    if isinstance(value, ast.Dict):
        if any(key is None for key in value.keys):
            msg = "host literal unpacking requires unmeasured execution"
            raise ValueError(msg)
        return {
            _expression(key, namespace): _expression(item, namespace)
            for key, item in zip(value.keys, value.values, strict=True)
            if key is not None
        }
    values = [_expression(item, namespace) for item in value.elts]
    return (
        tuple(values)
        if isinstance(value, ast.Tuple)
        else set(values)
        if isinstance(value, ast.Set)
        else values
    )


def _literal_operation(value: ast.expr, namespace: dict[str, object]) -> object:
    binary = {
        ast.Add: operator.add,
        ast.Sub: operator.sub,
        ast.Mult: operator.mul,
        ast.FloorDiv: operator.floordiv,
        ast.LShift: operator.lshift,
        ast.RShift: operator.rshift,
        ast.BitOr: operator.or_,
    }
    if isinstance(value, ast.BinOp) and type(value.op) in binary:
        return _literal_binary(value, namespace, binary)
    if isinstance(value, ast.UnaryOp) and isinstance(value.op, ast.USub | ast.UAdd):
        actual = _expression(value.operand, namespace)
        if not isinstance(actual, int | float):
            msg = "unsupported host literal unary arithmetic"
            raise TypeError(msg)
        return -actual if isinstance(value.op, ast.USub) else actual
    if isinstance(value, ast.Call):
        return _literal_call(value, namespace)
    msg = "host default or constant requires unmeasured execution"
    raise ValueError(msg)


def _literal_binary(
    value: ast.BinOp,
    namespace: dict[str, object],
    operations: dict[type[ast.operator], Callable[[int, int], int]],
) -> int:
    left, right = _expression(value.left, namespace), _expression(value.right, namespace)
    if type(left) is not int or type(right) is not int:
        msg = "unsupported host literal arithmetic types"
        raise TypeError(msg)
    if abs(left) > 1 << 64 or abs(right) > 1 << 64:
        msg = "unsupported host literal arithmetic extent"
        raise ValueError(msg)
    if isinstance(value.op, ast.LShift | ast.RShift) and not 0 <= right <= 64:
        msg = "unsupported host literal shift"
        raise ValueError(msg)
    return operations[type(value.op)](left, right)


def _literal_call(value: ast.Call, namespace: dict[str, object]) -> bytes:
    if not isinstance(value.func, ast.Attribute) or not isinstance(value.func.value, ast.Name):
        msg = "host literal call has unsupported origin"
        raise TypeError(msg)
    if (
        value.func.attr == "encode"
        and len(value.args) == 1
        and not value.keywords
        and _expression(value.args[0], namespace) == "ascii"
    ):
        text = _expression(value.func.value, namespace)
        if not isinstance(text, str):
            msg = "host receipt prefix requires a literal string binding"
            raise TypeError(msg)
        return text.encode("ascii")
    if (
        value.func.value.id != "bytes"
        or value.func.attr != "fromhex"
        or len(value.args) != 1
        or value.keywords
    ):
        msg = "host literal call requires unmeasured execution"
        raise ValueError(msg)
    argument = _expression(value.args[0], namespace)
    if not isinstance(argument, str):
        msg = "host bytes literal requires a hexadecimal string"
        raise TypeError(msg)
    return builtins.bytes.fromhex(argument)


def _value(value: object) -> object:
    if value is None or isinstance(value, str | bool | int):
        return value
    if isinstance(value, float):
        return ("float", value.hex())
    if isinstance(value, bytes):
        return ("bytes", value.hex())
    if isinstance(value, slice):
        return ("slice", _value(value.start), _value(value.stop), _value(value.step))
    if isinstance(value, tuple | list | set | frozenset | dict):
        return _collection_value(value)
    if value is Ellipsis:
        return ("ellipsis",)
    msg = "unsupported mutable or executable host binding"
    raise ValueError(msg)


def _collection_value(
    value: tuple[object, ...]
    | list[object]
    | set[object]
    | frozenset[object]
    | dict[object, object],
) -> object:
    if isinstance(value, tuple | list):
        return (type(value).__name__, [_value(item) for item in value])
    if isinstance(value, set | frozenset):
        items = [_value(item) for item in value]
        return (
            type(value).__name__,
            sorted(items, key=lambda item: json.dumps(item, sort_keys=True)),
        )
    entries = [(_value(key), _value(item)) for key, item in value.items()]
    return ("dict", sorted(entries, key=lambda item: json.dumps(item[0], sort_keys=True)))


def _code(value: CodeType) -> str:
    constants = [
        _code(item) if isinstance(item, CodeType) else _value(item) for item in value.co_consts
    ]
    encoded = json.dumps(
        [
            value.co_code.hex(),
            value.co_exceptiontable.hex(),
            value.co_argcount,
            value.co_posonlyargcount,
            value.co_kwonlyargcount,
            value.co_nlocals,
            value.co_stacksize,
            value.co_flags,
            value.co_names,
            value.co_varnames,
            value.co_freevars,
            value.co_cellvars,
            value.co_qualname,
            constants,
        ],
        sort_keys=True,
        separators=(",", ":"),
    ).encode()
    return hashlib.sha256(encoded).hexdigest()
