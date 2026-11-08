"""Internal producer wire semantics match the retained public V1 reference."""

import json
from dataclasses import FrozenInstanceError, replace

import pytest

from flitzis_looper import stem_execution as public
from flitzis_looper import stem_execution_protocol as protocol


def _values() -> dict[str, object]:
    return {"backend": "demucs", "model_name": "htdemucs", "device": "cuda"}


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("schema_version", False),
        ("schema_version", True),
        ("schema_version", 1.0),
        ("schema_version", 2),
        ("backend", "unknown"),
        ("backend", 1),
        ("model_name", ""),
        ("model_name", "x" * 257),
        ("model_name", 1),
        ("device", "gpu"),
        ("cpu_fallback", 0),
        ("cpu_fallback", "false"),
        ("cpu_fallback", True),
        ("intra_op_threads", 1),
        ("inter_op_threads", 1),
        ("cuda_peak_bytes", -1),
        ("cuda_peak_bytes", 2**63),
        ("cuda_peak_bytes", True),
        ("cuda_peak_bytes", 1.0),
        ("unknown", 1),
    ],
)
def test_invalid_fields_match_public_json_reference(field: str, value: object) -> None:
    line = protocol.STEM_EXECUTION_RESULT + json.dumps(_values() | {field: value})
    arguments = {
        "backend": "demucs",
        "model_name": "htdemucs",
        "device": "cuda",
        "cpu_fallback": False,
    }
    assert public.completed_execution(line, **arguments) is None
    assert protocol.completed_execution(line, **arguments) is None


@pytest.mark.parametrize("backend", ["demucs", "bs-roformer"])
@pytest.mark.parametrize("device", ["cpu", "cuda"])
@pytest.mark.parametrize("model", ["model", 'é🎶"\\\n\t\u2028', "x" * 256])
@pytest.mark.parametrize("threads", [None, (1, 1)])
def test_exact_valid_emission_and_boundary_conversion(
    backend: str, device: str, model: str, threads: tuple[int, int] | None
) -> None:
    peak = 2**63 - 1 if device == "cuda" else None
    line = protocol.completed_execution_line(
        backend, model, device, threads=threads, cuda_peak_bytes=peak
    )
    assert line == public.completed_execution_line(
        backend, model, device, threads=threads, cuda_peak_bytes=peak
    )
    for ending in ("", "\n", "\r\n"):
        args = {
            "backend": backend,
            "model_name": model,
            "device": device,
            "cpu_fallback": device == "cpu",
        }
        actual = protocol.completed_execution(line + ending, **args)
        expected = public.completed_execution(line + ending, **args)
        assert actual is not None
        assert expected is not None
        assert protocol.execution_fields(actual) == expected.model_dump()
        assert public.public_execution(actual) == expected
        assert isinstance(public.public_execution(actual), public.StemExecution)
        with pytest.raises(FrozenInstanceError):
            actual.model_name = "changed"


@pytest.mark.parametrize(
    "payload",
    [
        "null",
        "[]",
        "{}",
        "{",
        '{"backend":"demucs"}garbage',
        '{"backend":"bad","backend":"demucs","model_name":"htdemucs","device":"cuda"}',
        '{"backend":"demucs","model_name":"htdemucs","device":"cuda","device":"cpu"}',
        '{"backend":"demucs","model_name":"\\ud800","device":"cuda"}',
        '{"backend":"demucs","model_name":"\\ud83c\\udfb6","device":"cuda"}',
        '{"backend":"demucs","model_name":"htdemucs","device":"cuda","cuda_peak_bytes":NaN}',
        '\ufeff{"backend":"demucs","model_name":"htdemucs","device":"cuda"}',
    ],
)
def test_duplicate_unicode_and_malformed_json_match_original_policy(payload: str) -> None:
    line = protocol.STEM_EXECUTION_RESULT + payload
    for model in ("htdemucs", "🎶"):
        args = {"backend": "demucs", "model_name": model, "device": "cuda", "cpu_fallback": False}
        actual, expected = (
            protocol.completed_execution(line, **args),
            public.completed_execution(line, **args),
        )
        assert (actual is None) == (expected is None)
        if actual is not None:
            assert protocol.execution_fields(actual) == expected.model_dump()


@pytest.mark.parametrize("separator", ["\r", "\v", "\f", "\x85", "\u2028", "\u2029"])
def test_only_one_exact_lf_framed_bounded_receipt_is_accepted(separator: str) -> None:
    line = protocol.completed_execution_line("demucs", "htdemucs", "cpu")
    args = {"backend": "demucs", "model_name": "htdemucs", "device": "cpu", "cpu_fallback": False}
    for stdout in ("diagnostic" + separator + line, line + "\n" + line, line + " " * 1000):
        assert protocol.completed_execution(stdout, **args) is None
    assert (
        protocol.completed_execution("diagnostic" + separator + "text\n" + line, **args) is not None
    )
    for key, value in (("backend", "bs-roformer"), ("model_name", "other"), ("device", "cuda")):
        assert protocol.completed_execution(line, **(args | {key: value})) is None


@pytest.mark.parametrize("threads", [(True, True), (1.0, 1.0), (0, 0), (2, 2), (1, None)])
def test_thread_pairs_are_strict_and_complete(threads: tuple) -> None:
    with pytest.raises(ValueError, match="Incoherent completed execution"):
        protocol.completed_execution_line("demucs", "htdemucs", "cpu", threads=threads)


@pytest.mark.parametrize("model", ["\ud800", "a\ud800z", "\ud83c\udfb6"])
def test_invalid_python_unicode_is_rejected_by_both_emitters(model: str) -> None:
    for module in (public, protocol):
        with pytest.raises(ValueError, match=r"unicode|surrogates"):
            module.completed_execution_line("demucs", model, "cpu")


def test_fallback_peak_and_internal_progress_remain_coherent() -> None:
    receipt = protocol.ExecutionReceipt(backend="demucs", model_name="htdemucs", device="cpu")
    with pytest.raises(ValueError, match="Incoherent completed execution"):
        replace(receipt, cuda_peak_bytes=0)
    with pytest.raises(ValueError, match="Incoherent completed execution"):
        replace(receipt, device="cuda", cpu_fallback=True)
    events = []
    progress = protocol.ExecutionProgress(lambda p, s, e: events.append((p, s, e)))
    protocol.report_execution(progress, receipt)
    progress(0.85, "Aligning")
    assert events == [(None, None, receipt), (0.85, "Aligning", receipt)]
    protocol.report_execution(lambda *_: None, receipt)
    assert public.public_execution(None) is None
    public_receipt = public.public_execution(receipt)
    assert public.public_execution(public_receipt) is public_receipt


def test_public_progress_facade_converts_internal_receipts_for_external_callers() -> None:
    receipt = protocol.ExecutionReceipt(backend="demucs", model_name="htdemucs", device="cpu")
    events = []
    progress = public.StemExecutionProgress(lambda p, s, e: events.append((p, s, e)))
    protocol.report_execution(progress, receipt)
    progress(0.85, "Aligning")
    expected = public.public_execution(receipt)
    assert events == [(None, None, expected), (0.85, "Aligning", expected)]
    assert all(isinstance(event[2], public.StemExecution) for event in events)


@pytest.mark.parametrize("identity", ["3" * 64, None])
def test_optional_actual_device_identity_preserves_public_and_legacy_wire(
    identity: str | None,
) -> None:
    line = protocol.completed_execution_line(
        "demucs",
        "htdemucs",
        "cuda",
        threads=(1, 1),
        cuda_peak_bytes=123,
        cuda_device_identity=identity,
    )
    assert line == public.completed_execution_line(
        "demucs",
        "htdemucs",
        "cuda",
        threads=(1, 1),
        cuda_peak_bytes=123,
        cuda_device_identity=identity,
    )
    assert ("cuda_device_identity" in line) == (identity is not None)
    args = {"backend": "demucs", "model_name": "htdemucs", "device": "cuda", "cpu_fallback": False}
    receipt = protocol.completed_execution(line, **args)
    assert receipt is not None
    assert receipt.cuda_device_identity == identity
    assert public.public_execution(receipt) == public.completed_execution(line, **args)


@pytest.mark.parametrize("identity", [True, 1, "", "3" * 63, "G" * 64, "3" * 64 + "\n"])
def test_invalid_device_identity_is_unknown_in_both_protocols(identity: object) -> None:
    line = protocol.STEM_EXECUTION_RESULT + json.dumps(
        _values() | {"cuda_device_identity": identity}
    )
    args = {"backend": "demucs", "model_name": "htdemucs", "device": "cuda", "cpu_fallback": False}
    assert protocol.completed_execution(line, **args) is None
    assert public.completed_execution(line, **args) is None


def test_cpu_cannot_carry_cuda_device_identity() -> None:
    for module in (public, protocol):
        with pytest.raises(ValueError, match=r"CUDA identity|Incoherent"):
            module.completed_execution_line(
                "demucs", "htdemucs", "cpu", cuda_device_identity="3" * 64
            )
