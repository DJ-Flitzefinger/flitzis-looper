"""Fixed-geometry HTDemucs export for an admitted, disposable CPU producer."""

import argparse
import ast
import hashlib
import io
import json
import sys
from importlib import import_module, metadata, util
from pathlib import Path
from typing import TYPE_CHECKING, Any, BinaryIO

if TYPE_CHECKING:
    from collections.abc import Callable

CHECKPOINT = "8726e21a993978c7ba086d3872e7608d7d5bfca646ca4aca459ffda844faa8b4"
CHECKPOINT_BYTES = 84_141_911
CONTRACT = "htdemucs-4.0.1-955717e8-stereo-b1-v1"
LOWERING = "torchscript-opset18-dense"
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


def producer():
    """Verify this original producer and its separately pinned ONNX toolchain."""
    versions = {name: metadata.version(name) for name in ("demucs", "einops", "torch", "onnx")}
    if (
        versions != {"demucs": "4.0.1", "einops": "0.6.1", "torch": TORCH, "onnx": "1.23.2"}
        or sys.platform != "win32"
        or sys.version_info[:2] != (3, 14)
    ):
        msg = "requires the pinned Windows CP314 HTDemucs and ONNX producer"
        raise ValueError(msg)
    return versions, import_module("torch"), import_module("onnx")


def load_model(torch, stream: BinaryIO, module):
    """Load only the pinned stock class and its complete half-precision state."""
    package = torch.load(stream, map_location="cpu", weights_only=False)
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


def inspect_graph(onnx, path: Path):
    """Validate fixed named float32 tensors and forbid all external data."""
    if not 0 < path.stat().st_size <= MAX_GRAPH_BYTES:
        msg = "HTDemucs graph exceeds the inline extent"
        raise ValueError(msg)
    graph = onnx.load(path, load_external_data=False)
    check_inline(onnx, graph)
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
    return graph


def export(
    checkpoint: Path,
    mix_path: Path,
    output: Path,
    *,
    admit: Callable[[], None],
) -> dict:
    """Export inline weights and complete original references under caller admission.

    The disposable owner reserves the prospective producer working set, including
    exporter/graph copies, and applies the physical/commit and process limits
    before admitting this callback. A refusal happens before model construction.
    """
    if output.exists():
        msg = "HTDemucs output already exists"
        raise FileExistsError(msg)
    sources = source_files()
    code = core_code(sources["demucs/htdemucs.py"])
    admit()
    versions, torch, onnx = producer()
    if torch.version.git_version != TORCH_COMMIT or torch.get_num_threads() != 1:
        msg = "requires the pinned Torch build and an explicit serial CPU trial"
        raise ValueError(msg)
    module = import_module("demucs.htdemucs")
    with checkpoint.open("rb") as stream, io.BytesIO(stream.read(CHECKPOINT_BYTES + 1)) as owned:
        # Pin an immutable snapshot before the class-bearing pickle is parsed.
        # The independent producer admission includes this additional owner.
        check_checkpoint(owned)
        model = load_model(torch, owned, module)
    output.mkdir(exist_ok=False)
    graph = output / "htdemucs.onnx"
    mix_digest = execute(torch, model, code, vars(module), mix_path, graph)
    del model
    checked = inspect_graph(onnx, graph)
    onnx.helper.set_model_props(
        checked,
        {
            "flitzis_htdemucs_contract": CONTRACT,
            "checkpoint_sha256": CHECKPOINT,
            "torch_version": TORCH,
            "torch_commit": TORCH_COMMIT,
            "source_closure_sha256": source_digest(),
            "lowering": LOWERING,
            "sources": ",".join(SOURCES),
        },
    )
    onnx.save_model(checked, graph, save_as_external_data=False)
    del checked
    onnx.checker.check_model(graph, full_check=True)
    if graph.stat().st_size > MAX_GRAPH_BYTES or source_files() != sources:
        msg = "HTDemucs source closure or final graph extent changed"
        raise ValueError(msg)
    artifacts = {}
    for path in sorted(output.iterdir()):
        with path.open("rb") as stream:
            artifacts[path.name] = {
                "bytes": path.stat().st_size,
                "sha256": hashlib.file_digest(stream, "sha256").hexdigest(),
            }
    receipt = {
        "contract": CONTRACT,
        "checkpoint_sha256": CHECKPOINT,
        "versions": versions,
        "sources": SOURCE_PINS,
        "source_closure_sha256": source_digest(),
        "lowering": LOWERING,
        "mix_sha256": mix_digest,
        "shapes": SHAPES,
        "artifacts": artifacts,
    }
    (output / "export.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    return receipt


def execute(torch, model, code, globals_, mix_path: Path, graph: Path) -> str:
    """Keep the original reference path separate from export-only tracing."""
    numpy = import_module("numpy")
    extent = 2 * 343980 * 4
    with mix_path.open("rb") as stream:
        raw = stream.read(extent + 1)
    if len(raw) != extent:
        msg = "expected complete release-sized stereo input"
        raise ValueError(msg)
    digest = hashlib.sha256(raw).hexdigest()
    array = numpy.frombuffer(raw, dtype="<f4").copy()
    del raw
    if not numpy.isfinite(array).all():
        msg = "nonfinite HTDemucs input waveform"
        raise ValueError(msg)
    mix = torch.from_numpy(array.reshape(SHAPES["waveform"]))
    namespace = {}
    exec(code, globals_, namespace)  # Executed source has an exact pinned digest.
    normalized = namespace["normalized"]

    class Core(torch.nn.Module):
        def __init__(self, original):
            super().__init__()
            self.model = original

        def forward(self, spectral, waveform):
            return normalized(self.model, spectral, waveform)

    with torch.inference_mode():
        branches, pcm = original_forward(model, mix)
        inputs = (branches["spectral"], branches["waveform"])
        wrapper = Core(model).eval()
        for name, value in zip(tuple(SHAPES)[2:], wrapper(*inputs), strict=True):
            torch.testing.assert_close(value, branches[name], atol=0, rtol=0)
        for name, value in {**branches, "original_pcm": pcm}.items():
            if not torch.isfinite(value).all():
                msg = f"nonfinite original HTDemucs {name}"
                raise ValueError(msg)
            value.numpy().astype("<f4", copy=False).tofile(graph.with_name(f"{name}.f32"))
        del branches, pcm, value
        # Stock eval's fused MHA is unavailable to the legacy ONNX translator.
        # Only export tracing changes dispatch. The complete references above
        # use the original model. Provider arithmetic needs its own comparison.
        previous = torch.backends.mha.get_fastpath_enabled()
        try:
            torch.backends.mha.set_fastpath_enabled(False)
            torch.onnx.export(
                wrapper,
                inputs,
                graph,
                dynamo=False,
                opset_version=18,
                input_names=list(SHAPES)[:2],
                output_names=list(SHAPES)[2:],
                external_data=False,
            )
        finally:
            torch.backends.mha.set_fastpath_enabled(previous)
    return digest


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
