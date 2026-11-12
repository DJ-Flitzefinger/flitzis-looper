"""Fixed-geometry HTDemucs export for an admitted, disposable CPU producer."""

import argparse
import ast
import ctypes
import hashlib
import io
import json
import mmap
import os
import platform
import stat
import sys
import sysconfig
from collections import Counter
from contextlib import ExitStack, contextmanager
from ctypes import wintypes
from dataclasses import asdict, dataclass
from enum import Enum
from importlib import import_module, metadata, util
from itertools import pairwise
from math import prod
from pathlib import Path
from shutil import disk_usage
from struct import pack
from tempfile import TemporaryFile
from threading import Lock
from types import FunctionType, MethodType, SimpleNamespace
from typing import TYPE_CHECKING, Any, BinaryIO

if TYPE_CHECKING:
    from collections.abc import Callable

CHECKPOINT = "8726e21a993978c7ba086d3872e7608d7d5bfca646ca4aca459ffda844faa8b4"
CHECKPOINT_BYTES = 84_141_911
STATE_TENSORS = 533
STATE_HALF_BYTES = 83_968_912
STATE_BANK_BYTES = 167_939_968  # Includes per-tensor padding for stock CPU alignment.
STATE_ALIGNMENT = 64
STATE_RUNTIME_PINS = {
    "torch/lib/c10.dll": "0a8422bf6449094af91688d3452852a3e028401d69fa8e8655e44e15022db971",
    "torch/lib/torch_cpu.dll": ("de412dd735a2d880bd40f0ef88f2333af846e698e28065912026b650ae34fd1a"),
    "torch/lib/torch_python.dll": (
        "25887cacf9cf8acf6fd08dbac87b91f92e34afba6ef19eb54e297ad75ea7d572"
    ),
    "torch/serialization.py": "e035546090ba2ced9d358807dd4fabbc3dc3518ad7b6a08329c5c457addc11bd",
    "torch/nn/modules/module.py": (
        "34f90ee759ca2d26adb458c9c7d1a98fc207cc92c4d3dcbb12fa0cf7e6ac5941"
    ),
    "torchaudio/__init__.py": "fb25996556e5580d3a1d0e1c512f2c46c69266e82a69cab0d00eef4042f18d81",
    "torchaudio/lib/_torchaudio.pyd": (
        "71f8712570c11c7613e177cbb0066de002fad9ce04eca1c2970f010a7b04b22d"
    ),
    "torchaudio/lib/libtorchaudio.pyd": (
        "a8d2dd5c13d41f39a3243bdcec39006e65b4f84de3cb3aa0ecd88fd672983f87"
    ),
}
ATTENTION_RUNTIME_PINS = {
    "torch.nn.functional": "e409a97896241e0dfb8c23fbf1f09967ecf5e65ec9626aec0d97d9cc5d727d50",
    "torch._ops": "aed883c8a88051df15c7e8793f8dd1ff875e7b065c0f3ffede80113744e4104e",
    "torch.nn.modules.activation": (
        "3268980df05b7eeb49d2703f8723361823aebe6fc79e8b16c78afdacb0c38b1e"
    ),
    "torch.nn.modules.transformer": (
        "3d5b587174a642611bf84e45a879bfaf7e19ef4a5e4607a0ea89d59b817d4d79"
    ),
    "torch.utils._python_dispatch": (
        "b1d3fe4d4e0ff43ac4f8426d1b14b4b481a475d17aad073b3334f68cf58d6c5b"
    ),
}
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
    "_globals": "6125429ba650e6d122037a6bf89249be72248ddd5d9fd62c72cb2c4b555d59a6",
    "registration": "ff0a203f914e09556497ceec62c9d20935cbd574b515b22cfd7064fd78f901c1",
    "symbolic_helper": "26dbbeec9ab526916ff62543add7c88fde57a11d8f2f04546e15ce1ac061c46d",
}
TORCH = "2.12.0+cu130"
TORCH_CPU = "2.12.0+cpu"
TORCH_CPU_EXTENSION = "4015fbb9ab66c319de062ec748bababfeffa1ebaa5db22fc65aaa4faaaab250c"
TORCH_COMMIT = "7661cd9c6b841b62b7f411aa52ec51f05457263b"
SOURCES = ("drums", "bass", "other", "vocals")
SHAPES = {
    "spectral": (1, 2, 2, 2048, 336),
    "waveform": (1, 2, 343980),
    "decoded_spectral": (1, 4, 2, 2, 2048, 336),
    "decoded_waveform": (1, 4, 2, 343980),
}
MAX_GRAPH_BYTES = 192 * 1024 * 1024
MAX_SKELETON_BYTES = 8 * 1024 * 1024
MAX_MANIFEST_BYTES = 256 * 1024
STREAM_CHUNK_BYTES = 256 * 1024
# Fixed release parameters and folded tensor inputs are float32 or int64.
INITIALIZER_TYPES = {"float32": (1, 4), "int64": (7, 8)}
PCM_SHAPE = SHAPES["decoded_waveform"]
PAD2_SHAPE = (1, 4, 2, 2049, 340)
C2R_INPUT_SHAPE = (8, 340, 2049)
C2R_INPUT_BYTES = prod(C2R_INPUT_SHAPE) * 8
C2R_INPUT_GEOMETRY = (C2R_INPUT_SHAPE, (696660, 1, 340), 0, C2R_INPUT_BYTES)
C2R_PACK_SHAPE = (2720, 2049)
C2R_PACK_GEOMETRY = (C2R_PACK_SHAPE, (2049, 1), 0, C2R_INPUT_BYTES)
ISTFT_FRAME_SHAPE = (8, 340, 4096)
ISTFT_FRAME_BYTES = prod(ISTFT_FRAME_SHAPE) * 4
ISTFT_FRAME_GEOMETRY = (ISTFT_FRAME_SHAPE, (1392640, 4096, 1), 0, ISTFT_FRAME_BYTES)
WAVEFORM_CALLER_DENSE = ((1, 48, 85995), (4127760, 85995, 1), 0, 16_511_040)
WAVEFORM_SKIP_REWRITE = ((1, 96, 85995), (8255520, 85995, 1), 0, 33_022_080)
WAVEFORM_CALLER_CROP = ((1, 48, 85995), (4128000, 86000, 1), 2, 16_512_000)
SPECTRAL_SKIP_ADD_DENSE = ((1, 48, 512, 336), (8257536, 172032, 336, 1), 0, 33_030_144)
SPECTRAL_CALLER_CROP = ((1, 48, 512, 336), (8322048, 173376, 336, 1), 672, 33_288_192)
SPECTRAL_SCALED_EMBEDDING = ((1, 48, 512, 336), (16128, 336, 16128, 1), 0, 33_030_144)
SPECTRAL_REWRITE_SHAPE = (1, 96, 512, 336)
SPECTRAL_REWRITE_BYTES = prod(SPECTRAL_REWRITE_SHAPE) * 4
SPECTRAL_REWRITE_DENSE = (
    SPECTRAL_REWRITE_SHAPE,
    (16515072, 172032, 336, 1),
    0,
    SPECTRAL_REWRITE_BYTES,
)
SPECTRAL_DCONV_DENSE = ((512, 96, 1, 336), (32256, 336, 336, 1), 0, 66_060_288)
SPECTRAL_DCONV_RETURN = ((512, 96, 336), (32256, 336, 1), 0, SPECTRAL_DCONV_DENSE[3])
SPECTRAL_DCONV_GLU = ((512, 48, 336), (16128, 336, 1), 0, SPECTRAL_SKIP_ADD_DENSE[3])
SPECTRAL_DCONV_RESIDUAL = ((512, 48, 336), (336, 172032, 1), 0, SPECTRAL_SKIP_ADD_DENSE[3])
SPECTRAL_TRANSPOSE_SHAPE = (1, 16, 2052, 336)
SPECTRAL_TRANSPOSE_BYTES = prod(SPECTRAL_TRANSPOSE_SHAPE) * 4
SPECTRAL_TRANSPOSE_PUBLIC_STRIDES = (11031552, 1, 5376, 16)
SPECTRAL_TRANSPOSE_DENSE = (
    SPECTRAL_TRANSPOSE_SHAPE,
    (11031552, 689472, 336, 1),
    0,
    SPECTRAL_TRANSPOSE_BYTES,
)
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

    PREPARE_STATE = "prepare-state"
    REFERENCE = "reference"
    TRACE = "trace"
    ASSEMBLE = "assemble"
    REWRITE = "rewrite"
    CHECK = "check"


class SpectralConvolutionBackend(Enum):
    """Independently budgeted backend for a mapped spectral convolution."""

    MKLDNN = "Mkldnn"
    SLOW2D = "Slow2d"


@dataclass(frozen=True)
class SpectralConvolutionAdmission:
    """The caller's branch choices for both eager forwards."""

    rewrite: SpectralConvolutionBackend
    first_k1: SpectralConvolutionBackend
    second_k1: SpectralConvolutionBackend

    def __post_init__(self) -> None:
        if any(type(backend) is not SpectralConvolutionBackend for backend in self.backends):
            msg = "unexpected HTDemucs spectral convolution admission"
            raise ValueError(msg)

    @property
    def backends(self) -> tuple[SpectralConvolutionBackend, ...]:
        return self.rewrite, self.first_k1, self.second_k1


class CrossAttentionBackend(Enum):
    """Independently budgeted CPU SDPA route for one cross direction."""

    FLASH = 1
    MATH = 0


@dataclass(frozen=True)
class EagerAttentionAdmission:
    """The fixed native self route and caller-selected cross routes in both forwards."""

    spectral_queries: CrossAttentionBackend
    temporal_queries: CrossAttentionBackend

    def __post_init__(self) -> None:
        if any(
            type(backend) is not CrossAttentionBackend
            for backend in (self.spectral_queries, self.temporal_queries)
        ):
            msg = "unexpected HTDemucs cross-attention admission"
            raise ValueError(msg)


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
class ModelStateBank:
    """Identities returned by the retired checkpoint conversion phase."""

    manifest: Artifact
    weights: Artifact


@dataclass(frozen=True, slots=True)
class StateTensor:
    """One independently owned CPU FP32 parameter and its FP16 source extent."""

    name: str
    dtype: str
    shape: tuple[int, ...]
    stride: tuple[int, ...]
    offset: int
    size: int
    checkpoint_offset: int
    requires_grad: bool


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
    torch_version: str = TORCH


@dataclass(frozen=True, slots=True)
class SeparatedTrace:
    """Parameter-free graph and initializer custody from a retired CPU trace."""

    skeleton: Artifact
    manifest: Artifact
    weights: Artifact
    reference: ReferenceArtifacts


@dataclass(frozen=True, slots=True)
class Initializer:
    """One named, ordered slice of the separate initializer bank."""

    name: str
    dtype: str
    shape: tuple[int, ...]
    size: int
    sha256: str

    def __post_init__(self) -> None:
        Artifact(self.size, self.sha256)
        if (
            not isinstance(self.name, str)
            or not self.name
            or self.dtype not in INITIALIZER_TYPES
            or any(type(dim) is not int or dim <= 0 for dim in self.shape)
            or self.size != prod(self.shape) * INITIALIZER_TYPES[self.dtype][1]
        ):
            msg = "invalid HTDemucs initializer metadata"
            raise ValueError(msg)


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


def method_definition(
    source: bytes, class_name: str, method_name: str = "forward"
) -> ast.FunctionDef:
    model = next(
        node
        for node in ast.parse(source).body
        if isinstance(node, ast.ClassDef) and node.name == class_name
    )
    return next(
        node
        for node in model.body
        if isinstance(node, ast.FunctionDef) and node.name == method_name
    )


def release_decoder_locals(forward: ast.FunctionDef) -> None:
    """Drop the pinned decoder's caller-owned intermediates after their last use."""
    decoder = next(
        node
        for node in forward.body
        if isinstance(node, ast.For) and ast.unparse(node.iter) == "enumerate(self.decoder)"
    )
    waveform = decoder.body[-1]
    split = waveform.body[-1]
    # Only the empty waveform decoder consumes the spectral pre-transpose
    # tensor. Its sliced view must survive until that call returns.
    split.body.extend(ast.parse("del pre").body)
    split.orelse[:0] = ast.parse("del pre").body
    split.orelse.extend(ast.parse("del skip").body)
    waveform.body.extend(ast.parse("del _").body)
    waveform.orelse = ast.parse("del pre").body
    decoder.body[2:2] = ast.parse("del skip").body


def redirect_frequency_embedding_add(forward: ast.FunctionDef) -> None:
    """Change only the pinned Add after the stock embedding and scale."""
    encoder = next(
        node
        for node in forward.body
        if isinstance(node, ast.For) and ast.unparse(node.iter) == "enumerate(self.encoder)"
    )
    embedding = next(
        node
        for node in encoder.body
        if isinstance(node, ast.If)
        and ast.unparse(node.test) == "idx == 0 and self.freq_emb is not None"
    )
    addition = embedding.body[-1]
    if ast.unparse(addition) != "x = x + self.freq_emb_scale * emb":
        msg = "unexpected HTDemucs frequency embedding addition source"
        raise ValueError(msg)
    addition.value = ast.parse(
        "_mapped_embedding_add(x, self.freq_emb_scale * emb)", mode="eval"
    ).body


def core_code(
    source: bytes, *, release_decoder: bool = False, map_embedding_add: bool = False
) -> Any:
    """Extract the learned statements between normalization and raw restoration."""
    forward = method_definition(source, "HTDemucs")
    if release_decoder:
        release_decoder_locals(forward)
    if map_embedding_add:
        redirect_frequency_embedding_add(forward)
    start = next(i for i, node in enumerate(forward.body) if ast.unparse(node) == "saved = []")
    stop = next(
        i
        for i, node in enumerate(forward.body)
        if ast.unparse(node) == "x = x * std[:, None] + mean[:, None]"
    )
    prefix = ast.parse(
        "def normalized(self, spectral, xt, _mapped_embedding_add=None):\n"
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


def torch_producer(cpu_overlay: Path | None = None):
    """Verify the Torch/Demucs producer without importing ONNX."""
    check_platform()
    expected_version = TORCH if cpu_overlay is None else TORCH_CPU
    versions = {"demucs": "4.0.1", "einops": "0.6.1", "torch": expected_version, "numpy": "2.4.0"}
    for name, version in versions.items():
        if metadata.version(name) != version:
            msg = f"unexpected HTDemucs producer package: {name}"
            raise ValueError(msg)
    torch = import_module("torch")
    if str(torch.__version__) != expected_version or torch.version.git_version != TORCH_COMMIT:
        msg = "requires the pinned Torch build"
        raise ValueError(msg)
    if cpu_overlay is not None:
        root = cpu_overlay.resolve(strict=True)
        extension = Path(torch._C.__file__).resolve(strict=True)
        if (
            Path(torch.__file__).resolve(strict=True) != root / "torch/__init__.py"
            or extension != root / "torch/_C.cp314-win_amd64.pyd"
            or hashlib.sha256(extension.read_bytes()).hexdigest() != TORCH_CPU_EXTENSION
            or any(
                getattr(torch.version, backend) is not None for backend in ("cuda", "hip", "xpu")
            )
        ):
            msg = "requires the pinned CPU-only Torch overlay"
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
    check_model_geometry(model)
    return model


def check_model_geometry(model) -> None:
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


@contextmanager
def sealed_read(path: Path):
    """Open a local regular file with Windows write and delete sharing denied."""
    winapi = import_module("_winapi")
    msvcrt = import_module("msvcrt")
    share_read = 1
    open_reparse_point = 0x00200000
    volume_name_guid = 1
    handle = winapi.CreateFile(
        os.path.abspath(path),
        winapi.GENERIC_READ,
        share_read,
        0,
        winapi.OPEN_EXISTING,
        open_reparse_point,
        0,
    )
    fd = None
    try:
        final_name = ctypes.WinDLL("kernel32", use_last_error=True).GetFinalPathNameByHandleW
        final_name.argtypes = (wintypes.HANDLE, wintypes.LPWSTR, wintypes.DWORD, wintypes.DWORD)
        final_name.restype = wintypes.DWORD
        buffer = ctypes.create_unicode_buffer(32768)
        length = final_name(handle, buffer, len(buffer), volume_name_guid)
        if not 0 < length < len(buffer) or not buffer.value.startswith("\\\\?\\Volume{"):
            msg = "HTDemucs state requires a local volume"
            raise ValueError(msg)
        fd = msvcrt.open_osfhandle(handle, os.O_RDONLY | os.O_BINARY | os.O_NOINHERIT)
    finally:
        if fd is None:
            winapi.CloseHandle(handle)

    try:
        with os.fdopen(fd, "rb", closefd=False) as stream:
            info = os.fstat(fd)
            if (
                not stat.S_ISREG(info.st_mode)
                or info.st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT
            ):
                msg = "HTDemucs state requires a regular file"
                raise ValueError(msg)
            yield stream
    finally:
        os.close(fd)


def check_mapped_runtime(torch, cpu_overlay: Path) -> None:
    """Verify the metadata/assignment implementation and actual CPU TorchAudio images."""
    if sys.version_info[:3] != (3, 14, 2):
        msg = "HTDemucs mapped state requires the pinned CPython mmap implementation"
        raise ValueError(msg)
    root = cpu_overlay.resolve(strict=True)
    for name, digest in STATE_RUNTIME_PINS.items():
        with (root / name).open("rb") as stream:
            if hashlib.file_digest(stream, "sha256").hexdigest() != digest:
                msg = f"pinned HTDemucs mapped-state runtime differs: {name}"
                raise ValueError(msg)
    audio = import_module("torchaudio")
    if (
        str(audio.__version__) != "2.11.0+cpu"
        or Path(audio.__file__).resolve(strict=True) != root / "torchaudio/__init__.py"
        or Path(import_module("torch.serialization").__file__).resolve(strict=True)
        != root / "torch/serialization.py"
        or Path(import_module("torch.nn.modules.module").__file__).resolve(strict=True)
        != root / "torch/nn/modules/module.py"
        or torch.__future__.get_swap_module_params_on_conversion()
    ):
        msg = "unexpected HTDemucs mapped-state runtime"
        raise ValueError(msg)
    module_handle = ctypes.WinDLL("kernel32", use_last_error=True).GetModuleHandleW
    module_handle.argtypes = (wintypes.LPCWSTR,)
    module_handle.restype = wintypes.HMODULE
    audio_libraries = {Path(library).resolve(strict=True) for library in torch.ops.loaded_libraries}
    for name in STATE_RUNTIME_PINS:
        if not name.endswith((".dll", ".pyd")):
            continue
        path = root / name
        image = module_handle(str(path))
        if (
            not image
            or image != module_handle(path.name)
            or (path.suffix == ".pyd" and path not in audio_libraries)
        ):
            msg = f"missing exact loaded HTDemucs image: {name}"
            raise ValueError(msg)


def state_storage_geometry(value) -> tuple:
    return (
        tuple(value.shape),
        value.stride(),
        value.storage_offset(),
        value.untyped_storage().nbytes(),
    )


def model_state_layout(torch, checkpoint: BinaryIO, module):
    """Derive the stock destination layout without allocating either weight set."""
    check_checkpoint(checkpoint)
    package = torch.load(checkpoint, map_location="meta", weights_only=False)
    state = package["state"]
    if package["klass"] is not module.HTDemucs or package["args"] or len(state) != STATE_TENSORS:
        msg = "unexpected HTDemucs state package"
        raise ValueError(msg)
    if torch.get_default_dtype() != torch.float32 or torch.get_default_device().type != "cpu":
        msg = "unexpected HTDemucs stock constructor defaults"
        raise ValueError(msg)
    with torch.device("meta"):
        model = module.HTDemucs(**package["kwargs"]).eval()
    check_model_geometry(model)
    parameters = dict(model.named_parameters(remove_duplicate=False))
    if (
        list(state) != list(parameters)
        or list(parameters) != list(model.state_dict())
        or next(model.buffers(), None) is not None
    ):
        msg = "unexpected HTDemucs state names or buffers"
        raise ValueError(msg)

    layout = []
    end = 0
    for name, target in parameters.items():
        start = (end + STATE_ALIGNMENT - 1) // STATE_ALIGNMENT * STATE_ALIGNMENT
        tensor = describe_state_tensor(torch, name, state[name], target, start)
        layout.append(tensor)
        end = tensor.offset + tensor.size
    ranges = sorted((t.checkpoint_offset, t.checkpoint_offset + t.size // 2) for t in layout)
    if (
        any(left[1] > right[0] for left, right in pairwise(ranges))
        or sum(stop - start for start, stop in ranges) != STATE_HALF_BYTES
        or len({p.untyped_storage()._cdata for p in parameters.values()}) != STATE_TENSORS
        or end != STATE_BANK_BYTES
    ):
        msg = "unexpected HTDemucs state ownership or extent"
        raise ValueError(msg)
    return model, layout


def describe_state_tensor(torch, name, source, target, offset) -> StateTensor:
    source_offset = getattr(source.untyped_storage(), "_checkpoint_offset", None)
    half_bytes = target.numel() * 2
    shape, stride, _, _ = state_storage_geometry(target)
    if (
        (source.dtype, target.dtype) != (torch.float16, torch.float32)
        or (source.device.type, target.device.type) != ("meta", "meta")
        or state_storage_geometry(source) != (shape, stride, 0, half_bytes)
        or state_storage_geometry(target) != (shape, stride, 0, half_bytes * 2)
        or not (source.is_contiguous() and target.is_contiguous() and half_bytes)
    ):
        msg = f"unexpected HTDemucs state layout: {name}"
        raise ValueError(msg)
    if (
        type(source_offset) is not int
        or source_offset % STATE_ALIGNMENT != 0
        or not 0 < source_offset <= CHECKPOINT_BYTES - half_bytes
    ):
        msg = f"unexpected HTDemucs state storage: {name}"
        raise ValueError(msg)
    return StateTensor(
        name, "float32", shape, stride, offset, half_bytes * 2, source_offset, target.requires_grad
    )


def state_manifest(layout: list[StateTensor], digests: list[str]) -> bytes:
    return json.dumps(
        {
            "format": "htdemucs-cpu-f32-state-v1",
            "checkpoint": asdict(Artifact(CHECKPOINT_BYTES, CHECKPOINT)),
            "constructor": "demucs.htdemucs.HTDemucs",
            "source_closure_sha256": source_digest(),
            "torch": TORCH_CPU,
            "torch_commit": TORCH_COMMIT,
            "tensors": [
                dict(asdict(tensor), sha256=digest)
                for tensor, digest in zip(layout, digests, strict=True)
            ],
        },
        separators=(",", ":"),
    ).encode("utf-8")


def prepare_model_state(
    checkpoint: Path,
    output: Path,
    *,
    cpu_overlay: Path,
    admit: Callable[[Phase], None],
) -> ModelStateBank:
    """Convert the exact checkpoint in one separately admitted CPU child."""
    begin_phase(Phase.PREPARE_STATE, admit)
    sources = source_files()
    torch = torch_producer(cpu_overlay)
    check_mapped_runtime(torch, cpu_overlay)
    module = import_module("demucs.htdemucs")
    bank_digest = hashlib.sha256()
    digests = []
    with sealed_read(checkpoint) as source:
        _, layout = model_state_layout(torch, source, module)
        output.mkdir(exist_ok=False)
        with (output / "state.f32").open("xb") as bank:
            for tensor in layout:
                padding = bytes(tensor.offset - bank.tell())
                bank.write(padding)
                bank_digest.update(padding)
                source.seek(tensor.checkpoint_offset)
                remaining = tensor.size // 2
                digest = hashlib.sha256()
                while remaining:
                    size = min(remaining, STREAM_CHUNK_BYTES)
                    half = bytearray(size)
                    if source.readinto(half) != size:
                        msg = "truncated sealed HTDemucs checkpoint"
                        raise ValueError(msg)
                    values = torch.frombuffer(half, dtype=torch.float16).to(dtype=torch.float32)
                    with memoryview(values.numpy()).cast("B") as raw:
                        bank.write(raw)
                        bank_digest.update(raw)
                        digest.update(raw)
                    remaining -= size
                digests.append(digest.hexdigest())
            if bank.tell() != STATE_BANK_BYTES:
                msg = "incomplete HTDemucs state bank"
                raise ValueError(msg)
    if source_files() != sources:
        msg = "HTDemucs source closure changed during state conversion"
        raise ValueError(msg)
    encoded = state_manifest(layout, digests)
    if len(encoded) > MAX_MANIFEST_BYTES:
        msg = "HTDemucs state manifest exceeds its export extent"
        raise ValueError(msg)
    with (output / "state.json").open("xb") as stream:
        stream.write(encoded)
    return ModelStateBank(
        Artifact(len(encoded), hashlib.sha256(encoded).hexdigest()),
        Artifact(STATE_BANK_BYTES, bank_digest.hexdigest()),
    )


def mapped_state_tensors(torch, mapping: mmap.mmap, layout: list[StateTensor]):
    # frombuffer retains this exporter in the C++ DataPtr, including through JIT
    # aliases. Its live buffer export prevents explicit mmap.close() as well.
    # The mmap owns the duplicate of the same FILE_SHARE_READ handle.
    owner = memoryview(mapping)
    return {
        tensor.name: torch.frombuffer(
            owner,
            dtype=torch.float32,
            count=tensor.size // 4,
            offset=tensor.offset,
        ).as_strided(tensor.shape, tensor.stride)
        for tensor in layout
    }


def load_producer_model(
    torch,
    checkpoint: Path,
    module,
    cpu_overlay: Path | None,
    mapped_state: tuple[Path, ModelStateBank] | None,
):
    """Use mapped stock state only when the caller supplies retired bank custody."""
    if mapped_state is None:
        return load_model(torch, checkpoint, module)
    if cpu_overlay is None:
        msg = "HTDemucs mapped state requires the pinned CPU producer"
        raise ValueError(msg)
    check_mapped_runtime(torch, cpu_overlay)
    root, custody = mapped_state
    if custody.weights.size != STATE_BANK_BYTES:
        msg = "unexpected HTDemucs mapped-state extent"
        raise ValueError(msg)
    manifest = read_artifact(root / "state.json", custody.manifest, MAX_MANIFEST_BYTES)
    with sealed_read(checkpoint) as source:
        model, layout = model_state_layout(torch, source, module)
    with sealed_read(root / "state.f32") as bank:
        if os.fstat(bank.fileno()).st_size != custody.weights.size:
            msg = "unexpected HTDemucs mapped-state file size"
            raise ValueError(msg)
        mapping = mmap.mmap(bank.fileno(), custody.weights.size, access=mmap.ACCESS_READ)
    with memoryview(mapping) as raw:
        if hashlib.sha256(raw).hexdigest() != custody.weights.sha256:
            msg = "unexpected HTDemucs mapped-state file digest"
            raise ValueError(msg)
        digests = [hashlib.sha256(raw[t.offset : t.offset + t.size]).hexdigest() for t in layout]
    if manifest != state_manifest(layout, digests):
        msg = "unexpected HTDemucs mapped-state manifest"
        raise ValueError(msg)
    state = mapped_state_tensors(torch, mapping, layout)
    model.load_state_dict(state, strict=True, assign=True)
    for (name, value), tensor in zip(model.named_parameters(), layout, strict=True):
        if (
            (name, value.requires_grad) != (tensor.name, tensor.requires_grad)
            or (value.device.type, value.dtype) != ("cpu", torch.float32)
            or state_storage_geometry(value) != (tensor.shape, tensor.stride, 0, tensor.size)
            or value.data_ptr() != state[name].data_ptr()
            or value.data_ptr() % STATE_ALIGNMENT
        ):
            msg = f"HTDemucs mapped-state assignment differs: {name}"
            raise ValueError(msg)
    return model


def mapped_reference_forward(
    model, module, source: bytes, pad2, ispectro, *, embedding_add=None
) -> MethodType:
    """Keep the pinned CPU CAC forward and retire its unused tensor owners."""
    name = "demucs/htdemucs.py"
    if (
        hashlib.sha256(source).hexdigest() != SOURCE_PINS[name]
        or type(model) is not module.HTDemucs
        or (model.training, model.cac is True, model.hop_length) != (False, True, 1024)
        or {"forward", "_ispec"}.intersection(vars(model))
    ):
        msg = "mapped HTDemucs reference requires the exact stock CAC eval forward"
        raise ValueError(msg)

    forward = method_definition(source, "HTDemucs")
    mask = next(
        i for i, node in enumerate(forward.body) if ast.unparse(node) == "zout = self._mask(z, x)"
    )
    magnitude = next(i for i, node in enumerate(forward.body) if ast.unparse(node) == "x = mag")
    # The pinned CAC mask reads only x. Drop z after magnitude conversion and
    # let x own the magnitude backing until normalization overwrites it.
    # The stock _ispec rebinds z at each pad. Transfer the packed tensor
    # out of the caller so its first pad can actually retire the backing.
    ispec = method_definition(source, "HTDemucs", "_ispec")
    ispec.name = "_consume_ispec"
    ispec.args.args[1].arg = "packed"
    second_pad = next(node for node in ispec.body if ast.unparse(node) == "z = F.pad(z, (2, 2))")
    second_pad.value.func = ast.Name(id="_mapped_pad2", ctx=ast.Load())
    inverse = next(
        node
        for node in ast.walk(ispec)
        if isinstance(node, ast.Call) and ast.unparse(node.func) == "ispectro"
    )
    inverse.func = ast.Name(id="_mapped_ispectro", ctx=ast.Load())
    ispec.body[:0] = ast.parse("z = packed.pop()").body
    for node in ast.walk(forward.body[mask + 1]):
        if isinstance(node, ast.Call) and ast.unparse(node.func) == "self._ispec":
            node.func = ast.Name(id=ispec.name, ctx=ast.Load())
            node.args.insert(0, ast.Name(id="self", ctx=ast.Load()))
    forward.body[mask : mask + 1] = [
        ispec,
        *ast.parse("zout = [self._mask(None, x)]\ndel x").body,
    ]
    forward.body.insert(magnitude + 1, ast.parse("del z, mag").body[0])
    release_decoder_locals(forward)
    if embedding_add is not None:
        redirect_frequency_embedding_add(forward)
    tree = ast.parse(
        "def bind(_mapped_pad2, _mapped_ispectro, _mapped_embedding_add):\n    return forward"
    )
    tree.body[0].body.insert(0, forward)
    namespace = {}
    exec(compile(ast.fix_missing_locations(tree), name, "exec"), vars(module), namespace)
    return MethodType(namespace["bind"](pad2, ispectro, embedding_add), model)


def mapped_reference_scratch(output: Path, size: int, label: str) -> memoryview:
    if disk_usage(output).free < size:
        msg = f"insufficient disk space for the HTDemucs {label}"
        raise OSError(msg)
    # On Windows TemporaryFile creates a unique delete-on-close file. mmap
    # duplicates the same handle, and the exported buffer keeps its view alive
    # even if a native operator fails with a tensor retained by a traceback.
    with TemporaryFile(dir=output, prefix=f".htdemucs-{label}-") as stream:
        stream.truncate(size)
        mapping = mmap.mmap(stream.fileno(), size, access=mmap.ACCESS_WRITE)
    return memoryview(mapping)


@contextmanager
def mapped_attention_score(output: Path, tokens: int):
    """Own one reusable, writable score view for the exact native self-attention path."""
    yield mapped_reference_scratch(output, HEADS * tokens * tokens * 4, "score")


def check_reference_inference_dispatch(torch, operation: str) -> None:
    if (
        not torch.is_inference_mode_enabled()
        or torch.is_autocast_enabled("cpu")
        or torch._C._len_torch_dispatch_stack()
    ):
        msg = f"unexpected HTDemucs {operation} dispatch state"
        raise ValueError(msg)


def check_reference_pad2(torch, z, padding) -> None:
    if (
        (padding, type(z)) != ((2, 2), torch.Tensor)
        or (z.device.type, z.dtype, z.layout) != ("cpu", torch.complex64, torch.strided)
        or state_storage_geometry(z)
        != ((1, 4, 2, 2049, 336), (5507712, 1376928, 688464, 336, 1), 0, 44061696)
        or (z.is_conj(), z.is_neg()) != (False, False)
    ):
        msg = "unexpected HTDemucs second pad geometry"
        raise ValueError(msg)
    check_reference_inference_dispatch(torch, "second pad")


def mapped_reference_pad2(torch, output: Path):
    """Redirect the stock second pad's output allocation into one file view."""
    aten = torch.ops.aten
    cpu = torch._C.DispatchKeySet(torch._C.DispatchKey.CPU)
    stock_pad = torch.nn.functional.pad
    size = prod(PAD2_SHAPE) * 8
    empty_options = {
        "dtype": torch.complex64,
        "layout": torch.strided,
        "device": torch.device("cpu"),
        "memory_format": torch.contiguous_format,
    }

    def pad2(z, padding):
        check_reference_pad2(torch, z, padding)
        mapped = None

        def allocate(op, args, options):
            nonlocal mapped
            if op in {aten.fill_.Scalar, aten.narrow.default, aten.copy_.default}:
                return op(*args, **options)
            if (
                op != aten.empty.memory_format
                or mapped is not None
                or tuple(args[0]) != PAD2_SHAPE
                or options != empty_options
            ):
                msg = "unexpected HTDemucs second pad allocation"
                raise ValueError(msg)
            owner = mapped_reference_scratch(output, size, "pad2")
            mapped = torch.frombuffer(owner, dtype=torch.complex64).view(PAD2_SHAPE)
            if mapped.data_ptr() % STATE_ALIGNMENT or mapped.untyped_storage().nbytes() != size:
                msg = "unexpected HTDemucs mapped second pad storage"
                raise ValueError(msg)
            return mapped

        def dispatch(op, args, options):
            if (
                op != aten.pad.default
                or len(args) != 2
                or args[0]._cdata != z._cdata
                or tuple(args[1]) != padding
                or options
            ):
                msg = "HTDemucs second pad left the stock constant padding path"
                raise ValueError(msg)
            # The stock pad wrapper routes these exact options to this kernel.
            with reference_dispatch_mode(allocate):
                return aten.constant_pad_nd.default.redispatch(cpu, *args)

        with reference_dispatch_mode(dispatch):
            padded = stock_pad(z, padding)
        if mapped is None or padded._cdata != mapped._cdata:
            msg = "HTDemucs second pad did not return the mapped allocation"
            raise ValueError(msg)
        return padded

    return pad2


def tensor_backing(value):
    return value._cdata, value.untyped_storage()._cdata, value.data_ptr()


def check_frame_backing(value, backing, geometry) -> None:
    if tensor_backing(value) != backing or state_storage_geometry(value) != geometry:
        msg = "HTDemucs iSTFT changed the mapped frame backing"
        raise ValueError(msg)


def allocate_c2r_frames(torch, output: Path, operands, options, step):
    expected = {"dtype": torch.float32, "layout": torch.strided, "device": torch.device("cpu")}
    if step != 0 or tuple(operands[0]) != ISTFT_FRAME_SHAPE or options != expected:
        msg = "unexpected HTDemucs C2R frame allocation"
        raise ValueError(msg)
    owner = mapped_reference_scratch(output, ISTFT_FRAME_BYTES, "c2r-frames")
    mapped = torch.frombuffer(owner, dtype=torch.float32).view(ISTFT_FRAME_SHAPE)
    backing = tensor_backing(mapped)
    if backing[2] % STATE_ALIGNMENT or mapped.untyped_storage().resizable():
        msg = "unexpected HTDemucs C2R frame storage"
        raise ValueError(msg)
    check_frame_backing(mapped, backing, ISTFT_FRAME_GEOMETRY)
    return mapped, backing


def native_c2r_pack(torch, output: Path, func, args, options, input_backing, frame_backing):
    """Replace the contiguous clone's allocation inside the stock MKL reshape."""
    aten = torch.ops.aten
    cpu = torch._C.DispatchKeySet(torch._C.DispatchKey.CPU)
    contiguous = {"memory_format": torch.contiguous_format}
    empty_options = {
        "dtype": torch.complex64,
        "layout": torch.strided,
        "device": torch.device("cpu"),
        **contiguous,
    }
    clone_geometry = (C2R_INPUT_SHAPE, (696660, 2049, 1), 0, C2R_INPUT_BYTES)
    if (
        options
        or len(args) != 2
        or tuple(args[1]) != (-1, C2R_PACK_SHAPE[1])
        or tensor_backing(args[0])[1:] != input_backing
        or state_storage_geometry(args[0]) != C2R_INPUT_GEOMETRY
    ):
        msg = "unexpected HTDemucs native C2R pack reshape"
        raise ValueError(msg)
    source = args[0]._cdata
    live_spans = (
        (*input_backing, C2R_INPUT_BYTES),
        (*frame_backing[1:], ISTFT_FRAME_BYTES),
    )
    backing = None
    copied = False
    viewed = False

    def allocate(op, args, options):
        nonlocal backing, copied
        if (
            op == aten.empty_like.default
            and backing is None
            and len(args) == 1
            and args[0]._cdata == source
            and options == empty_options
        ):
            owner = mapped_reference_scratch(output, C2R_INPUT_BYTES, "c2r-pack")
            mapped = torch.frombuffer(owner, dtype=torch.complex64).view(C2R_INPUT_SHAPE)
            backing = tensor_backing(mapped)
            _, storage, address = backing
            if (
                address % STATE_ALIGNMENT
                or mapped.untyped_storage().resizable()
                or state_storage_geometry(mapped) != clone_geometry
                or any(
                    storage == other_storage
                    or (
                        address < other_address + other_size
                        and other_address < address + C2R_INPUT_BYTES
                    )
                    for other_storage, other_address, other_size in live_spans
                )
            ):
                msg = "unexpected HTDemucs C2R pack storage"
                raise ValueError(msg)
            return mapped
        if (
            (op, backing is not None, copied, bool(options), len(args))
            == (aten.copy_.default, True, False, False, 2)
            and tensor_backing(args[0]) == backing
            and args[1]._cdata == source
        ):
            copied = True
            return op(*args)
        msg = "unexpected HTDemucs native C2R clone operation"
        raise ValueError(msg)

    def reshape(op, args, options):
        nonlocal viewed
        if (
            op == aten.clone.default
            and backing is None
            and len(args) == 1
            and args[0]._cdata == source
            and options == contiguous
        ):
            with reference_dispatch_mode(allocate):
                return op.redispatch(cpu, *args, **options)
        if (
            (op, copied, viewed, bool(options), len(args))
            == (aten._unsafe_view.default, True, False, False, 2)
            and tensor_backing(args[0]) == backing
            and tuple(args[1]) == C2R_PACK_SHAPE
        ):
            viewed = True
            return op(*args)
        msg = "unexpected HTDemucs native C2R reshape operation"
        raise ValueError(msg)

    with reference_dispatch_mode(reshape):
        pack = func.redispatch(cpu, *args)
    if (
        not viewed
        or tensor_backing(pack)[1:] != backing[1:]
        or state_storage_geometry(pack) != C2R_PACK_GEOMETRY
    ):
        msg = "HTDemucs native C2R reshape did not return the mapped pack"
        raise ValueError(msg)
    return pack


def native_c2r_frames(torch, output: Path, func, args):
    """Supply the stock MKL C2R input pack and output frame storages."""
    aten = torch.ops.aten
    cpu = torch._C.DispatchKeySet(torch._C.DispatchKey.CPU)
    shape, stride, _, size = ISTFT_FRAME_GEOMETRY
    batch = (2720, 4096)
    batch_stride = (4096, 1)
    batch_geometry = (batch, batch_stride, 0, size)
    input_backing = tensor_backing(args[0])[1:]
    backing = None
    step = 0

    def dispatch(op, operands, options):
        nonlocal backing, step
        if op == aten.empty.memory_format:
            mapped, backing = allocate_c2r_frames(torch, output, operands, options, step)
            step = 1
            return mapped
        if op == aten.reshape.default:
            if step != 1:
                msg = "unexpected HTDemucs C2R pack allocation order"
                raise ValueError(msg)
            step = 2
            return native_c2r_pack(torch, output, op, operands, options, input_backing, backing)
        if op == aten.resize_.default:
            if (
                step != 2
                or tuple(operands[1]) != batch
                or options != {"memory_format": torch.contiguous_format}
            ):
                msg = "unexpected HTDemucs C2R batch resize"
                raise ValueError(msg)
            check_frame_backing(operands[0], backing, ISTFT_FRAME_GEOMETRY)
            resized = op(*operands, **options)
            check_frame_backing(resized, backing, batch_geometry)
            step = 3
            return resized
        if op == aten.as_strided_.default:
            if (
                step != 3
                or options
                or len(operands) != 4
                or (tuple(operands[1]), tuple(operands[2]), operands[3]) != (shape, stride, 0)
            ):
                msg = "unexpected HTDemucs C2R frame stride restoration"
                raise ValueError(msg)
            check_frame_backing(operands[0], backing, batch_geometry)
            restored = op(*operands)
            check_frame_backing(restored, backing, ISTFT_FRAME_GEOMETRY)
            step = 4
            return restored
        return op(*operands, **options)

    # The pinned CPU resize changes only metadata for this exact capacity.
    # DFTI writes the same pointer and the kernel then restores its strides.
    with reference_dispatch_mode(dispatch):
        frames = func.redispatch(cpu, *args)
    if step != 4:
        msg = "incomplete HTDemucs native C2R frame allocation"
        raise ValueError(msg)
    check_frame_backing(frames, backing, ISTFT_FRAME_GEOMETRY)
    return frames


def mapped_windowed_frames(torch, output: Path, frames, hann):
    """Use the stock CPU multiply kernel with a separate file-backed output."""
    owner = mapped_reference_scratch(output, ISTFT_FRAME_BYTES, "windowed-frames")
    mapped = torch.frombuffer(owner, dtype=torch.float32).view(ISTFT_FRAME_SHAPE)
    backing = tensor_backing(mapped)
    check_frame_backing(mapped, backing, ISTFT_FRAME_GEOMETRY)
    if (
        backing[2] % STATE_ALIGNMENT
        or mapped.untyped_storage().resizable()
        or backing[1] in {frames.untyped_storage()._cdata, hann.untyped_storage()._cdata}
    ):
        msg = "unexpected HTDemucs mapped windowed frame storage"
        raise ValueError(msg)

    # Both structured CPU overloads use mul_stub. With matching F32 options and
    # contiguous output of the final shape, TensorIterator uses this data pointer.
    windowed = torch.ops.aten.mul.out(frames, hann, out=mapped)
    check_frame_backing(windowed, backing, ISTFT_FRAME_GEOMETRY)
    return windowed


def native_mapped_istft(torch, output: Path, func, args):
    """Use mapped frames through the pinned iSTFT CPU kernel."""
    aten = torch.ops.aten
    cpu = torch._C.DispatchKeySet(torch._C.DispatchKey.CPU)
    storage = args[0].untyped_storage()._cdata
    hop_length = args[2]
    c2r_backing = None
    hann_backing = tensor_backing(args[4])[1:]
    windowed_backing = None
    used_signal_ola = False

    def istft_kernel(op, args, options):
        nonlocal c2r_backing, windowed_backing, used_signal_ola
        if op == aten._fft_c2r.default:
            if (
                (c2r_backing, bool(options), len(args)) != (None, False, 4)
                or (tuple(args[1]), args[2], args[3]) != ((2,), 1, 4096)
                or args[0].untyped_storage()._cdata != storage
                or state_storage_geometry(args[0]) != C2R_INPUT_GEOMETRY
            ):
                msg = "unexpected HTDemucs C2R input"
                raise ValueError(msg)
            frames = native_c2r_frames(torch, output, op, args)
            c2r_backing = tensor_backing(frames)
            return frames
        if op == aten.mul.Tensor and not used_signal_ola:
            if (
                (c2r_backing is not None, windowed_backing, bool(options), len(args))
                != (True, None, False, 2)
                or any(
                    type(value) is not torch.Tensor
                    or (value.device.type, value.dtype, value.layout)
                    != ("cpu", torch.float32, torch.strided)
                    or value.is_conj()
                    or value.is_neg()
                    for value in args
                )
                or tensor_backing(args[1])[1:] != hann_backing
                or state_storage_geometry(args[1])[:3] != ((1, 1, 4096), (4096, 4096, 1), 0)
            ):
                msg = "unexpected HTDemucs iSTFT window multiply"
                raise ValueError(msg)
            check_frame_backing(args[0], c2r_backing, ISTFT_FRAME_GEOMETRY)
            windowed = mapped_windowed_frames(torch, output, *args)
            windowed_backing = tensor_backing(windowed)
            return windowed
        if op == aten.unfold_backward.default and not used_signal_ola:
            if (
                windowed_backing is None
                or options
                or len(args) != 5
                or (tuple(args[1]), *args[2:]) != ((8, 351232), 1, 4096, hop_length)
            ):
                msg = "unexpected HTDemucs iSTFT signal overlap-add"
                raise ValueError(msg)
            check_frame_backing(args[0], windowed_backing, ISTFT_FRAME_GEOMETRY)
            used_signal_ola = True
        return op(*args, **options)

    with reference_dispatch_mode(istft_kernel):
        result = func.redispatch(cpu, *args)
    if not used_signal_ola:
        msg = "HTDemucs stock iSTFT did not overlap-add the mapped windowed frames"
        raise ValueError(msg)
    return result


def mapped_reference_ispectro(torch, output: Path, stock_ispectro):
    """Map the fixed stock iSTFT frame backings through signal overlap-add."""
    aten = torch.ops.aten
    input_bytes = prod(PAD2_SHAPE) * 8

    def ispectro(z, hop_length, *, length):
        if (
            (hop_length, length, type(z)) != (1024, 347136, torch.Tensor)
            or (z.device.type, z.dtype, z.layout) != ("cpu", torch.complex64, torch.strided)
            or (z.is_conj(), z.is_neg()) != (False, False)
            or state_storage_geometry(z)
            != (PAD2_SHAPE, (5573280, 1393320, 696660, 340, 1), 0, input_bytes)
        ):
            msg = "unexpected HTDemucs mapped iSTFT input"
            raise ValueError(msg)
        check_reference_inference_dispatch(torch, "mapped iSTFT")
        storage = z.untyped_storage()._cdata
        used_istft = False

        def dispatch(op, args, options):
            nonlocal used_istft
            if op != aten.istft.default:
                return op(*args, **options)
            if (
                (used_istft, bool(options), len(args)) != (False, False, 9)
                or args[1:4] != (4096, hop_length, 4096)
                or args[5:] != (True, True, None, length)
                or args[0].untyped_storage()._cdata != storage
                or state_storage_geometry(args[0])
                != ((8, 2049, 340), (696660, 340, 1), 0, input_bytes)
            ):
                msg = "unexpected HTDemucs stock iSTFT call"
                raise ValueError(msg)
            used_istft = True
            return native_mapped_istft(torch, output, op, args)

        with reference_dispatch_mode(dispatch):
            result = stock_ispectro(z, hop_length, length=length)
        if not used_istft:
            msg = "HTDemucs stock iSTFT did not use the mapped native path"
            raise ValueError(msg)
        return result

    return ispectro


def attention_score_tensor(torch, owner: memoryview, shape: tuple[int, int, int]):
    score = torch.frombuffer(owner, dtype=torch.float32).view(shape)
    if score.data_ptr() % STATE_ALIGNMENT or score.untyped_storage().nbytes() != len(owner):
        msg = "unexpected mapped HTDemucs attention score storage"
        raise ValueError(msg)
    return score


def reference_dispatch_mode(dispatch):
    mode = import_module("torch.utils._python_dispatch").TorchDispatchMode

    class ReferenceDispatch(mode):
        def __init__(self, handler):
            super().__init__()
            self.handler = handler

        @classmethod
        def _should_skip_dynamo(cls):
            # This scope is used only by eager inference.
            return False

        def __torch_dispatch__(self, func, types, args=(), kwargs=None):  # noqa: PLW3201
            return self.handler(func, args, kwargs or {})

    return ReferenceDispatch(dispatch)


def check_mapped_convolution_operands(torch, operands, geometry):
    for value, expected in zip(operands, geometry, strict=True):
        if (
            type(value) not in {torch.Tensor, torch.nn.Parameter}
            or (value.device.type, value.dtype, value.layout)
            != ("cpu", torch.float32, torch.strided)
            or value.is_neg()
            or state_storage_geometry(value) != expected
        ):
            msg = "unexpected HTDemucs mapped convolution operands"
            raise ValueError(msg)


def mapped_float_return_storage(torch, output: Path, inputs, geometry, label):
    shape, strides, offset, size = geometry
    owner = mapped_reference_scratch(output, size, label)
    mapped = torch.frombuffer(owner, dtype=torch.float32).view(shape)
    if mapped.stride() != strides:
        mapped = mapped.as_strided(shape, strides, offset)
    _, storage, address = tensor_backing(mapped)
    # A cropped operand's data pointer skips the prefix of its live backing.
    input_storages = [value.untyped_storage() for value in inputs]
    live_spans = tuple(
        (storage._cdata, storage.data_ptr(), storage.nbytes()) for storage in input_storages
    )
    if (
        address % STATE_ALIGNMENT
        or mapped.untyped_storage().resizable()
        or state_storage_geometry(mapped) != geometry
        or any(
            storage == other_storage or (address < start + other_size and start < address + size)
            for other_storage, start, other_size in live_spans
        )
    ):
        msg = f"unexpected HTDemucs mapped {label} storage"
        raise ValueError(msg)
    return mapped


def native_mapped_convolution_return(torch, output: Path, func, args, cpu, geometry, label):
    """Supply the ABCD destination of the stock oneDNN public reorder."""
    aten = torch.ops.aten
    empty_options = {
        "dtype": torch.float32,
        "layout": torch.strided,
        "device": torch.device("cpu"),
    }
    contiguous = {"memory_format": torch.contiguous_format}
    shape = geometry[0]
    stage = 0
    backing = None

    def dispatch(op, operands, options):
        nonlocal stage, backing
        if op == aten.empty.memory_format and stage in {0, 1} and options == empty_options:
            if stage == 0 and tuple(operands[0]) == (0,):
                stage = 1
                return op(*operands, **options)
            if stage == 1 and tuple(operands[0]) == shape:
                mapped = mapped_float_return_storage(torch, output, args[:3], geometry, label)
                backing = tensor_backing(mapped)
                stage = 2
                return mapped
        if backing is not None and operands and tensor_backing(operands[0]) == backing:
            if (
                op == aten.as_strided_.default
                and stage == 2
                and (tuple(operands[1]), tuple(operands[2])) == geometry[:2]
                and operands[3:] in {(), (None,), (0,)}
                and not options
            ):
                stage = 3
                return op(*operands)
            if op == aten.contiguous.default and stage == 3 and len(operands) == 1 and not options:
                # The public descriptor is already contiguous, so this must keep the view.
                stage = 4
                return op(*operands)
            if (
                op == aten.resize_.default
                and stage in {3, 4}
                and len(operands) == 2
                and tuple(operands[1]) == shape
                and options == contiguous
            ):
                stage = 5
                return op(*operands, **options)
        msg = "unexpected HTDemucs mapped convolution allocation or layout operation"
        raise ValueError(msg)

    with reference_dispatch_mode(dispatch):
        dense = func.redispatch(cpu, *args)
    if stage != 5 or tensor_backing(dense) != backing or state_storage_geometry(dense) != geometry:
        msg = "unexpected HTDemucs mapped convolution dense return"
        raise ValueError(msg)
    return dense


def check_spectral_convolution_environment() -> None:
    # oneDNN reads the Windows process environment, which can differ from
    # Python's os.environ snapshot.
    get_variable = ctypes.WinDLL("kernel32", use_last_error=True).GetEnvironmentVariableW
    get_variable.argtypes = (wintypes.LPCWSTR, wintypes.LPWSTR, wintypes.DWORD)
    get_variable.restype = wintypes.DWORD
    envvar_not_found = 203
    for name in (
        "ONEDNN_MAX_CPU_ISA",
        "DNNL_MAX_CPU_ISA",
        "ONEDNN_CPU_ISA_HINTS",
        "DNNL_CPU_ISA_HINTS",
        "ONEDNN_DEFAULT_FPMATH_MODE",
        "DNNL_DEFAULT_FPMATH_MODE",
        "ATEN_CPU_CAPABILITY",
    ):
        ctypes.set_last_error(0)
        if get_variable(name, None, 0) or ctypes.get_last_error() != envvar_not_found:
            msg = f"unexpected HTDemucs spectral convolution environment: {name}"
            raise ValueError(msg)


def check_spectral_convolution_admission(
    cpu_overlay: Path | None, admission: SpectralConvolutionAdmission | None
) -> None:
    if cpu_overlay is None or type(admission) is not SpectralConvolutionAdmission:
        msg = "mapped HTDemucs reference requires prior spectral convolution admission"
        raise ValueError(msg)
    # oneDNN can cache environment defaults at first use.
    check_spectral_convolution_environment()


def check_spectral_convolution_runtime(torch, backend: SpectralConvolutionBackend) -> None:
    check_spectral_convolution_environment()
    native = torch._C
    runtime = (
        native._has_mkldnn,
        native._get_mkldnn_enabled(),
        native._get_fp32_precision_getter("mkldnn", "conv"),
        native._get_mkldnn_deterministic(),
        bool(native._get_onednn_allow_tf32()),
        torch.are_deterministic_algorithms_enabled(),
        torch.get_num_threads(),
        torch.get_num_interop_threads(),
        native._get_cpu_capability(),
    )
    expected = (
        True,
        backend is SpectralConvolutionBackend.MKLDNN,
        "none",
        False,
        False,
        False,
        1,
        1,
        "AVX512",
    )
    if runtime != expected:
        msg = f"unexpected HTDemucs spectral convolution runtime for {backend.value}"
        raise ValueError(msg)


def check_spectral_convolution_backend(torch, operands, params, backend) -> None:
    check_spectral_convolution_runtime(torch, backend)
    source, weight, _ = operands
    # Convolution.cpp use_mkldnn at TORCH_COMMIT, specialized to exact unit
    # stride/dilation, group 1 and one CPU thread. The K1 lift inserts one
    # spatial element without changing the batch or element count.
    batch = source.shape[0]
    filter_height, filter_width = (1, 1, *weight.shape[2:])[-2:]
    if backend is SpectralConvolutionBackend.MKLDNN and not (
        (batch >= 16 or filter_height != 1 or filter_width != 1)
        and (batch > 1 or source.numel() > 20_480)
    ):
        msg = "HTDemucs spectral convolution fails the pinned MKLDNN predicate"
        raise ValueError(msg)
    selected = torch._C._select_conv_backend(*operands, *params)
    if selected != getattr(torch._C._ConvBackend, backend.value):
        msg = f"unexpected HTDemucs spectral convolution backend: {selected}"
        raise ValueError(msg)


def native_spectral_convolution_operands(operands, spatial, kernel_size, *, mkldnn):
    padding, stride, _ = spatial
    if mkldnn:
        if len(operands) == 7 and tuple(map(tuple, operands[3:6])) == spatial and operands[6] == 1:
            return operands[:3]
    elif 4 <= len(operands) <= 6:
        # The dispatcher can elide the trailing default stride and padding.
        defaults = ((1, 1), (0, 0))
        supplied = tuple(map(tuple, operands[4:]))
        if tuple(operands[2]) == kernel_size and supplied + defaults[len(supplied) :] == (
            stride,
            padding,
        ):
            return *operands[:2], operands[3]
    msg = "unexpected HTDemucs native spectral convolution parameters"
    raise ValueError(msg)


def checked_spectral_lift(torch, alias, geometry, original_backing):
    check_mapped_convolution_operands(torch, (alias,), (geometry,))
    backing = tensor_backing(alias)
    if backing[1:] != original_backing[1:]:
        msg = "unexpected HTDemucs spectral DConv lift backing"
        raise ValueError(msg)
    return backing


def redispatch_mapped_convolution(
    torch, output: Path, func, args, cpu, geometry, label, backend, *, lift=None
):
    aten = torch.ops.aten
    stride, padding, dilation = map(tuple, args[3:6])
    if lift is not None:
        # Native Conv1d unsqueezes input and weight before selecting again.
        stride, padding, dilation = (1, *stride), (0, *padding), (1, *dilation)
    mkldnn = backend is SpectralConvolutionBackend.MKLDNN
    kernel = aten.mkldnn_convolution.default if mkldnn else aten.thnn_conv2d.default
    original = args[:3]
    original_backings = tuple(tensor_backing(value) for value in original)
    native_backings = list(original_backings)
    lifted_count = 0
    native_backing = None
    dense_geometry = geometry if lift is None else SPECTRAL_DCONV_RETURN
    kernel_size = tuple(original[1].shape[2:]) if lift is None else lift[1][0][2:]

    def dispatch(op, operands, options):
        nonlocal lifted_count, native_backing
        before_kernel = native_backing is None
        if (
            (op, before_kernel, len(operands)) == (aten.contiguous.default, True, 1)
            and options in ({}, {"memory_format": torch.contiguous_format})
            and tensor_backing(operands[0]) in {*original_backings, *native_backings}
            and operands[0].is_contiguous()
        ):
            return op(*operands, **options)
        rank_view = (
            op in {aten.unsqueeze.default, aten.squeeze.dim}
            and lift is not None
            and not options
            and len(operands) == 2
            and operands[1] == 2
        )
        if (
            (op, before_kernel, rank_view) == (aten.unsqueeze.default, True, True)
            and lifted_count < 2
            and tensor_backing(operands[0]) == original_backings[lifted_count]
        ):
            alias = op(*operands)
            native_backings[lifted_count] = checked_spectral_lift(
                torch, alias, lift[lifted_count], original_backings[lifted_count]
            )
            lifted_count += 1
            return alias
        if (op, before_kernel, rank_view) == (aten.squeeze.dim, False, True) and tensor_backing(
            operands[0]
        ) == native_backing:
            return op(*operands)
        if op != kernel or options or native_backing is not None or (lift and lifted_count != 2):
            msg = f"unexpected HTDemucs {label} native dispatch for {backend.value}: {op}"
            raise ValueError(msg)

        check_spectral_convolution_runtime(torch, backend)
        observed = native_spectral_convolution_operands(
            operands, (padding, stride, dilation), kernel_size, mkldnn=mkldnn
        )
        if tuple(map(tensor_backing, observed)) != tuple(native_backings):
            msg = f"unexpected HTDemucs {label} native operands"
            raise ValueError(msg)
        if mkldnn:
            result = native_mapped_convolution_return(
                torch, output, op, operands, cpu, geometry, label
            )
        else:
            result = op(*operands)
        check_mapped_convolution_operands(torch, (result,), (geometry,))
        native_backing = tensor_backing(result)
        return result

    with reference_dispatch_mode(dispatch):
        dense = func.redispatch(cpu, *args)
    if (
        native_backing is None
        or tensor_backing(dense)[1:] != native_backing[1:]
        or state_storage_geometry(dense) != dense_geometry
    ):
        msg = f"unexpected HTDemucs {label} native convolution return"
        raise ValueError(msg)
    return dense


def mapped_convolution_dispatch(torch, operands, wrappers, params, redispatch):
    aten = torch.ops.aten
    cpu = torch._C.DispatchKeySet(torch._C.DispatchKey.CPU)
    expected_tensors = tuple(value._cdata for value in operands)

    def dispatch(op, args, options):
        native = op == aten._convolution.default
        expected = (*params, False, False, True, True) if native else wrappers.get(op)
        supplied = args[3:]
        if (
            expected is None
            or options
            or tuple(value._cdata for value in args[:3]) != expected_tensors
            or tuple(tuple(value) if isinstance(value, list) else value for value in supplied)
            != expected
        ):
            msg = "unexpected HTDemucs mapped convolution"
            raise ValueError(msg)
        if op in wrappers:
            with reference_dispatch_mode(dispatch):
                return op.redispatch(cpu, *args)
        return redispatch(op, args, cpu)

    return reference_dispatch_mode(dispatch)


def check_spectral_pointwise_operands(torch, operands, geometries, label) -> None:
    for value, geometry in zip(operands, geometries, strict=True):
        if (
            type(value) is not torch.Tensor
            or (value.device.type, value.dtype, value.layout)
            != ("cpu", torch.float32, torch.strided)
            or value.is_neg()
            or state_storage_geometry(value) != geometry
        ):
            msg = f"unexpected HTDemucs {label} operands"
            raise ValueError(msg)


def mapped_frequency_embedding_add(torch, output: Path):
    """Write the first spectral encoder skip with the stock CPU Add kernel."""
    geometry = SPECTRAL_SKIP_ADD_DENSE

    def add(x, scaled_embedding):
        label = "frequency embedding add"
        operands = (x, scaled_embedding)
        check_reference_inference_dispatch(torch, label)
        check_spectral_pointwise_operands(
            torch, operands, (geometry, SPECTRAL_SCALED_EMBEDDING), label
        )
        mapped = mapped_float_return_storage(torch, output, operands, geometry, "embedding-add")
        backing = tensor_backing(mapped)
        result = torch.ops.aten.add.out(*operands, out=mapped)
        if tensor_backing(result) != backing or state_storage_geometry(result) != geometry:
            msg = "unexpected HTDemucs frequency embedding add return"
            raise ValueError(msg)
        return result

    return add


@contextmanager
def mapped_spectral_caller_gelu(torch, decoder, source: bytes, output: Path):
    """Write the penultimate spectral decoder's post-crop GELU to a file view."""
    name = "demucs/hdemucs.py"
    globals_ = type(decoder).forward.__globals__
    if (
        hashlib.sha256(source).hexdigest() != SOURCE_PINS[name]
        or type(decoder) is not globals_.get("HDecLayer")
        or "forward" in vars(decoder)
        or (
            decoder.training,
            decoder.freq,
            decoder.last,
            decoder.empty,
            decoder.chin,
            decoder.pad,
            type(decoder.conv_tr),
            type(decoder.norm2),
            decoder.conv_tr.out_channels,
        )
        != (False, True, False, False, 96, 2, torch.nn.ConvTranspose2d, torch.nn.Identity, 48)
    ):
        msg = "unexpected HTDemucs penultimate spectral decoder"
        raise ValueError(msg)

    def gelu(z):
        label = "spectral caller GELU"
        geometry = SPECTRAL_SKIP_ADD_DENSE
        check_reference_inference_dispatch(torch, label)
        check_spectral_pointwise_operands(torch, (z,), (SPECTRAL_CALLER_CROP,), label)
        mapped = mapped_float_return_storage(torch, output, (z,), geometry, "caller-gelu")
        backing = tensor_backing(mapped)
        result = torch.ops.aten.gelu.out(z, approximate="none", out=mapped)
        if tensor_backing(result) != backing or state_storage_geometry(result) != geometry:
            msg = "unexpected HTDemucs spectral caller GELU return"
            raise ValueError(msg)
        return result

    forward = method_definition(source, "HDecLayer")
    post_crop = forward.body[-2]
    if ast.unparse(post_crop) != "if not self.last:\n    z = F.gelu(z)":
        msg = "unexpected HTDemucs spectral caller GELU source"
        raise ValueError(msg)
    post_crop.body[0].value.func = ast.Name(id="_mapped_gelu", ctx=ast.Load())
    tree = ast.parse("def bind(_mapped_gelu):\n    return forward")
    tree.body[0].body.insert(0, forward)
    namespace = {}
    exec(compile(ast.fix_missing_locations(tree), name, "exec"), globals_, namespace)
    decoder.forward = MethodType(namespace["bind"](gelu), decoder)
    try:
        yield
    finally:
        del decoder.forward


@contextmanager
def mapped_waveform_activation(torch, layer, source, class_name, label, activate):
    name = "demucs/hdemucs.py"
    globals_ = type(layer).forward.__globals__
    if (
        hashlib.sha256(source).hexdigest() != SOURCE_PINS[name]
        or type(layer) is not globals_.get(class_name)
        or "forward" in vars(layer)
        or (layer.training, layer.freq, layer.empty, type(layer.norm2))
        != (False, False, False, torch.nn.Identity)
    ):
        msg = f"unexpected HTDemucs {label} module"
        raise ValueError(msg)

    forward = method_definition(source, class_name)
    activation = forward.body[-2].body[-1]
    if ast.unparse(activation) not in {"z = F.glu(z, dim=1)", "z = F.gelu(z)"}:
        msg = f"unexpected HTDemucs {label} source"
        raise ValueError(msg)
    activation.value.func = ast.Name(id="_mapped_activation", ctx=ast.Load())
    tree = ast.parse("def bind(_mapped_activation):\n    return forward")
    tree.body[0].body.insert(0, forward)
    namespace = {}
    exec(compile(ast.fix_missing_locations(tree), name, "exec"), globals_, namespace)
    layer.forward = MethodType(namespace["bind"](activate), layer)
    try:
        yield
    finally:
        del layer.forward


def write_waveform_activation(torch, output, value, input_geometry, label, op, **options):
    check_reference_inference_dispatch(torch, label)
    check_spectral_pointwise_operands(torch, (value,), (input_geometry,), label)
    geometry = WAVEFORM_CALLER_DENSE
    mapped = mapped_float_return_storage(torch, output, (value,), geometry, label)
    backing = tensor_backing(mapped)
    result = op(value, **options, out=mapped)
    if tensor_backing(result) != backing or state_storage_geometry(result) != geometry:
        msg = f"unexpected HTDemucs {label} return"
        raise ValueError(msg)
    return result


def mapped_waveform_skip_glu(torch, encoder, source: bytes, output: Path):
    """Map the first waveform encoder's stock Rewrite GLU return."""
    rewrite = encoder.rewrite
    if type(rewrite) is not torch.nn.Conv1d or (
        rewrite.in_channels,
        rewrite.out_channels,
        rewrite.kernel_size,
        rewrite.stride,
        rewrite.padding,
        rewrite.dilation,
        rewrite.groups,
        rewrite.padding_mode,
    ) != (48, 96, (1,), (1,), (0,), (1,), 1, "zeros"):
        msg = "unexpected HTDemucs first waveform encoder rewrite"
        raise ValueError(msg)
    label = "waveform-skip-glu"

    def glu(value, *, dim):
        return write_waveform_activation(
            torch,
            output,
            value,
            WAVEFORM_SKIP_REWRITE,
            label,
            torch.ops.aten.glu.out,
            dim=dim,
        )

    return mapped_waveform_activation(torch, encoder, source, "HEncLayer", label, glu)


def mapped_waveform_caller_gelu(torch, decoder, source: bytes, output: Path):
    """Map the penultimate waveform decoder's stock post-crop GELU return."""
    transpose = decoder.conv_tr
    if (
        type(transpose) is not torch.nn.ConvTranspose1d
        or (decoder.last, decoder.chin, decoder.pad) != (False, 96, 2)
        or (
            transpose.in_channels,
            transpose.out_channels,
            transpose.kernel_size,
            transpose.stride,
            transpose.padding,
            transpose.output_padding,
            transpose.dilation,
            transpose.groups,
        )
        != (96, 48, (8,), (4,), (0,), (0,), (1,), 1)
    ):
        msg = "unexpected HTDemucs penultimate waveform decoder transpose"
        raise ValueError(msg)
    label = "waveform-caller-gelu"

    def gelu(value):
        return write_waveform_activation(
            torch,
            output,
            value,
            WAVEFORM_CALLER_CROP,
            label,
            torch.ops.aten.gelu.out,
            approximate="none",
        )

    return mapped_waveform_activation(torch, decoder, source, "HDecLayer", label, gelu)


@contextmanager
def mapped_spectral_skip_add(torch, decoder, source: bytes, output: Path):
    """Use a file-backed destination for the pinned final spectral skip addition."""
    name = "demucs/hdemucs.py"
    globals_ = type(decoder).forward.__globals__
    if (
        hashlib.sha256(source).hexdigest() != SOURCE_PINS[name]
        or type(decoder) is not globals_.get("HDecLayer")
        or "forward" in vars(decoder)
        or (
            decoder.training, decoder.freq, decoder.last, decoder.empty, decoder.chin, decoder.pad,
            type(decoder.rewrite), type(decoder.norm1), decoder.dconv is not None,
        )
        != (False, True, True, False, 48, 2, torch.nn.Conv2d, torch.nn.Identity, True)
    ):
        msg = "unexpected HTDemucs final spectral skip addition module"
        raise ValueError(msg)

    geometry = SPECTRAL_SKIP_ADD_DENSE

    def add(x, skip):
        check_reference_inference_dispatch(torch, "spectral skip addition")
        for value in (x, skip):
            if (
                type(value) is not torch.Tensor
                or (value.device.type, value.dtype, value.layout)
                != ("cpu", torch.float32, torch.strided)
                or value.is_neg()
                or state_storage_geometry(value) != geometry
            ):
                msg = "unexpected HTDemucs spectral skip addition operands"
                raise ValueError(msg)
        mapped = mapped_float_return_storage(torch, output, (x, skip), geometry, "skip-add")
        backing = tensor_backing(mapped)
        result = torch.ops.aten.add.out(x, skip, out=mapped)
        if tensor_backing(result) != backing or state_storage_geometry(result) != geometry:
            msg = "unexpected HTDemucs spectral skip addition return"
            raise ValueError(msg)
        return result

    forward = method_definition(source, "HDecLayer")
    nonempty = next(
        node for node in forward.body
        if isinstance(node, ast.If) and ast.unparse(node.test) == "not self.empty"
    )
    addition = nonempty.body[0]
    if ast.unparse(addition) != "x = x + skip":
        msg = "unexpected HTDemucs final spectral skip addition source"
        raise ValueError(msg)
    addition.value = ast.parse("_mapped_add(x, skip)", mode="eval").body
    tree = ast.parse("def bind(_mapped_add):\n    return forward")
    tree.body[0].body.insert(0, forward)
    namespace = {}
    exec(compile(ast.fix_missing_locations(tree), name, "exec"), globals_, namespace)
    decoder.forward = MethodType(namespace["bind"](add), decoder)
    try:
        yield
    finally:
        del decoder.forward


@contextmanager
def mapped_spectral_rewrite(
    torch, conv, output: Path, *, backend=SpectralConvolutionBackend.MKLDNN
):
    """Map the pinned final spectral Conv2d return for mapped eager references."""
    stride, padding, dilation, output_padding = (1, 1), (1, 1), (1, 1), (0, 0)
    if (
        type(conv) is not torch.nn.Conv2d
        or "forward" in vars(conv)
        or (conv.kernel_size, conv.stride, conv.padding, conv.dilation)
        != ((3, 3), stride, padding, dilation)
        or (conv.groups, conv.padding_mode) != (1, "zeros")
    ):
        msg = "unexpected HTDemucs final spectral rewrite module"
        raise ValueError(msg)
    stock = conv.forward
    aten = torch.ops.aten
    convolution_args = (stride, padding, dilation, False, output_padding, 1)
    wrappers = {
        aten.conv2d.default: (stride, padding),
        aten.convolution.default: convolution_args,
    }
    geometry = (
        SPECTRAL_SKIP_ADD_DENSE,
        ((96, 48, 3, 3), (432, 9, 3, 1), 0, 165888),
        ((96,), (1,), 0, 384),
    )

    def redispatch(op, args, cpu):
        return redispatch_mapped_convolution(
            torch, output, op, args, cpu, SPECTRAL_REWRITE_DENSE, "rewrite-dense", backend
        )

    def forward(source):
        check_reference_inference_dispatch(torch, "spectral rewrite")
        operands = (source, conv.weight, conv.bias)
        check_mapped_convolution_operands(torch, operands, geometry)
        check_spectral_convolution_backend(torch, operands, convolution_args, backend)
        with mapped_convolution_dispatch(torch, operands, wrappers, convolution_args, redispatch):
            return stock(source)

    conv.forward = forward
    try:
        yield
    finally:
        del conv.forward


@contextmanager
def mapped_spectral_dconv_pointwise(
    torch, conv, output: Path, *, backend=SpectralConvolutionBackend.MKLDNN
):
    """Map a pointwise return of the final spectral DConv."""
    stride, padding, dilation = (1,), (0,), (1,)
    if (
        type(conv) is not torch.nn.Conv1d
        or "forward" in vars(conv)
        or (conv.in_channels, conv.out_channels, conv.groups, conv.padding_mode)
        != (6, 96, 1, "zeros")
        or (conv.kernel_size, conv.stride, conv.padding, conv.dilation)
        != ((1,), stride, padding, dilation)
    ):
        msg = "unexpected HTDemucs final spectral DConv pointwise module"
        raise ValueError(msg)
    stock = conv.forward
    aten = torch.ops.aten
    convolution_args = (stride, padding, dilation, False, (0,), 1)
    wrappers = {aten.conv1d.default: (), aten.convolution.default: convolution_args}
    bias = ((96,), (1,), 0, 384)
    geometry = (
        ((512, 6, 336), (2016, 336, 1), 0, 4_128_768),
        ((96, 6, 1), (6, 1, 1), 0, 2_304),
        bias,
    )
    lifted = (
        ((512, 6, 1, 336), (2016, 336, 336, 1), 0, 4_128_768),
        ((96, 6, 1, 1), (6, 1, 1, 1), 0, 2_304),
        bias,
    )

    def redispatch(op, args, cpu):
        return redispatch_mapped_convolution(
            torch,
            output,
            op,
            args,
            cpu,
            SPECTRAL_DCONV_DENSE,
            "dconv-pointwise",
            backend,
            lift=lifted,
        )

    def forward(source):
        check_reference_inference_dispatch(torch, "spectral DConv pointwise")
        operands = (source, conv.weight, conv.bias)
        # Backend selection may make a rank-three input contiguous.
        check_mapped_convolution_operands(torch, operands, geometry)
        check_spectral_convolution_backend(torch, operands, convolution_args, backend)
        with mapped_convolution_dispatch(torch, operands, wrappers, convolution_args, redispatch):
            return stock(source)

    conv.forward = forward
    try:
        yield
    finally:
        del conv.forward


@contextmanager
def mapped_spectral_dconv_tail(torch, dconv, source: bytes, output: Path):
    """Map the second GLU and final residual Add in the pinned spectral DConv."""
    name = "demucs/demucs.py"
    stock = (
        "def forward(self, x):\n"
        "    for layer in self.layers:\n"
        "        x = x + layer(x)\n"
        "    return x"
    )
    if (
        hashlib.sha256(source).hexdigest() != SOURCE_PINS[name]
        or ast.unparse(method_definition(source, "DConv")) != stock
    ):
        msg = "unexpected HTDemucs final spectral DConv source"
        raise ValueError(msg)
    if (
        type(dconv) is not type(dconv).forward.__globals__.get("DConv")
        or "forward" in vars(dconv)
        or (dconv.training, dconv.channels, dconv.compress, dconv.depth) != (False, 48, 8, 2)
        or len(dconv.layers) != 2
    ):
        msg = "unexpected HTDemucs final spectral DConv module"
        raise ValueError(msg)

    first, second = dconv.layers
    if any(type(layer) is not torch.nn.Sequential or len(layer) != 7 for layer in (first, second)):
        msg = "unexpected HTDemucs final spectral DConv layers"
        raise ValueError(msg)
    norm, glu = second[4], second[5]
    if (
        type(norm) is not torch.nn.GroupNorm
        or (norm.num_groups, norm.num_channels) != (1, 96)
        or type(glu) is not torch.nn.GLU
        or glu.dim != 1
        or "forward" in vars(glu)
    ):
        msg = "unexpected HTDemucs final spectral DConv GLU module"
        raise ValueError(msg)

    def write(label, op, inputs, geometries, geometry, **options):
        check_reference_inference_dispatch(torch, label)
        check_spectral_pointwise_operands(torch, inputs, geometries, label)
        mapped = mapped_float_return_storage(torch, output, inputs, geometry, label)
        backing = tensor_backing(mapped)
        result = op(*inputs, **options, out=mapped)
        if tensor_backing(result) != backing or state_storage_geometry(result) != geometry:
            msg = f"unexpected HTDemucs mapped {label} return"
            raise ValueError(msg)
        return result

    def glu_forward(value):
        return write(
            "dconv-glu", torch.ops.aten.glu.out, (value,), (SPECTRAL_DCONV_RETURN,),
            SPECTRAL_DCONV_GLU, dim=glu.dim,
        )

    def forward(x):
        # The stock first residual Add creates an independent backing.
        x = x + first(x)  # noqa: PLR6104
        return write(
            "dconv-add", torch.ops.aten.add.out, (x, second(x)),
            (SPECTRAL_DCONV_RESIDUAL, SPECTRAL_DCONV_GLU), SPECTRAL_DCONV_RESIDUAL,
        )

    glu.forward = glu_forward
    dconv.forward = forward
    try:
        yield
    finally:
        del dconv.forward
        del glu.forward


def check_spectral_transpose_operands(torch, operands):
    # The stock DConv view gives the singleton batch axis its folded stride.
    # A dense ABCD input also has the canonical contiguous batch stride.
    source, weight, bias = operands
    batch_stride = source.stride(0)
    if batch_stride not in {172032, 8257536}:
        msg = "unexpected HTDemucs spectral transpose batch stride"
        raise ValueError(msg)
    geometry = (
        ((1, 48, 512, 336), (batch_stride, 172032, 336, 1), 0, 33030144),
        ((48, 16, 8, 1), (128, 8, 1, 1), 0, 24576),
        ((16,), (1,), 0, 64),
    )
    check_mapped_convolution_operands(torch, (source, weight, bias), geometry)


def mapped_spectral_transpose_storage(torch, output: Path, label: str, live_spans):
    owner = mapped_reference_scratch(output, SPECTRAL_TRANSPOSE_BYTES, label)
    mapped = torch.frombuffer(owner, dtype=torch.float32).view(SPECTRAL_TRANSPOSE_SHAPE)
    _, storage, address = tensor_backing(mapped)
    if (
        address % STATE_ALIGNMENT
        or mapped.untyped_storage().resizable()
        or state_storage_geometry(mapped) != SPECTRAL_TRANSPOSE_DENSE
        or any(
            storage == other_storage
            or (address < start + size and start < address + SPECTRAL_TRANSPOSE_BYTES)
            for other_storage, start, size in live_spans
        )
    ):
        msg = "unexpected HTDemucs spectral transpose mapped storage"
        raise ValueError(msg)
    return mapped


def native_spectral_contiguous(torch, output: Path, func, source, cpu, live_spans):
    """Supply the stock contiguous clone's destination while keeping its CPU copy."""
    aten = torch.ops.aten
    source_backing = tensor_backing(source)
    contiguous = {"memory_format": torch.contiguous_format}
    empty_options = {
        "dtype": torch.float32,
        "layout": torch.strided,
        "device": torch.device("cpu"),
        **contiguous,
    }
    backing = None
    copied = False

    def allocate(op, args, options):
        nonlocal backing, copied
        if (
            op == aten.empty_like.default
            and backing is None
            and len(args) == 1
            and tensor_backing(args[0]) == source_backing
            and options == empty_options
        ):
            mapped = mapped_spectral_transpose_storage(torch, output, "transpose-dense", live_spans)
            backing = tensor_backing(mapped)
            return mapped
        if (
            (op, backing is not None, copied, bool(options), len(args))
            == (aten.copy_.default, True, False, False, 2)
            and tensor_backing(args[0]) == backing
            and tensor_backing(args[1]) == source_backing
        ):
            result = op(*args)
            copied = True
            return result
        msg = "unexpected HTDemucs spectral transpose clone operation"
        raise ValueError(msg)

    def clone(op, args, options):
        if (
            op != aten.clone.default
            or backing is not None
            or len(args) != 1
            or tensor_backing(args[0]) != source_backing
            or options != contiguous
        ):
            msg = "unexpected HTDemucs spectral transpose contiguous operation"
            raise ValueError(msg)
        with reference_dispatch_mode(allocate):
            return op.redispatch(cpu, *args, **options)

    with reference_dispatch_mode(clone):
        dense = func.redispatch(cpu, source)
    if (
        not copied
        or tensor_backing(dense) != backing
        or state_storage_geometry(dense) != SPECTRAL_TRANSPOSE_DENSE
    ):
        msg = "unexpected HTDemucs spectral transpose copy result"
        raise ValueError(msg)
    return dense


def restore_spectral_public_layout(op, operands, options, backing, restored):
    if (
        restored
        or tensor_backing(operands[0]) != backing
        or (tuple(operands[1]), tuple(operands[2]))
        != (SPECTRAL_TRANSPOSE_SHAPE, SPECTRAL_TRANSPOSE_PUBLIC_STRIDES)
        or operands[3:] not in {(), (None,), (0,)}
        or options
    ):
        msg = "unexpected HTDemucs spectral transpose public layout"
        raise ValueError(msg)
    return op(*operands, **options)


def resize_spectral_dense(torch, op, operands, options, backing, resized):
    if (
        resized
        or len(operands) != 2
        or tensor_backing(operands[0]) != backing
        or tuple(operands[1]) != SPECTRAL_TRANSPOSE_SHAPE
        or options != {"memory_format": torch.contiguous_format}
    ):
        msg = "unexpected HTDemucs spectral transpose dense resize"
        raise ValueError(msg)
    dense = op(*operands, **options)
    if tensor_backing(dense) != backing:
        msg = "HTDemucs spectral transpose resize replaced the mapped storage"
        raise ValueError(msg)
    return dense


def native_spectral_transpose(torch, output: Path, func, args, cpu):
    """Supply the public and contiguous allocations in the stock dense conversion."""
    aten = torch.ops.aten
    empty_options = {
        "dtype": torch.float32,
        "layout": torch.strided,
        "device": torch.device("cpu"),
    }
    operand_spans = tuple(
        (*tensor_backing(value)[1:], value.untyped_storage().nbytes()) for value in args[:3]
    )
    public_geometry = (
        SPECTRAL_TRANSPOSE_SHAPE,
        SPECTRAL_TRANSPOSE_PUBLIC_STRIDES,
        0,
        SPECTRAL_TRANSPOSE_BYTES,
    )
    placeholder = False
    public_backing = None
    public_layout = False
    dense_backing = None
    resized = False

    def dispatch(op, operands, options):
        nonlocal placeholder, public_backing, public_layout, dense_backing, resized
        if op == aten.empty.memory_format and public_backing is None:
            shape = tuple(operands[0])
            if not placeholder and shape == (0,) and options == empty_options:
                placeholder = True
                return op(*operands, **options)
            if not placeholder or shape != SPECTRAL_TRANSPOSE_SHAPE or options != empty_options:
                msg = "unexpected HTDemucs spectral transpose allocation order"
                raise ValueError(msg)
            mapped = mapped_spectral_transpose_storage(
                torch, output, "transpose-public", operand_spans
            )
            public_backing = tensor_backing(mapped)
            return mapped
        if op == aten.as_strided_.default and public_backing is not None:
            result = restore_spectral_public_layout(
                op, operands, options, public_backing, public_layout
            )
            public_layout = True
            return result
        if op == aten.contiguous.default and public_layout:
            if (
                dense_backing is not None
                or len(operands) != 1
                or options
                or tensor_backing(operands[0]) != public_backing
                or state_storage_geometry(operands[0]) != public_geometry
            ):
                msg = "unexpected HTDemucs spectral transpose public source"
                raise ValueError(msg)
            live_spans = (*operand_spans, (*public_backing[1:], SPECTRAL_TRANSPOSE_BYTES))
            dense = native_spectral_contiguous(torch, output, op, operands[0], cpu, live_spans)
            dense_backing = tensor_backing(dense)
            return dense
        if op == aten.resize_.default and dense_backing is not None:
            dense = resize_spectral_dense(torch, op, operands, options, dense_backing, resized)
            resized = True
            return dense
        return op(*operands, **options)

    with reference_dispatch_mode(dispatch):
        dense = func.redispatch(cpu, *args)
    if (
        not resized
        or tensor_backing(dense) != dense_backing
        or state_storage_geometry(dense) != SPECTRAL_TRANSPOSE_DENSE
    ):
        msg = "unexpected HTDemucs spectral transpose dense return"
        raise ValueError(msg)
    return dense


@contextmanager
def mapped_spectral_transpose(torch, conv, output: Path):
    """Map the native public and dense returns at the final spectral convolution."""
    stride, padding, dilation, output_padding = (4, 1), (0, 0), (1, 1), (0, 0)
    if (
        type(conv) is not torch.nn.ConvTranspose2d
        or "forward" in vars(conv)
        or (conv.stride, conv.padding, conv.dilation, conv.output_padding)
        != (stride, padding, dilation, output_padding)
        or (conv.groups, conv.padding_mode) != (1, "zeros")
    ):
        msg = "unexpected HTDemucs final spectral transpose module"
        raise ValueError(msg)
    stock = conv.forward
    aten = torch.ops.aten
    cpu = torch._C.DispatchKeySet(torch._C.DispatchKey.CPU)
    transpose_args = (stride, padding, dilation, True, output_padding, 1)
    wrapper_args = {
        aten.conv_transpose2d.input: (stride,),
        aten.convolution.default: transpose_args,
    }
    convolution_ops = {*wrapper_args, aten._convolution.default}

    def forward(source):
        check_reference_inference_dispatch(torch, "spectral transpose")
        operands = (source, conv.weight, conv.bias)
        backend = torch._C._select_conv_backend(*operands, *transpose_args)
        if backend == torch._C._ConvBackend.SlowTranspose2d:
            return stock(source)
        # The pinned Python enum omits the name for MkldnnTranspose.
        if backend.value != 9:
            msg = "unexpected HTDemucs spectral transpose backend"
            raise ValueError(msg)
        check_spectral_transpose_operands(torch, operands)
        # The recursive dispatcher may outlive this call until Python collects its cycle.
        expected_tensors = tuple(value._cdata for value in operands)
        used_native = False

        def dispatch(op, args, options):
            nonlocal used_native
            wrapper = op in wrapper_args
            expected = wrapper_args.get(op, transpose_args)
            parameters = args[3:] if wrapper else args[3:-4]
            if (
                op not in convolution_ops
                or len(args) != 3 + len(expected) + (0 if wrapper else 4)
                or options
                or tuple(value._cdata for value in args[:3]) != expected_tensors
                or tuple(tuple(value) if isinstance(value, list) else value for value in parameters)
                != expected
            ):
                msg = "unexpected HTDemucs spectral transpose convolution"
                raise ValueError(msg)
            if wrapper:
                with reference_dispatch_mode(dispatch):
                    return op.redispatch(cpu, *args)
            if used_native:
                msg = "unexpected HTDemucs native spectral transpose convolution"
                raise ValueError(msg)
            used_native = True
            return native_spectral_transpose(torch, output, op, args, cpu)

        with reference_dispatch_mode(dispatch):
            dense = stock(source)
        if not used_native:
            msg = "missing HTDemucs native spectral transpose"
            raise ValueError(msg)
        return dense

    conv.forward = forward
    try:
        yield
    finally:
        del conv.forward


def native_attention_with_score(torch, owner: memoryview, tokens: int, func, args):
    """Redispatch to the pinned native CPU MHA with one substituted allocation."""
    aten = torch.ops.aten
    cpu = torch._C.DispatchKeySet(torch._C.DispatchKey.CPU)
    packed = ((HEADS, tokens, WIDTH), (tokens * WIDTH, WIDTH, 1))
    transposed = ((HEADS, WIDTH, tokens), (tokens * WIDTH, 1, WIDTH))
    score = attention_score_tensor(torch, owner, (HEADS, tokens, tokens))
    storage = score.untyped_storage()._cdata
    ptr = score.data_ptr()
    q_ptr = 0
    step = 0

    def aliases_score(tensor):
        return tensor.untyped_storage()._cdata == storage and tensor.data_ptr() == ptr

    def dispatch(op, operands, options):
        nonlocal step, score, q_ptr
        if op == aten.bmm.default:
            q, k = operands
            if (
                step != 0
                or (q.shape, q.stride()) != packed
                or (k.shape, k.stride()) != transposed
                or (q.dtype, k.dtype) != (torch.float32, torch.float32)
            ):
                msg = "unexpected HTDemucs score operands"
                raise ValueError(msg)
            q_ptr = q.data_ptr()
            step = 1
            return aten.bmm.out(q, k, out=score)
        if op == aten._softmax.out:
            logits, dim, half_to_float = operands
            if (
                (step, dim, half_to_float) != (1, 3, False)
                or logits.shape != (1, HEADS, tokens, tokens)
                or not logits.is_contiguous()
                or not aliases_score(logits)
                or not aliases_score(options["out"])
            ):
                msg = "unexpected HTDemucs in-place score softmax"
                raise ValueError(msg)
            step = 2
        if op == aten.bmm.out:
            probabilities, value = operands
            if (
                step != 2
                or not aliases_score(probabilities)
                or (value.shape, value.stride()) != packed
                or options["out"].data_ptr() != q_ptr
            ):
                msg = "unexpected HTDemucs attention value write"
                raise ValueError(msg)
            step = 3
            score = None  # The native score view owns the storage through this value BMM.
        return op(*operands, **options)

    # A fresh inner mode sees the C++ kernel's ATen calls. Redispatch only the
    # outer call to CPU so it does not enter the Python fallback again.
    with reference_dispatch_mode(dispatch):
        result = func.redispatch(cpu, *args)
    if step != 3 or result[1] is not None:
        msg = "incomplete HTDemucs mapped score lifecycle"
        raise ValueError(msg)
    return result


def native_self_attention(torch, tokens: int, native_score):
    """Guard the stock native MHA entry before any projection or score."""
    aten = torch.ops.aten
    stock = torch.nn.MultiheadAttention.forward

    def forward(attention, query, key, value, **options):
        if (
            query is not key
            or key is not value
            or query.shape != (1, tokens, HEADS * WIDTH)
            or (query.device.type, query.dtype) != ("cpu", torch.float32)
        ):
            msg = "unexpected HTDemucs self-attention input"
            raise ValueError(msg)
        if (
            attention.training
            or not attention.batch_first
            or options.get("need_weights") is not False
            or not torch.is_inference_mode_enabled()
            or not torch.backends.mha.get_fastpath_enabled()
        ):
            msg = "expected pinned CPU inference HTDemucs self-attention"
            raise ValueError(msg)
        if (
            torch.is_autocast_enabled("cpu")
            or torch._C._len_torch_dispatch_stack()
            or options.get("attn_mask") is not None
            or options.get("key_padding_mask") is not None
            or options.get("is_causal")
        ):
            msg = "unexpected HTDemucs self-attention dispatch state"
            raise ValueError(msg)
        seen = False

        def dispatch(func, args, kwargs):
            nonlocal seen
            # The pinned unmasked Python fast path reaches the native operator
            # before any other ATen op. Refuse fallback before projections/SDPA.
            if func != aten._native_multi_head_attention.default:
                msg = "HTDemucs self-attention left the native path"
                raise ValueError(msg)
            if (
                seen
                or kwargs
                or args[3:5] != (HEADS * WIDTH, HEADS)
                or args[9:11] != (None, False)
                or args[12:] not in {(), (None,)}
            ):
                msg = "unexpected HTDemucs native self-attention options"
                raise ValueError(msg)
            seen = True
            return native_score(func, args, attention, query)

        with reference_dispatch_mode(dispatch):
            result = stock(attention, query, key, value, **options)
        if not seen:
            msg = "missing native HTDemucs self-attention"
            raise ValueError(msg)
        return result

    return forward


def mapped_self_attention(torch, owner: memoryview, tokens: int):
    """Keep the stock MHA entry point and intercept only its native score."""

    def mapped_score(func, args, _attention, _query):
        return native_attention_with_score(torch, owner, tokens, func, args)

    return native_self_attention(torch, tokens, mapped_score)


def temporal_self_attention(torch):
    """Require the native temporal route with the separately charged private score."""

    def private_score(func, args, attention, query):
        if args[11:12] not in {(), (True,)} or any(
            actual._cdata != expected._cdata
            for actual, expected in zip(
                (*args[:3], *args[5:9]),
                (query, query, query, *eager_attention_parameters(attention)),
                strict=True,
            )
        ):
            msg = "unexpected HTDemucs temporal native attention operands"
            raise ValueError(msg)
        return func(*args)

    stock = native_self_attention(torch, TIME_TOKENS, private_score)

    def forward(attention, query, key, value, **options):
        check_eager_attention_input(torch, query, TIME_TOKENS)
        check_eager_attention_module(torch, attention, options)
        return stock(attention, query, key, value, **options)

    return forward


def eager_attention_parameters(attention):
    return (
        attention.in_proj_weight,
        attention.in_proj_bias,
        attention.out_proj.weight,
        attention.out_proj.bias,
    )


def check_eager_attention_tensor(torch, tensor, geometry) -> None:
    if type(tensor) not in {torch.Tensor, torch.nn.Parameter} or (
        tensor.device.type,
        tensor.dtype,
        tensor.layout,
        tensor.is_nested,
        tensor.is_conj(),
        tensor.is_neg(),
        state_storage_geometry(tensor),
    ) != ("cpu", torch.float32, torch.strided, False, False, False, geometry):
        msg = "unexpected HTDemucs eager attention tensor"
        raise ValueError(msg)


def check_eager_attention_input(torch, tensor, tokens: int) -> None:
    embed = HEADS * WIDTH
    check_eager_attention_tensor(
        torch, tensor, ((1, tokens, embed), (tokens * embed, embed, 1), 0, tokens * embed * 4)
    )


def check_eager_attention_runtime(torch) -> None:
    check_reference_inference_dispatch(torch, "eager attention")
    check_spectral_convolution_environment()
    if (
        torch.is_grad_enabled(),
        torch.get_num_threads(),
        torch.get_num_interop_threads(),
        torch._C._get_cpu_capability(),
        torch._C._get_fp32_precision_getter("mkldnn", "matmul"),
        torch.get_float32_matmul_precision(),
    ) != (False, 1, 1, "AVX512", "none", "highest"):
        msg = "unexpected HTDemucs eager attention runtime"
        raise ValueError(msg)


def check_eager_attention_module(torch, attention, options) -> None:
    check_eager_attention_runtime(torch)
    if (
        attention.training,
        attention.batch_first,
        attention._qkv_same_embed_dim,
        attention.embed_dim,
        attention.num_heads,
        attention.bias_k is None,
        attention.bias_v is None,
        attention.add_zero_attn,
        options.get("need_weights") is False,
        options.get("key_padding_mask") is None,
        options.get("attn_mask") is None,
        options.get("is_causal", False) is False,
        options.get("average_attn_weights", True) is True,
    ) != (False, True, True, HEADS * WIDTH, HEADS, True, True, False, True, True, True, True, True):
        msg = "unexpected HTDemucs eager attention module or options"
        raise ValueError(msg)
    embed = HEADS * WIDTH
    for tensor, shape, strides in zip(
        eager_attention_parameters(attention),
        ((3 * embed, embed), (3 * embed,), (embed, embed), (embed,)),
        ((embed, 1), (1,), (embed, 1), (1,)),
        strict=True,
    ):
        check_eager_attention_tensor(torch, tensor, (shape, strides, 0, prod(shape) * 4))


def eager_cross_sdpa(torch, queries: int, keys: int, backend: CrossAttentionBackend):
    aten = torch.ops.aten
    embed = HEADS * WIDTH
    strides = (embed, WIDTH, embed, 1)

    def sdpa(q, k, v, attn_mask, dropout_p, is_causal):
        check_eager_attention_runtime(torch)
        if attn_mask is not None or dropout_p != 0.0 or is_causal is not False:
            msg = "unexpected HTDemucs cross SDPA options"
            raise ValueError(msg)
        for tensor, tokens, offset, elements in (
            (q, queries, 0, queries * embed),
            (k, keys, 0, 2 * keys * embed),
            (v, keys, keys * embed, 2 * keys * embed),
        ):
            check_eager_attention_tensor(
                torch, tensor, ((1, HEADS, tokens, WIDTH), strides, offset, elements * 4)
            )
        q_storage, k_storage, v_storage = (value.untyped_storage()._cdata for value in (q, k, v))
        if k_storage != v_storage or q_storage == k_storage:
            msg = "unexpected HTDemucs cross SDPA projection storage"
            raise ValueError(msg)
        selected = aten._fused_sdp_choice.default(q, k, v)
        if selected != backend.value:
            msg = f"HTDemucs cross SDPA selected {selected}, admission requires {backend.name}"
            raise ValueError(msg)
        # The pinned unmasked CPU composite calls these same operators. Invoke
        # the selected operator directly so a second choice cannot switch routes.
        if backend is CrossAttentionBackend.FLASH:
            return aten._scaled_dot_product_flash_attention_for_cpu.default(q, k, v)[0]
        return aten._scaled_dot_product_attention_math.default(q, k, v)[0]

    return sdpa


def eager_cross_attention(torch, queries: int, keys: int, backend: CrossAttentionBackend):
    """Keep the pinned stock MHA projection code and bind its local SDPA call."""
    functional = torch.nn.functional
    projection = functional.multi_head_attention_forward
    stock = torch.nn.MultiheadAttention.forward
    guarded_projection = FunctionType(
        projection.__code__,
        {
            **projection.__globals__,
            "scaled_dot_product_attention": eager_cross_sdpa(torch, queries, keys, backend),
        },
        projection.__name__,
        projection.__defaults__,
        projection.__closure__,
    )
    guarded_projection.__kwdefaults__ = projection.__kwdefaults__
    local_functional = SimpleNamespace(
        _canonical_mask=functional._canonical_mask,
        _none_or_dtype=functional._none_or_dtype,
        multi_head_attention_forward=guarded_projection,
    )
    guarded_forward = FunctionType(
        stock.__code__,
        {**stock.__globals__, "F": local_functional},
        stock.__name__,
        stock.__defaults__,
        stock.__closure__,
    )
    guarded_forward.__kwdefaults__ = stock.__kwdefaults__

    def forward(attention, query, key, value, **options):
        check_eager_attention_module(torch, attention, options)
        if query is key or key is not value:
            msg = "unexpected HTDemucs cross-attention aliases"
            raise ValueError(msg)
        check_eager_attention_input(torch, query, queries)
        check_eager_attention_input(torch, key, keys)
        return guarded_forward(attention, query, key, value, **options)

    return forward


@contextmanager
def mapped_reference_attention(torch, model, output: Path, admission: EagerAttentionAdmission):
    """Guard the ten eager attention calls and reuse the spectral score extent."""
    attention_type = torch.nn.MultiheadAttention
    transformer = model.crosstransformer
    spectral, temporal = transformer.layers, transformer.layers_t
    spectral_self = [spectral[index].self_attn for index in (0, 2, 4)]
    temporal_self = [temporal[index].self_attn for index in (0, 2, 4)]
    spectral_cross = [spectral[index].cross_attn for index in (1, 3)]
    temporal_cross = [temporal[index].cross_attn for index in (1, 3)]
    sites = spectral_self + temporal_self + spectral_cross + temporal_cross
    if len({id(layer) for layer in sites}) != 10 or any(
        type(layer) is not attention_type or "forward" in vars(layer) for layer in sites
    ):
        msg = "unexpected HTDemucs eager attention modules"
        raise ValueError(msg)
    with mapped_attention_score(output, SPECTRAL_TOKENS) as owner, ExitStack() as restore:
        routes = (
            (spectral_self, mapped_self_attention(torch, owner, SPECTRAL_TOKENS)),
            (temporal_self, temporal_self_attention(torch)),
            (
                spectral_cross,
                eager_cross_attention(
                    torch, SPECTRAL_TOKENS, TIME_TOKENS, admission.spectral_queries
                ),
            ),
            (
                temporal_cross,
                eager_cross_attention(
                    torch, TIME_TOKENS, SPECTRAL_TOKENS, admission.temporal_queries
                ),
            ),
        )
        for layers, forward in routes:
            for layer in layers:
                layer.forward = MethodType(forward, layer)
                restore.callback(delattr, layer, "forward")
        yield


def original_forward(torch, model, mix, output: Path, *, mapped_forward: MethodType | None = None):
    """Write the four stock hook tensors at their source lifetime boundary."""
    artifacts = {}

    def capture(name, value):
        tensor = value.detach().view(SHAPES[name])
        if name in {"spectral", "waveform"} and not tensor.is_contiguous():
            msg = f"unexpected original HTDemucs input layout: {name}"
            raise ValueError(msg)
        artifacts[name] = write_tensor(torch, output / f"{name}.f32", tensor, SHAPES[name])

    with ExitStack() as hooks:
        hooks.enter_context(
            model.encoder[0].register_forward_pre_hook(lambda _, args: capture("spectral", args[0]))
        )
        hooks.enter_context(
            model.tencoder[0].register_forward_pre_hook(
                lambda _, args: capture("waveform", args[0])
            )
        )
        hooks.enter_context(
            model.decoder[-1].register_forward_hook(
                lambda _, _args, result: capture("decoded_spectral", result[0])
            )
        )
        hooks.enter_context(
            model.tdecoder[-1].register_forward_hook(
                lambda _, _args, result: capture("decoded_waveform", result[0])
            )
        )
        if mapped_forward is not None:
            if mix.device.type != "cpu" or not torch.is_inference_mode_enabled():
                msg = "mapped HTDemucs reference requires CPU inference mode"
                raise ValueError(msg)
            model.forward = mapped_forward
            hooks.callback(delattr, model, "forward")
        pcm = model(mix)
    if artifacts.keys() != SHAPES.keys():
        msg = "missing original HTDemucs hook output"
        raise ValueError(msg)
    return artifacts, pcm


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


def graph_value_producer(graph, name: str):
    matches = [node for node in graph.node if name in node.output]
    if len(matches) != 1:
        msg = f"unexpected HTDemucs ONNX producer: {name}"
        raise ValueError(msg)
    return matches[0]


def check_trace_output_reshape(onnx, graph, name: str, shape: tuple[int, ...]) -> None:
    reshape = graph_value_producer(graph, name)
    allowzero = onnx.helper.make_attribute("allowzero", 0)
    if (
        (reshape.domain, reshape.op_type, len(reshape.input)) != ("", "Reshape", 2)
        or list(reshape.output) != [name]
        or not all(reshape.input)
        or list(reshape.attribute) not in ([], [allowzero])
    ):
        msg = f"unexpected HTDemucs traced output Reshape: {name}"
        raise ValueError(msg)

    shape_name = reshape.input[1]
    constant = graph_value_producer(graph, shape_name)
    tensor = onnx.TensorProto(
        data_type=onnx.TensorProto.INT64,
        dims=[len(shape)],
        raw_data=pack(f"<{len(shape)}q", *shape),
    )
    if (
        (constant.domain, constant.op_type, len(constant.input)) != ("", "Constant", 0)
        or list(constant.output) != [shape_name]
        or list(constant.attribute) != [onnx.helper.make_attribute("value", tensor)]
    ):
        msg = f"unexpected HTDemucs traced output shape constant: {name}"
        raise ValueError(msg)


def bind_trace_output_shapes(onnx, graph) -> None:
    """Resolve legacy output symbols from the fixed terminal Reshape targets."""
    outputs = graph.graph.output
    if [value.name for value in outputs] != list(SHAPES)[2:]:
        msg = "unexpected HTDemucs traced output names"
        raise ValueError(msg)
    updates = []
    for value in outputs:
        tensor = value.type.tensor_type
        shape = SHAPES[value.name]
        dimensions = tensor.shape.dim
        if tensor.elem_type != onnx.TensorProto.FLOAT or len(dimensions) != len(shape):
            msg = f"unexpected HTDemucs traced output type or rank: {value.name}"
            raise ValueError(msg)
        pending = []
        for dimension, extent in zip(dimensions, shape, strict=True):
            if dimension.dim_value == extent:
                continue
            if not dimension.dim_param:
                msg = f"unexpected HTDemucs traced output dimension: {value.name}"
                raise ValueError(msg)
            pending.append((dimension, extent))
        if pending:
            check_trace_output_reshape(onnx, graph.graph, value.name, shape)
            updates.extend(pending)
    for dimension, extent in updates:
        dimension.dim_value = extent


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
    numpy = import_module("numpy")
    array = numpy.empty(shape, dtype="<f4")
    digest = hashlib.sha256()
    with path.open("rb") as stream, memoryview(array).cast("B") as raw:
        for start in range(0, extent, STREAM_CHUNK_BYTES):
            chunk = raw[start : start + STREAM_CHUNK_BYTES]
            if stream.readinto(chunk) != chunk.nbytes:
                msg = f"truncated HTDemucs tensor: {path.name}"
                raise ValueError(msg)
            digest.update(chunk)
            if not numpy.isfinite(numpy.frombuffer(chunk, dtype="<f4")).all():
                msg = f"nonfinite HTDemucs tensor: {path.name}"
                raise ValueError(msg)
        if stream.read(1) or digest.hexdigest() != artifact.sha256:
            msg = f"HTDemucs tensor differs: {path.name}"
            raise ValueError(msg)
    return torch.from_numpy(array)


def tensor_blocks(torch, path: Path, value, shape: tuple[int, ...]):
    """Iterate raw little-endian float32 blocks, including strided decoder views."""
    if tuple(value.shape) != shape or value.dtype != torch.float32:
        msg = f"invalid original HTDemucs tensor: {path.name}"
        raise ValueError(msg)
    numpy = import_module("numpy")
    with numpy.nditer(
        value.numpy(),
        flags=["external_loop", "buffered"],
        op_flags=["readonly", "contig"],
        op_dtypes=numpy.dtype("<f4"),
        order="C",
        buffersize=STREAM_CHUNK_BYTES // 4,
    ) as blocks:
        for block in blocks:
            if not numpy.isfinite(block).all():
                msg = f"nonfinite original HTDemucs tensor: {path.name}"
                raise ValueError(msg)
            yield block


def write_tensor(torch, path: Path, value, shape: tuple[int, ...]) -> Artifact:
    extent = prod(shape) * 4
    digest = hashlib.sha256()
    with path.open("xb") as stream:
        for block in tensor_blocks(torch, path, value, shape):
            with memoryview(block).cast("B") as raw:
                if stream.write(raw) != raw.nbytes:
                    msg = f"incomplete original HTDemucs tensor write: {path.name}"
                    raise OSError(msg)
                digest.update(raw)
        if stream.tell() != extent:
            msg = f"incomplete original HTDemucs tensor: {path.name}"
            raise ValueError(msg)
    return Artifact(extent, digest.hexdigest())


def check_reference_tensor(torch, path: Path, artifact: Artifact, value, shape: tuple[int, ...]):
    """Check numeric equality with the exact persisted original decoder bytes."""
    if artifact.size != prod(shape) * 4:
        msg = f"unexpected HTDemucs decoder reference extent: {path.name}"
        raise ValueError(msg)
    numpy = import_module("numpy")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in tensor_blocks(torch, path, value, shape):
            raw = stream.read(block.nbytes)
            if len(raw) != block.nbytes:
                msg = f"truncated HTDemucs decoder reference: {path.name}"
                raise ValueError(msg)
            if not numpy.array_equal(block, numpy.frombuffer(raw, dtype="<f4")):
                msg = f"HTDemucs extracted core differs: {path.name}"
                raise ValueError(msg)
            digest.update(raw)
        if (
            stream.tell() != artifact.size
            or stream.read(1)
            or digest.hexdigest() != artifact.sha256
        ):
            msg = f"HTDemucs decoder reference differs: {path.name}"
            raise ValueError(msg)


def core_wrapper(
    torch, model, source: bytes, globals_, *, release_decoder: bool = False, embedding_add=None
):
    namespace = {}
    # The caller has verified the source bytes.
    code = core_code(
        source, release_decoder=release_decoder, map_embedding_add=embedding_add is not None
    )
    exec(code, globals_, namespace)
    normalized = namespace["normalized"]

    class Core(torch.nn.Module):
        def __init__(self, original):
            super().__init__()
            self.model = original

        def forward(self, spectral, waveform):
            return normalized(self.model, spectral, waveform, embedding_add)

    return Core(model).eval()


def check_mapped_reference_admission(
    cpu_overlay: Path | None,
    spectral: SpectralConvolutionAdmission | None,
    attention: EagerAttentionAdmission | None,
) -> None:
    check_spectral_convolution_admission(cpu_overlay, spectral)
    if type(attention) is not EagerAttentionAdmission:
        msg = "mapped HTDemucs reference requires prior eager attention admission"
        raise ValueError(msg)


def capture_reference(
    checkpoint: Path,
    mix_path: Path,
    mix: Artifact,
    output: Path,
    *,
    admit: Callable[[Phase], None],
    cpu_overlay: Path | None = None,
    mapped_state: tuple[Path, ModelStateBank] | None = None,
    spectral_admission: SpectralConvolutionAdmission | None = None,
    attention_admission: EagerAttentionAdmission | None = None,
) -> ReferenceArtifacts:
    """Run the stock forward and check the extracted boundary in an admitted child.

    The caller owns admission from startup through process, file and pipe reader
    retirement and accepts these identities only after successful retirement.
    For mapped state, admission includes the exact backend at each spectral
    convolution and cross-attention direction in both forwards.
    """
    if mapped_state is not None:
        check_mapped_reference_admission(cpu_overlay, spectral_admission, attention_admission)
    begin_phase(Phase.REFERENCE, admit)
    sources = source_files()
    torch = torch_producer(cpu_overlay)
    module = import_module("demucs.htdemucs")
    if mapped_state is not None:
        for backend in spectral_admission.backends:
            check_spectral_convolution_runtime(torch, backend)
        root = cpu_overlay.resolve(strict=True)
        for name, digest in ATTENTION_RUNTIME_PINS.items():
            expected = root / (name.replace(".", "/") + ".py")
            actual = Path(import_module(name).__file__).resolve(strict=True)
            if actual != expected or hashlib.sha256(actual.read_bytes()).hexdigest() != digest:
                msg = f"unexpected HTDemucs attention runtime: {name}"
                raise ValueError(msg)
    output.mkdir(exist_ok=False)
    model = load_producer_model(torch, checkpoint, module, cpu_overlay, mapped_state)
    waveform = read_tensor(torch, mix_path, mix, SHAPES["waveform"])
    embedding_add = (
        mapped_frequency_embedding_add(torch, output) if mapped_state is not None else None
    )
    wrapper = core_wrapper(
        torch,
        model,
        sources["demucs/htdemucs.py"],
        vars(module),
        release_decoder=mapped_state is not None,
        embedding_add=embedding_add,
    )
    mapped_forward = (
        mapped_reference_forward(
            model,
            module,
            sources["demucs/htdemucs.py"],
            mapped_reference_pad2(torch, output),
            mapped_reference_ispectro(torch, output, module.ispectro),
            embedding_add=embedding_add,
        )
        if mapped_state is not None
        else None
    )
    with torch.inference_mode(), ExitStack() as reference:
        if mapped_state is not None:
            reference.enter_context(
                mapped_reference_attention(torch, model, output, attention_admission)
            )
            reference.enter_context(
                mapped_spectral_caller_gelu(
                    torch, model.decoder[-2], sources["demucs/hdemucs.py"], output
                )
            )
            reference.enter_context(
                mapped_waveform_skip_glu(
                    torch, model.tencoder[0], sources["demucs/hdemucs.py"], output
                )
            )
            reference.enter_context(
                mapped_waveform_caller_gelu(
                    torch, model.tdecoder[-2], sources["demucs/hdemucs.py"], output
                )
            )
            reference.enter_context(
                mapped_spectral_skip_add(
                    torch, model.decoder[-1], sources["demucs/hdemucs.py"], output
                )
            )
            reference.enter_context(
                mapped_spectral_rewrite(
                    torch, model.decoder[-1].rewrite, output, backend=spectral_admission.rewrite
                )
            )
            for layer, backend in zip(
                model.decoder[-1].dconv.layers,
                (spectral_admission.first_k1, spectral_admission.second_k1),
                strict=True,
            ):
                reference.enter_context(
                    mapped_spectral_dconv_pointwise(torch, layer[3], output, backend=backend)
                )
            reference.enter_context(
                mapped_spectral_dconv_tail(
                    torch, model.decoder[-1].dconv, sources["demucs/demucs.py"], output
                )
            )
            reference.enter_context(
                mapped_spectral_transpose(torch, model.decoder[-1].conv_tr, output)
            )
        artifacts, pcm = original_forward(
            torch, model, waveform, output, mapped_forward=mapped_forward
        )
        del waveform
        original_pcm = write_tensor(torch, output / "original_pcm.f32", pcm, PCM_SHAPE)
        del pcm
        inputs = tuple(
            read_tensor(torch, output / f"{name}.f32", artifacts[name], SHAPES[name])
            for name in ("spectral", "waveform")
        )
        for name, value in zip(tuple(SHAPES)[2:], wrapper(*inputs), strict=True):
            check_reference_tensor(
                torch, output / f"{name}.f32", artifacts[name], value, SHAPES[name]
            )
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


@contextmanager
def trace_position_buffers(torch, transformer, source: bytes):
    """Use source-generated position data without tracing its construction."""
    upstream = import_module("demucs.transformer")
    if (
        type(transformer) is not upstream.CrossTransformerEncoder
        or "forward" in vars(transformer)
        or (
            transformer.emb,
            transformer.sin_random_shift,
            transformer.max_period,
            transformer.weight_pos_embed,
        )
        != ("sin", 0, 10000, 1)
        or torch.get_default_dtype() != torch.float32
        or torch.get_default_device().type != "cpu"
    ):
        msg = "unexpected HTDemucs trace position embedding configuration"
        raise ValueError(msg)

    channels = WIDTH * HEADS
    height = SPECTRAL_TOKENS // SHAPES["spectral"][-1]
    width = SHAPES["spectral"][-1]
    spectral_shape = (1, channels, height, width)
    waveform_shape = (1, channels, TIME_TOKENS)
    axis_shape = (1, channels // 2, height, width)
    replacements = {
        "pos_emb_2d = create_2d_sin_embedding(C, Fr, T1, x.device, self.max_period)": (
            f"torch.cat((self.trace_positions_width.expand{axis_shape}, "
            f"self.trace_positions_height.expand{axis_shape}), dim=1)"
        ),
        "pos_emb = self._get_pos_embedding(T2, B, C, x.device)": "self.trace_positions_time",
    }
    forward = method_definition(source, "CrossTransformerEncoder")
    for statement in forward.body:
        if isinstance(statement, ast.Assign):
            expression = replacements.pop(ast.unparse(statement), None)
            if expression is not None:
                statement.value = ast.parse(expression, mode="eval").body
    if replacements:
        msg = "HTDemucs trace position embedding source differs"
        raise ValueError(msg)
    tree = ast.fix_missing_locations(ast.Module(body=[forward], type_ignores=[]))
    namespace = {}
    exec(
        compile(tree, "demucs/transformer.py (trace positions)", "exec"), vars(upstream), namespace
    )

    def check_geometry(_module, args):
        for value, shape in zip(args, (spectral_shape, waveform_shape), strict=True):
            if (
                tuple(value.shape) != shape
                or value.dtype != torch.float32
                or value.device.type != "cpu"
            ):
                msg = "unexpected HTDemucs trace transformer input"
                raise ValueError(msg)

    # The upstream spectral function repeats each channel half along one axis.
    # Clone the two lanes so the initializer bank does not retain the full table.
    spectral = upstream.create_2d_sin_embedding(channels, height, width, "cpu", 10000)
    positions = {
        "trace_positions_width": spectral[:, : channels // 2, :1, :].clone(
            memory_format=torch.contiguous_format
        ),
        "trace_positions_height": spectral[:, channels // 2 :, :, :1].clone(
            memory_format=torch.contiguous_format
        ),
    }
    del spectral
    positions["trace_positions_time"] = upstream.create_sin_embedding(
        TIME_TOKENS, channels, device="cpu", max_period=10000
    )
    with ExitStack() as cleanup:
        for name, value in positions.items():
            transformer.register_buffer(name, value)
            cleanup.callback(delattr, transformer, name)
        cleanup.enter_context(transformer.register_forward_pre_hook(check_geometry))
        transformer.forward = namespace["forward"].__get__(transformer)
        cleanup.callback(delattr, transformer, "forward")
        yield


@contextmanager
def infer_shapes_without_initializers(torch):
    """Keep geometry inference while withholding learned values from its graph sweeps."""
    infer = torch._C._jit_pass_onnx_graph_shape_type_inference
    sweeps = 0

    def infer_graph(graph, _parameters, opset):
        nonlocal sweeps
        sweeps += 1
        # Input TensorTypes already describe the parameters' shapes and dtypes.
        # Seeding ConstantValueMap with their values can serialize them into
        # per-node inference models or evaluate weight-dependent expressions.
        infer(graph, {}, opset)

    torch._C._jit_pass_onnx_graph_shape_type_inference = infer_graph
    try:
        yield
        if sweeps != 2:
            msg = "unexpected number of HTDemucs legacy shape sweeps"
            raise ValueError(msg)
    finally:
        torch._C._jit_pass_onnx_graph_shape_type_inference = infer


@contextmanager
def trace_with_metadata_outputs(torch, utils):
    """Release the eager trace output backings before ONNX lowering."""
    create_graph = utils._create_jit_graph

    def create_graph_with_metadata(model, args):
        graph, parameters, outputs, module = create_graph(model, args)
        if not isinstance(outputs, tuple) or len(outputs) != 2:
            msg = "HTDemucs trace requires two output examples"
            raise ValueError(msg)
        examples = []
        for output in outputs:
            if (
                not isinstance(output, torch.Tensor)
                or output.device.type != "cpu"
                or output.dtype != torch.float32
                or output.layout != torch.strided
            ):
                msg = "HTDemucs trace output examples must be strided CPU float32 tensors"
                raise ValueError(msg)
            # The pinned C++ output-shape pass reads only TensorType metadata.
            # ZeroTensor represents the storage extent with a null data pointer.
            extent = output.untyped_storage().nbytes() // output.element_size()
            backing = torch._efficientzerotensor(
                (extent,), dtype=output.dtype, device=output.device
            )
            example = backing.as_strided(output.shape, output.stride(), output.storage_offset())
            examples.append(example.requires_grad_(output.requires_grad))
        return graph, parameters, tuple(examples), module

    utils._create_jit_graph = create_graph_with_metadata
    try:
        yield
    finally:
        utils._create_jit_graph = create_graph


def export_skeleton(torch, wrapper, inputs):
    """Use the pinned legacy lowering while leaving weights as named graph inputs."""
    utils = import_module("torch.onnx._internal.torchscript_exporter.utils")
    settings = utils.GLOBALS
    operator = torch._C._onnx.OperatorExportTypes.ONNX
    if settings.in_onnx_export:
        msg = "nested HTDemucs legacy graph export"
        raise ValueError(msg)
    previous = (
        settings.in_onnx_export,
        settings.export_onnx_opset_version,
        settings.operator_export_type,
        settings.onnx_shape_inference,
        settings.autograd_inlining,
    )
    settings.in_onnx_export = True
    settings.export_onnx_opset_version = 18
    settings.operator_export_type = operator
    settings.onnx_shape_inference = True
    settings.autograd_inlining = True
    try:
        utils._trigger_symbolic_function_registration()
        utils._setup_trace_module_map(wrapper, export_modules_as_functions=False)
        with (
            utils.exporter_context(wrapper, torch._C._onnx.TrainingMode.EVAL, verbose=False),
            infer_shapes_without_initializers(torch),
        ):
            with trace_with_metadata_outputs(torch, utils):
                graph, weights, examples = utils._model_to_graph(
                    wrapper,
                    inputs,
                    input_names=list(SHAPES)[:2],
                    output_names=list(SHAPES)[2:],
                    operator_export_type=operator,
                    training=torch._C._onnx.TrainingMode.EVAL,
                    do_constant_folding=False,
                    dynamic_axes={},
                )
            del examples
            torch._C._jit_pass_dce_allow_deleting_nodes_with_side_effects(graph)
            weights = torch._C._jit_pass_onnx_deduplicate_initializers(
                graph,
                weights,
                wrapper.training,
            )
            torch._C._jit_pass_onnx_assign_scoped_names_for_node_and_value(graph)
            # The binding converts every deferred tensor to bytes if deferral
            # is enabled. An empty initializer map avoids that full owner.
            proto, exported, external, _ = graph._export_onnx(
                initializers={},
                onnx_opset_version=18,
                dynamic_axes={},
                defer_weight_export=False,
                operator_export_type=operator,
                strip_doc_string=True,
                keep_initializers_as_inputs=False,
                custom_opsets={MARKER_DOMAIN: 1},
                add_node_names=True,
                onnx_file_path="",
                node_attr_to_name={},
            )
        names = [item.debugName() for item in graph.inputs()][2:]
        if exported or external or len(names) != len(weights) or set(names) != weights.keys():
            msg = "HTDemucs legacy graph has unmatched parameter inputs"
            raise ValueError(msg)
        return proto, {name: weights[name] for name in names}
    finally:
        (
            settings.in_onnx_export,
            settings.export_onnx_opset_version,
            settings.operator_export_type,
            settings.onnx_shape_inference,
            settings.autograd_inlining,
        ) = previous
        utils._reset_trace_module_map()


def write_initializers(path: Path, weights) -> tuple[list[Initializer], Artifact]:
    """Write one contiguous initializer at a time and record the bytes written."""
    manifest = []
    digest = hashlib.sha256()
    with path.open("xb") as stream:
        for name, value in weights.items():
            array = value.detach().contiguous().numpy()
            with memoryview(array).cast("B") as raw:
                if not raw.nbytes or stream.tell() + raw.nbytes > MAX_GRAPH_BYTES:
                    msg = "HTDemucs initializer bank exceeds the export extent"
                    raise ValueError(msg)
                manifest.append(
                    Initializer(
                        name,
                        array.dtype.name,
                        tuple(array.shape),
                        raw.nbytes,
                        hashlib.sha256(raw).hexdigest(),
                    )
                )
                if stream.write(raw) != raw.nbytes:
                    msg = "incomplete HTDemucs initializer bank write"
                    raise OSError(msg)
                digest.update(raw)
            del array
        extent = stream.tell()
    return manifest, Artifact(extent, digest.hexdigest())


def trace_core_separated(
    checkpoint: Path,
    references: Path,
    reference: ReferenceArtifacts,
    output: Path,
    *,
    cpu_overlay: Path,
    admit: Callable[[Phase], None],
    mapped_state: tuple[Path, ModelStateBank] | None = None,
) -> SeparatedTrace:
    """Trace the fixed learned core in an admitted CPU-only Torch child."""
    begin_phase(Phase.TRACE, admit)
    sources = source_files()
    torch = torch_producer(cpu_overlay)
    module = import_module("demucs.htdemucs")
    inputs = (
        read_tensor(torch, references / "spectral.f32", reference.spectral, SHAPES["spectral"]),
        read_tensor(torch, references / "waveform.f32", reference.waveform, SHAPES["waveform"]),
    )
    model = load_producer_model(torch, checkpoint, module, cpu_overlay, mapped_state)
    wrapper = core_wrapper(torch, model, sources["demucs/htdemucs.py"], vars(module))
    previous = torch.backends.mha.get_fastpath_enabled()
    attention = import_module("torch.nn.attention")
    try:
        torch.backends.mha.set_fastpath_enabled(False)
        with (
            torch.inference_mode(),
            mark_attention(torch),
            attention.sdpa_kernel(attention.SDPBackend.FLASH_ATTENTION),
            trace_position_buffers(torch, model.crosstransformer, sources["demucs/transformer.py"]),
        ):
            proto, weights = export_skeleton(torch, wrapper, inputs)
    finally:
        torch.backends.mha.set_fastpath_enabled(previous)
    del wrapper, model, inputs
    if source_files() != sources or not 0 < len(proto) <= MAX_SKELETON_BYTES or not weights:
        msg = "HTDemucs separated trace source or skeleton differs"
        raise ValueError(msg)

    output.mkdir(exist_ok=False)
    skeleton_path = output / "skeleton.onnx"
    weights_path = output / "weights.bin"
    manifest_path = output / "initializers.json"
    with skeleton_path.open("xb") as stream:
        stream.write(proto)
    manifest, bank = write_initializers(weights_path, weights)
    encoded_manifest = json.dumps([asdict(item) for item in manifest]).encode("utf-8")
    if len(encoded_manifest) > MAX_MANIFEST_BYTES:
        msg = "HTDemucs initializer manifest exceeds its export extent"
        raise ValueError(msg)
    with manifest_path.open("xb") as stream:
        stream.write(encoded_manifest)
    return SeparatedTrace(
        Artifact(len(proto), hashlib.sha256(proto).hexdigest()),
        Artifact(len(encoded_manifest), hashlib.sha256(encoded_manifest).hexdigest()),
        bank,
        reference,
    )


def length_field(field: int, size: int) -> bytes:
    """Encode a protobuf length-delimited field header."""
    raw = bytearray()
    for value in (field * 8 + 2, size):
        number = value
        while number >= 128:
            raw.append((number & 127) | 128)
            number >>= 7
        raw.append(number)
    return bytes(raw)


def initializer_headers(onnx, model, manifest: list[Initializer]) -> list[bytes]:
    """Bind every initializer to its declared graph input and encode its metadata."""
    values = model.graph.input
    names = [value.name for value in values]
    if (
        model.graph.initializer
        or model.graph.sparse_initializer
        or names != [*tuple(SHAPES)[:2], *(item.name for item in manifest)]
        or len(set(names)) != len(names)
    ):
        msg = "HTDemucs initializer manifest does not match the graph inputs"
        raise ValueError(msg)
    headers = []
    for value, item in zip(values[2:], manifest, strict=True):
        kind, _ = INITIALIZER_TYPES[item.dtype]
        tensor = value.type.tensor_type
        shape = tensor.shape.dim
        if (
            tensor.elem_type != kind
            or not tensor.HasField("shape")
            or any(not dim.HasField("dim_value") for dim in shape)
            or tuple(dim.dim_value for dim in shape) != tuple(item.shape)
        ):
            msg = f"HTDemucs initializer type or shape differs: {item.name}"
            raise ValueError(msg)
        header = onnx.TensorProto(name=item.name, data_type=kind, dims=item.shape)
        raw_field = header.DESCRIPTOR.fields_by_name["raw_data"].number
        headers.append(header.SerializeToString() + length_field(raw_field, item.size))
    del values[2:]
    return headers


def copy_initializers(
    source: BinaryIO,
    write: Callable[[bytes], None],
    expected: Artifact,
    entries,
    field: int,
) -> None:
    """Verify each exact byte range and the complete bank while streaming it."""
    bank = hashlib.sha256()
    for item, header in entries:
        write(length_field(field, len(header) + item.size))
        write(header)
        tensor = hashlib.sha256()
        remaining = item.size
        while remaining:
            chunk = source.read(min(STREAM_CHUNK_BYTES, remaining))
            if not chunk:
                msg = f"truncated HTDemucs initializer: {item.name}"
                raise ValueError(msg)
            tensor.update(chunk)
            bank.update(chunk)
            write(chunk)
            remaining -= len(chunk)
        if tensor.hexdigest() != item.sha256:
            msg = f"HTDemucs initializer digest differs: {item.name}"
            raise ValueError(msg)
    if source.read(1) or bank.hexdigest() != expected.sha256:
        msg = "HTDemucs initializer bank differs"
        raise ValueError(msg)


def assemble_trace(
    separated: Path,
    trace: SeparatedTrace,
    output: Path,
    *,
    overlay: Path,
    admit: Callable[[Phase], None],
) -> TraceArtifact:
    """Assemble an exact inline marked trace in a fresh ONNX-only child."""
    begin_phase(Phase.ASSEMBLE, admit)
    onnx = onnx_producer(overlay)
    skeleton = read_artifact(separated / "skeleton.onnx", trace.skeleton, MAX_SKELETON_BYTES)
    model = parse_inline_graph(onnx, skeleton)
    del skeleton
    manifest_bytes = read_artifact(
        separated / "initializers.json",
        trace.manifest,
        MAX_MANIFEST_BYTES,
    )
    manifest = [Initializer(**item) for item in json.loads(manifest_bytes)]
    if not manifest or sum(item.size for item in manifest) != trace.weights.size:
        msg = "HTDemucs initializer bank extent differs"
        raise ValueError(msg)
    headers = initializer_headers(onnx, model, manifest)
    field = model.graph.DESCRIPTOR.fields_by_name["initializer"].number
    graph_field = model.DESCRIPTOR.fields_by_name["graph"].number
    graph = model.graph.SerializeToString()
    model.ClearField("graph")
    envelope = model.SerializeToString()
    entries = list(zip(manifest, headers, strict=True))
    graph_size = len(graph) + sum(
        len(length_field(field, len(header) + item.size)) + len(header) + item.size
        for item, header in entries
    )
    graph_header = length_field(graph_field, graph_size)
    extent = len(envelope) + len(graph_header) + graph_size
    if extent > MAX_GRAPH_BYTES:
        msg = "HTDemucs assembled trace exceeds the inline extent"
        raise ValueError(msg)
    digest = hashlib.sha256()
    with (separated / "weights.bin").open("rb") as source, output.open("xb") as target:

        def write(raw: bytes) -> None:
            if target.write(raw) != len(raw):
                msg = "incomplete HTDemucs assembled trace write"
                raise OSError(msg)
            digest.update(raw)

        for raw in (envelope, graph_header, graph):
            write(raw)
        copy_initializers(source, write, trace.weights, entries, field)
        if target.tell() != extent:
            msg = "HTDemucs assembled trace extent differs"
            raise ValueError(msg)
    return TraceArtifact(Artifact(extent, digest.hexdigest()), trace.reference, TORCH_CPU)


def graph_metadata(trace: TraceArtifact, attention_sha256: str) -> dict[str, str]:
    """Bind the graph to the release and the upstream original references."""
    reference = trace.reference
    return {
        "flitzis_htdemucs_contract": CONTRACT,
        "checkpoint_sha256": CHECKPOINT,
        "torch_version": trace.torch_version,
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
    bind_trace_output_shapes(onnx, graph)
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
