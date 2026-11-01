"""Reject executable/unsupported checkpoint metadata and corrupted weight streams."""

import io
import math
import struct
import zipfile

import pytest

from scripts.bs_learned_weights import (
    PREFIX,
    Decoder,
    Mapping,
    Storage,
    Tensor,
    copy_storage,
    schema,
    validate_metadata,
)


def _string(value: str) -> bytes:
    encoded = value.encode()
    return b"X" + struct.pack("<I", len(encoded)) + encoded


def _tensor_descriptor() -> bytes:
    return (
        b"\x80\x02ccollections\nOrderedDict\n)R"
        + _string("weight")
        + b"ctorch._utils\n_rebuild_tensor_v2\n(("
        + _string("storage")
        + b"ctorch\nFloatStorage\n"
        + _string("0")
        + _string("cpu")
        + b"K\x06tQK\x00K\x02K\x03\x86K\x03K\x01\x86\x89"
        + b"ccollections\nOrderedDict\n)RtRs."
    )


def test_inert_metadata_preserves_tensor_descriptor_and_fixed_schema() -> None:
    decoder = Decoder()
    state = decoder.parse(_tensor_descriptor())
    assert state.values == {"weight": Tensor(Storage("0", 6), 0, (2, 3), (3, 1))}
    assert decoder.storages == {"0": Storage("0", 6)}
    expected = dict(schema())
    assert len(expected) == 1355
    assert sum(math.prod(shape) for shape in expected.values()) == 131704612


@pytest.mark.parametrize(
    ("data", "message"),
    [
        (b"\x80\x02cos\nsystem\n)R.", "not a valid Global"),
        (b"\x80\x02ctorch\nFloatStorage\n)R.", "Unsupported metadata reducer"),
        (b"\x80\x02]q\x00.", "Unsupported checkpoint opcode"),
        (_tensor_descriptor() + b"trailing", "trailing pickle bytes"),
        (_tensor_descriptor()[:-1], "exhausted"),
        (
            _tensor_descriptor().replace(_string("cpu"), _string("cuda:0")),
            "storage type or location",
        ),
    ],
)
def test_unknown_reducers_opcodes_devices_and_incomplete_metadata_fail(
    data: bytes, message: str
) -> None:
    with pytest.raises(ValueError, match=message):
        Decoder().parse(data)


@pytest.mark.parametrize("version", [True, 2, "1"])
def test_module_metadata_cannot_enable_loader_flags_or_noninteger_version(version: object) -> None:
    state = Mapping(metadata=Mapping({"": Mapping({"version": version})}))
    with pytest.raises(ValueError, match="Unsupported module version/load metadata"):
        validate_metadata(state)
    state.metadata = Mapping({"": Mapping({"version": 1, "assign_to_params_buffers": True})})
    with pytest.raises(ValueError, match="Unsupported module version/load metadata"):
        validate_metadata(state)
    validate_metadata(Mapping(metadata=Mapping({"": Mapping({"version": 1})})))


def _archive(raw: bytes) -> io.BytesIO:
    data = io.BytesIO()
    with zipfile.ZipFile(data, "w", compression=zipfile.ZIP_STORED) as archive:
        archive.writestr(PREFIX + "data/0", raw)
    data.seek(0)
    return data


def test_streaming_checks_finite_extent_and_actual_zip_crc() -> None:
    raw = struct.pack("<ff", 1.0, -2.0)
    output = io.BytesIO()
    with zipfile.ZipFile(_archive(raw)) as archive:
        copy_storage(archive, Storage("0", 2), output)
    assert output.getvalue() == raw
    with (
        zipfile.ZipFile(_archive(struct.pack("<ff", 1.0, float("nan")))) as archive,
        pytest.raises(ValueError, match="Nonfinite"),
    ):
        copy_storage(archive, Storage("0", 2), io.BytesIO())
    with zipfile.ZipFile(_archive(raw)) as archive, pytest.raises(ValueError, match="Truncated"):
        copy_storage(archive, Storage("0", 3), io.BytesIO())
    data = _archive(raw)
    encoded = bytearray(data.getvalue())
    payload = 30 + len((PREFIX + "data/0").encode())
    encoded[payload] ^= 1
    with (
        zipfile.ZipFile(io.BytesIO(encoded)) as archive,
        pytest.raises(zipfile.BadZipFile, match="CRC"),
    ):
        copy_storage(archive, Storage("0", 2), io.BytesIO())
