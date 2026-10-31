"""A terminal host/unknown failure must never authorize another model attempt."""

import json
import runpy
from pathlib import Path

import pytest

VERIFY = runpy.run_path(
    str(Path(__file__).resolve().parents[4] / "scripts/beat_this_trial_fault.py")
)["confirmed_arena_fault"]
GRAPH = "a" * 64
CREATE = (
    "Creating BFCArena for Cuda with following configs: initial_chunk_size_bytes: 1048576"
    " memory limit: 1 arena_extend_strategy: 0"
)
EXTEND = (
    "Extending BFCArena for Cuda. bin_num:0 (requested) num_bytes: 4 (actual) rounded_bytes:256"
)
ERROR = (
    "Exception during loading: bfc_arena.cc:329 onnxruntime::BFCArena::AllocateRawInternal "
    "Available memory of 0 is smaller than requested bytes of 256"
)


def _packet() -> dict:
    return {
        "trial_only": True,
        "trial_cuda_arena_fault": True,
        "graph_sha256": GRAPH,
        "gpu_memory_limit": 1,
        "thread": "ThreadId(1)",
        "rounded_bytes": 256,
        "error": ERROR,
    }


def _logs(*messages: str, thread: str = "ThreadId(1)", location: str = "bfc_arena.cc:306") -> bytes:
    return b"\n".join(
        b"ORT_TRIAL_LOG "
        + json.dumps({"thread": thread, "location": location, "message": message}).encode()
        for message in messages
    )


def test_device_arena_refusal_needs_independent_logs_and_dedicated_exit() -> None:
    stdout = json.dumps(_packet()).encode()
    assert VERIFY(70, stdout, _logs(CREATE, EXTEND), GRAPH)
    for code in (0, 1, -1):
        assert not VERIFY(code, stdout, _logs(CREATE, EXTEND), GRAPH)
    for logs in (
        b"",
        _logs(CREATE),
        _logs(EXTEND),
        _logs(EXTEND, CREATE),
        _logs(CREATE, CREATE, EXTEND),
        _logs(CREATE, EXTEND, EXTEND.replace("Cuda.", "Cpu.")),
        _logs(CREATE, EXTEND, EXTEND.replace("Cuda.", "CudaPinned.")),
        _logs(CREATE, EXTEND, thread="ThreadId(2)"),
        _logs(CREATE, EXTEND, location="unrecognized.cc:1"),
        _logs(CREATE.replace("limit: 1 ", "limit: 10 "), EXTEND),
        _logs(CREATE, EXTEND.replace("bytes:256", "bytes:512")),
        _logs(CREATE, EXTEND.replace("bytes: 4 ", "bytes: 0 ")),
    ):
        assert not VERIFY(70, stdout, logs, GRAPH)


@pytest.mark.parametrize(
    ("key", "value"),
    [
        ("graph_sha256", "b" * 64),
        ("gpu_memory_limit", True),
        ("gpu_memory_limit", 2),
        ("rounded_bytes", True),
        ("rounded_bytes", 0),
        ("rounded_bytes", 255),
        ("rounded_bytes", 512),
        ("thread", "other"),
        ("trial_only", False),
        ("error", "bad allocation"),
        ("error", "BFCArena allocation failed"),
        ("error", ERROR.replace("of 256", "of 512")),
        ("extra", "unknown"),
    ],
)
def test_receipt_alone_cannot_reclassify_terminal_errors(key: str, value: object) -> None:
    packet = _packet()
    packet[key] = value
    assert not VERIFY(70, json.dumps(packet).encode(), _logs(CREATE, EXTEND), GRAPH)


def test_corruption_or_truncated_diagnostics_remain_terminal() -> None:
    for stdout in (b"[]", b"null", b"{}", b"\xff", b"{" * 16385):
        assert not VERIFY(70, stdout, _logs(CREATE, EXTEND), GRAPH)
    for logs in (b"ORT_TRIAL_LOG {", b"ORT_TRIAL_LOG []", b"x" * 65537):
        assert not VERIFY(70, json.dumps(_packet()).encode(), logs, GRAPH)


def test_saturated_original_reader_tail_cannot_prove_complete_history() -> None:
    valid = _logs(CREATE, EXTEND)
    saturated = b"x" * (65536 - len(valid) - 1) + b"\n" + valid
    assert len(saturated) == 65536
    assert not VERIFY(70, json.dumps(_packet()).encode(), saturated, GRAPH)
