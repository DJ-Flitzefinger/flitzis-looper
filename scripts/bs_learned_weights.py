"""Strict Torch-free export of the ONE SHA-pinned BS release checkpoint.

Interpret inert metadata descriptors; never unpickle or execute reducers/classes.
Generated weights are private trial files, not full model/producer provenance.
"""

# The locked ONNX tooling imports this decoder with Python 3.12.
from __future__ import annotations

import argparse
import array
import hashlib
import math
import os
import pickletools
import sys
import zipfile
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import TYPE_CHECKING, Never, cast

if TYPE_CHECKING:
    from collections.abc import Iterator
    from io import BufferedReader
    from typing import BinaryIO

CONFIG_SHA256 = "d8afb980318d0c08b9c2e24a7adc00d4f3150320c127a7e4de861800d1321939"
CHECKPOINT_SHA256 = "3e9daecd70aaed5b5a0d1f861cc4d77eaa45afb3fc6301b1cf32c1be0f5868fb"
PREFIX = "model_bs_roformer_ep_17_sdr_9.6568/"
BANDS = (2,) * 24 + (4,) * 12 + (12,) * 8 + (24,) * 8 + (48,) * 8 + (128, 129)
MARK = object()


def fail(message: str) -> Never:
    """Reject unsupported metadata without any Torch or generic-pickle fallback."""
    raise ValueError(message)


def schema() -> Iterator[tuple[str, tuple[int, ...]]]:
    """Yield the complete effective release state in the native decoder order."""
    for band, bins in enumerate(BANDS):
        prefix = f"band_split.to_features.{band}"
        yield f"{prefix}.0.gamma", (bins * 4,)
        yield f"{prefix}.1.weight", (384, bins * 4)
        yield f"{prefix}.1.bias", (384,)
    for block in range(8):
        for axis in range(2):
            prefix = f"layers.{block}.{axis}.layers.0"
            for suffix, shape in (
                ("0.rotary_embed.freqs", (32,)),
                ("0.norm.gamma", (384,)),
                ("0.to_qkv.weight", (1536, 384)),
                ("0.to_gates.weight", (8, 384)),
                ("0.to_gates.bias", (8,)),
                ("0.to_out.0.weight", (384, 512)),
                ("1.net.0.gamma", (384,)),
                ("1.net.1.weight", (1536, 384)),
                ("1.net.1.bias", (1536,)),
                ("1.net.4.weight", (384, 1536)),
                ("1.net.4.bias", (384,)),
            ):
                yield f"{prefix}.{suffix}", shape
    yield "final_norm.gamma", (384,)
    for stem in range(4):
        for band, bins in enumerate(BANDS):
            prefix = f"mask_estimators.{stem}.to_freqs.{band}.0"
            yield f"{prefix}.0.weight", (768, 384)
            yield f"{prefix}.0.bias", (768,)
            yield f"{prefix}.2.weight", (bins * 8, 768)
            yield f"{prefix}.2.bias", (bins * 8,)


class Global(Enum):
    """Inert tags; these are never invoked as Python constructors."""

    MAPPING = "collections OrderedDict"
    STORAGE = "torch FloatStorage"
    TENSOR = "torch._utils _rebuild_tensor_v2"


@dataclass
class Mapping:
    """Descriptor-only mapping, including explicitly checked version metadata."""

    values: dict[str, object] = field(default_factory=dict)
    metadata: object = None

    def insert(self, key: object, value: object) -> None:
        if not isinstance(key, str) or key in self.values:
            fail("Duplicate or nonstring checkpoint metadata key")
        self.values[key] = value


@dataclass(frozen=True)
class Storage:
    """A bounded CPU FloatStorage descriptor, never a Torch allocation."""

    key: str
    count: int


@dataclass(frozen=True)
class Tensor:
    """An inert contiguous float32 tensor descriptor."""

    storage: Storage
    offset: int
    shape: tuple[int, ...]
    stride: tuple[int, ...]


def storage_descriptor(value: object) -> Storage:
    """Validate the original CPU float32 persistent ID without loading a device."""
    if not isinstance(value, tuple) or len(value) != 5:
        fail("Unsupported storage descriptor")
    tag, dtype, key, location, count = cast("tuple[object, ...]", value)
    if tag != "storage" or dtype is not Global.STORAGE or location != "cpu":
        fail("Unsupported storage type or location")
    if not isinstance(key, str) or not key.isascii() or not key.isdigit():
        fail("Invalid storage key")
    if type(count) is not int or not 0 < count <= 2_000_000:
        fail("Invalid storage extent")
    return Storage(key, count)


def tensor_descriptor(args: tuple[object, ...]) -> Tensor:
    """Accept only source-exact TensorV2 arguments with empty hooks/flags."""
    if len(args) not in {6, 7} or (len(args) == 7 and args[6] is not None):
        fail("Unsupported tensor rebuild arguments or flags")
    storage, offset, shape, stride, gradient, hooks = args[:6]
    if not isinstance(storage, Storage) or type(offset) is not int or type(gradient) is not bool:
        fail("Unsupported tensor descriptor")
    if not isinstance(hooks, Mapping) or hooks.values or hooks.metadata is not None:
        fail("Tensor load hooks are not supported")
    if not isinstance(shape, tuple) or not isinstance(stride, tuple):
        fail("Unsupported tensor shape/stride")
    if not 1 <= len(shape) <= 2 or len(shape) != len(stride):
        fail("Unsupported tensor dimensions")
    if any(type(n) is not int or not 0 < n <= 2048 for n in (*shape, *stride)):
        fail("Invalid tensor shape or stride")
    return Tensor(
        storage,
        offset,
        cast("tuple[int, ...]", shape),
        cast("tuple[int, ...]", stride),
    )


class Decoder:
    """Interpret the small allowlisted protocol2 metadata grammar without pickle.load."""

    def __init__(self) -> None:
        self.stack: list[object] = []
        self.memo: dict[int, object] = {}
        self.storages: dict[str, Storage] = {}

    def reference(self, code: str, value: object) -> bool:
        """Interpret bounded memo access independently of atom parsing."""
        if code not in {"BINPUT", "LONG_BINPUT", "BINGET", "LONG_BINGET"}:
            return False
        if code in {"BINPUT", "LONG_BINPUT", "BINGET", "LONG_BINGET"}:
            if type(value) is not int or not 0 <= value < 100_000:
                fail("Invalid metadata memo index")
            index = value
            if code.endswith("INPUT"):
                if index in self.memo or not self.stack:
                    fail("Invalid metadata memo write")
                self.memo[index] = self.stack[-1]
            else:
                self.stack.append(self.memo[index])
        return True

    def atom(self, code: str, value: object) -> bool:
        """Handle bounded atoms, with globals represented as inert tags."""
        if code == "GLOBAL":
            self.stack.append(Global(value))
        elif code in {"BININT", "BININT1", "BININT2"}:
            if type(value) is not int:
                fail("Invalid integer metadata")
            self.stack.append(value)
        elif code == "BINUNICODE":
            if not isinstance(value, str) or len(value) > 1024:
                fail("Invalid string metadata")
            self.stack.append(value)
        elif code in {"NONE", "NEWFALSE", "NEWTRUE"}:
            self.stack.append({"NONE": None, "NEWFALSE": False, "NEWTRUE": True}[code])
        else:
            return False
        return True

    def sequence(self, code: str) -> bool:
        """Construct inert tuples and mark-delimited aggregates."""
        if code == "MARK":
            self.stack.append(MARK)
        elif code == "EMPTY_TUPLE":
            self.stack.append(())
        elif code == "TUPLE":
            self.stack.append(tuple(self.marked()))
        elif code in {"TUPLE1", "TUPLE2", "TUPLE3"}:
            count = int(code[-1])
            if len(self.stack) < count:
                fail("Incomplete tensor tuple")
            values = tuple(self.stack[-count:])
            del self.stack[-count:]
            self.stack.append(values)
        else:
            return False
        return True

    def marked(self) -> list[object]:
        """Consume one actual metadata mark; never synthesize missing operands."""
        for index in range(len(self.stack) - 1, -1, -1):
            if self.stack[index] is MARK:
                result = self.stack[index + 1 :]
                del self.stack[index:]
                return result
        fail("Missing metadata mark")
        raise AssertionError

    def mapping(self, code: str) -> bool:
        """Handle dictionary writes and the single explicit metadata BUILD form."""
        if code == "EMPTY_DICT":
            self.stack.append(Mapping())
        elif code in {"SETITEM", "SETITEMS"}:
            if code == "SETITEMS":
                values = self.marked()
            else:
                values = self.stack[-2:]
                del self.stack[-2:]
            target = self.stack[-1]
            if not isinstance(target, Mapping) or len(values) % 2:
                fail("Invalid metadata mapping")
            for key, value in zip(values[::2], values[1::2], strict=True):
                target.insert(key, value)
        elif code == "BUILD":
            state = self.stack.pop()
            target = self.stack[-1]
            if not isinstance(state, Mapping) or set(state.values) != {"_metadata"}:
                fail("Unsupported checkpoint state BUILD")
            if not isinstance(target, Mapping) or target.metadata is not None:
                fail("Unsupported metadata target")
            target.metadata = state.values["_metadata"]
        else:
            return False
        return True

    def reduce(self, code: str) -> bool:
        """Interpret descriptors only; no callable/class from the input is executed."""
        if code == "BINPERSID":
            value = storage_descriptor(self.stack.pop())
            if value.key in self.storages and self.storages[value.key] != value:
                fail("Conflicting storage metadata")
            self.storages[value.key] = value
            self.stack.append(value)
        elif code == "REDUCE":
            args = self.stack.pop()
            tag = self.stack.pop()
            if not isinstance(args, tuple):
                fail("Invalid metadata reducer arguments")
            if tag is Global.MAPPING and args == ():
                self.stack.append(Mapping())
            elif tag is Global.TENSOR:
                self.stack.append(tensor_descriptor(cast("tuple[object, ...]", args)))
            else:
                fail("Unsupported metadata reducer")
        else:
            return False
        return True

    def parse(self, data: bytes) -> Mapping:
        """Decode one bounded complete direct state dictionary, rejecting unknown opcodes."""
        if len(data) > 1 << 20 or data[:2] != b"\x80\x02":
            fail("Unsupported checkpoint metadata protocol/size")
        for count, (opcode, value, position) in enumerate(pickletools.genops(data)):
            if position is None:
                fail("Missing metadata byte position")
            if count > 100_000 or len(self.stack) > 10_000:
                fail("Checkpoint metadata exceeds bounded grammar")
            code = opcode.name
            if code == "PROTO":
                if position != 0 or value != 2:
                    fail("Invalid metadata protocol")
            elif code == "STOP":
                if (
                    position + 1 != len(data)
                    or len(self.stack) != 1
                    or not isinstance(self.stack[0], Mapping)
                ):
                    fail("Incomplete metadata or trailing pickle bytes")
                return self.stack[0]
            elif not (
                self.reference(code, value)
                or self.atom(code, value)
                or self.sequence(code)
                or self.mapping(code)
                or self.reduce(code)
            ):
                fail(f"Unsupported checkpoint opcode: {code}")
        fail("Missing checkpoint metadata end")
        raise AssertionError


def verify(handle: BufferedReader, count: int, digest: str) -> None:
    """Authenticate the entire same-open-file artifact before interpreting metadata."""
    if (
        os.fstat(handle.fileno()).st_size != count
        or hashlib.file_digest(handle, "sha256").hexdigest() != digest
    ):
        fail("Pinned BS artifact identity mismatch")
    handle.seek(0)


def validate_metadata(state: Mapping) -> None:
    """Accept only the original default module-version metadata, without load flags."""
    if state.metadata is not None:
        if not isinstance(state.metadata, Mapping):
            fail("Unsupported module metadata")
        for value in state.metadata.values.values():
            if (
                not isinstance(value, Mapping)
                or value.metadata is not None
                or value.values != {"version": 1}
                or type(value.values.get("version")) is not int
            ):
                fail("Unsupported module version/load metadata")


def validate_state(
    state: Mapping, archive: zipfile.ZipFile, storages: dict[str, Storage]
) -> dict[str, Tensor]:
    """Validate all names/layouts/metadata and exact referenced ZIP storage extents."""
    expected = dict(schema())
    if set(state.values) != set(expected):
        fail("Incomplete or unknown BS state tensors")
    validate_metadata(state)
    result: dict[str, Tensor] = {}
    for name, shape in expected.items():
        value = state.values[name]
        stride = (1,) if len(shape) == 1 else (shape[1], 1)
        if (
            not isinstance(value, Tensor)
            or value.offset != 0
            or value.shape != shape
            or value.stride != stride
        ):
            fail(f"Unsupported tensor layout: {name}")
        tensor = value
        if tensor.storage.count != math.prod(shape):
            fail("Unexpected tensor storage extent")
        result[name] = tensor
    for storage in storages.values():
        if archive.getinfo(PREFIX + "data/" + storage.key).file_size != storage.count * 4:
            fail("Incomplete tensor storage")
    return result


def copy_storage(archive: zipfile.ZipFile, storage: Storage, output: BinaryIO | None) -> None:
    """Stream exact finite float32 bytes through ZIP CRC EOF, without tensor allocation."""
    remaining = storage.count * 4
    with archive.open(PREFIX + "data/" + storage.key) as member:
        while remaining:
            block = member.read(min(65536, remaining))
            if not block or len(block) % 4:
                fail("Truncated storage")
            values = array.array("f")
            if values.itemsize != 4:
                fail("Unsupported native float representation")
            values.frombytes(block)
            if sys.byteorder != "little":
                values.byteswap()
            if not all(map(math.isfinite, values)):
                fail("Nonfinite BS weights")
            if output is not None:
                output.write(block)
            remaining -= len(block)
        if member.read(1):
            fail("Trailing storage bytes")


def archive_metadata(archive: zipfile.ZipFile) -> tuple[bytes, set[str]]:
    """Bound inventory and all metadata extents before reading their payloads."""
    entries = archive.infolist()
    names = {entry.filename for entry in entries}
    if (
        len(entries) != 1345
        or len(names) != len(entries)
        or sum(e.file_size for e in entries) > 527_385_512
    ):
        fail("Unsupported ZIP inventory")
    if any(e.compress_type != zipfile.ZIP_STORED or e.flag_bits & 1 for e in entries):
        fail("Unsupported ZIP compression/encryption")
    metadata = archive.getinfo(PREFIX + "data.pkl")
    for name, count in (("byteorder", 6), ("version", 2), (".data/serialization_id", 40)):
        if archive.getinfo(PREFIX + name).file_size != count:
            fail("Unsupported fixed ZIP metadata extent")
    if (
        metadata.file_size > 1 << 20
        or archive.read(PREFIX + "byteorder") != b"little"
        or archive.read(PREFIX + "version") != b"3\n"
    ):
        fail("Unsupported checkpoint metadata/byte order/version")
    if len(archive.read(PREFIX + ".data/serialization_id")) != 40:
        fail("Invalid serialization identity")
    return archive.read(metadata), names


def export_archive(archive: zipfile.ZipFile, output: BinaryIO) -> None:
    """Validate the exact archive grammar, then serialize effective release state."""
    metadata, names = archive_metadata(archive)
    decoder = Decoder()
    state = decoder.parse(metadata)
    tensors = validate_state(state, archive, decoder.storages)
    allowed = {PREFIX + p for p in ("data.pkl", "byteorder", "version", ".data/serialization_id")}
    allowed.update(PREFIX + "data/" + key for key in decoder.storages)
    if allowed != names:
        fail("Unknown or unreferenced ZIP member")
    output.write(b"FLBSNN01")
    seen: set[str] = set()
    for name, _shape in schema():
        effective = name
        if name.endswith("rotary_embed.freqs"):
            # Original default copy_ load traverses EVERY shared module alias;
            # block7 is the final writer of each shared time/frequency parameter.
            axis = name.split(".")[2]
            effective = f"layers.7.{axis}.layers.0.0.rotary_embed.freqs"
        storage = tensors[effective].storage
        copy_storage(archive, storage, output)
        seen.add(storage.key)
    for key in decoder.storages.keys() - seen:
        copy_storage(archive, decoder.storages[key], None)


def export(checkpoint: Path, config: Path, output: Path) -> None:
    """Create one exclusive private trial export after complete original pin admission."""
    with checkpoint.open("rb") as source, config.open("rb") as configuration:
        verify(source, 527_385_512, CHECKPOINT_SHA256)
        verify(configuration, 4566, CONFIG_SHA256)
        with zipfile.ZipFile(source) as archive, output.open("xb") as handle:
            export_archive(archive, handle)
    if output.stat().st_size != 526_818_456:
        fail("Incomplete fixed-schema export")
    if any(name.split(".")[0] in {"torch", "numpy", "onnx"} for name in sys.modules):
        fail("Tensor runtime imported by the Torch-free exporter")
    with output.open("rb") as handle:
        digest = hashlib.file_digest(handle, "sha256").hexdigest()
        sys.stdout.write(
            f"BS trial weights bytes={output.stat().st_size} sha256={digest} "
            "tensors=1355 torch_imported=false\n"
        )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("config", type=Path)
    parser.add_argument("output", type=Path)
    options = parser.parse_args()
    export(options.checkpoint, options.config, options.output)
