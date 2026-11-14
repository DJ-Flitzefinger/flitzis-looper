from threading import Lock
from typing import TYPE_CHECKING

from flitzis_looper.controller.bs_roformer_generation import BsRoformerStemGenerationBackend
from flitzis_looper.controller.native_demucs_cpu import NativeDemucsCpuBackend
from flitzis_looper.controller.stem_generation import (
    DemucsStemGenerationBackend,
    StemGenerationBackend,
    StemGenerationError,
    StemGenerationRequest,
    StemGenerationResult,
    StemProgressCallback,
    default_demucs_model_cache_dir,
)

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller.bs_roformer_onnx import BsOnnxStemGenerationBackend
    from flitzis_looper.models import StemSeparator


def separator_model_cache_dir(
    separator: StemSeparator,
    *,
    env: dict[str, str] | None = None,
    project_root: Path | None = None,
) -> Path:
    """Resolve only model-cache paths, without imports, scans or downloads."""
    root = default_demucs_model_cache_dir(env=env, project_root=project_root)
    return root / "bs-roformer-musdb18hq" if separator == "bs-roformer:musdb18hq" else root


class SelectedStemGenerationBackend:
    """Route each captured request to one of two bounded file/artifact adapters."""

    def __init__(self, backends: dict[StemSeparator, StemGenerationBackend] | None = None) -> None:
        self._native_demucs = NativeDemucsCpuBackend() if backends is None else None
        self._allows_native_bs = backends is None
        self._bs_onnx: BsOnnxStemGenerationBackend | None = None
        self._bs_onnx_lock = Lock()
        self._backends: dict[StemSeparator, StemGenerationBackend] = (
            {
                "demucs:htdemucs": DemucsStemGenerationBackend(),
                "bs-roformer:musdb18hq": BsRoformerStemGenerationBackend(),
            }
            if backends is None
            else dict(backends)
        )

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        """Use the admitted selection even when Settings changes during processing."""
        if request.native_demucs_cpu and self._native_demucs is not None:
            return self._native_demucs.generate(request, progress)
        if (
            request.separator == "bs-roformer:musdb18hq"
            and request.bs_onnx_manifest is not None
            and self._allows_native_bs
        ):
            with self._bs_onnx_lock:
                if self._bs_onnx is None:
                    # This route has its own provenance contract, independent of
                    # the measured Torch adapter declarations.
                    from flitzis_looper.controller.bs_roformer_onnx import (  # noqa: PLC0415
                        BsOnnxStemGenerationBackend,
                    )

                    self._bs_onnx = BsOnnxStemGenerationBackend()
            return self._bs_onnx.generate(request, progress)
        backend = self._backends.get(request.separator)
        if backend is None:
            msg = f"Unsupported stem separator: {request.separator}"
            raise StemGenerationError(msg)
        return backend.generate(request, progress)

    def accept_calibration(
        self, request: StemGenerationRequest, result: StemGenerationResult
    ) -> None:
        """Send final verified cost data to the original captured backend only."""
        if request.bs_onnx_manifest is not None:
            if self._bs_onnx is not None:
                self._bs_onnx.accept_calibration(request, result)
            return
        backend = self._backends.get(request.separator)
        receiver = getattr(backend, "accept_calibration", None)
        if callable(receiver):
            receiver(request, result)
