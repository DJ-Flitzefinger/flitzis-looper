"""Fixed-geometry HTDemucs export for an admitted, disposable CPU producer."""

import argparse
import ast
import hashlib
import io
import json
import platform
import sys
import sysconfig
from collections import Counter
from contextlib import contextmanager
from dataclasses import dataclass
from enum import Enum
from importlib import import_module, metadata, util
from math import prod
from pathlib import Path
from threading import Lock
from typing import TYPE_CHECKING, Any, BinaryIO

if TYPE_CHECKING:
    from collections.abc import Callable

CHECKPOINT = "8726e21a993978c7ba086d3872e7608d7d5bfca646ca4aca459ffda844faa8b4"
CHECKPOINT_BYTES = 84_141_911
CONTRACT = "htdemucs-4.0.1-955717e8-stereo-b1-v1"
BLOCK = 64
HEADS = 8
WIDTH = 64
TIME_TOKENS = 1344
SPECTRAL_TOKENS = 2688
ATTENTION_SITES = {
    (SPECTRAL_TOKENS, SPECTRAL_TOKENS): 3,
    (TIME_TOKENS, TIME_TOKENS): 3,
    (SPECTRAL_TOKENS, TIME_TOKENS): 2,
    (TIME_TOKENS, SPECTRAL_TOKENS): 2,
}
LOWERING = "torchscript-opset18-scan64"
MARKER_DOMAIN = "flitzis.htdemucs.export"
SYMBOLIC = "aten::scaled_dot_product_attention"
SYMBOLIC_SHA256 = "0b1102d89850cb6e1eff667b5500cc0bea0d2a6c83051b91838acdc117aabc6b"
TRACE_SOURCE_PINS = {
    "utils": "2a01910c4972939180bdd75ff264ba7105f7360e8a47c3328e566cfe4100b649",
    "onnx_proto_utils": "9da95f9437a5d1e5bad430b0a8c5d060d6ae26d20bb5b61d56537542c2c97efa",
    "symbolic_opset14": SYMBOLIC_SHA256,
}
TORCH = "2.12.0+cu130"
TORCH_COMMIT = "7661cd9c6b841b62b7f411aa52ec51f05457263b"
SOURCES = ("drums", "bass", "other", "vocals")
SHAPES = {
    "spectral": (1, 2, 2, 2048, 336),
    "waveform": (1, 2, 343980),
    "decoded_spectral": (1, 4, 2, 2, 2048, 336),
    "decoded_waveform": (1, 4, 2, 343980),
}
MAX_GRAPH_BYTES = 192 * 1024 * 1024
PCM_SHAPE = SHAPES["decoded_waveform"]
ONNX_PINS = {
    "onnx": (
        "1.23.2",
        "onnx.onnx_cpp2py_export",
        "6489f9fa4e03a5d2d82814568c3a22ff759153ea80012fe865ea089ac2cec7ca",
    ),
    "protobuf": (
        "6.33.6",
        "google._upb._message",
        "72970bb66676a4e34ba5fe6ffd8f853b78ea1ef7e4ad1082bb80287a78d47923",
    ),
    "ml-dtypes": (
        "0.5.4",
        "ml_dtypes._ml_dtypes_ext",
        "716d223a11cbd4eefe1d2798610fd55a7a69f7b0c7a752bf3cb7dc27be56ba93",
    ),
    "numpy": (
        "2.4.0",
        "numpy._core._multiarray_umath",
        "17ffaa9a6b9dcc1c84b7d9455c5fc3b463b525a10245b305928e7830e8117892",
    ),
    "typing-extensions": (
        "4.15.0",
        "typing_extensions",
        "433d11d170d3a24d2eb065ebc1bfe848cea7e3d7ce68567ab52bea2d4c2f7ed8",
    ),
}
SOURCE_PINS = {
    "demucs/__init__.py": "2eeae8de4a5f91ba7b7e91cb7c2460787c10afc17e720b8773cd66e8e5a10e26",
    "demucs/htdemucs.py": "3404ed68d35b946f9c54b54201851bb4171dd7861a498945ce53b1df1144b034",
    "demucs/hdemucs.py": "5b98a5ee262bf6b872a0db23eeb60d024debc4be1ed237c551284af305343864",
    "demucs/demucs.py": "4ab138b111d98f0a6ad6ed0c8b8c2f42578fd187c6890104b2251089a74f0d2d",
    "demucs/transformer.py": "d99b3706f590397382abed1da384c539805c8fc3a0abb6f6ac79e711649c6f8d",
    "demucs/states.py": "37375543dad61a7dc549caf6f165c0500d903313159c70cf893d47718194b865",
    "demucs/spec.py": "8a63153a012703fd5ed2e3d012e91ad624597c7296e9d82f40a26c4249f2bff7",
    "demucs/utils.py": "be4a0005ed5ec2f23628c84be39082f5c3c0f6775049b982e24f2852ae0dcf5f",
    "demucs/remote/htdemucs.yaml": (
        "239c445d0b14454d541ad8bd9bb271c9e536d267e8a4625208744cbb2e7bb66c"
    ),
    "einops/__init__.py": "10cff3eccc32d0d71cbb482774fcb7f50d87483ad98e5b76b5afc4c0c69e990d",
    "einops/einops.py": "6d5af8549c684fb0a4b2261e8207a18ffcb638102f7e2f52a4d1e101f0f722a6",
    "einops/_backends.py": "78ad88d8478d2f33500956723f2771cefbe9f636f57c6a070c3fc8bd3024d664",
    "einops/parsing.py": "0a1474b0a066376cfefa5c84a59bc4c4dee065d8d3826e15ddc7ab6b461fe003",
}


class Phase(Enum):
    """Independent producer owners for one fixed-release export."""

    REFERENCE = "reference"
    TRACE = "trace"
    REWRITE = "rewrite"
    CHECK = "check"


_phase_lock = Lock()
_started_phases: set[Phase] = set()


def begin_phase(phase: Phase, admit: Callable[[Phase], None]) -> None:
    """Check admission before any heavyweight import and consume this child."""
    with _phase_lock:
        if _started_phases or "torch" in sys.modules or "onnx" in sys.modules:
            msg = "HTDemucs producer phase requires a fresh process"
            raise ValueError(msg)
        _started_phases.add(phase)
    admitted = False
    try:
        admit(phase)
        admitted = True
    finally:
        if not admitted:
            with _phase_lock:
                _started_phases.remove(phase)


@dataclass(frozen=True, slots=True)
class Artifact:
    """Extent and SHA-256 of the bytes accepted from a retired stage."""

    size: int
    sha256: str

    def __post_init__(self) -> None:
        if type(self.size) is not int or self.size <= 0:
            msg = "invalid HTDemucs artifact extent"
            raise ValueError(msg)
        if len(self.sha256) != 64 or any(char not in "0123456789abcdef" for char in self.sha256):
            msg = "invalid HTDemucs artifact digest"
            raise ValueError(msg)


@dataclass(frozen=True, slots=True)
class ReferenceArtifacts:
    """Immutable identities from one complete original forward."""

    mix: Artifact
    spectral: Artifact
    waveform: Artifact
    decoded_spectral: Artifact
    decoded_waveform: Artifact
    original_pcm: Artifact

    def __post_init__(self) -> None:
        shapes = {**SHAPES, "mix": SHAPES["waveform"], "original_pcm": PCM_SHAPE}
        for name, shape in shapes.items():
            if getattr(self, name).size != prod(shape) * 4:
                msg = f"unexpected HTDemucs reference extent: {name}"
                raise ValueError(msg)


@dataclass(frozen=True, slots=True)
class TraceArtifact:
    """Legacy trace bytes bound to their original reference inputs and outputs."""

    graph: Artifact
    reference: ReferenceArtifacts


def read_artifact(path: Path, expected: Artifact, limit: int) -> bytes:
    """Read at most the admitted extent plus one and hash the bytes used."""
    if expected.size > limit:
        msg = "invalid HTDemucs artifact extent"
        raise ValueError(msg)
    with path.open("rb") as stream:
        raw = stream.read(expected.size + 1)
    if len(raw) != expected.size or hashlib.sha256(raw).hexdigest() != expected.sha256:
        msg = f"HTDemucs artifact differs: {path.name}"
        raise ValueError(msg)
    return raw


def describe_artifact(path: Path, limit: int) -> Artifact:
    """Record a completed regular file by streaming a bounded extent."""
    with path.open("rb") as stream:
        size = stream.seek(0, 2)
        if not 0 < size <= limit:
            msg = f"invalid HTDemucs artifact extent: {path.name}"
            raise ValueError(msg)
        stream.seek(0)
        digest = hashlib.sha256()
        remaining = size
        while remaining:
            chunk = stream.read(min(remaining, 256 * 1024))
            if not chunk:
                msg = f"truncated HTDemucs artifact: {path.name}"
                raise ValueError(msg)
            digest.update(chunk)
            remaining -= len(chunk)
        if stream.read(1):
            msg = f"extended HTDemucs artifact: {path.name}"
            raise ValueError(msg)
    return Artifact(size, digest.hexdigest())


def source_digest() -> str:
    """Return the canonical identity recorded in the graph and export receipt."""
    return hashlib.sha256(
        json.dumps(SOURCE_PINS, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


def source_files() -> dict[str, bytes]:
    """Read and verify the exact local Python sources used by the core."""
    roots = {}
    for name in ("demucs", "einops"):
        spec = util.find_spec(name)
        if spec is None or spec.origin is None:
            msg = f"missing pinned {name} producer"
            raise ValueError(msg)
        roots[name] = Path(spec.origin).resolve(strict=True).parent
    sources = {}
    for name, expected in SOURCE_PINS.items():
        package, relative = name.split("/", 1)
        raw = (roots[package] / relative).read_bytes()
        if hashlib.sha256(raw).hexdigest() != expected:
            msg = f"pinned producer source differs: {name}"
            raise ValueError(msg)
        sources[name] = raw
    return sources


def check_checkpoint(stream: BinaryIO) -> None:
    """Check all bytes and rewind the same handle before any pickle load."""
    stream.seek(0, 2)
    if stream.tell() != CHECKPOINT_BYTES:
        msg = "unexpected HTDemucs checkpoint size"
        raise ValueError(msg)
    stream.seek(0)
    if hashlib.file_digest(stream, "sha256").hexdigest() != CHECKPOINT:
        msg = "unexpected HTDemucs checkpoint digest"
        raise ValueError(msg)
    stream.seek(0)


def core_code(source: bytes) -> Any:
    """Extract the learned statements between normalization and raw restoration."""
    model = next(
        node
        for node in ast.parse(source).body
        if isinstance(node, ast.ClassDef) and node.name == "HTDemucs"
    )
    forward = next(
        node for node in model.body if isinstance(node, ast.FunctionDef) and node.name == "forward"
    )
    start = next(i for i, node in enumerate(forward.body) if ast.unparse(node) == "saved = []")
    stop = next(
        i
        for i, node in enumerate(forward.body)
        if ast.unparse(node) == "x = x * std[:, None] + mean[:, None]"
    )
    prefix = ast.parse(
        "def normalized(self, spectral, xt):\n"
        "    x = spectral.reshape(1, 4, 2048, 336)\n"
        "    B, C, Fq, T = x.shape\n"
    )
    function = prefix.body[0]
    function.body.extend(forward.body[start:stop])
    function.body.extend(
        ast.parse("return x.reshape(1, 4, 2, 2, 2048, 336), xt.reshape(1, 4, 2, 343980)").body
    )
    return compile(
        ast.fix_missing_locations(prefix), "demucs/htdemucs.py (normalized core)", "exec"
    )


def check_platform() -> None:
    if (
        sys.platform != "win32"
        or sys.version_info[:2] != (3, 14)
        or platform.machine() != "AMD64"
        or sysconfig.get_config_var("Py_GIL_DISABLED")
    ):
        msg = "requires the pinned Windows CP314 HTDemucs producer"
        raise ValueError(msg)


def torch_producer():
    """Verify the Torch/Demucs producer without importing ONNX."""
    check_platform()
    versions = {"demucs": "4.0.1", "einops": "0.6.1", "torch": TORCH, "numpy": "2.4.0"}
    for name, version in versions.items():
        if metadata.version(name) != version:
            msg = f"unexpected HTDemucs producer package: {name}"
            raise ValueError(msg)
    torch = import_module("torch")
    if torch.version.git_version != TORCH_COMMIT:
        msg = "requires the pinned Torch build"
        raise ValueError(msg)
    torch.set_num_threads(1)
    torch.set_num_interop_threads(1)
    if torch.get_num_threads() != 1 or torch.get_num_interop_threads() != 1:
        msg = "requires the pinned Torch build and explicit serial CPU dispatch"
        raise ValueError(msg)
    return torch


def onnx_producer(overlay: Path):
    """Verify the checked export-only overlay already on the isolated import path."""
    check_platform()
    overlay = overlay.resolve(strict=True)
    donor = (Path(sys.prefix) / "Lib/site-packages").resolve(strict=True)
    for name, (version, module, expected) in ONNX_PINS.items():
        if metadata.version(name) != version:
            msg = f"unexpected HTDemucs export package: {name}"
            raise ValueError(msg)
        path = Path(import_module(module).__file__).resolve(strict=True)
        root = donor if name in {"numpy", "typing-extensions"} else overlay
        with path.open("rb") as stream:
            if (
                not path.is_relative_to(root)
                or hashlib.file_digest(stream, "sha256").hexdigest() != expected
            ):
                msg = f"unexpected HTDemucs export module: {name}"
                raise ValueError(msg)
    if import_module("google.protobuf.internal.api_implementation").Type() != "upb":
        msg = "requires the pinned ONNX protobuf implementation"
        raise ValueError(msg)
    return import_module("onnx")


def load_model(torch, checkpoint: Path, module):
    """Load only the pinned stock class and its complete half-precision state."""
    with io.BytesIO(
        read_artifact(checkpoint, Artifact(CHECKPOINT_BYTES, CHECKPOINT), CHECKPOINT_BYTES)
    ) as snapshot:
        package = torch.load(snapshot, map_location="cpu", weights_only=False)
    if package["klass"] is not module.HTDemucs or package["args"] or len(package["state"]) != 533:
        msg = "unexpected original HTDemucs checkpoint structure"
        raise ValueError(msg)
    model = import_module("demucs.states").load_model(package, strict=True).eval()
    geometry = (
        tuple(model.sources),
        model.audio_channels,
        model.samplerate,
        model.nfft,
        model.cac,
        model.use_train_segment,
        model.valid_length(343980),
        model.bottom_channels,
    )
    if geometry != (SOURCES, 2, 44100, 4096, True, True, 343980, 512):
        msg = "unexpected original HTDemucs release geometry"
        raise ValueError(msg)
    return model


def original_forward(model, mix):
    """Capture exact original normalized inputs, both raw decoder outputs and PCM."""
    captured = {}
    hooks = [
        model.encoder[0].register_forward_pre_hook(
            lambda _, args: captured.update(spectral=args[0].detach())
        ),
        model.tencoder[0].register_forward_pre_hook(
            lambda _, args: captured.update(waveform=args[0].detach())
        ),
        model.decoder[-1].register_forward_hook(
            lambda _, _args, result: captured.update(decoded_spectral=result[0].detach())
        ),
        model.tdecoder[-1].register_forward_hook(
            lambda _, _args, result: captured.update(decoded_waveform=result[0].detach())
        ),
    ]
    try:
        pcm = model(mix)
    finally:
        for hook in hooks:
            hook.remove()
    return {name: captured[name].reshape(shape) for name, shape in SHAPES.items()}, pcm


def check_inline(onnx, graph) -> None:
    """Check nested tensors as well as ordinary initializers."""
    pending = [graph]
    while pending:
        message = pending.pop()
        if isinstance(message, onnx.TensorProto) and (
            message.external_data or message.data_location == onnx.TensorProto.EXTERNAL
        ):
            msg = "HTDemucs graph references external data"
            raise ValueError(msg)
        for field, value in message.ListFields():
            if field.type == field.TYPE_MESSAGE:
                pending.extend(value if field.is_repeated else (value,))


@contextmanager
def mark_attention(torch):
    """Mark only the original unmasked, dropout-free legacy SDPA export."""
    exporter = "torch.onnx._internal.torchscript_exporter"
    registration = import_module(f"{exporter}.registration")
    symbolic_helper = import_module(f"{exporter}.symbolic_helper")
    symbolic_opset14 = import_module(f"{exporter}.symbolic_opset14")

    for name, expected in TRACE_SOURCE_PINS.items():
        module = import_module(f"{exporter}.{name}")
        if hashlib.sha256(Path(module.__file__).read_bytes()).hexdigest() != expected:
            msg = f"HTDemucs Torch legacy exporter source differs: {name}"
            raise ValueError(msg)
    group = registration.registry.get_function_group(SYMBOLIC)
    if group is None or group.get(18) is not symbolic_opset14.scaled_dot_product_attention:
        msg = "HTDemucs Torch legacy attention symbolic is overridden"
        raise ValueError(msg)

    @symbolic_helper.parse_args("v", "v", "v", "v", "f", "b", "v", "b")
    def symbolic(g, query, key, value, mask, dropout, causal, scale, gqa):
        if (
            not symbolic_helper._is_none(mask)
            or dropout != 0
            or causal
            or not symbolic_helper._is_none(scale)
            or gqa
        ):
            msg = "unsupported HTDemucs attention options"
            raise ValueError(msg)
        inputs = [g.original_node.inputsAt(index).type() for index in range(3)]
        shapes = [tensor.sizes() for tensor in inputs]
        if any(
            tensor.scalarType() != "Float"
            or shape is None
            or len(shape) != 4
            or shape[:2] != [1, HEADS]
            or shape[3] != WIDTH
            for tensor, shape in zip(inputs, shapes, strict=True)
        ):
            msg = "unsupported HTDemucs attention tensor geometry"
            raise ValueError(msg)
        queries, keys, values = (shape[2] for shape in shapes)
        if (queries, keys) not in ATTENTION_SITES or keys != values:
            msg = "unsupported HTDemucs attention sequence lengths"
            raise ValueError(msg)
        # Match the exact legacy symbolic's separate Q and K pre-scaling.
        root_scale = g.op("Sqrt", symbolic_opset14._attention_scale(g, query))
        q = g.op("Mul", query, root_scale)
        k = g.op("Mul", g.op("Transpose", key, perm_i=[0, 1, 3, 2]), root_scale)
        return g.op(
            f"{MARKER_DOMAIN}::Attention",
            q,
            k,
            value,
            queries_i=queries,
            keys_i=keys,
        ).setType(query.type())

    torch.onnx.register_custom_op_symbolic(SYMBOLIC, symbolic, 18)
    try:
        yield
    finally:
        torch.onnx.unregister_custom_op_symbolic(SYMBOLIC, 18)


def lower_attention(onnx, model) -> None:
    """Replace the exact release's marked SDPA sites with sequential standard Scan."""
    graph = model.graph
    imports = [(entry.domain, entry.version) for entry in model.opset_import]
    if set(imports) != {("", 18), (MARKER_DOMAIN, 1)} or len(imports) != 2:
        msg = "unexpected HTDemucs export marker opsets"
        raise ValueError(msg)

    # Include existing node/initializer names and captures in the fresh namespace.
    names = {
        value.name for value in (*graph.input, *graph.output, *graph.value_info, *graph.initializer)
    }
    for node in graph.node:
        names.update((*node.input, *node.output))
        if any(
            attr.type in {onnx.AttributeProto.GRAPH, onnx.AttributeProto.GRAPHS}
            for attr in node.attribute
        ):
            msg = "unexpected subgraph in the HTDemucs trace"
            raise ValueError(msg)

    lowered = []
    sites = Counter()
    for node in graph.node:
        if node.domain != MARKER_DOMAIN:
            if node.op_type in {"Attention", "Scan", "Softmax"}:
                msg = "unexpected unmarked HTDemucs attention operator"
                raise ValueError(msg)
            lowered.append(node)
            continue
        dims = {
            attr.name: attr.i for attr in node.attribute if attr.type == onnx.AttributeProto.INT
        }
        pair = (dims.get("queries"), dims.get("keys"))
        if (
            (node.op_type, len(node.input), len(node.output), len(node.attribute))
            != ("Attention", 3, 1, 2)
            or not all((*node.input, *node.output))
            or pair not in ATTENTION_SITES
        ):
            msg = "invalid HTDemucs attention export marker"
            raise ValueError(msg)
        prefix = f"{node.output[0]}.scan64."
        if any(name.startswith(prefix) for name in names):
            msg = "HTDemucs attention name collision"
            raise ValueError(msg)
        sites[pair] += 1
        lowered.extend(scan_nodes(onnx, prefix, node.input, node.output[0], pair[0]))

    if sites != ATTENTION_SITES:
        msg = f"unexpected HTDemucs attention site counts: {dict(sites)}"
        raise ValueError(msg)
    del graph.node[:]
    graph.node.extend(lowered)
    del model.opset_import[:]
    model.opset_import.add(domain="", version=18)


def scan_nodes(onnx, prefix, inputs, output, queries):
    """Scan contiguous query blocks, capturing the complete scaled K and V."""
    h = onnx.helper
    q, k, v = inputs
    tile, scores, weights, context = (
        prefix + name for name in ("tile", "scores", "weights", "context")
    )
    tile_shape = [1, HEADS, BLOCK, WIDTH]
    body = h.make_graph(
        [
            h.make_node("MatMul", [tile, k], [scores]),
            h.make_node("Softmax", [scores], [weights], axis=-1),
            h.make_node("MatMul", [weights, v], [context]),
        ],
        prefix + "body",
        [h.make_tensor_value_info(tile, onnx.TensorProto.FLOAT, tile_shape)],
        [h.make_tensor_value_info(context, onnx.TensorProto.FLOAT, tile_shape)],
    )
    blocked, ordered, scanned, merged = (
        prefix + name for name in ("blocked", "ordered", "scanned", "merged")
    )
    return [
        h.make_node(
            "Constant",
            [],
            [prefix + "block_shape"],
            value=h.make_tensor(
                "", onnx.TensorProto.INT64, [5], [1, HEADS, queries // BLOCK, BLOCK, WIDTH]
            ),
        ),
        h.make_node("Reshape", [q, prefix + "block_shape"], [blocked]),
        h.make_node("Transpose", [blocked], [ordered], perm=[2, 0, 1, 3, 4]),
        h.make_node("Scan", [ordered], [scanned], num_scan_inputs=1, body=body),
        h.make_node("Transpose", [scanned], [merged], perm=[1, 2, 0, 3, 4]),
        h.make_node(
            "Constant",
            [],
            [prefix + "result_shape"],
            value=h.make_tensor("", onnx.TensorProto.INT64, [4], [1, HEADS, queries, WIDTH]),
        ),
        h.make_node("Reshape", [merged, prefix + "result_shape"], [output]),
    ]


def scan64_digest(onnx, graph) -> str:
    """Attest each complete Scan64 chain in the independently parsed final graph."""
    nodes = graph.graph.node
    queries = Counter()
    digest = hashlib.sha256()
    for index, node in enumerate(nodes):
        if node.op_type == "Scan":
            count, canonical = _checked_scan64(onnx, nodes, index)
            queries[count] += 1
            for member in canonical:
                raw = member.SerializeToString(deterministic=True)
                digest.update(len(raw).to_bytes(8, "little"))
                digest.update(raw)
        elif node.op_type in {"Attention", "Softmax"} or any(
            attr.type in {onnx.AttributeProto.GRAPH, onnx.AttributeProto.GRAPHS}
            for attr in node.attribute
        ):
            msg = "HTDemucs has attention or subgraphs outside its ten Scan64 chains"
            raise ValueError(msg)
    if queries != {SPECTRAL_TOKENS: 5, TIME_TOKENS: 5}:
        msg = "HTDemucs Scan64 query-site counts differ from the release"
        raise ValueError(msg)
    return digest.hexdigest()


def _checked_scan64(onnx, nodes, index: int):
    if index < 3 or index + 4 > len(nodes):
        msg = "HTDemucs has an incomplete Scan64 chain"
        raise ValueError(msg)
    chain = nodes[index - 3 : index + 4]
    try:
        prefix = chain[0].output[0].removesuffix("block_shape")
        count = chain[0].attribute[0].t.int64_data[2] * BLOCK
        body = next(attr.g for attr in chain[3].attribute if attr.name == "body")
        k, v = body.node[0].input[1], body.node[2].input[1]
        q, output = chain[1].input[0], chain[-1].output[0]
    except (IndexError, StopIteration) as err:
        msg = "HTDemucs Scan64 body or geometry is missing"
        raise ValueError(msg) from err
    if count not in {SPECTRAL_TOKENS, TIME_TOKENS} or prefix != f"{output}.scan64.":
        msg = "HTDemucs Scan64 prefix or query extent differs"
        raise ValueError(msg)
    canonical = scan_nodes(onnx, prefix, (q, k, v), output, count)
    if chain != canonical:
        msg = "HTDemucs Scan64 chain differs from the canonical lowering"
        raise ValueError(msg)
    # Key lengths and all ten Q/K pairs are checked on the pinned trace
    # markers by lower_attention, before these exact bytes are produced.
    return count, canonical


def parse_inline_graph(onnx, raw: bytes):
    """Parse exact immutable bytes without resolving any external data."""
    graph = onnx.load_model_from_string(raw)
    check_inline(onnx, graph)
    return graph


def inspect_graph(onnx, graph) -> None:
    """Validate the final fixed named float32 tensors and standard opset."""
    if (
        graph.functions
        or any(node.domain not in {"", "ai.onnx"} for node in graph.graph.node)
        or [(entry.domain, entry.version) for entry in graph.opset_import] != [("", 18)]
    ):
        msg = "unsupported HTDemucs ONNX domains or opset"
        raise ValueError(msg)
    for actual, expected in (
        (graph.graph.input, tuple(SHAPES)[:2]),
        (graph.graph.output, tuple(SHAPES)[2:]),
    ):
        if [value.name for value in actual] != list(expected):
            msg = "unexpected HTDemucs ONNX tensor names"
            raise ValueError(msg)
        for value in actual:
            tensor = value.type.tensor_type
            if (
                tensor.elem_type != onnx.TensorProto.FLOAT
                or tuple(dim.dim_value for dim in tensor.shape.dim) != SHAPES[value.name]
            ):
                msg = f"unexpected HTDemucs ONNX signature: {value.name}"
                raise ValueError(msg)


def read_tensor(torch, path: Path, artifact: Artifact, shape: tuple[int, ...]):
    extent = prod(shape) * 4
    if artifact.size != extent:
        msg = f"unexpected HTDemucs tensor extent: {path.name}"
        raise ValueError(msg)
    raw = read_artifact(path, artifact, extent)
    numpy = import_module("numpy")
    array = numpy.frombuffer(raw, dtype="<f4").reshape(shape).copy()
    del raw
    if not numpy.isfinite(array).all():
        msg = f"nonfinite HTDemucs tensor: {path.name}"
        raise ValueError(msg)
    return torch.from_numpy(array)


def write_tensor(torch, path: Path, value, shape: tuple[int, ...]) -> Artifact:
    if (
        tuple(value.shape) != shape
        or value.dtype != torch.float32
        or not torch.isfinite(value).all()
    ):
        msg = f"invalid original HTDemucs tensor: {path.name}"
        raise ValueError(msg)
    extent = prod(shape) * 4
    with path.open("xb") as stream:
        value.numpy().astype("<f4", copy=False).tofile(stream)
    artifact = describe_artifact(path, extent)
    if artifact.size != extent:
        msg = f"incomplete original HTDemucs tensor: {path.name}"
        raise ValueError(msg)
    return artifact


def core_wrapper(torch, model, source: bytes, globals_):
    namespace = {}
    exec(core_code(source), globals_, namespace)  # The caller has verified the source bytes.
    normalized = namespace["normalized"]

    class Core(torch.nn.Module):
        def __init__(self, original):
            super().__init__()
            self.model = original

        def forward(self, spectral, waveform):
            return normalized(self.model, spectral, waveform)

    return Core(model).eval()


def capture_reference(
    checkpoint: Path,
    mix_path: Path,
    mix: Artifact,
    output: Path,
    *,
    admit: Callable[[Phase], None],
) -> ReferenceArtifacts:
    """Run the stock forward and check the extracted boundary in an admitted child.

    The caller owns admission from startup through process, file and pipe reader
    retirement and accepts these identities only after successful retirement.
    """
    begin_phase(Phase.REFERENCE, admit)
    sources = source_files()
    output.mkdir(exist_ok=False)
    torch = torch_producer()
    module = import_module("demucs.htdemucs")
    waveform = read_tensor(torch, mix_path, mix, SHAPES["waveform"])
    model = load_model(torch, checkpoint, module)
    wrapper = core_wrapper(torch, model, sources["demucs/htdemucs.py"], vars(module))
    with torch.inference_mode():
        branches, pcm = original_forward(model, waveform)
        inputs = (branches["spectral"], branches["waveform"])
        for name, value in zip(tuple(SHAPES)[2:], wrapper(*inputs), strict=True):
            torch.testing.assert_close(value, branches[name], atol=0, rtol=0)
        artifacts = {
            name: write_tensor(torch, output / f"{name}.f32", branches[name], shape)
            for name, shape in SHAPES.items()
        }
        original_pcm = write_tensor(torch, output / "original_pcm.f32", pcm, PCM_SHAPE)
    if source_files() != sources:
        msg = "HTDemucs source closure changed during reference capture"
        raise ValueError(msg)
    return ReferenceArtifacts(mix=mix, original_pcm=original_pcm, **artifacts)


def trace_core(
    checkpoint: Path,
    references: Path,
    reference: ReferenceArtifacts,
    output: Path,
    *,
    overlay: Path,
    admit: Callable[[Phase], None],
) -> TraceArtifact:
    """Trace only the learned core, in a fresh admitted child using captured inputs."""
    begin_phase(Phase.TRACE, admit)
    sources = source_files()
    torch = torch_producer()
    # Legacy torch.onnx.export uses this parser internally while Torch is live.
    onnx_producer(overlay)
    module = import_module("demucs.htdemucs")
    inputs = (
        read_tensor(torch, references / "spectral.f32", reference.spectral, SHAPES["spectral"]),
        read_tensor(torch, references / "waveform.f32", reference.waveform, SHAPES["waveform"]),
    )
    with output.open("xb") as stream:
        model = load_model(torch, checkpoint, module)
        wrapper = core_wrapper(torch, model, sources["demucs/htdemucs.py"], vars(module))
        # Stock eval's fused MHA has no legacy ONNX translation. Only trace changes dispatch.
        previous = torch.backends.mha.get_fastpath_enabled()
        try:
            torch.backends.mha.set_fastpath_enabled(False)
            with torch.inference_mode(), mark_attention(torch):
                torch.onnx.export(
                    wrapper,
                    inputs,
                    stream,
                    dynamo=False,
                    opset_version=18,
                    input_names=list(SHAPES)[:2],
                    output_names=list(SHAPES)[2:],
                    custom_opsets={MARKER_DOMAIN: 1},
                    external_data=False,
                )
        finally:
            torch.backends.mha.set_fastpath_enabled(previous)
    if source_files() != sources:
        msg = "HTDemucs source closure changed during tracing"
        raise ValueError(msg)
    return TraceArtifact(describe_artifact(output, MAX_GRAPH_BYTES), reference)


def graph_metadata(trace: TraceArtifact, attention_sha256: str) -> dict[str, str]:
    """Bind the graph to the release and the upstream original references."""
    reference = trace.reference
    return {
        "flitzis_htdemucs_contract": CONTRACT,
        "checkpoint_sha256": CHECKPOINT,
        "torch_version": TORCH,
        "torch_commit": TORCH_COMMIT,
        "source_closure_sha256": source_digest(),
        "lowering": LOWERING,
        "attention_query_block": str(BLOCK),
        "attention_sha256": attention_sha256,
        "sources": ",".join(SOURCES),
        "trace_sha256": trace.graph.sha256,
        "mix_sha256": reference.mix.sha256,
        **{f"{name}_sha256": getattr(reference, name).sha256 for name in (*SHAPES, "original_pcm")},
    }


def rewrite_graph(
    trace_path: Path,
    trace: TraceArtifact,
    output: Path,
    *,
    overlay: Path,
    admit: Callable[[Phase], None],
) -> Artifact:
    """Lower the checked inline trace in an admitted ONNX-only child."""
    begin_phase(Phase.REWRITE, admit)
    onnx = onnx_producer(overlay)
    graph = parse_inline_graph(onnx, read_artifact(trace_path, trace.graph, MAX_GRAPH_BYTES))
    lower_attention(onnx, graph)
    inspect_graph(onnx, graph)
    onnx.helper.set_model_props(graph, graph_metadata(trace, scan64_digest(onnx, graph)))
    serialized = graph.SerializeToString()
    if not 0 < len(serialized) <= MAX_GRAPH_BYTES:
        msg = "HTDemucs rewritten graph exceeds the inline extent"
        raise ValueError(msg)
    with output.open("xb") as stream:
        stream.write(serialized)
    return Artifact(len(serialized), hashlib.sha256(serialized).hexdigest())


def check_graph(
    path: Path,
    expected: Artifact,
    trace: TraceArtifact,
    *,
    overlay: Path,
    admit: Callable[[Phase], None],
) -> Artifact:
    """Check exactly the rewritten bytes in a fresh admitted ONNX-only child."""
    begin_phase(Phase.CHECK, admit)
    onnx = onnx_producer(overlay)
    raw = read_artifact(path, expected, MAX_GRAPH_BYTES)
    graph = parse_inline_graph(onnx, raw)
    inspect_graph(onnx, graph)
    props = [(item.key, item.value) for item in graph.metadata_props]
    if dict(props) != graph_metadata(trace, scan64_digest(onnx, graph)) or len(props) != len(
        dict(props)
    ):
        msg = "HTDemucs rewritten graph provenance differs"
        raise ValueError(msg)
    del graph
    # The bytes overload keeps custody across the C++ parser and full shape check.
    onnx.checker.check_model(raw, full_check=True)
    return expected


def main() -> None:
    """Check original pins and extract the boundary without starting a model."""
    parser = argparse.ArgumentParser(description=main.__doc__)
    parser.add_argument("--checkpoint", type=Path, required=True)
    args = parser.parse_args()
    sources = source_files()
    core_code(sources["demucs/htdemucs.py"])
    with args.checkpoint.open("rb") as stream:
        check_checkpoint(stream)
    try:
        onnx_version = metadata.version("onnx")
    except metadata.PackageNotFoundError:
        onnx_version = None
    sys.stdout.write(
        json.dumps({
            "contract": CONTRACT,
            "checkpoint": CHECKPOINT,
            "source_closure_sha256": source_digest(),
            "shapes": SHAPES,
            "onnx_version": onnx_version,
        })
        + "\n"
    )


if __name__ == "__main__":
    main()
