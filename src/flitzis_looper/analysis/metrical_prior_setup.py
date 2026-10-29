"""Explicit offline setup and bounded verification for the optional nominal prior."""

import argparse
import hashlib
import json
import os
import re
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import TYPE_CHECKING, cast
from uuid import uuid4

if TYPE_CHECKING:
    from threading import Event

PROFILE_ID = "tempocnn-0.0.8-deeptemp-k16-3-tfjs4.22-node25.5-cpu-f32-v1"
E4_SHA256 = "e4f8ba95983a74624692a044c18301493443a08af1a04833b589c7a1959022ba"
FRONTEND_SHA256 = "301407566bf84094cee436996fa72d3d2b7687b8a2459128a640fe5f0cfd6100"
REFERENCE_NODE_SHA256 = "9bbf0037fe210a04a75c812badc38c003ae900cbc1c05aca4ce52b6bbeaa0f21"
PYTHON_SHA256 = "f598fb950a86a895d8f9b4755fc9b38c48adc7a15732a342e55c17a3c3499602"
PYTHON_DLL_SHA256 = "70713cbbbcacd1ca70f802e87683d85aea185da1aae9bd31d6c02be51cfee819"
NODE_SHA256 = "d7f55cbc1b1ca9cc2815d7b8c58f94e4822a68cdf5ca55b4f2c9625581b92839"
NUMERIC_PROFILE_SHA256 = "64ed1537f5c73ea5569a7bfdf985de16d2836e9eb23930c098d8ed0c8e6ae056"
INFERENCE_SHA256 = "bd51cf2c20d411ff747f2918d5ef2c3cb0470c32d881f45946b50de1ef4154df"
SOURCE_INVENTORY_SHA256 = "3416eb77108830e1c85a10bd9273ad191893752e8bb90c1c29798d945560bc3a"
_MODEL_ARCHIVE_SHA256 = "d2bdb20ed767919764cffd87d3246b1af3dc040741eceecda87876e52a4312d3"
_MODEL_GRAPH_SHA256 = "f93693c5a913b69aa378438665937600c09286f57714359155f2763496aba184"
_MODEL_SHARD_SHA256 = "f7688e8868706813cd74610442f9923fca51162c1ebde6cf7fecfd62ebd24a15"
MODEL_HASHES = {
    "upstream/deeptemp-k16-3-tfjs.zip": _MODEL_ARCHIVE_SHA256,
    "upstream/deeptemp-k16-3-tfjs/model.json": _MODEL_GRAPH_SHA256,
    "upstream/deeptemp-k16-3-tfjs/group1-shard1of1.bin": _MODEL_SHARD_SHA256,
}
_MAX_MANIFEST = 65536
_HASH_CHUNK = 65536


def digest(path: Path, cancel: Event | None = None) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(_HASH_CHUNK):
            if cancel is not None and cancel.is_set():
                message = "cancelled"
                raise RuntimeError(message)
            value.update(block)
    return value.hexdigest()


def _unique(pairs: list[tuple[str, object]]) -> dict[str, object]:
    value: dict[str, object] = {}
    for key, item in pairs:
        if key in value:
            message = "Duplicate prior metadata field"
            raise ValueError(message)
        value[key] = item
    return value


def _nonfinite(_value: str) -> None:
    message = "Nonfinite prior metadata"
    raise ValueError(message)


def metadata_object(value: object) -> dict[str, object]:
    if not isinstance(value, dict) or any(not isinstance(key, str) for key in value):
        message = "Prior metadata requires an object with string keys"
        raise TypeError(message)
    return cast("dict[str, object]", value)


def metadata_array(value: object) -> list[object]:
    if not isinstance(value, list):
        message = "Prior metadata requires an array"
        raise TypeError(message)
    return cast("list[object]", value)


def metadata_string(value: object) -> str:
    if not isinstance(value, str):
        message = "Prior metadata requires a string"
        raise TypeError(message)
    return value


def metadata_integer(value: object) -> int:
    if type(value) is not int:
        message = "Prior metadata requires an integer"
        raise ValueError(message)
    return value


def metadata_number(value: object) -> float:
    if type(value) not in {int, float}:
        message = "Prior metadata requires a number"
        raise ValueError(message)
    return float(cast("int | float", value))


def read_json(path: Path, maximum: int = _MAX_MANIFEST) -> dict[str, object]:
    if path.stat().st_size > maximum:
        message = "Prior metadata size limit"
        raise ValueError(message)
    result = json.loads(
        path.read_text(encoding="utf-8"), object_pairs_hook=_unique, parse_constant=_nonfinite
    )
    return metadata_object(result)


def write_json_new(path: Path, value: object) -> None:
    with path.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, sort_keys=True, separators=(",", ":"), allow_nan=False)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())


def _keys(value: dict[str, object], expected: set[str]) -> None:
    if set(value) != expected:
        message = "Unexpected prior metadata fields"
        raise ValueError(message)


def _safe_file(root: Path, relative: object) -> Path:
    relative = metadata_string(relative)
    name = PurePosixPath(relative)
    if (
        name.is_absolute()
        or any(part in {"", ".", ".."} for part in name.parts)
        or "\\" in relative
    ):
        message = "Invalid prior installation file path"
        raise ValueError(message)
    current = root
    for part in name.parts:
        current /= part
        if current.is_symlink() or current.is_junction():
            message = "Prior installation contains a reparse path"
            raise ValueError(message)
    if not current.resolve(strict=True).is_relative_to(root.resolve(strict=True)):
        message = "Prior file escaped its retained installation"
        raise ValueError(message)
    return current


@dataclass(frozen=True, slots=True)
class PriorProfileToken:
    installation_root: Path
    profile_sha256: str
    numeric_profile_sha256: str

    def wire(self) -> dict[str, str | int]:
        return {
            "schema_version": 1,
            "installation_root": str(self.installation_root),
            "profile_sha256": self.profile_sha256,
            "numeric_profile_sha256": self.numeric_profile_sha256,
        }


def _verify_header(token: PriorProfileToken, cancel: Event | None) -> dict[str, object]:
    root = token.installation_root
    if (
        not root.is_absolute()
        or not re.fullmatch(r"installation-[0-9a-f]{32}", root.name)
        or root.is_symlink()
        or root.is_junction()
    ):
        message = "Invalid retained prior installation"
        raise ValueError(message)
    manifest_path = _safe_file(root, "manifest.json")
    if digest(manifest_path, cancel) != token.profile_sha256:
        message = "Prior profile manifest changed"
        raise ValueError(message)
    manifest = read_json(manifest_path)
    _keys(
        manifest,
        {
            "schema_version",
            "profile_id",
            "numeric_profile_sha256",
            "python",
            "node",
            "frontend",
            "inference",
            "inventory",
        },
    )
    if (
        type(manifest["schema_version"]) is not int
        or manifest["schema_version"] != 1
        or manifest["profile_id"] != PROFILE_ID
        or manifest["numeric_profile_sha256"] != token.numeric_profile_sha256
    ):
        message = "Prior profile identity differs"
        raise ValueError(message)
    numeric = _safe_file(root, "numeric-profile.json")
    if (
        digest(numeric, cancel) != token.numeric_profile_sha256
        or token.numeric_profile_sha256 != NUMERIC_PROFILE_SHA256
    ):
        message = "Prior numeric profile changed"
        raise ValueError(message)
    numeric_value = read_json(numeric)
    if numeric_value.get("numeric_profile_id") != PROFILE_ID:
        message = "Prior numeric profile is unsupported"
        raise ValueError(message)
    for relative, expected in MODEL_HASHES.items():
        if digest(_safe_file(root, relative), cancel) != expected:
            message = "Prior model/archive identity differs"
            raise ValueError(message)
    return manifest


def _verify_inventory_row(
    root: Path, line: bytes, previous: str, cancel: Event | None
) -> tuple[str, int]:
    if len(line) > 4096 or not line.endswith(b"\n"):
        message = "Prior inventory row exceeds its bound"
        raise ValueError(message)
    row = metadata_object(json.loads(line, object_pairs_hook=_unique, parse_constant=_nonfinite))
    _keys(row, {"path", "bytes", "sha256"})
    relative, size = metadata_string(row["path"]), metadata_integer(row["bytes"])
    if relative <= previous or size < 0:
        message = "Prior inventory order/geometry differs"
        raise ValueError(message)
    path = _safe_file(root, relative)
    if "__pycache__" in path.parts or path.suffix in {".pyc", ".pyo", ".nbc", ".nbi"}:
        message = "Mutable code caches are not installed prior inputs"
        raise ValueError(message)
    if path.stat().st_size != size or digest(path, cancel) != row["sha256"]:
        message = "Prior installed file changed"
        raise ValueError(message)
    return relative, size


def _verify_inventory(root: Path, inventory: dict[str, object], cancel: Event | None) -> int:
    _keys(inventory, {"path", "sha256", "count", "total_bytes"})
    if inventory["path"] != "files.jsonl" or not 1 <= metadata_integer(inventory["count"]) <= 65536:
        message = "Prior inventory bound is invalid"
        raise ValueError(message)
    index = _safe_file(root, "files.jsonl")
    if digest(index, cancel) != inventory["sha256"]:
        message = "Prior inventory changed"
        raise ValueError(message)
    previous, count, total = "", 0, 0
    with index.open("rb") as stream:
        while line := stream.readline(4097):
            relative, size = _verify_inventory_row(root, line, previous, cancel)
            previous, count, total = relative, count + 1, total + size
    if count != inventory["count"] or total != inventory["total_bytes"]:
        message = "Prior inventory extent differs"
        raise ValueError(message)
    return count


def _verify_layout(
    root: Path, manifest: dict[str, object], count: int, cancel: Event | None
) -> None:
    if _closure_file_count(root, cancel) != count + 2:
        message = "Prior closure contains unlisted files"
        raise ValueError(message)
    for key, expected in (
        ("python", PYTHON_SHA256),
        ("node", NODE_SHA256),
        ("frontend", FRONTEND_SHA256),
    ):
        entry = metadata_object(manifest[key])
        if (
            digest(_safe_file(root, entry["path"]), cancel) != entry["sha256"]
            or entry["sha256"] != expected
        ):
            message = "Prior executable/source pin differs"
            raise ValueError(message)
    if (
        digest(_safe_file(root, "python/python312.dll"), cancel) != PYTHON_DLL_SHA256
        or digest(_safe_file(root, "prepare-features.py"), cancel) != E4_SHA256
    ):
        message = "Prior Python/helper closure differs"
        raise ValueError(message)
    inference = metadata_object(manifest["inference"])
    if (
        digest(_safe_file(root, inference["path"]), cancel) != inference["sha256"]
        or inference["sha256"] != INFERENCE_SHA256
    ):
        message = "Prior classifier entrypoint changed"
        raise ValueError(message)


def verify_profile(token: PriorProfileToken, cancel: Event | None = None) -> dict[str, object]:
    """Stream the complete inventory; no installed-file dictionary survives this call."""
    manifest = _verify_header(token, cancel)
    count = _verify_inventory(
        token.installation_root, metadata_object(manifest["inventory"]), cancel
    )
    _verify_layout(token.installation_root, manifest, count, cancel)
    return manifest


def _closure_file_count(root: Path, cancel: Event | None) -> int:
    pending, count = [root], 0
    while pending:
        with os.scandir(pending.pop()) as entries:
            for entry in entries:
                if cancel is not None and cancel.is_set():
                    message = "cancelled"
                    raise RuntimeError(message)
                path = Path(entry.path)
                if (
                    path.is_symlink()
                    or path.is_junction()
                    or path.name == "__pycache__"
                    or path.suffix in {".pyc", ".pyo", ".nbc", ".nbi"}
                ):
                    message = "Prior closure contains mutable caches or reparse paths"
                    raise ValueError(message)
                if entry.is_dir(follow_symlinks=False):
                    pending.append(path)
                elif entry.is_file(follow_symlinks=False):
                    count += 1
                    if count > 65538:
                        message = "Prior closure file count exceeds its bound"
                        raise ValueError(message)
    return count


def token_from_wire(value: dict[str, object]) -> PriorProfileToken:
    _keys(
        value, {"schema_version", "installation_root", "profile_sha256", "numeric_profile_sha256"}
    )
    if type(value["schema_version"]) is not int or value["schema_version"] != 1:
        message = "Unsupported prior profile token"
        raise ValueError(message)
    for name in ("profile_sha256", "numeric_profile_sha256"):
        if re.fullmatch(r"[0-9a-f]{64}", metadata_string(value[name])) is None:
            message = "Invalid prior profile digest"
            raise ValueError(message)
    return PriorProfileToken(
        Path(metadata_string(value["installation_root"])),
        metadata_string(value["profile_sha256"]),
        metadata_string(value["numeric_profile_sha256"]),
    )


def probe_metrical_prior_profile(installation_root: str) -> str:
    root = Path(installation_root)
    if not root.is_absolute():
        message = "Prior installation root must be absolute"
        raise ValueError(message)
    pointer = read_json(root / "current.json", 4096)
    _keys(pointer, {"schema_version", "directory", "profile_sha256", "numeric_profile_sha256"})
    if type(pointer["schema_version"]) is not int or pointer["schema_version"] != 1:
        message = "Unsupported prior installation pointer"
        raise ValueError(message)
    token = token_from_wire({
        "schema_version": 1,
        "installation_root": str(root / metadata_string(pointer["directory"])),
        "profile_sha256": pointer["profile_sha256"],
        "numeric_profile_sha256": pointer["numeric_profile_sha256"],
    })
    if token.installation_root.parent != root:
        message = "Prior pointer escaped its installation root"
        raise ValueError(message)
    verify_profile(token)
    return json.dumps(token.wire(), separators=(",", ":"))


def activate_profile(root: Path, installation: Path) -> PriorProfileToken:
    if not root.is_absolute() or installation.parent != root:
        message = "Prior activation requires a retained direct child"
        raise ValueError(message)
    manifest = read_json(installation / "manifest.json")
    token = PriorProfileToken(
        installation,
        digest(installation / "manifest.json"),
        metadata_string(manifest["numeric_profile_sha256"]),
    )
    verify_profile(token)
    pointer = {
        "schema_version": 1,
        "directory": installation.name,
        "profile_sha256": token.profile_sha256,
        "numeric_profile_sha256": token.numeric_profile_sha256,
    }
    temporary = root / f".current-{uuid4().hex}.json"
    write_json_new(temporary, pointer)
    temporary.replace(root / "current.json")
    return token


def _copy_file(source: Path, target: Path, expected: str | None = None) -> None:
    if (
        source.is_symlink()
        or source.is_junction()
        or (expected is not None and digest(source) != expected)
    ):
        message = "Prior setup source identity differs"
        raise ValueError(message)
    if target.exists():
        if digest(target) == digest(source):
            return
        message = "Conflicting owned prior setup input"
        raise ValueError(message)
    target.parent.mkdir(parents=True, exist_ok=True)
    with source.open("rb") as incoming, target.open("xb") as outgoing:
        shutil.copyfileobj(incoming, outgoing, _HASH_CHUNK)
        outgoing.flush()
        os.fsync(outgoing.fileno())
    if digest(source) != digest(target):
        message = "Prior setup copy differs"
        raise ValueError(message)


def _stage_inventory(
    root: Path, source: Path, python_base: Path, node: Path, assets: Path
) -> dict[str, object]:
    if any(not path.is_absolute() for path in (root, source, python_base, node, assets)):
        message = "Prior setup requires absolute source and destination paths"
        raise ValueError(message)
    if any(
        digest(path) != expected
        for path, expected in (
            (python_base / "python.exe", PYTHON_SHA256),
            (python_base / "python312.dll", PYTHON_DLL_SHA256),
            (node, NODE_SHA256),
        )
    ):
        message = "Prior setup requires the pinned actual CPython base and Node executable"
        raise ValueError(message)
    inventory_path = source / "runtime-pins.json"
    if digest(inventory_path) != SOURCE_INVENTORY_SHA256:
        message = "Private runtime source inventory differs"
        raise ValueError(message)
    return read_json(inventory_path, 4 * 1024 * 1024)


def _copy_private_packages(source: Path, installed: Path, inventory: dict[str, object]) -> None:
    for value in metadata_array(inventory["files"]):
        row = metadata_object(value)
        relative = metadata_string(row["path"])
        if not relative.startswith(("python-packages/", "node_modules/")):
            message = "Unexpected private runtime source entry"
            raise ValueError(message)
        _copy_file(
            _safe_file(source, relative), installed / relative, metadata_string(row["sha256"])
        )


def _copy_python_base(python_base: Path, installed: Path) -> None:
    for path in sorted(python_base.rglob("*")):
        relative = path.relative_to(python_base)
        if (
            "__pycache__" in relative.parts
            or relative.parts[:2] == ("Lib", "site-packages")
            or path.suffix in {".pyc", ".pyo"}
        ):
            continue
        if path.is_file():
            _copy_file(path, installed / "python" / relative)


def _copy_frontend_assets(assets: Path, installed: Path) -> None:
    for path in sorted(assets.rglob("*")):
        if (
            path.is_file()
            and "__pycache__" not in path.parts
            and path.suffix not in {".pyc", ".pyo"}
        ):
            _copy_file(path, installed / path.relative_to(assets))


def _copy_source_assets(source: Path, installed: Path) -> None:
    for name in ("requirements.in", "requirements.lock", "package.json", "package-lock.json"):
        _copy_file(source / name, installed / name)
    for name in ("deeptemp-k16-3-tfjs.zip",):
        _copy_file(
            source / "upstream" / name,
            installed / "upstream" / name,
            MODEL_HASHES[f"upstream/{name}"],
        )
    for name in ("model.json", "group1-shard1of1.bin"):
        _copy_file(
            source / "upstream" / "deeptemp-k16-3-tfjs" / name,
            installed / "upstream" / "deeptemp-k16-3-tfjs" / name,
            MODEL_HASHES[f"upstream/deeptemp-k16-3-tfjs/{name}"],
        )


def _write_inventory(installed: Path) -> tuple[int, int]:
    count, total = 0, 0
    with (installed / "files.jsonl").open("x", encoding="utf-8", newline="\n") as output:
        for path in sorted(
            (
                path
                for path in installed.rglob("*")
                if path.is_file() and path.name != "files.jsonl"
            ),
            key=lambda path: path.relative_to(installed).as_posix(),
        ):
            size = path.stat().st_size
            row = {
                "path": path.relative_to(installed).as_posix(),
                "bytes": size,
                "sha256": digest(path),
            }
            line = json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n"
            if len(line.encode("utf-8")) > 4096:
                message = "Prior installed filename exceeds inventory row bound"
                raise ValueError(message)
            output.write(line)
            count, total = count + 1, total + size
        output.flush()
        os.fsync(output.fileno())
    return count, total


def _write_manifest(installed: Path, count: int, total: int) -> None:
    manifest = {
        "schema_version": 1,
        "profile_id": PROFILE_ID,
        "numeric_profile_sha256": digest(installed / "numeric-profile.json"),
        "python": {"path": "python/python.exe", "sha256": PYTHON_SHA256, "version": "3.12.13"},
        "node": {"path": "node/node.exe", "sha256": NODE_SHA256, "version": "v25.5.0"},
        "frontend": {"path": "prepare-features-streamed.py", "sha256": FRONTEND_SHA256},
        "inference": {"path": "infer-prior.cjs", "sha256": digest(installed / "infer-prior.cjs")},
        "inventory": {
            "path": "files.jsonl",
            "sha256": digest(installed / "files.jsonl"),
            "count": count,
            "total_bytes": total,
        },
    }
    write_json_new(installed / "manifest.json", manifest)


def stage_profile(
    root: Path, source: Path, python_base: Path, node: Path, assets: Path
) -> PriorProfileToken:
    """Explicit Root-only offline copy; never called from analysis or readiness."""
    inventory = _stage_inventory(root, source, python_base, node, assets)
    root.mkdir(parents=True, exist_ok=True)
    installed = root / f"installation-{uuid4().hex}"
    installed.mkdir()
    _copy_private_packages(source, installed, inventory)
    del inventory
    _copy_python_base(python_base, installed)
    _copy_file(node, installed / "node" / "node.exe", NODE_SHA256)
    _copy_frontend_assets(assets, installed)
    _copy_source_assets(source, installed)
    count, total = _write_inventory(installed)
    _write_manifest(installed, count, total)
    token = PriorProfileToken(
        installed, digest(installed / "manifest.json"), digest(installed / "numeric-profile.json")
    )
    verify_profile(token)
    return token


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--install-dir", type=Path, required=True)
    parser.add_argument("--source-dir", type=Path)
    parser.add_argument("--python-base", type=Path)
    parser.add_argument("--node-executable", type=Path)
    parser.add_argument(
        "--assets",
        type=Path,
        default=Path(__file__).resolve().parents[3] / "workers" / "metrical_prior",
    )
    parser.add_argument("--activate", type=Path)
    args = parser.parse_args()
    if args.activate is not None:
        token = activate_profile(args.install_dir, args.activate)
    else:
        if args.source_dir is None or args.python_base is None or args.node_executable is None:
            parser.error("Staging needs explicit verified source, Python base and Node paths")
        token = stage_profile(
            args.install_dir, args.source_dir, args.python_base, args.node_executable, args.assets
        )
    sys.stdout.write(json.dumps(token.wire()) + "\n")


if __name__ == "__main__":
    main()
