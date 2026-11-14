"""Prepare the pinned BS-RoFormer mask graph and its private external weights."""

import argparse
import hashlib
import math
import os
import sys
import zipfile
from pathlib import Path

import onnx
from onnx import ModelProto, NodeProto, TensorProto, helper

from scripts import bs_learned_weights as weights

GRAPH_NAME = "bs-mask.onnx"
AMP_GRAPH_NAME = "bs-mask-amp.onnx"
WEIGHTS_NAME = "weights.bsnn"
WEIGHTS_MAGIC = b"FLBSNN01"
WEIGHTS_BYTES = 526_818_456
WEIGHTS_SHA256 = "51619069b73bfcbdc444a16a4ebc8cf53ea8d3466fce67b594412a02b854cbdf"
ONNX_VERSION = "1.19.1"
OPSET = 20
IR_VERSION = 9
STEPS = 1101
FEATURES = 4100
DIM = 384
BANDS = len(weights.BANDS)
HEADS = 8
HEAD_WIDTH = 64
SOURCES = 4
QUERY_TILE = 64


class Nodes:
    """Build deterministic standard ONNX nodes and small embedded constants."""

    def __init__(self, prefix: str = "v", *, amp: bool = False) -> None:
        self.nodes: list[NodeProto] = []
        self.initializers: list[TensorProto] = []
        self.amp = amp
        self._prefix = prefix
        self._constants: dict[tuple[int, tuple[int, ...], tuple[float, ...]], str] = {}

    def outputs(self, kind: str, count: int, *inputs: str, **attributes: object) -> list[str]:
        """Append a node with uniquely named outputs, in dependency order."""
        name = f"{self._prefix}{len(self.nodes)}_{kind}"
        result = [f"{name}_{index}" for index in range(count)]
        self.nodes.append(helper.make_node(kind, inputs, result, name=name, **attributes))
        return result

    def op(self, kind: str, *inputs: str, **attributes: object) -> str:
        """Append a node with one output and return that output name."""
        return self.outputs(kind, 1, *inputs, **attributes)[0]

    def constant(self, dtype: int, shape: tuple[int, ...], values: tuple[float, ...]) -> str:
        """Intern small constants for reuse within the current graph."""
        key = (dtype, shape, values)
        if key not in self._constants:
            name = f"{self._prefix}_constant_{len(self._constants)}"
            self.initializers.append(helper.make_tensor(name, dtype, shape, values))
            self._constants[key] = name
        return self._constants[key]

    def ints(self, *values: int) -> str:
        """Return an int64 vector initializer."""
        return self.constant(TensorProto.INT64, (len(values),), values)

    def scalar(self, dtype: int, value: float) -> str:
        """Return a rank-zero initializer."""
        return self.constant(dtype, (), (value,))

    def reshape(self, value: str, *shape: int) -> str:
        """Reshape with checked geometry at the worker input/output boundary."""
        return self.op("Reshape", value, self.ints(*shape))

    def slice(self, value: str, start: int, end: int, axis: int) -> str:
        """Slice one fixed range along an axis."""
        return self.op("Slice", value, self.ints(start), self.ints(end), self.ints(axis))

    def float32(self, value: str) -> str:
        """Widen values at an original autocast FP32 boundary."""
        return self.op("Cast", value, to=TensorProto.FLOAT) if self.amp else value

    def float16(self, value: str) -> str:
        """Round values at an original autocast FP16 boundary."""
        return self.op("Cast", value, to=TensorProto.FLOAT16) if self.amp else value


def external_weights() -> list[TensorProto]:
    """Map the exact effective FP32 state to the single relative data file."""
    offset = len(WEIGHTS_MAGIC)
    result = []
    for name, shape in weights.schema():
        length = 4 * math.prod(shape)
        tensor = TensorProto(name=name, dims=shape, data_type=TensorProto.FLOAT)
        tensor.data_location = TensorProto.EXTERNAL
        for key, value in (
            ("location", WEIGHTS_NAME),
            ("offset", str(offset)),
            ("length", str(length)),
        ):
            tensor.external_data.add(key=key, value=value)
        result.append(tensor)
        offset += length
    if offset != WEIGHTS_BYTES:
        msg = "BS-RoFormer effective weight layout differs"
        raise RuntimeError(msg)
    return result


def rms(graph: Nodes, value: str, gamma: str, width: int) -> str:
    """Match the original L2 denominator clamp and subsequent scale order."""
    value = graph.float32(value)
    squared = graph.op("Mul", value, value)
    norm = graph.op("Sqrt", graph.op("ReduceSum", squared, graph.ints(-1), keepdims=1))
    denominator = graph.op("Max", norm, graph.scalar(TensorProto.FLOAT, 1e-12))
    normalized = graph.op("Div", value, denominator)
    scaled = graph.op("Mul", normalized, graph.scalar(TensorProto.FLOAT, math.sqrt(width)))
    return graph.op("Mul", scaled, gamma)


def linear(graph: Nodes, value: str, prefix: str, *, bias: bool = True) -> str:
    """Multiply by the stored output-major matrix and optionally add its bias."""
    if graph.amp:
        return amp_linear(graph, value, prefix, bias=bias)
    matrix = graph.op("Transpose", f"{prefix}.weight", perm=[1, 0])
    output = graph.op("MatMul", value, matrix)
    return graph.op("Add", output, f"{prefix}.bias") if bias else output


def amp_linear(graph: Nodes, value: str, prefix: str, *, bias: bool) -> str:
    """Use half operands and only one output rounding, including the bias."""
    leading = graph.op("Shape", value, end=2)
    flattened = graph.op("Flatten", graph.float16(value), axis=2)
    weight = graph.float16(f"{prefix}.weight")
    inputs = (flattened, weight, graph.float16(f"{prefix}.bias")) if bias else (flattened, weight)
    output = graph.op("Gemm", *inputs, transB=1)
    output_width = graph.op("Shape", output, start=1)
    return graph.op("Reshape", output, graph.op("Concat", leading, output_width, axis=0))


def rotary(graph: Nodes, value: str, cos: str, sin: str, steps: int) -> str:
    """Rotate adjacent pairs of each head, preserving the checkpoint frequencies."""
    even = graph.op(
        "Slice", value, graph.ints(0), graph.ints(HEAD_WIDTH), graph.ints(-1), graph.ints(2)
    )
    odd = graph.op(
        "Slice", value, graph.ints(1), graph.ints(HEAD_WIDTH), graph.ints(-1), graph.ints(2)
    )
    real = graph.op("Sub", graph.op("Mul", even, cos), graph.op("Mul", odd, sin))
    imag = graph.op("Add", graph.op("Mul", odd, cos), graph.op("Mul", even, sin))
    pairs = graph.op(
        "Concat",
        graph.op("Unsqueeze", real, graph.ints(-1)),
        graph.op("Unsqueeze", imag, graph.ints(-1)),
        axis=-1,
    )
    return graph.reshape(pairs, 0, HEADS, steps, HEAD_WIDTH)


def attend(graph: Nodes, query: str, key: str, value: str) -> str:
    """Compute noncausal, unmasked attention over the complete key axis."""
    scores = graph.op("MatMul", query, key)
    return graph.op("MatMul", graph.op("Softmax", scores, axis=-1), value)


def tiled_attention(graph: Nodes, query: str, key: str, value: str) -> str:
    """Bound the time-score extent by splitting only the independent queries."""
    full_tiles, tail = divmod(STEPS, QUERY_TILE)
    covered = full_tiles * QUERY_TILE
    tile = Nodes(f"tile_{len(graph.nodes)}_")
    start = tile.op("Mul", "iteration", tile.ints(QUERY_TILE))
    end = tile.op("Add", start, tile.ints(QUERY_TILE))
    rows = tile.op("Slice", query, start, end, tile.ints(2))
    context = attend(tile, rows, key, value)
    condition = tile.op("Identity", "continue")
    body = helper.make_graph(
        tile.nodes,
        "time_attention_tile",
        [
            helper.make_tensor_value_info("iteration", TensorProto.INT64, []),
            helper.make_tensor_value_info("continue", TensorProto.BOOL, []),
        ],
        [
            helper.make_tensor_value_info(condition, TensorProto.BOOL, []),
            helper.make_tensor_value_info(
                context, TensorProto.FLOAT, ["axial_batch", HEADS, QUERY_TILE, HEAD_WIDTH]
            ),
        ],
        tile.initializers,
    )
    scan = graph.op(
        "Loop",
        graph.scalar(TensorProto.INT64, full_tiles),
        graph.scalar(TensorProto.BOOL, 1),
        body=body,
    )
    ordered = graph.op("Transpose", scan, perm=[1, 2, 0, 3, 4])
    joined = graph.reshape(ordered, 0, HEADS, covered, HEAD_WIDTH)
    if tail:
        last = attend(graph, graph.slice(query, covered, STEPS, 2), key, value)
        joined = graph.op("Concat", joined, last, axis=2)
    return joined


def attention_context(graph: Nodes, query: str, key: str, value: str, steps: int) -> str:
    """Match default math SDPA accumulation and cast the context back to FP16."""
    if graph.amp:
        scale = graph.scalar(TensorProto.FLOAT, HEAD_WIDTH**-0.25)
        query = graph.op("Mul", graph.float32(query), scale)
        key = graph.op("Mul", graph.float32(key), scale)
        value = graph.float32(value)
    key = graph.op("Transpose", key, perm=[0, 1, 3, 2])
    context = (
        tiled_attention(graph, query, key, value)
        if steps == STEPS
        else attend(graph, query, key, value)
    )
    return graph.float16(context)


def rotary_angles(graph: Nodes, prefix: str, steps: int) -> tuple[str, str]:
    """Preserve half-angle rounding of the original CUDA autocast outer product."""
    positions = graph.constant(TensorProto.FLOAT, (steps, 1), tuple(range(steps)))
    frequencies = graph.float16(f"{prefix}.rotary_embed.freqs")
    angles = graph.op("Mul", graph.float16(positions), frequencies)
    angles = graph.float32(angles)
    return graph.float16(graph.op("Cos", angles)), graph.float16(graph.op("Sin", angles))


def attention(graph: Nodes, tensor: str, prefix: str, steps: int) -> str:
    """Apply the original normalized QKV and gated attention projection."""
    normalized = rms(graph, tensor, f"{prefix}.norm.gamma", DIM)
    packed = linear(graph, normalized, f"{prefix}.to_qkv", bias=False)
    packed = graph.reshape(packed, 0, steps, 3, HEADS, HEAD_WIDTH)
    packed = graph.op("Transpose", packed, perm=[2, 0, 3, 1, 4])
    query, key, value = (
        graph.op("Gather", packed, graph.scalar(TensorProto.INT64, i)) for i in range(3)
    )
    cos, sin = rotary_angles(graph, prefix, steps)
    query = rotary(graph, query, cos, sin, steps)
    if not graph.amp:
        query = graph.op("Mul", query, graph.scalar(TensorProto.FLOAT, HEAD_WIDTH**-0.5))
    key = rotary(graph, key, cos, sin, steps)
    context = attention_context(graph, query, key, value, steps)
    gate = graph.op("Sigmoid", linear(graph, normalized, f"{prefix}.to_gates"))
    gate = graph.op("Unsqueeze", graph.op("Transpose", gate, perm=[0, 2, 1]), graph.ints(-1))
    gated = graph.op("Transpose", graph.op("Mul", context, gate), perm=[0, 2, 1, 3])
    return linear(
        graph, graph.reshape(gated, 0, steps, HEADS * HEAD_WIDTH), f"{prefix}.to_out.0", bias=False
    )


def transformer(graph: Nodes, value: str, prefix: str, steps: int) -> str:
    """Preserve both original pre-normalized residuals and exact GELU."""
    value = graph.op("Add", value, attention(graph, value, f"{prefix}.0", steps))
    feed = rms(graph, value, f"{prefix}.1.net.0.gamma", DIM)
    feed = graph.op("Gelu", linear(graph, feed, f"{prefix}.1.net.1"), approximate="none")
    return graph.op("Add", value, linear(graph, feed, f"{prefix}.1.net.4"))


def project_bands(graph: Nodes) -> str:
    """Project the release's 62 band slices to the common latent width."""
    widths = tuple(bins * 4 for bins in weights.BANDS)
    bands = graph.outputs("Split", BANDS, "features", graph.ints(*widths), axis=-1)
    projected = []
    for index, (band, width) in enumerate(zip(bands, widths, strict=True)):
        prefix = f"band_split.to_features.{index}"
        value = linear(graph, rms(graph, band, f"{prefix}.0.gamma", width), f"{prefix}.1")
        projected.append(graph.op("Unsqueeze", value, graph.ints(-2)))
    return graph.op("Concat", *projected, axis=-2)


def project_masks(graph: Nodes, value: str) -> str:
    """Emit independent Tanh/GLU mask heads with stem-major output ordering."""
    bands = [
        graph.op("Gather", value, graph.scalar(TensorProto.INT64, index), axis=2)
        for index in range(BANDS)
    ]
    sources = []
    for source in range(SOURCES):
        masks = []
        for index, (band, bins) in enumerate(zip(bands, weights.BANDS, strict=True)):
            prefix = f"mask_estimators.{source}.to_freqs.{index}.0"
            expanded = graph.op("Tanh", linear(graph, band, f"{prefix}.0"))
            packed = linear(graph, expanded, f"{prefix}.2")
            mask, gate = graph.outputs("Split", 2, packed, graph.ints(bins * 4, bins * 4), axis=-1)
            # Torch's GLU computes sigmoid and multiply in FP32, rounding just
            # once to half. The final worker interface remains FP32.
            mask = graph.op("Mul", graph.float32(mask), graph.op("Sigmoid", graph.float32(gate)))
            masks.append(graph.float16(mask))
        sources.append(graph.op("Unsqueeze", graph.op("Concat", *masks, axis=-1), graph.ints(1)))
    return graph.op("Concat", *sources, axis=1)


def mask_model(*, amp: bool = False) -> ModelProto:
    """Build a four-source learned graph for B1 and B2."""
    graph = Nodes(amp=amp)
    value = project_bands(graph)
    for block in range(8):
        time = graph.op("Transpose", value, perm=[0, 2, 1, 3])
        time = transformer(
            graph, graph.reshape(time, -1, STEPS, DIM), f"layers.{block}.0.layers.0", STEPS
        )
        time = graph.op("Transpose", graph.reshape(time, -1, BANDS, STEPS, DIM), perm=[0, 2, 1, 3])
        freq = transformer(
            graph, graph.reshape(time, -1, BANDS, DIM), f"layers.{block}.1.layers.0", BANDS
        )
        value = graph.reshape(freq, -1, STEPS, BANDS, DIM)
    output = project_masks(graph, rms(graph, value, "final_norm.gamma", DIM))
    output = graph.float32(output)
    graph.nodes.append(helper.make_node("Identity", [output], ["masks"], name="masks"))
    model_graph = helper.make_graph(
        graph.nodes,
        "bs_roformer_musdb18hq_masks",
        [helper.make_tensor_value_info("features", TensorProto.FLOAT, ["batch", STEPS, FEATURES])],
        [
            helper.make_tensor_value_info(
                "masks", TensorProto.FLOAT, ["batch", SOURCES, STEPS, FEATURES]
            )
        ],
        graph.initializers + external_weights(),
    )
    return helper.make_model(
        model_graph,
        producer_name="flitzis-looper",
        ir_version=IR_VERSION,
        opset_imports=[helper.make_opsetid("", OPSET)],
    )


def graph_bytes(*, amp: bool = False) -> bytes:
    """Serialize the exact recipe with the pinned export tool version."""
    if onnx.__version__ != ONNX_VERSION:
        msg = f"BS-RoFormer export requires onnx=={ONNX_VERSION}"
        raise RuntimeError(msg)
    return mask_model(amp=amp).SerializeToString(deterministic=True)


def verify_weights(path: Path) -> str:
    """Verify the complete effective data extent and digest."""
    with path.open("rb") as handle:
        if (
            os.fstat(handle.fileno()).st_size != WEIGHTS_BYTES
            or handle.read(len(WEIGHTS_MAGIC)) != WEIGHTS_MAGIC
        ):
            msg = "BS-RoFormer external weight extent or header differs"
            raise ValueError(msg)
        handle.seek(0)
        digest = hashlib.file_digest(handle, "sha256").hexdigest()
    if digest != WEIGHTS_SHA256:
        msg = "BS-RoFormer external weight digest differs"
        raise ValueError(msg)
    return digest


def export(checkpoint: Path, config: Path, directory: Path) -> None:
    """Create one exclusive output from the verified original release files."""
    graph = graph_bytes()
    directory.mkdir(exist_ok=False)
    data = directory / WEIGHTS_NAME
    # Keep the exact input handles through parsing. The standalone trial CLI
    # also audits its module closure, which intentionally excludes ONNX.
    with checkpoint.open("rb") as source, config.open("rb") as configuration:
        weights.verify(source, 527_385_512, weights.CHECKPOINT_SHA256)
        weights.verify(configuration, 4566, weights.CONFIG_SHA256)
        with zipfile.ZipFile(source) as archive, data.open("xb") as handle:
            weights.export_archive(archive, handle)
    digest = verify_weights(data)
    write_graph(directory / GRAPH_NAME, graph)
    sys.stdout.write(f"{WEIGHTS_NAME} bytes={WEIGHTS_BYTES} sha256={digest}\n")


def write_graph(path: Path, graph: bytes) -> None:
    """Write an exclusive topology file independently of checkpoint data."""
    with path.open("xb") as handle:
        handle.write(graph)
    sys.stdout.write(f"{path.name} bytes={len(graph)} sha256={hashlib.sha256(graph).hexdigest()}\n")


def graph_name(*, amp: bool) -> str:
    """Return the file name of the selected canonical topology."""
    return AMP_GRAPH_NAME if amp else GRAPH_NAME


def check(directory: Path, *, amp: bool = False) -> None:
    """Verify the file set and run the complete ONNX checker in a separate phase."""
    graph = graph_bytes(amp=amp)
    path = directory / graph_name(amp=amp)
    with path.open("rb") as handle:
        if handle.read(len(graph) + 1) != graph:
            msg = "BS-RoFormer graph bytes differ from the canonical export"
            raise ValueError(msg)
    digest = verify_weights(directory / WEIGHTS_NAME)
    onnx.checker.check_model(str(path.resolve(strict=True)), full_check=True)
    sys.stdout.write(
        f"ONNX checker passed graph_sha256={hashlib.sha256(graph).hexdigest()} "
        f"weights_sha256={digest}\n"
    )


def main() -> None:
    """Expose separate export and validation commands for isolated supervision."""
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    export_parser = commands.add_parser("export")
    export_parser.add_argument("checkpoint", type=Path)
    export_parser.add_argument("config", type=Path)
    export_parser.add_argument("directory", type=Path)
    check_parser = commands.add_parser("check")
    check_parser.add_argument("directory", type=Path)
    check_parser.add_argument("--amp", action="store_true")
    graph_parser = commands.add_parser("graph")
    graph_parser.add_argument("directory", type=Path)
    graph_parser.add_argument("--amp", action="store_true")
    options = parser.parse_args()
    if options.command == "check":
        check(options.directory, amp=options.amp)
    elif options.command == "graph":
        write_graph(options.directory / graph_name(amp=options.amp), graph_bytes(amp=options.amp))
    else:
        export(options.checkpoint, options.config, options.directory)


if __name__ == "__main__":
    main()
