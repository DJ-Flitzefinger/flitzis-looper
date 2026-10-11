"""Bounded actual input-tool witnesses for the supported offline BS worker.

The parent keeps complete Native file guards. This child records only its own
two launched tools and an exclusively born diagnostic, never deletion rights.
"""

import hashlib
import json
import os
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, BinaryIO, cast

from flitzis_looper.encoder_provenance import (
    EncoderProof,
    _file_witness,
    _matches_images,
    _module_images,
    supported_encoder_images,
)
from flitzis_looper.stem_cancellation import (
    StemInferenceCancelledError,
    check_cancelled,
    communicate_cancellable,
    tool_environment,
)

if TYPE_CHECKING:
    from collections.abc import Callable

BS_INPUT_CONTRACT_ENV = "FLITZIS_BS_INPUT_CONTRACT"
BS_INPUT_RECEIPT = ".bs-input-execution.json"
BS_INPUT_RESULT = "FLITZIS_BS_INPUT_BIRTH_V1:"


@dataclass(slots=True)
class _InputRecord:
    path: Path
    handle: BinaryIO
    contract: str
    runs: list[dict[str, object]]


@dataclass(slots=True)
class _InputAttemptState:
    record: _InputRecord | None = None
    completed: tuple[str, int, int, str] | None = None


_state = _InputAttemptState()


def bs_input_contract(executable: Path) -> str | None:
    """Capture the exact shared decoder/probe files held by the parent guard."""
    images = supported_encoder_images(executable)
    probe = executable.with_name("ffprobe.exe")
    if not images or not probe.is_file() or probe.with_name("ffprobe.exe.local").exists():
        return None
    return json.dumps([_file_witness(path) for path in (*images, probe)], separators=(",", ":"))


def begin_input_attempt(output: Path) -> None:
    """Exclusively create this child attempt's diagnostic before either tool launch."""
    encoded = os.environ.get(BS_INPUT_CONTRACT_ENV)
    if encoded is None or _state.record is not None:
        return
    _state.completed = None
    path = output / BS_INPUT_RECEIPT
    try:
        handle = path.open("xb")
    except OSError:
        return
    _state.record = _InputRecord(path, handle, encoded, [])


def finish_input_attempt() -> None:
    """Close this child's incomplete diagnostic during normal or cancelled unwind."""
    record = _state.record
    if record is not None:
        record.handle.close()
        _state.record = None


def _tool_images(kind: str) -> list[list[str | int]] | None:
    encoded = os.environ.get(BS_INPUT_CONTRACT_ENV)
    if encoded is None or os.name != "nt":
        return None
    try:
        contract = json.loads(encoded)
        if not _valid_contract(contract):
            return None
        contract = cast("list[list[str | int]]", contract)
        if kind == "ffmpeg":
            return contract[:8]
        if kind == "ffprobe":
            return [contract[8], *contract[1:8]]
    except TypeError, ValueError:
        return None
    return None


def _valid_contract(value: object) -> bool:
    return (
        isinstance(value, list)
        and len(value) == 9
        and all(
            isinstance(item, list)
            and len(item) == 4
            and isinstance(item[0], str)
            and all(type(number) is int and number >= 0 for number in item[1:])
            for item in value
        )
    )


def _inspect_input(
    process: subprocess.Popen[bytes],
    expected: list[list[str | int]],
    checkpoint: Callable[[str], None] | None = None,
) -> EncoderProof | None:
    deadline = time.monotonic() + 2
    while process.poll() is None and time.monotonic() < deadline:
        check_cancelled(checkpoint, "input_tool_witness")
        try:
            if _matches_images(_module_images(process.pid), expected):
                return {"pid": process.pid, "images": expected}
        except OSError, TypeError, ValueError:
            pass  # Loader snapshots may race; a missing observation stays UNKNOWN.
        time.sleep(0.001)
    return None


def run_input_tool(
    args: list[str], *, checkpoint: Callable[[str], None] | None = None
) -> subprocess.CompletedProcess[str]:
    """Preserve probe/decode arguments and capture actual launched image witnesses."""
    kind = args[0]
    expected = _tool_images(kind)
    check_cancelled(checkpoint, "before_input_tool")
    if expected is None and checkpoint is None:
        return subprocess.run(
            args, check=True, capture_output=True, text=True, env=tool_environment()
        )
    actual = args if expected is None else [str(expected[0][0]), *args[1:]]
    with subprocess.Popen(
        actual, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=tool_environment()
    ) as process:
        try:
            proof = None if expected is None else _inspect_input(process, expected, checkpoint)
            stdout, stderr = communicate_cancellable(process, checkpoint)
        except StemInferenceCancelledError:
            process.kill()
            process.communicate()
            raise
        assert stdout is not None
        assert stderr is not None
        result = subprocess.CompletedProcess(
            actual, process.returncode, stdout.decode(), stderr.decode(errors="replace")
        )
    result.check_returncode()
    check_cancelled(checkpoint, "after_input_tool")
    if expected is not None:
        _record_input(kind, actual, proof)
    return result


def _record_input(kind: str, argv: list[str], proof: EncoderProof | None) -> None:
    record = _state.record
    if record is None:
        return
    try:
        held = os.fstat(record.handle.fileno())
        current = record.path.stat()
        if (held.st_dev, held.st_ino) != (current.st_dev, current.st_ino):
            record.handle.close()
            _state.record = None
            return
        record.runs.append({"kind": kind, "argv": argv, "proof": proof})
        encoded = json.dumps(
            {"contract": record.contract, "runs": record.runs}, separators=(",", ":")
        ).encode("utf-8")
        record.handle.seek(0)
        record.handle.write(encoded)
        record.handle.truncate()
        record.handle.flush()
        if len(record.runs) == 2 and all(run["proof"] is not None for run in record.runs):
            _state.completed = (
                str(record.path),
                held.st_dev,
                held.st_ino,
                hashlib.sha256(encoded).hexdigest(),
            )
    except OSError:
        _state.completed = None
    finally:
        if len(record.runs) >= 2:
            record.handle.close()
            _state.record = None


def input_completion_line() -> str:
    """Return this actual child's exclusive birth identity and complete diagnostic hash."""
    return BS_INPUT_RESULT + json.dumps(
        [] if _state.completed is None else [_state.completed], separators=(",", ":")
    )


def verified_input_output(output: Path, contract: str | None, stdout: str) -> bool:
    """Require two actual tools and the child's born FileID/body digest before reuse stamping."""
    if contract is None:
        return False
    try:
        encoded = _read_input_receipt(output, stdout)
        if encoded is None:
            return False
        body = json.loads(encoded)
        expected = json.loads(contract)
        runs = body["runs"]
        return (
            body["contract"] == contract
            and len(expected) == 9
            and len(runs) == 2
            and [run["kind"] for run in runs] == ["ffprobe", "ffmpeg"]
            and runs[0]["proof"]["images"] == [expected[8], *expected[1:8]]
            and runs[1]["proof"]["images"] == expected[:8]
            and runs[0]["argv"][0] == expected[8][0]
            and runs[1]["argv"][0] == expected[0][0]
            and runs[0]["proof"]["pid"] != runs[1]["proof"]["pid"]
        )
    except OSError, IndexError, KeyError, TypeError, ValueError:
        return False


def _read_input_receipt(output: Path, stdout: str) -> bytes | None:
    claims = [
        line.removeprefix(BS_INPUT_RESULT)
        for line in stdout.splitlines()
        if line.startswith(BS_INPUT_RESULT)
    ]
    if len(claims) != 1:
        return None
    receipts = json.loads(claims[0])
    if len(receipts) != 1 or len(receipts[0]) != 4:
        return None
    path, device, identity, digest = receipts[0]
    target = Path(path).resolve(strict=True)
    if target != (output / BS_INPUT_RECEIPT).resolve(strict=True):
        return None
    with target.open("rb") as handle:
        stat = os.fstat(handle.fileno())
        if (stat.st_dev, stat.st_ino) != (device, identity):
            return None
        encoded = handle.read(32769)
    if len(encoded) > 32768 or hashlib.sha256(encoded).hexdigest() != digest:
        return None
    return encoded
