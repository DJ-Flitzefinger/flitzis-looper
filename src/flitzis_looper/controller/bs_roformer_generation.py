import sys
import tempfile
import time
from pathlib import Path

from flitzis_looper.bs_input_provenance import BS_INPUT_CONTRACT_ENV, verified_input_output
from flitzis_looper.controller.bs_roformer_assets import (
    CHECKPOINT_FILENAME,
    CONFIG_FILENAME,
    MODEL_IDENTITY,
    MODEL_NAME,
    verify_model_assets,
)
from flitzis_looper.controller.processing_code import fresh_python_source_environment
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
    detect_torch_cuda_capability,
    run_command,
)
from flitzis_looper.controller.stem_provider import (
    CudaCapability,
    ProviderCalibration,
    ProviderPlan,
    StemProviderSelector,
)
from flitzis_looper.inference_provider import is_cuda_failure
from flitzis_looper.stem_execution import completed_execution, report_execution

__all__ = ["BsRoformerStemGenerationBackend", "verify_model_assets"]


class BsRoformerStemGenerationBackend:
    """Run the exact MUSDB18HQ model behind the shared offline artifact boundary."""

    def __init__(
        self,
        *,
        command_runner: CommandRunner = run_command,
        cuda_detector: CudaDetector = detect_torch_cuda_capability,
    ) -> None:
        self._command_runner = command_runner
        self._cuda_detector = cuda_detector
        self._provider_selector = StemProviderSelector()

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
        env.pop(BS_INPUT_CONTRACT_ENV, None)
        if request.encoder_contract is not None:
            env[BS_INPUT_CONTRACT_ENV] = request.encoder_contract
        _require_demucs_audio_tools(env, self._command_runner)
        plan = self._provider_plan(request, env)
        devices = plan.devices
        started = time.monotonic()
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
                with fresh_python_source_environment(
                    env, output, source_roots=request.python_source_roots
                ) as child_env:
                    result = self._command_runner(args, cwd=Path.cwd(), env=child_env)
                if result.returncode != 0:
                    diagnostic = _command_diagnostic(result)
                    if (
                        device == "cuda"
                        and "cpu" in devices
                        and is_cuda_failure(result.returncode, result.stdout)
                    ):
                        cuda_error = diagnostic
                        progress(0.05, "CUDA failed; retrying same BS-RoFormer model on CPU")
                        continue
                    raise StemGenerationError(diagnostic)
                execution = completed_execution(
                    result.stdout,
                    backend="bs-roformer",
                    model_name=MODEL_NAME,
                    device=device,
                    cpu_fallback=cuda_error is not None,
                )
                report_execution(progress, execution)
                _write_project_cache_artifacts(
                    output_root=output,
                    cache_dir=request.cache_dir,
                    target_shape=request.target_shape,
                    progress=progress,
                )
                decoder_verified = verified_input_output(
                    output, request.encoder_contract, result.stdout
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
                decoder_verified=decoder_verified,
                execution=execution,
                provider_calibration=ProviderCalibration(
                    plan,
                    time.monotonic() - started,
                    verified=decoder_verified,
                    observed_at=time.monotonic(),
                ),
            )
        msg = "BS-RoFormer did not run"
        raise StemGenerationError(msg)

    def accept_calibration(
        self, _request: StemGenerationRequest, result: StemGenerationResult
    ) -> None:
        """Promote diagnostics only after the original controller's final guards."""
        if result.provider_calibration is not None:
            self._provider_selector.accept(result.provider_calibration, result.execution)

    def _devices(
        self, request: StemGenerationRequest, env: dict[str, str]
    ) -> tuple[StemDevice, ...]:
        return self._provider_plan(request, env).devices

    def _provider_plan(self, request: StemGenerationRequest, env: dict[str, str]) -> ProviderPlan:
        if request.device_policy == "cpu":
            return self._provider_selector.cpu_plan(request, MODEL_NAME)
        try:
            with (
                tempfile.TemporaryDirectory(prefix="flitzis-provider-") as name,
                fresh_python_source_environment(
                    env, Path(name), source_roots=request.python_source_roots
                ) as child_env,
            ):
                observed = self._cuda_detector(child_env, self._command_runner)
        except OSError:
            observed = False
        capability = observed if isinstance(observed, CudaCapability) else CudaCapability(observed)
        return self._provider_selector.plan(request, MODEL_NAME, capability)
