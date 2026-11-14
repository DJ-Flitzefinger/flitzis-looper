from typing import TYPE_CHECKING

from flitzis_looper.controller.stem_generation import (
    StemGenerationBackend,
    StemGenerationError,
    StemGenerationRequest,
    StemGenerationResult,
    StemProgressCallback,
    default_stem_model_cache_dir,
)

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.models import StemSeparator


def separator_model_cache_dir(
    separator: StemSeparator,
    *,
    env: dict[str, str] | None = None,
    project_root: Path | None = None,
) -> Path:
    """Resolve only model-cache paths, without imports, scans or downloads."""
    root = default_stem_model_cache_dir(env=env, project_root=project_root)
    return root / "bs-roformer-musdb18hq" if separator == "bs-roformer:musdb18hq" else root


class SelectedStemGenerationBackend:
    """Route the captured separator to its ONNX worker adapter."""

    def __init__(self, backends: dict[StemSeparator, StemGenerationBackend] | None = None) -> None:
        if backends is None:
            from flitzis_looper.controller.bs_roformer_onnx import (  # noqa: PLC0415
                BsOnnxStemGenerationBackend,
            )
            from flitzis_looper.controller.htdemucs_onnx import (  # noqa: PLC0415
                HtdemucsOnnxBackend,
            )

            backends = {
                "demucs:htdemucs": HtdemucsOnnxBackend(),
                "bs-roformer:musdb18hq": BsOnnxStemGenerationBackend(),
            }
        self._backends = dict(backends)

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        """Use the admitted selection even when Settings changes during processing."""
        backend = self._backends.get(request.separator)
        if backend is None:
            msg = f"Unsupported stem separator: {request.separator}"
            raise StemGenerationError(msg)
        return backend.generate(request, progress)

    def accept_calibration(
        self, request: StemGenerationRequest, result: StemGenerationResult
    ) -> None:
        """Send verified cost data to the backend for the captured selection."""
        receiver = getattr(self._backends.get(request.separator), "accept_calibration", None)
        if callable(receiver):
            receiver(request, result)
