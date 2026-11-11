"""Write the deterministic, nonlearned ONNX fixture for demucs_inline_load_control."""

import argparse
import hashlib
import struct
import sys
from pathlib import Path

WIDTH = 4096
ROW_BYTES = 4 * WIDTH
WEIGHT_BYTES = WIDTH * ROW_BYTES


def varint(value: int) -> bytes:
    encoded = bytearray()
    while value >= 128:
        encoded.append((value & 127) | 128)
        value >>= 7
    encoded.append(value)
    return bytes(encoded)


def integer(field: int, value: int) -> bytes:
    return varint(field << 3) + varint(value)


def prefix(field: int, size: int) -> bytes:
    return varint((field << 3) | 2) + varint(size)


def message(field: int, value: bytes) -> bytes:
    return prefix(field, len(value)) + value


def value_info(name: bytes) -> bytes:
    # ValueInfoProto -> TypeProto.tensor_type -> TensorShapeProto.
    shape = b"".join(message(1, integer(1, dim)) for dim in (1, WIDTH))
    tensor_type = integer(1, 1) + message(2, shape)
    return message(1, name) + message(2, message(1, tensor_type))


def initializer_header(name: bytes) -> bytes:
    # TensorProto with only float32 raw_data, dimensions and name.
    tensor = integer(1, WIDTH) * 2 + integer(2, 1) + message(8, name)
    tensor += prefix(9, WEIGHT_BYTES)
    return prefix(5, len(tensor) + WEIGHT_BYTES) + tensor


def chunks():
    # ModelProto, GraphProto and two standard MatMul NodeProtos (opset 18).
    graph = message(2, b"inline_load_control") + message(11, value_info(b"x"))
    weights = []
    for initializer, output, period in ((b"w1", b"y1", 17), (b"w2", b"y2", 19)):
        node = message(1, b"x") + message(1, initializer)
        node += message(2, output) + message(4, b"MatMul")
        graph += message(1, node) + message(12, value_info(output))
        row = struct.pack(f"<{WIDTH}f", *(i % period - period // 2 for i in range(WIDTH)))
        weights.append((initializer_header(initializer), row))
    graph_size = len(graph) + sum(len(header) + WEIGHT_BYTES for header, _ in weights)
    yield integer(1, 8) + message(8, integer(2, 18)) + prefix(7, graph_size) + graph
    for header, row in weights:
        yield header
        for _ in range(WIDTH):
            yield row


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    path = parser.parse_args().output
    digest = hashlib.sha256()
    size = 0
    with path.open("xb") as stream:
        for chunk in chunks():
            stream.write(chunk)
            digest.update(chunk)
            size += len(chunk)
    sys.stdout.write(f"{size} {digest.hexdigest()}\n")


if __name__ == "__main__":
    main()
