"""Host and native witnesses for completed ONNX stem producers."""

import hashlib
import json
import re
from pathlib import Path
from types import BuiltinFunctionType
from typing import TYPE_CHECKING

import flitzis_looper_audio as native
from flitzis_looper.analysis import process as analysis_process
from flitzis_looper.controller import stem_artifacts
from flitzis_looper.controller.stem_identity import TRANSFORM_POLICY

if TYPE_CHECKING:
    from flitzis_looper.controller.stem_generation import StemGenerationRequest


def _host_processing_paths(request: StemGenerationRequest) -> tuple[Path, ...]:
    """Name ordered executed host roles independently of optional encoder images."""
    package = Path(__file__).parent.parent
    return (
        package / "controller" / "stem_artifacts.py",
        package / "pcm16_wav.py",
        package / "controller" / "stem_generation.py",
        package / "bs_input_provenance.py",
        package / "encoder_provenance.py",
        package / "controller" / "processing_code.py",
        package / "controller" / "stem_process.py",
        package / "analysis" / "process.py",
        package / "analysis" / "windows_job.py",
        package / "analysis" / "linux_process.py",
        package / "analysis" / "contracts.py",
        package / "stem_cancellation.py",
        package / "stem_execution_protocol.py",
        package / "offline_runtime.py",
        package / "system_resources.py",
        package / "controller" / "stem_resources.py",
        package / "inference_provider.py",
        package / "controller" / "stem_provider.py",
        package / "controller" / "stem_identity.py",
        package / "controller" / "stem_separators.py",
        package / "__init__.py",
    )


def native_conversion_identity() -> tuple[str, str] | None:
    """Bind both executed Native bridges, without claiming full loaded-image attestation."""
    identity = native.stem_conversion_identity
    converter = getattr(stem_artifacts, "StemConversionReservation", None)
    process_type = getattr(analysis_process, "PreparedWindowsProcess", None)
    process_identity = getattr(analysis_process, "offline_process_identity", None)
    if (
        converter is not native.StemConversionReservation
        or not isinstance(identity, BuiltinFunctionType)
        or identity.__module__ != native.StemConversionReservation.__module__
    ):
        return None
    if (
        process_type is not getattr(native, "PreparedWindowsProcess", None)
        or process_identity is not getattr(native, "offline_process_identity", None)
        or not isinstance(process_identity, BuiltinFunctionType)
        or process_identity.__module__ != identity.__module__
    ):
        return None
    try:
        policy, digest = identity(converter)
        process_policy, process_digest = process_identity(process_type)
    except TypeError, ValueError:
        return None
    if policy != TRANSFORM_POLICY or re.fullmatch(r"[0-9a-f]{64}", digest) is None:
        return None
    if (
        process_policy != "prepared-windows-process-v1"
        or re.fullmatch(r"[0-9a-f]{64}", process_digest) is None
    ):
        return None
    payload = json.dumps(
        ((policy, digest), (process_policy, process_digest)), separators=(",", ":")
    )
    return policy, hashlib.sha256(payload.encode("ascii")).hexdigest()
