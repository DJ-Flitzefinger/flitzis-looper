"""Closed worker-lock marker evaluation for Windows AMD64 Python3.12.13.

This target is independent of the parent App interpreter and never probes CUDA.
Unknown expressions fail closed; no lock text is evaluated as Python code.
"""

import ast
import re
from typing import NoReturn, cast

TARGET = {
    "implementation_name": "cpython",
    "implementation_version": "3.12.13",
    "os_name": "nt",
    "platform_machine": "AMD64",
    "platform_python_implementation": "CPython",
    "platform_release": "",
    "platform_system": "Windows",
    "platform_version": "",
    "python_full_version": "3.12.13",
    "python_version": "3.12",
    "sys_platform": "win32",
    "extra": "",
}
_VERSION_NAMES = {"implementation_version", "python_full_version", "python_version"}


def _bad() -> NoReturn:
    message = "unsupported_worker_lock_marker"
    raise ValueError(message)


def _value(node: ast.expr) -> str:
    if isinstance(node, ast.Name) and node.id in TARGET:
        return TARGET[node.id]
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    _bad()


def _version(value: str) -> tuple[int, ...]:
    if re.fullmatch(r"[0-9]+(?:\.[0-9]+){0,3}", value) is None:
        _bad()
    parts = tuple(int(part) for part in value.split("."))
    return (*parts, *(0 for _ in range(4 - len(parts))))


def _compare(node: ast.Compare) -> bool:
    if len(node.ops) != 1 or len(node.comparators) != 1:
        _bad()
    left, right = _value(node.left), _value(node.comparators[0])
    op = node.ops[0]
    if isinstance(op, ast.In):
        return left in right
    if isinstance(op, ast.NotIn):
        return left not in right
    names = [v.id for v in (node.left, node.comparators[0]) if isinstance(v, ast.Name)]
    if any(n in _VERSION_NAMES for n in names):
        a, b = _version(left), _version(right)
        order = (a > b) - (a < b)
    else:
        order = (left > right) - (left < right)
    checks = {
        ast.Eq: order == 0,
        ast.NotEq: order != 0,
        ast.Lt: order < 0,
        ast.LtE: order <= 0,
        ast.Gt: order > 0,
        ast.GtE: order >= 0,
    }
    result = checks.get(type(op))
    if result is None:
        _bad()
    return result


def _expression(node: ast.expr) -> bool:
    if isinstance(node, ast.Compare):
        return _compare(node)
    if isinstance(node, ast.BoolOp):
        values = [_expression(value) for value in node.values]
        if isinstance(node.op, ast.And):
            return all(values)
        if isinstance(node.op, ast.Or):
            return any(values)
    _bad()


def marker_applies(marker: str | None) -> bool:
    if marker is None:
        return True
    if not isinstance(marker, str) or len(marker) > 8192:
        _bad()
    try:
        expression = ast.parse(marker, mode="eval").body
    except (SyntaxError, ValueError) as error:
        message = "unsupported_worker_lock_marker"
        raise ValueError(message) from error
    return _expression(expression)


def _objects(value: object) -> list[dict[str, object]]:
    if not isinstance(value, list):
        _bad()
    result = []
    for item in value:
        if not isinstance(item, dict) or any(not isinstance(key, str) for key in item):
            _bad()
        result.append(cast("dict[str, object]", item))
    return result


def _text(value: object) -> str:
    if not isinstance(value, str):
        _bad()
    return value


def _marker(value: object) -> bool:
    return marker_applies(None if value is None else _text(value))


def _resolution_applies(package: dict[str, object]) -> bool:
    markers = package.get("resolution-markers")
    if markers is None:
        return True
    if not isinstance(markers, list):
        _bad()
    return any(_marker(marker) for marker in markers)


def expected_packages(lock: dict[str, object], root_name: str) -> dict[str, str]:
    packages = _objects(lock["package"])
    roots = [p for p in packages if p["name"] == root_name]
    if len(roots) != 1:
        _bad()
    pending = _objects(roots[0].get("dependencies", []))
    required: dict[str, str] = {}
    while pending:
        edge = pending.pop()
        if not _marker(edge.get("marker")):
            continue
        candidates = [
            p
            for p in packages
            if p["name"] == edge["name"]
            and ("version" not in edge or edge["version"] == p["version"])
            and _resolution_applies(p)
        ]
        if len(candidates) != 1:
            _bad()
        package = candidates[0]
        name, version = _text(package["name"]), _text(package["version"])
        if name in required:
            if required[name] != version:
                _bad()
            continue
        required[name] = version
        pending.extend(_objects(package.get("dependencies", [])))
    return dict(sorted(required.items()))
