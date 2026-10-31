"""Actual numeric bridge, provenance and pre-allocation admission; no inference."""

import runpy
import struct
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace

import pytest

import flitzis_looper.stem_execution as execution
import flitzis_looper_audio as native
from flitzis_looper.controller import processing_code, stem_artifacts, stem_provenance
from flitzis_looper.controller.processing_code import loaded_code_identity
from flitzis_looper.controller.stem_generation import AudioShape
from flitzis_looper.controller.stem_identity import TRANSFORM_POLICY
from flitzis_looper_audio import StemConversionReservation, stem_conversion_identity
from tests.flitzis_looper.controller.test_stem_artifacts import _outputs
from tests.flitzis_looper.controller.test_stem_identity import _request


def _sources() -> tuple:
    return ((48000, 1, 2, 0, struct.pack("<hh", 1, 2)),) * 4


def test_real_native_identity_binds_compiled_core_and_executed_wrapper() -> None:
    policy, digest = stem_conversion_identity()
    assert policy == TRANSFORM_POLICY
    assert len(digest) == 64
    assert stem_provenance.native_conversion_identity() == (policy, digest)


def test_actual_guarded_artifact_and_process_roles_remain_known_with_native_conversion() -> None:
    roles = {
        "stem_artifacts.py",
        "pcm16_wav.py",
        "stem_process.py",
        "process.py",
        "windows_job.py",
        "contracts.py",
        "stem_cancellation.py",
    }
    paths = tuple(
        path for path in stem_provenance._host_processing_paths(_request()) if path.name in roles
    )
    assert len(paths) == 7
    held = native.SealedProcessingFiles([str(path) for path in paths])
    assert loaded_code_identity(paths) is not None
    held.verify()


@pytest.mark.parametrize("separator", ["demucs:htdemucs", "bs-roformer:musdb18hq"])
def test_unsupported_full_inference_host_is_not_relabelled_known_by_converter(
    separator: str,
) -> None:
    # This exact unsupported validator also reproduces on unchanged a5992b0d.
    # A native converter identity cannot certify the remaining inference host.
    paths = stem_provenance._host_processing_paths(replace(_request(), separator=separator))
    held = native.SealedProcessingFiles([str(path) for path in paths])
    manifests = {path.resolve(): processing_code._manifest(path) for path in paths}
    origin = Path(execution.__file__).resolve()
    with pytest.raises(ValueError, match="unsupported decorator policy"):
        processing_code._check_namespace(vars(execution), manifests[origin], manifests)
    assert loaded_code_identity(paths) is None
    held.verify()


def test_portable_converter_preflight_accepts_actual_native_and_refuses_stale_api() -> None:
    check = runpy.run_path(
        str(Path(__file__).resolve().parents[4] / "scripts/portable/bootstrap.py")
    )["check_stem_conversion_api"]
    check(native)
    for missing in ("StemConversionReservation", "stem_conversion_identity"):
        stale = SimpleNamespace(
            StemConversionReservation=StemConversionReservation,
            stem_conversion_identity=stem_conversion_identity,
        )
        delattr(stale, missing)
        with pytest.raises(AssertionError):
            check(stale)
    with pytest.raises(AssertionError, match="digest invalid"):
        check(
            SimpleNamespace(
                StemConversionReservation=StemConversionReservation,
                stem_conversion_identity=lambda: (TRANSFORM_POLICY, "x" * 64),
            )
        )


def test_substituted_actual_converter_cannot_claim_known_native_producer(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(stem_artifacts, "StemConversionReservation", object)
    assert stem_provenance.native_conversion_identity() is None


def test_canonical_native_type_cannot_be_rebound_or_impersonated(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with pytest.raises(TypeError):
        StemConversionReservation.convert = lambda *_: ()
    with pytest.raises(ValueError, match="unsupported actual"):
        stem_conversion_identity(object)
    monkeypatch.setattr(stem_artifacts, "StemConversionReservation", object)
    monkeypatch.setattr(stem_provenance.native, "StemConversionReservation", object)
    assert stem_provenance.native_conversion_identity() is None


def test_reservation_is_single_use_and_validates_immutable_exact_spans() -> None:
    reservation = StemConversionReservation(1024, 1024)
    result = reservation.convert(_sources(), (48000, 1, 2), 0, 2)
    assert result[0] == struct.pack("<hh", 1, 2)
    with pytest.raises(ValueError, match="already consumed"):
        reservation.convert(_sources(), (48000, 1, 2), 0, 2)
    with pytest.raises(ValueError, match="span"):
        StemConversionReservation(1024, 1024).convert(
            ((48000, 1, 2, 1, b"\0\0"),) * 4, (48000, 1, 2), 0, 2
        )
    with pytest.raises(TypeError):
        StemConversionReservation(1024, 1024).convert(
            ((48000, 1, 2, 0, bytearray(4)),) * 4, (48000, 1, 2), 0, 2
        )


def test_prospective_admission_failure_precedes_every_source_read(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    output, cache = tmp_path / "model", tmp_path / "generation"
    _outputs(output)
    reads = []
    original = stem_artifacts.PcmWavReader.readframes

    def read(reader: object, frames: int) -> bytes:
        reads.append(frames)
        return original(reader, frames)

    def reject(_size: int, _limit: int) -> None:
        msg = "injected prospective host pressure"
        raise RuntimeError(msg)

    monkeypatch.setattr(stem_artifacts, "StemConversionReservation", reject)
    monkeypatch.setattr(stem_artifacts.PcmWavReader, "readframes", read)
    with pytest.raises(RuntimeError, match="prospective host pressure"):
        stem_artifacts.write_project_cache_artifacts(
            output_root=output,
            cache_dir=cache,
            target_shape=AudioShape(48000, 2, 37),
            progress=lambda *_: None,
        )
    assert reads == []
    assert list(cache.iterdir()) == []
