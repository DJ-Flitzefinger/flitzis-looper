"""Describe and verify the setup-owned Windows BS-RoFormer ONNX installation."""

import hashlib
import json
import re
import stat
from dataclasses import dataclass
from pathlib import Path

from flitzis_looper.analysis.execution import NATIVE_CPU_PACKAGE_SHA, NATIVE_CUDA_PACKAGE_SHA

BS_ONNX_MANIFEST_ENV = "FLITZIS_BS_ROFORMER_ONNX_MANIFEST"
BS_ONNX_EXECUTABLE = "native-bs-roformer-worker.exe"
BS_ONNX_RUNTIME_EXECUTABLE = "runtime/bs-roformer/native-bs-roformer-worker.exe"
BS_ONNX_MODELS_DIRECTORY = "models/bs-roformer-onnx"
BS_ONNX_GRAPH = "bs-mask.onnx"
BS_ONNX_CUDA_GRAPH = "bs-mask-amp.onnx"
BS_ONNX_WEIGHTS = "weights.bsnn"
BS_ONNX_WEIGHTS_SHA256 = "51619069b73bfcbdc444a16a4ebc8cf53ea8d3466fce67b594412a02b854cbdf"
BS_ONNX_WEIGHTS_BYTES = 526_818_456
BS_ONNX_CUDA_PROVIDER_SHA256 = "110929d2552bc775828e7a5c18c5ef9ab90898fc57a57868623fe2fe8c82d494"
_CRT_DLLS = ("msvcp140.dll", "msvcp140_1.dll", "vcruntime140.dll", "vcruntime140_1.dll")
_CPU_RUNTIME_SHA256 = "4929eb4f6a3aebe2c117850a2e1354c48bc004bfa5ff4d805c2ce0ecbef2915d"
_CUDA_RUNTIME_SHA256 = "7ebc26a630b9fb721539e7821b918cf9f4a9aa94b856dcc153b81a7b140b240d"
_DIGEST = re.compile(r"[0-9a-f]{64}")
_DLL = re.compile(r"[A-Za-z0-9_.-]+\.dll")


class BsOnnxInstallationError(ValueError):
    """The selected installation could not be established before inference."""


def bs_onnx_worker_cwd(system_root: str) -> Path:
    """Use a short Windows working directory while worker inputs stay absolute."""
    directory = Path(system_root)
    if not directory.is_absolute() or not directory.is_dir():
        msg = "BS-RoFormer worker requires a valid Windows system directory"
        raise BsOnnxInstallationError(msg)
    return directory


@dataclass(frozen=True, slots=True)
class BsOnnxInstallation:
    """Immutable identities of the model and worker selected by setup."""

    manifest: Path
    executable_sha256: str
    graph_sha256: str
    cuda_graph_sha256: str | None
    crt_files: tuple[tuple[str, int, str], ...]

    @property
    def root(self) -> Path:
        """Return the installation directory shared by the worker and its assets."""
        return self.manifest.parent

    @property
    def executable(self) -> Path:
        """Return the standalone worker executable path."""
        return self.root / BS_ONNX_EXECUTABLE

    @property
    def cuda(self) -> bool:
        """Report whether setup provided the CUDA AMP graph and runtime."""
        return self.cuda_graph_sha256 is not None

    def graph_digest(self, *, cuda: bool) -> str:
        """Return the digest of the graph executed by the requested provider."""
        if not cuda:
            return self.graph_sha256
        if self.cuda_graph_sha256 is None:
            msg = "The BS-RoFormer ONNX installation has no CUDA graph"
            raise BsOnnxInstallationError(msg)
        return self.cuda_graph_sha256

    def package(self, *, cuda: bool) -> tuple[Path, str]:
        """Return the manifest and exact digest for the requested provider closure."""
        if cuda and not self.cuda:
            msg = "The BS-RoFormer ONNX installation has no CUDA package"
            raise BsOnnxInstallationError(msg)
        directory = "cuda" if cuda else "ort"
        digest = NATIVE_CUDA_PACKAGE_SHA if cuda else NATIVE_CPU_PACKAGE_SHA
        return self.root / directory / "step23-package.json", digest


def _regular_file(path: Path) -> int:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or (
        getattr(info, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT
    ):
        msg = f"BS-RoFormer ONNX asset is not an ordinary file: {path.name}"
        raise BsOnnxInstallationError(msg)
    return info.st_size


def _read_json(path: Path, *, sha256: str | None = None) -> object:
    with path.open("rb") as stream:
        data = stream.read(8193)
    if not 0 < len(data) <= 8192 or (
        sha256 is not None and hashlib.sha256(data).hexdigest() != sha256
    ):
        msg = f"BS-RoFormer ONNX manifest identity differs: {path.name}"
        raise BsOnnxInstallationError(msg)
    return json.loads(data)


def _required_digest(value: object, role: str) -> str:
    if not isinstance(value, str) or not _DIGEST.fullmatch(value):
        msg = f"Invalid BS-RoFormer ONNX digest for {role}"
        raise BsOnnxInstallationError(msg)
    return value


def _crt_inventory(value: object) -> tuple[tuple[str, int, str], ...]:
    if type(value) is not dict or set(value) != set(_CRT_DLLS):
        msg = "BS-RoFormer ONNX requires the private CRT inventory"
        raise BsOnnxInstallationError(msg)
    files = []
    for name in _CRT_DLLS:
        item = value[name]
        if type(item) is not list or len(item) != 2 or type(item[0]) is not int or item[0] <= 0:
            msg = f"Invalid BS-RoFormer ONNX CRT extent: {name}"
            raise BsOnnxInstallationError(msg)
        files.append((name, item[0], _required_digest(item[1], name)))
    return tuple(files)


def load_bs_onnx_installation(manifest: Path) -> BsOnnxInstallation:
    """Read a fixed-layout installation manifest without loading ORT or CUDA."""
    if manifest.name != "manifest.json" or not manifest.is_absolute():
        msg = "BS-RoFormer ONNX requires its absolute installation manifest"
        raise BsOnnxInstallationError(msg)
    _regular_file(manifest)
    value = _read_json(manifest)
    if (
        type(value) is not dict
        or set(value)
        != {"schema_version", "executable_sha256", "graph_sha256", "cuda_graph_sha256", "crt_files"}
        or type(value["schema_version"]) is not int
        or value["schema_version"] != 1
    ):
        msg = "Invalid BS-RoFormer ONNX installation manifest"
        raise BsOnnxInstallationError(msg)
    graph = _required_digest(value["graph_sha256"], BS_ONNX_GRAPH)
    cuda = value["cuda_graph_sha256"]
    if cuda is not None:
        cuda = _required_digest(cuda, BS_ONNX_CUDA_GRAPH)
        if cuda == graph:
            msg = "BS-RoFormer ONNX CUDA AMP graph must differ from the CPU FP32 graph"
            raise BsOnnxInstallationError(msg)
    return BsOnnxInstallation(
        manifest,
        _required_digest(value["executable_sha256"], BS_ONNX_EXECUTABLE),
        graph,
        cuda,
        _crt_inventory(value["crt_files"]),
    )


def _verify_file(path: Path, size: int | None, sha256: str) -> None:
    actual_size = _regular_file(path)
    if actual_size > 0 and (size is None or size == actual_size):
        with path.open("rb") as stream:
            if hashlib.file_digest(stream, "sha256").hexdigest() == sha256:
                return
    msg = f"BS-RoFormer ONNX asset identity differs: {path.name}"
    raise BsOnnxInstallationError(msg)


def _verify_runtime(installation: BsOnnxInstallation, *, cuda: bool) -> None:
    manifest, digest = installation.package(cuda=cuda)
    _regular_file(manifest)
    package = _read_json(manifest, sha256=digest)
    if type(package) is not dict or type(package.get("components")) is not list:
        msg = "Invalid BS-RoFormer ONNX Runtime package manifest"
        raise BsOnnxInstallationError(msg)
    for entry in package["components"]:
        # The manifest digest identifies the published inventory. Check each
        # name before treating it as a relative file location.
        if type(entry) is not dict or not _DLL.fullmatch(entry.get("name", "")):
            msg = "Invalid BS-RoFormer ONNX Runtime component name"
            raise BsOnnxInstallationError(msg)
        _verify_file(manifest.parent / entry["name"], entry["bytes"], entry["sha256"])
    runtime_digest = _CUDA_RUNTIME_SHA256 if cuda else _CPU_RUNTIME_SHA256
    _verify_file(manifest.with_name("step23-runtime.json"), None, runtime_digest)


def verify_bs_onnx_installation(installation: BsOnnxInstallation) -> None:
    """Check the complete staged package without running model inference."""
    _verify_file(installation.executable, None, installation.executable_sha256)
    _verify_file(installation.root / BS_ONNX_GRAPH, None, installation.graph_sha256)
    _verify_file(installation.root / BS_ONNX_WEIGHTS, BS_ONNX_WEIGHTS_BYTES, BS_ONNX_WEIGHTS_SHA256)
    for name, size, digest in installation.crt_files:
        _verify_file(installation.root / "crt" / name, size, digest)
    _verify_runtime(installation, cuda=False)
    if installation.cuda_graph_sha256 is not None:
        _verify_file(installation.root / BS_ONNX_CUDA_GRAPH, None, installation.cuda_graph_sha256)
        _verify_runtime(installation, cuda=True)
