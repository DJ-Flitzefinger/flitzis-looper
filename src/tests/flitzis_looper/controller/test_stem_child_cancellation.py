"""Actual owned HTDemucs codec cancellation at named checkpoints."""

import json
import os
import shutil
import sys
import time
from concurrent.futures import Future, ThreadPoolExecutor
from functools import partial
from pathlib import Path
from textwrap import dedent
from threading import Event

import pytest

from flitzis_looper.analysis import process
from flitzis_looper.analysis.contracts import WorkerLimits
from flitzis_looper.bs_input_provenance import BS_INPUT_RESULT
from flitzis_looper.controller import stem_process
from flitzis_looper.encoder_provenance import ENCODER_RESULT
from flitzis_looper.stem_cancellation import CANCEL_ACK_PREFIX, CANCEL_EXIT_CODE, CANCEL_PATH_ENV
from tests.flitzis_looper.analysis.test_owned_command import _observed_process

_STARTUP = dedent("""\
    import atexit
    import os
    import subprocess
    import sys
    import time
    from pathlib import Path
    from flitzis_looper.stem_cancellation import StemCancellation, tool_environment

    output = Path(os.environ['FLITZIS_FIXTURE_OUTPUT'])
    target = os.environ['FLITZIS_FIXTURE_STAGE']
    original_checkpoint = StemCancellation.checkpoint

    def checkpoint(self, stage):
        if stage == 'encoder_write' and target == 'blocked_codec_call':
            child_body = (
                'import os,time; from pathlib import Path; '
                + 'Path(' + repr(str(output / 'descendant'))
                + ').write_text(str(os.getpid())); '
                + 'time.sleep(60)'
            )
            subprocess.Popen([sys.executable, '-I', '-c', child_body], env=tool_environment())
            (output / 'ready').write_text(target)
            time.sleep(60)
        if stage == target:
            (output / 'ready').write_text(stage)
            while True:
                original_checkpoint(self, stage)
                time.sleep(.005)
        original_checkpoint(self, stage)

    StemCancellation.checkpoint = checkpoint
    atexit.register(lambda: (output / 'finished').write_text('True'))
    """)


def _fixture(tmp_path: Path, stage: str) -> tuple[list[str], dict[str, str], Path]:
    output = tmp_path / "output"
    output.mkdir()
    startup = tmp_path / "startup"
    startup.mkdir()
    (startup / "sitecustomize.py").write_text(_STARTUP, encoding="utf-8")
    executable = shutil.which("ffmpeg")
    assert executable is not None
    for kind in ("drums", "bass", "other", "vocals"):
        (output / f"{kind}.f32").write_bytes(bytes(256 * 8))
    repository = Path(__file__).resolve().parents[4]
    environment = dict(os.environ)
    environment.pop("FLITZIS_BS_INPUT_CONTRACT", None)
    environment.pop("FLITZIS_DEMUCS_ENCODER_CONTRACT", None)
    environment.update({
        "PYTHONPATH": os.pathsep.join((str(startup), str(repository / "src"))),
        "FLITZIS_FIXTURE_OUTPUT": str(output),
        "FLITZIS_FIXTURE_STAGE": stage,
    })
    arguments = [
        sys.executable,
        "-m",
        "flitzis_looper.htdemucs_codec",
        "encode",
        executable,
        str(output),
    ]
    return arguments, environment, output


def _wait_ready(path: Path, future: Future[process.OwnedCommandResult]) -> None:
    deadline = time.monotonic() + 30
    while not path.is_file() or not path.stat().st_size:
        if future.done():
            pytest.fail(f"own entrypoint exited before the checkpoint: {future.result()}")
        assert time.monotonic() < deadline, "own entrypoint failed to reach checkpoint"
        Event().wait(0.005)


def _run(
    arguments: list[str],
    environment: dict[str, str],
    temporary: Path,
    cancel: Event,
    retired: Event,
    *,
    cooperative_exit: bool = False,
) -> process.OwnedCommandResult:
    marker = temporary / "cancel"
    environment[CANCEL_PATH_ENV] = str(marker)
    return process.run_owned_command(
        arguments,
        cwd=temporary,
        env=environment,
        cancel=cancel,
        retired=retired,
        limits=WorkerLimits(
            timeout_seconds=40, reap_timeout_seconds=5.0 if cooperative_exit else 0.5
        ),
        request_abort=partial(stem_process._request_abort, marker),
        abort_grace_seconds=4.0 if cooperative_exit else 0.4,
        receipt_prefixes=(ENCODER_RESULT.encode(), BS_INPUT_RESULT.encode()),
    )


@pytest.mark.skipif(
    not shutil.which("ffmpeg"), reason="actual input/output tools require FFmpeg PATH"
)
@pytest.mark.parametrize("stage", ["encoder_start", "encoder_write", "codec_output_witness"])
def test_real_entry_cooperates_at_named_stage_and_exits_130(tmp_path: Path, stage: str) -> None:
    arguments, environment, output = _fixture(tmp_path, stage)
    cancel, retired = Event(), Event()
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(
            _run, arguments, environment, tmp_path, cancel, retired, cooperative_exit=True
        )
        try:
            _wait_ready(output / "ready", future)
            assert not retired.is_set()
            cancel.set()
            result = future.result(10)
        finally:
            cancel.set()
    assert result.reason == "cancelled"
    assert result.returncode == CANCEL_EXIT_CODE
    claims = [
        line.removeprefix(CANCEL_ACK_PREFIX)
        for line in result.stdout.decode().splitlines()
        if line.startswith(CANCEL_ACK_PREFIX)
    ]
    assert [json.loads(claim) for claim in claims] == [{"stage": stage}]
    assert ENCODER_RESULT.encode() not in result.stdout
    assert BS_INPUT_RESULT.encode() not in result.stdout
    assert (output / "finished").read_text() == "True"
    if stage == "codec_output_witness":
        assert all(
            (output / f"{kind}.wav").is_file() for kind in ("drums", "bass", "other", "vocals")
        )
    else:
        assert not (output / "vocals.wav").exists()
    assert retired.is_set()


@pytest.mark.skipif(
    os.name != "nt" or not shutil.which("ffmpeg"), reason="actual Windows own tree and tools"
)
def test_blocked_codec_call_escalates_and_retires_actual_descendant_and_pipe(
    tmp_path: Path,
) -> None:
    arguments, environment, output = _fixture(tmp_path, "blocked_codec_call")
    cancel, retired = Event(), Event()
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(_run, arguments, environment, tmp_path, cancel, retired)
        try:
            _wait_ready(output / "ready", future)
            _wait_ready(output / "descendant", future)
            with _observed_process(int((output / "descendant").read_text())) as (kernel, handle):
                assert kernel.WaitForSingleObject(handle, 0) == 258
                started = time.monotonic()
                cancel.set()
                result = future.result(10)
                assert 0.3 <= time.monotonic() - started < 5
                assert kernel.WaitForSingleObject(handle, 0) == 0
        finally:
            cancel.set()
    assert result.reason == "cancelled"
    assert result.returncode != CANCEL_EXIT_CODE
    assert CANCEL_ACK_PREFIX.encode() not in result.stdout
    assert ENCODER_RESULT.encode() not in result.stdout
    assert BS_INPUT_RESULT.encode() not in result.stdout
    assert not (output / "finished").exists(), "forced termination must not claim finally cleanup"
    assert retired.is_set(), "actual inherited output pipes must be settled before caller cleanup"


def test_own_marker_namespace_survives_delayed_actual_output_read_end(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    if not shutil.which("ffmpeg"):
        pytest.skip("actual input tools require FFmpeg PATH")
    arguments, environment, output = _fixture(tmp_path, "encoder_write")
    private = tmp_path / "job-private"
    cancel, eof, release = Event(), Event(), Event()
    original = process._CommandReader._read

    def read(reader: process._CommandReader) -> None:
        original(reader)
        eof.set()
        assert release.wait(10)

    monkeypatch.setattr(process._CommandReader, "_read", read)

    def run() -> None:
        with stem_process.stem_command_scope(cancel, directory=private):
            stem_process.run_stem_command(arguments, cwd=tmp_path, env=environment)

    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(run)
        try:
            deadline = time.monotonic() + 30
            while not (output / "ready").is_file():
                assert time.monotonic() < deadline
                Event().wait(0.005)
            cancel.set()
            assert eof.wait(10)
            assert not future.done()
            channels = list(private.glob(".stem-command-*/cancel"))
            assert len(channels) == 1
            assert channels[0].read_bytes() == b"cancel"
        finally:
            cancel.set()
            release.set()
        with pytest.raises(stem_process.StemCommandCancelledError):
            future.result(10)
    assert not list(private.glob(".stem-command-*"))
    assert (output / "finished").read_text() == "True"
