from typing import TYPE_CHECKING

from flitzis_looper.controller.bs_roformer_generation import BsRoformerStemGenerationBackend
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
        backend = self._backends.get(request.separator)
        if backend is None:
            msg = f"Unsupported stem separator: {request.separator}"
            raise StemGenerationError(msg)
        return backend.generate(request, progress)

    def accept_calibration(
        self, request: StemGenerationRequest, result: StemGenerationResult
    ) -> None:
        """Send final verified cost data to the original captured backend only."""
        backend = self._backends.get(request.separator)
        receiver = getattr(backend, "accept_calibration", None)
        if callable(receiver):
            receiver(request, result)
