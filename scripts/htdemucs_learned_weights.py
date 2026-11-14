"""Hydrate a pinned HTDemucs ONNX graph from its original FP16 ZIP storages."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import struct
import sys
from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import TYPE_CHECKING, NamedTuple
from zipfile import ZIP_STORED, ZipFile

if TYPE_CHECKING:
    from collections.abc import Sequence
    from io import BufferedReader, BufferedWriter

CHECKPOINT_BYTES = 84_141_911
CHECKPOINT_SHA256 = "8726e21a993978c7ba086d3872e7608d7d5bfca646ca4aca459ffda844faa8b4"
STORAGE_COUNT = 533
FP16_BYTES = 83_968_912
FP32_BYTES = 2 * FP16_BYTES
STORAGE_PATH = "archive/data/"
BLOCK_BYTES = 64 * 1024
# The template contains only fixed ONNX structure and nonlearned constants.
MAX_TEMPLATE_BYTES = 8 * 1024 * 1024
MAX_RECIPE_BYTES = 256 * 1024
SHA256 = re.compile(r"[0-9a-f]{64}")


class Storage(NamedTuple):
    offset: int
    member: int
    fp32_sha256: str


@dataclass(frozen=True)
class Pins:
    template: str
    recipe: str
    graph: str


def verify(stream: BufferedReader, extent: int, digest: str) -> None:
    """Authenticate the same open file used by the conversion."""
    if (
        not SHA256.fullmatch(digest)
        or os.fstat(stream.fileno()).st_size != extent
        or hashlib.file_digest(stream, "sha256").hexdigest() != digest
    ):
        msg = "HTDemucs asset differs from its pinned identity"
        raise ValueError(msg)
    stream.seek(0)


def load_recipe(path: Path, digest: str, template_size: int) -> list[Storage]:
    """Read ordered insertion points in the weightless template."""
    with path.open("rb") as stream:
        extent = os.fstat(stream.fileno()).st_size
        if not 0 < extent <= MAX_RECIPE_BYTES:
            msg = "Invalid HTDemucs recipe extent"
            raise ValueError(msg)
        verify(stream, extent, digest)
        manifest = json.load(stream)
    if not isinstance(manifest, dict) or set(manifest) != {"schema_version", "storages"}:
        msg = "Unsupported HTDemucs recipe"
        raise ValueError(msg)
    version, rows = manifest["schema_version"], manifest["storages"]
    if type(version) is not int or version != 1 or not isinstance(rows, list):
        msg = "Unsupported HTDemucs recipe version or storage table"
        raise ValueError(msg)
    if len(rows) != STORAGE_COUNT:
        msg = "Incomplete HTDemucs storage table"
        raise ValueError(msg)
    storages: list[Storage] = []
    previous = 0
    for row in rows:
        if not isinstance(row, list) or len(row) != len(Storage._fields):
            msg = "Invalid HTDemucs storage entry"
            raise ValueError(msg)
        offset, member, sha = row
        if (
            type(offset) is not int
            or not previous < offset < template_size
            or type(member) is not int
            or not 0 <= member < STORAGE_COUNT
        ):
            msg = "Invalid HTDemucs storage or insertion point"
            raise ValueError(msg)
        if not isinstance(sha, str) or not SHA256.fullmatch(sha):
            msg = "Invalid HTDemucs initializer digest"
            raise ValueError(msg)
        storages.append(Storage(offset, member, sha))
        previous = offset
    if len({storage.member for storage in storages}) != STORAGE_COUNT:
        msg = "Duplicate HTDemucs checkpoint storage"
        raise ValueError(msg)
    return storages


def verify_archive(archive: ZipFile) -> None:
    """Check storage and ZIP layout without interpreting checkpoint metadata."""
    expected = {f"{STORAGE_PATH}{index}" for index in range(STORAGE_COUNT)}
    entries = archive.infolist()
    names = {entry.filename for entry in entries}
    if (
        names != expected | {"archive/data.pkl", "archive/version"}
        or len(entries) != len(names)
        or any(entry.compress_type != ZIP_STORED or entry.flag_bits & 1 for entry in entries)
    ):
        msg = "Unsupported HTDemucs checkpoint ZIP layout"
        raise ValueError(msg)
    if (
        archive.getinfo("archive/data.pkl").file_size != 75_153
        or archive.getinfo("archive/version").file_size != 2
        or archive.read("archive/version") != b"3\n"
    ):
        msg = "Unsupported HTDemucs checkpoint metadata"
        raise ValueError(msg)
    sizes = [archive.getinfo(name).file_size for name in expected]
    if any(size <= 0 or size % 2 for size in sizes) or sum(sizes) != FP16_BYTES:
        msg = "Invalid HTDemucs checkpoint storage extents"
        raise ValueError(msg)


def insert_storage(archive: ZipFile, storage: Storage, output: BufferedWriter) -> None:
    """Convert straight from little-endian binary16 to binary32 without Torch."""
    digest = hashlib.sha256()
    extent = 0
    with archive.open(f"{STORAGE_PATH}{storage.member}") as source:
        expected = archive.getinfo(source.name).file_size
        while block := source.read(BLOCK_BYTES):
            count = len(block) // 2
            converted = struct.pack(f"<{count}f", *struct.unpack(f"<{count}e", block))
            extent += len(block)
            digest.update(converted)
            if output.write(converted) != len(converted):
                msg = "Incomplete HTDemucs initializer write"
                raise OSError(msg)
    if extent != expected or digest.hexdigest() != storage.fp32_sha256:
        msg = f"HTDemucs initializer differs from storage {storage.member}"
        raise ValueError(msg)


def copy_template(stream: BufferedReader, count: int, output: BufferedWriter) -> None:
    """Copy model bytes between the pinned initializer insertion points."""
    while count:
        block = stream.read(min(count, BLOCK_BYTES))
        if not block or output.write(block) != len(block):
            msg = "Incomplete HTDemucs graph template"
            raise OSError(msg)
        count -= len(block)


def hydrate(checkpoint: Path, template: Path, recipe: Path, output: Path, pins: Pins) -> int:
    """Build a verified ONNX in private staging and publish it without overwriting."""
    with checkpoint.open("rb") as source, template.open("rb") as graph_template:
        verify(source, CHECKPOINT_BYTES, CHECKPOINT_SHA256)
        template_size = os.fstat(graph_template.fileno()).st_size
        if not 0 < template_size <= MAX_TEMPLATE_BYTES:
            msg = "Invalid HTDemucs graph template extent"
            raise ValueError(msg)
        verify(graph_template, template_size, pins.template)
        storages = load_recipe(recipe, pins.recipe, template_size)
        graph_size = template_size + FP32_BYTES
        with ZipFile(source) as archive, TemporaryDirectory(dir=output.parent) as directory:
            verify_archive(archive)
            staged = Path(directory) / "htdemucs.onnx"
            previous = 0
            with staged.open("xb") as handle:
                for storage in storages:
                    copy_template(graph_template, storage.offset - previous, handle)
                    insert_storage(archive, storage, handle)
                    previous = storage.offset
                copy_template(graph_template, template_size - previous, handle)
                handle.flush()
                os.fsync(handle.fileno())
            with staged.open("rb") as handle:
                verify(handle, graph_size, pins.graph)
            if sys.platform == "win32":
                os.rename(staged, output)
            else:
                os.link(staged, output)
    return graph_size


def main(argv: Sequence[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("template", type=Path)
    parser.add_argument("recipe", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--template-sha256", required=True)
    parser.add_argument("--recipe-sha256", required=True)
    parser.add_argument("--graph-sha256", required=True)
    options = parser.parse_args(argv)
    pins = Pins(options.template_sha256, options.recipe_sha256, options.graph_sha256)
    extent = hydrate(options.checkpoint, options.template, options.recipe, options.output, pins)
    sys.stdout.write(f"HTDemucs ONNX graph bytes={extent} sha256={pins.graph}\n")


if __name__ == "__main__":
    main()
