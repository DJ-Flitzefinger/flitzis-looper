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
from threading import Lock
from types import MethodType
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


def forward_definition(source: bytes, class_name: str) -> ast.FunctionDef:
    model = next(
        node
        for node in ast.parse(source).body
        if isinstance(node, ast.ClassDef) and node.name == class_name
    )
    return next(
        node for node in model.body if isinstance(node, ast.FunctionDef) and node.name == "forward"
    )


def core_code(source: bytes) -> Any:
    """Extract the learned statements between normalization and raw restoration."""
    forward = forward_definition(source, "HTDemucs")
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


def mapped_reference_forward(model, module, source: bytes) -> MethodType:
    """Keep the pinned CPU CAC forward and release its unused spectral owners."""
    name = "demucs/htdemucs.py"
    if (
        hashlib.sha256(source).hexdigest() != SOURCE_PINS[name]
        or type(model) is not module.HTDemucs
        or model.training
        or model.cac is not True
        or "forward" in vars(model)
    ):
        msg = "mapped HTDemucs reference requires the exact stock CAC eval forward"
        raise ValueError(msg)

    forward = forward_definition(source, "HTDemucs")
    mask = next(
        i for i, node in enumerate(forward.body) if ast.unparse(node) == "zout = self._mask(z, x)"
    )
    magnitude = next(i for i, node in enumerate(forward.body) if ast.unparse(node) == "x = mag")
    # The pinned CAC mask reads only x. Drop z after magnitude conversion and
    # let x own the magnitude backing until normalization overwrites it.
    forward.body[mask] = ast.parse("zout = self._mask(None, x)").body[0]
    forward.body.insert(magnitude + 1, ast.parse("del z, mag").body[0])
    tree = ast.Module(body=[forward], type_ignores=[])
    namespace = {}
    exec(compile(ast.fix_missing_locations(tree), name, "exec"), vars(module), namespace)
    return MethodType(namespace["forward"], model)


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
    cpu_overlay: Path | None = None,
    mapped_state: tuple[Path, ModelStateBank] | None = None,
) -> ReferenceArtifacts:
    """Run the stock forward and check the extracted boundary in an admitted child.

    The caller owns admission from startup through process, file and pipe reader
    retirement and accepts these identities only after successful retirement.
    """
    begin_phase(Phase.REFERENCE, admit)
    sources = source_files()
    output.mkdir(exist_ok=False)
    torch = torch_producer(cpu_overlay)
    module = import_module("demucs.htdemucs")
    waveform = read_tensor(torch, mix_path, mix, SHAPES["waveform"])
    model = load_producer_model(torch, checkpoint, module, cpu_overlay, mapped_state)
    wrapper = core_wrapper(torch, model, sources["demucs/htdemucs.py"], vars(module))
    mapped_forward = (
        mapped_reference_forward(model, module, sources["demucs/htdemucs.py"])
        if mapped_state is not None
        else None
    )
    with torch.inference_mode():
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
    forward = forward_definition(source, "CrossTransformerEncoder")
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
