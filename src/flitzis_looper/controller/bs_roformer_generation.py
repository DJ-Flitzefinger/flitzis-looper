import sys
import tempfile
from pathlib import Path

from flitzis_looper.controller.bs_roformer_assets import (
    CHECKPOINT_FILENAME,
    CONFIG_FILENAME,
    MODEL_IDENTITY,
    MODEL_NAME,
    verify_model_assets,
)
from flitzis_looper.controller.stem_generation import (
    CommandRunner,
    CudaDetector,
    StemDevice,
    StemGenerationError,
    StemGenerationRequest,
    StemGenerationResult,
    StemProgressCallback,
    _command_diagnostic,
    _require_demucs_audio_tools,
    _validate_request,
    _write_project_cache_artifacts,
    demucs_cache_environment,
    detect_torch_cuda_available,
    run_command,
)


class BsRoformerStemGenerationBackend:
    """Run the exact MUSDB18HQ model behind the shared offline artifact boundary."""

    def __init__(
        self,
        *,
        command_runner: CommandRunner = run_command,
        cuda_detector: CudaDetector = detect_torch_cuda_available,
    ) -> None:
        self._command_runner = command_runner
        self._cuda_detector = cuda_detector

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        """Produce five aligned artifacts without publishing or downloading models."""
        _validate_request(request)
        try:
            verify_model_assets(request.model_cache_dir)
        except RuntimeError as error:
            raise StemGenerationError(str(error)) from error
        env = demucs_cache_environment(request.model_cache_dir)
        _require_demucs_audio_tools(env, self._command_runner)
        devices = self._devices(request, env)
        cuda_error = None
        request.cache_dir.mkdir(parents=True, exist_ok=True)
        for device in devices:
            progress(0.05, f"Running {MODEL_NAME} on {device.upper()}")
            with tempfile.TemporaryDirectory(prefix=".separator-", dir=request.cache_dir) as name:
                output = Path(name)
                args = [
                    sys.executable,
                    "-m",
                    "flitzis_bs_roformer",
                    "--source",
                    str(request.source_path),
                    "--output",
                    str(output),
                    "--checkpoint",
                    str(request.model_cache_dir / CHECKPOINT_FILENAME),
                    "--config",
                    str(request.model_cache_dir / CONFIG_FILENAME),
                    "--device",
                    device,
                ]
                result = self._command_runner(args, cwd=Path.cwd(), env=env)
                if result.returncode != 0:
                    diagnostic = _command_diagnostic(result)
                    if device == "cuda" and "cpu" in devices:
                        cuda_error = diagnostic
                        progress(0.05, "CUDA failed; retrying same BS-RoFormer model on CPU")
                        continue
                    raise StemGenerationError(diagnostic)
                _write_project_cache_artifacts(
                    output_root=output,
                    cache_dir=request.cache_dir,
                    target_shape=request.target_shape,
                    progress=progress,
                )
            diagnostic = MODEL_IDENTITY
            if cuda_error is not None:
                diagnostic += f"; CUDA failed; same model generated on CPU: {cuda_error}"
            return StemGenerationResult(
                backend_name="bs-roformer",
                model_name=MODEL_NAME,
                device=device,
                cpu_fallback=cuda_error is not None,
                artifact_count=5,
                diagnostic=diagnostic[:1000],
            )
        msg = "BS-RoFormer did not run"
        raise StemGenerationError(msg)

    def _devices(
        self, request: StemGenerationRequest, env: dict[str, str]
    ) -> tuple[StemDevice, ...]:
        if request.device_policy == "cpu":
            return ("cpu",)
        try:
            available = self._cuda_detector(env, self._command_runner)
        except OSError:
            available = False
        return ("cuda", "cpu") if available else ("cpu",)
