"""Independent fail-closed verifier for standalone ORT1.24.4 limit1 diagnostics.

This is not productive fallback policy. The caller uses the original owned
supervisor and waits for actual tree AND reader return before another process.
Missing/truncated/ambiguous diagnostics remain terminal, including host errors.
"""

import json
import re

_CREATE = "Creating BFCArena for Cuda with following configs:"
_EXTEND = re.compile(
    r"Extending BFCArena for Cuda\. bin_num:[0-9]+ \(requested\) num_bytes: "
    r"([0-9]+) \(actual\) rounded_bytes:([0-9]+)"
)
_ERROR = re.compile(r"Available memory of 0 is smaller than requested bytes of ([0-9]+)\s*\Z")


def confirmed_arena_fault(returncode: int, stdout: bytes, stderr: bytes, graph_sha256: str) -> bool:
    """Verify dedicated exit70 plus independently captured real allocator logs.

    A true result identifies controlled device-arena refusal before cudaMalloc,
    not physical VRAM exhaustion. All other outcomes retain terminal semantics.
    The caller must separately bind immutable source/model/options and settlement.
    """
    # The original reader retains a tail of exactly65536 bytes when saturated;
    # that tail cannot establish complete constructor/extension history.
    if returncode != 70 or len(stdout) > 16384 or len(stderr) >= 65536:
        return False
    try:
        receipt = json.loads(stdout)
        events = [
            json.loads(line.removeprefix(b"ORT_TRIAL_LOG "))
            for line in stderr.splitlines()
            if line.startswith(b"ORT_TRIAL_LOG ")
        ]
        return _receipt_matches(receipt, graph_sha256) and _logs_match(events, receipt)
    except ValueError, UnicodeError:
        return False


def _receipt_matches(receipt: object, graph_sha256: str) -> bool:
    if not isinstance(receipt, dict) or set(receipt) != {
        "trial_only",
        "trial_cuda_arena_fault",
        "graph_sha256",
        "gpu_memory_limit",
        "thread",
        "rounded_bytes",
        "error",
    }:
        return False
    if (
        receipt["trial_only"] is not True
        or receipt["trial_cuda_arena_fault"] is not True
        or receipt["graph_sha256"] != graph_sha256
        or type(receipt["gpu_memory_limit"]) is not int
        or receipt["gpu_memory_limit"] != 1
    ):
        return False
    if (
        type(receipt["rounded_bytes"]) is not int
        or not isinstance(receipt["thread"], str)
        or not isinstance(receipt["error"], str)
    ):
        return False
    error = receipt["error"]
    failure = _ERROR.search(error)
    if not failure or "bfc_arena.cc:" not in error or "BFCArena::AllocateRawInternal" not in error:
        return False
    rounded = receipt["rounded_bytes"]
    if rounded <= 0 or rounded % 256 or int(failure[1]) != rounded:
        return False
    return rounded <= 2**64 - 1


def _logs_match(events: list[object], receipt: dict) -> bool:
    creations = 0
    last = None
    for event in events:
        if not isinstance(event, dict) or any(
            not isinstance(event.get(key), str) for key in ("thread", "location", "message")
        ):
            return False
        message = event["message"]
        if "bfc_arena.cc:" not in event["location"]:
            if message.startswith(("Creating BFCArena for ", "Extending BFCArena for ")):
                return False
            continue
        if message.startswith(_CREATE):
            if " memory limit: 1 arena_extend_strategy:" not in message:
                return False
            creations += 1
            last = None
        if event["thread"] == receipt["thread"] and message.startswith("Extending BFCArena for "):
            match = _EXTEND.fullmatch(message)
            last = None
            if match:
                requested, actual = map(int, match.groups())
                if requested > 0 and (requested + 255) // 256 * 256 == actual:
                    last = actual
    return creations == 1 and last == receipt["rounded_bytes"]
