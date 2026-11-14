"""Defer HTDemucs ONNX imports until the captured native route is selected."""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from flitzis_looper.controller.htdemucs_onnx import HtdemucsOnnxBackend
    from flitzis_looper.controller.stem_generation import (
        StemGenerationRequest,
        StemGenerationResult,
        StemProgressCallback,
    )


class NativeDemucsCpuBackend:
    def __init__(self) -> None:
        self._backend: HtdemucsOnnxBackend | None = None

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        if self._backend is None:
            from flitzis_looper.controller.htdemucs_onnx import HtdemucsOnnxBackend  # noqa: PLC0415

            self._backend = HtdemucsOnnxBackend()
        return self._backend.generate(request, progress)
