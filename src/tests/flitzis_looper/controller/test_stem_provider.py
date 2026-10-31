"""Provider policy tests are distinct from actual device/quality evidence."""

import json
import os
import sys
import time
from dataclasses import asdict, replace
from pathlib import Path
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.bs_roformer_generation import BsRoformerStemGenerationBackend
from flitzis_looper.controller.stem_generation import (
    AudioShape,
    CommandResult,
    DemucsStemGenerationBackend,
    StemGenerationError,
    StemGenerationResult,
    run_command,
)
from flitzis_looper.controller.stem_provider import (
    CudaCapability,
    ProviderCalibration,
    ProviderPlan,
    StemProviderSelector,
    parse_cuda_capability,
)
from flitzis_looper.inference_provider import (
    CUDA_FAILURE_EXIT,
    CUDA_FAILURE_RECEIPT,
    CudaProviderError,
    cuda_operation,
    emit_cuda_failure,
    is_cuda_failure,
)
from flitzis_looper.stem_execution import StemExecution
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.controller.test_pair_stem_observers import _job, _resolve_identity, _rig
from tests.flitzis_looper.controller.test_stem_generation import (
    _install_htdemucs_checkpoint,
    _is_backend_preflight,
)
from tests.flitzis_looper.controller.test_stem_generation import (
    _request as _demucs_request,
)
from tests.flitzis_looper.controller.test_stem_identity import _request
from tests.flitzis_looper.controller.test_stem_separators import (
    _install_test_assets,
)
from tests.flitzis_looper.controller.test_stem_separators import (
    _request as _bs_request,
)

CAPABILITY = CudaCapability(available=True, identity="3" * 64, free_bytes=8 * 1024**3)


def _execution(device: str) -> StemExecution:
    return StemExecution.model_validate({
        "backend": "demucs",
        "model_name": "htdemucs",
        "device": device,
        "cuda_peak_bytes": 1024**3 if device == "cuda" else None,
    })


def test_passive_total_cost_changes_only_comparable_workload_and_retains_explicit_cpu() -> None:
    selector = StemProviderSelector()
    request = replace(_request(), producer_identity="2" * 64)
    initial = selector.plan(request, "htdemucs", CAPABILITY, now=0)
    assert initial.devices == ("cuda", "cpu")
    selector.observe(initial, _execution("cuda"), 10, verified=True, now=1)
    cpu = selector.plan(replace(request, device_policy="cpu"), "htdemucs", CAPABILITY, now=2)
    assert cpu.devices == ("cpu",)
    assert cpu.key == initial.key
    selector.observe(cpu, _execution("cpu"), 8, verified=True, now=2)
    assert selector.plan(request, "htdemucs", CAPABILITY, now=3).devices == ("cpu",)
    for different in (
        replace(request, producer_identity="4" * 64),
        replace(request, source_version="source|sha256-v1:" + "5" * 64),
        replace(request, demucs_shifts=request.demucs_shifts + 1),
        replace(request, target_shape=replace(request.target_shape, frame_count=513)),
    ):
        assert selector.plan(different, "htdemucs", CAPABILITY, now=3).devices == ("cuda", "cpu")
    assert selector.plan(request, "other", CAPABILITY, now=3).devices == ("cuda", "cpu")
    assert selector.plan(
        request, "htdemucs", replace(CAPABILITY, identity="6" * 64), now=3
    ).devices == ("cuda", "cpu")
    assert selector.plan(request, "htdemucs", CAPABILITY, now=603).devices == ("cuda", "cpu")


def test_unknown_producer_failed_unverified_or_fallback_run_never_teaches_preference() -> None:
    selector = StemProviderSelector()
    request = replace(_request(), producer_identity="2" * 64)
    plan = selector.plan(request, "htdemucs", CAPABILITY, now=0)
    selector.observe(plan, _execution("cuda"), 10, verified=True, now=1)
    for receipt, seconds, verified in (
        (None, 1, True),
        (_execution("cpu"), 1, False),
        (_execution("cpu").model_copy(update={"cpu_fallback": True}), 1, True),
        (_execution("cpu"), float("nan"), True),
        (_execution("cpu"), -1, True),
    ):
        selector.observe(plan, receipt, seconds, verified=verified, now=2)
    assert selector.plan(request, "htdemucs", CAPABILITY, now=3).devices == ("cuda", "cpu")
    assert selector.plan(_request(), "htdemucs", CAPABILITY, now=3).key is None
    assert selector.plan(request, "htdemucs", CudaCapability(available=True), now=3).key is None
    assert selector.plan(request, "htdemucs", CudaCapability(available=False), now=3).devices == (
        "cpu",
    )


def test_observed_cuda_peak_veto_and_noise_margin_keep_bounded_cache() -> None:
    selector = StemProviderSelector()
    request = replace(_request(), producer_identity="2" * 64)
    plan = selector.plan(request, "htdemucs", CAPABILITY, now=0)
    selector.observe(plan, _execution("cuda"), 10, verified=True, now=1)
    selector.observe(plan, _execution("cpu"), 9, verified=True, now=2)
    assert selector.plan(request, "htdemucs", CAPABILITY, now=3).devices == ("cuda", "cpu")
    low = replace(CAPABILITY, free_bytes=1024**3)
    assert selector.plan(request, "htdemucs", low, now=3).devices == ("cpu",)
    for index in range(33):
        row = selector.plan(
            replace(request, original_bytes=3000 + index), "htdemucs", CAPABILITY, now=4
        )
        selector.observe(row, _execution("cuda"), 10, verified=True, now=4)
    assert len(selector._costs) == 32
    assert selector.plan(request, "htdemucs", low, now=5).devices == ("cuda", "cpu")


@pytest.mark.parametrize(
    "stdout",
    ["", "ok", "{}", '{"available":1}', '{"available":true,"identity":"name","free_bytes":1}'],
)
def test_incomplete_capability_never_invents_identity_or_free_memory(stdout: str) -> None:
    assert parse_cuda_capability(0, stdout).identity is None
    assert parse_cuda_capability(0, stdout).free_bytes is None


def test_device_failure_marker_requires_narrow_scope_exit_and_single_receipt(
    capsys: pytest.CaptureFixture[str],
) -> None:
    message = "CUDA out of memory. Tried to allocate 1 GiB"
    with pytest.raises(CudaProviderError) as caught, cuda_operation("cuda"):
        raise RuntimeError(message)
    emit_cuda_failure(caught.value)
    output = capsys.readouterr()
    assert message in output.err
    assert is_cuda_failure(CUDA_FAILURE_EXIT, output.out)
    assert not is_cuda_failure(1, output.out)
    assert not is_cuda_failure(CUDA_FAILURE_EXIT, output.out + CUDA_FAILURE_RECEIPT + "\n")
    for device, diagnostic in (
        ("cpu", "CUDA out of memory"),
        ("cuda", "corrupt source"),
        ("cuda", "invalid model state"),
    ):
        with pytest.raises(RuntimeError) as failure, cuda_operation(device):
            raise RuntimeError(diagnostic)
        assert type(failure.value) is RuntimeError
    message = "CUDA out of memory in malformed source metadata"
    with pytest.raises(ValueError, match="malformed source"), cuda_operation("cuda"):
        raise ValueError(message)


def test_provider_plan_is_fixed_and_does_not_claim_execution() -> None:
    plan = ProviderPlan(("cuda", "cpu"))
    assert plan.key is None
    assert parse_cuda_capability(
        0, '{"available":true,"identity":"' + "3" * 64 + '","free_bytes":4096}'
    ) == CudaCapability(available=True, identity="3" * 64, free_bytes=4096)


def test_cpu_plan_does_not_extend_last_identity_age_and_promotion_is_idempotent() -> None:
    selector = StemProviderSelector()
    request = replace(_request(), producer_identity="2" * 64)
    cpu = replace(request, device_policy="cpu")
    assert selector.cpu_plan(cpu, "htdemucs", now=0).key is None
    plan = selector.plan(request, "htdemucs", CAPABILITY, now=1)
    assert selector.cpu_plan(cpu, "htdemucs", now=600).key == plan.key
    assert selector.cpu_plan(cpu, "htdemucs", now=601).key is None
    observation = ProviderCalibration(plan, 10, verified=True, observed_at=2)
    assert not selector._costs
    selector.accept(observation, _execution("cuda"), now=3)
    first = selector._costs[plan.key]["cuda"]
    selector.accept(observation, _execution("cuda"), now=4)
    assert selector._costs[plan.key]["cuda"] is first
    result = StemGenerationResult(
        "demucs",
        "htdemucs",
        "cuda",
        cpu_fallback=False,
        artifact_count=5,
        provider_calibration=observation,
    )
    assert json.loads(json.dumps(asdict(result)))["provider_calibration"]["seconds"] == 10


@pytest.mark.parametrize("backend_name", ["demucs", "bs"])
def test_both_cpu_routes_skip_probe_and_auto_probe_uses_fresh_guarded_sources(
    tmp_path: Path, backend_name: str
) -> None:
    roots = tmp_path / "source-root"
    roots.mkdir()
    request = replace(_request(), python_source_roots=(roots,))
    observed = []

    def detector(env, _runner):
        cache = Path(env["PYTHONPYCACHEPREFIX"])
        assert not list(cache.iterdir())
        assert env["PYTHONDONTWRITEBYTECODE"] == "1"
        assert env["PYTHONPATH"].split(os.pathsep)[0] == str(roots)
        observed.append(cache)
        return CAPABILITY

    backend = (
        DemucsStemGenerationBackend if backend_name == "demucs" else BsRoformerStemGenerationBackend
    )(cuda_detector=detector)
    assert backend._provider_plan(replace(request, device_policy="cpu"), {}).devices == ("cpu",)
    assert not observed
    assert backend._provider_plan(request, {}).devices == ("cuda", "cpu")
    assert len(observed) == 1
    assert not observed[0].exists()


@pytest.mark.parametrize("backend_name", ["demucs", "bs"])
@pytest.mark.parametrize("marker_count", [1, 2])
def test_actual_owned_child_marker_survives_diagnostic_tail_and_controls_both_adapters(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, backend_name: str, marker_count: int
) -> None:
    attempts = []

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        if _is_backend_preflight(args):
            return CommandResult(0, "ok", "")
        flag = "-d" if backend_name == "demucs" else "--device"
        device = args[args.index(flag) + 1]
        attempts.append(device)
        if device == "cuda":
            body = (
                "import sys\nfrom flitzis_looper.inference_provider import ("
                "CUDA_FAILURE_EXIT, CudaProviderError, emit_cuda_failure)\n"
                f"for _ in range({marker_count}):\n"
                "    emit_cuda_failure(CudaProviderError('CUDA out of memory'))\n"
                "sys.stdout.write('x'*150000+'\\n'); sys.stdout.flush()\n"
                "raise SystemExit(CUDA_FAILURE_EXIT)\n"
            )
            return run_command([sys.executable, "-c", body], cwd=cwd, env=env)
        if backend_name == "demucs":
            output = Path(args[args.index("-o") + 1]) / "htdemucs" / "source"
            output.mkdir(parents=True)
        else:
            output = Path(args[args.index("--output") + 1])
        for name in ("drums", "bass", "other", "vocals"):
            write_mono_pcm16_wav(output / f"{name}.wav", 44_100)
        return CommandResult(0, "", "")

    if backend_name == "demucs":
        source = tmp_path / "source.wav"
        write_mono_pcm16_wav(source, 44_100)
        request = _demucs_request(tmp_path, source, AudioShape(44_100, 1, 128))
        _install_htdemucs_checkpoint(request.model_cache_dir)
        backend = DemucsStemGenerationBackend(
            command_runner=runner, cuda_detector=lambda _env, _runner: True
        )
    else:
        request = _bs_request(tmp_path)
        _install_test_assets(request.model_cache_dir, monkeypatch)
        backend = BsRoformerStemGenerationBackend(
            command_runner=runner, cuda_detector=lambda _env, _runner: True
        )
    if marker_count == 1:
        result = backend.generate(request, lambda _percent, _stage: None)
        assert result.cpu_fallback
        assert attempts == ["cuda", "cpu"]
    else:
        with pytest.raises(StemGenerationError, match="CUDA out of memory"):
            backend.generate(request, lambda _percent, _stage: None)
        assert attempts == ["cuda"]


@pytest.mark.parametrize("backend_name", ["demucs", "bs"])
@pytest.mark.parametrize(
    "diagnostic",
    ["corrupt source", "invalid model state", "CUDA out of memory without device receipt"],
)
def test_both_real_adapters_do_not_retry_unclassified_failures(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, backend_name: str, diagnostic: str
) -> None:
    commands = []

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        if _is_backend_preflight(args):
            return CommandResult(0, "ok", "")
        commands.append(args)
        return CommandResult(1, "", diagnostic)

    if backend_name == "demucs":
        source = tmp_path / "source.wav"
        write_mono_pcm16_wav(source, 44_100)
        request = _demucs_request(tmp_path, source, AudioShape(44_100, 1, 128))
        _install_htdemucs_checkpoint(request.model_cache_dir)
        backend = DemucsStemGenerationBackend(
            command_runner=runner, cuda_detector=lambda _env, _runner: True
        )
    else:
        request = _bs_request(tmp_path)
        _install_test_assets(request.model_cache_dir, monkeypatch)
        backend = BsRoformerStemGenerationBackend(
            command_runner=runner, cuda_detector=lambda _env, _runner: True
        )
    with pytest.raises(StemGenerationError, match=diagnostic):
        backend.generate(request, lambda _percent, _stage: None)
    assert len(commands) == 1
    assert not list(request.cache_dir.glob("*.wav"))


@pytest.mark.parametrize("outcome", ["success", "throw", "demote", "cancel"])
def test_calibration_promotion_requires_final_positive_provenance_and_actual_reader_end(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, outcome: str
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    job = _job(rig)
    guard = _resolve_identity(rig, job, monkeypatch)
    request = job.resolved_request
    assert request is not None
    selector = StemProviderSelector()
    plan = selector.plan(request, "htdemucs", CAPABILITY)
    candidate = ProviderCalibration(plan, 1, verified=True, observed_at=time.monotonic())

    def accept(_request, result):
        assert job.phase == "settled"
        assert job.source is None
        selector.accept(candidate, result.execution)

    accepted = Mock(side_effect=accept)
    monkeypatch.setattr(rig.backend, "accept_calibration", accepted, raising=False)
    rig.backend.result = replace(
        rig.backend.result, provider_calibration=candidate, execution=_execution("cpu")
    )
    if outcome == "throw":
        guard.verify.side_effect = [None, RuntimeError("final producer changed")]
    if outcome == "demote":
        rig.backend.result = replace(rig.backend.result, backend_name="wrong")
    if outcome == "cancel":
        generate = rig.backend.generate

        def cancelled(request, progress):
            result = generate(request, progress)
            job.cancel_event.set()
            return result

        monkeypatch.setattr(rig.backend, "generate", cancelled)
    rig.controller._run_stem_backend(job)
    assert job.phase == "settled"
    assert job.source is None
    if outcome == "success":
        accepted.assert_called_once()
    else:
        accepted.assert_not_called()
        assert not selector._costs
