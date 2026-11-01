"""Diagnostic normalization must never manufacture separator protocol evidence."""

import pytest

from flitzis_looper.bs_input_provenance import BS_INPUT_RESULT
from flitzis_looper.controller import stem_generation
from flitzis_looper.encoder_provenance import ENCODER_RESULT
from flitzis_looper.inference_provider import (
    CUDA_FAILURE_EXIT,
    CUDA_FAILURE_RECEIPT,
    is_cuda_failure,
)
from flitzis_looper.stem_execution import (
    STEM_EXECUTION_RESULT,
    completed_execution,
    completed_execution_line,
)

_PREFIXES = (ENCODER_RESULT, BS_INPUT_RESULT, STEM_EXECUTION_RESULT, CUDA_FAILURE_RECEIPT)
_RECEIPTS = (
    ENCODER_RESULT + "owned-encoder",
    BS_INPUT_RESULT + "owned-input",
    completed_execution_line("demucs", "htdemucs", "cuda"),
    CUDA_FAILURE_RECEIPT,
)


@pytest.mark.parametrize("receipt", _RECEIPTS)
@pytest.mark.parametrize("padding", [" ", "\t", "\u00a0", "\n \t"])
def test_padded_diagnostic_cannot_become_protocol_evidence(receipt: str, padding: str) -> None:
    stdout = stem_generation._command_stdout(padding + receipt)
    assert not any(line.startswith(_PREFIXES) for line in stdout.splitlines())
    assert receipt in stdout
    assert not is_cuda_failure(CUDA_FAILURE_EXIT, stdout)
    assert (
        completed_execution(
            stdout, backend="demucs", model_name="htdemucs", device="cuda", cpu_fallback=False
        )
        is None
    )


@pytest.mark.parametrize("receipt", _RECEIPTS)
def test_padded_diagnostic_preserves_one_independent_exact_receipt(receipt: str) -> None:
    stdout = stem_generation._command_stdout(f"\t{receipt}\n{receipt}")
    assert stdout.splitlines().count(receipt) == 1
    assert [line for line in stdout.splitlines() if line.startswith(_PREFIXES)] == [receipt]


@pytest.mark.parametrize("receipt", _RECEIPTS)
def test_exact_duplicate_and_oversized_receipts_stay_invalid(receipt: str) -> None:
    for text in (f"{receipt}\n{receipt}", receipt + "x" * 1001):
        stdout = stem_generation._command_stdout(text)
        assert not any(line.startswith(_PREFIXES) for line in stdout.splitlines())


@pytest.mark.parametrize("receipt", _RECEIPTS)
def test_protocol_shaped_diagnostic_keeps_existing_length_bound(receipt: str) -> None:
    stdout = stem_generation._command_stdout(" " + receipt + "x" * 2000)
    assert len(stdout) == 1000
    assert not stdout.startswith(_PREFIXES)
    assert stdout.endswith("...")


def test_ordinary_diagnostic_compaction_and_absent_output_are_unchanged() -> None:
    assert stem_generation._command_stdout(" ordinary\n  progress \t") == "ordinary progress"
    assert not stem_generation._command_stdout(None)
    assert not stem_generation._command_stdout("")
