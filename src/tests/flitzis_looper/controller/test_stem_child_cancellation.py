"""Actual owned entrypoint cancellation with explicitly non-neural fixture models.

The stock Demucs recursion and BS decode/overlap-add/writers run in real children.
Private startup fixtures substitute only model assets/network construction and a
named checkpoint gate. These tests make no release-model or musical quality claim.
"""

import json
import os
import shutil
import struct
import sys
import time
import wave
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
    import hashlib
    import os
    import subprocess
    import sys
    import time
    from pathlib import Path
    from types import SimpleNamespace
    import torch
    from torch import nn
    from flitzis_looper.stem_cancellation import StemCancellation, tool_environment

    output = Path(os.environ['FLITZIS_FIXTURE_OUTPUT'])
    output.mkdir(parents=True, exist_ok=True)
    target = os.environ['FLITZIS_FIXTURE_STAGE']
    variant = os.environ['FLITZIS_FIXTURE_VARIANT']
    seen = 0
    original_checkpoint = StemCancellation.checkpoint

    def checkpoint(self, stage):
        global seen
        if stage == target:
            seen += 1
            # The second Demucs batch boundary proves stock recursive apply uses
            # the same explicit checkpoint, rather than only the CLI wrapper.
            recursive = variant == 'demucs' and target == 'before_model_batch'
            if not recursive or seen == 2:
                (output / 'ready').write_text(stage)
                while True:
                    original_checkpoint(self, stage)
                    time.sleep(.005)
        original_checkpoint(self, stage)

    StemCancellation.checkpoint = checkpoint

    def finished():
        clean = not list(output.rglob('*.part')) and not list(output.glob('.decode-*'))
        (output / 'finished').write_text(str(clean))

    atexit.register(finished)

    class NonNeuralCopy(nn.Module):
        sources = ['drums', 'bass', 'other', 'vocals']
        audio_channels = 2
        samplerate = 44100
        segment = 1

        def forward(self, data):
            if target == 'blocked_model_call':
                child_body = (
                    'import os,time; from pathlib import Path; '
                    + 'Path(' + repr(str(output / 'descendant'))
                    + ').write_text(str(os.getpid())); '
                    + 'time.sleep(60)'
                )
                subprocess.Popen(
                    [sys.executable, '-I', '-c', child_body], env=tool_environment()
                )
                (output / 'ready').write_text(target)
                time.sleep(60)
            return data.unsqueeze(1).repeat(1, 4, 1, 1) * .25

    if variant == 'demucs':
        import demucs.separate
        demucs.separate.get_model_from_args = lambda args: NonNeuralCopy()
    else:
        from flitzis_bs_roformer import inference, model
        config = output / 'config'
        config.write_bytes(b'explicit non-neural cancellation fixture')
        weights = output / 'weights'
        torch.save({}, weights)
        model.CONFIG_BYTES = config.stat().st_size
        model.CONFIG_SHA256 = hashlib.sha256(config.read_bytes()).hexdigest()
        model.CHECKPOINT_BYTES = weights.stat().st_size
        model.CHECKPOINT_SHA256 = hashlib.sha256(weights.read_bytes()).hexdigest()
        model._network_module = lambda: SimpleNamespace(
            BSRoformer=lambda **kwargs: NonNeuralCopy()
        )
        original_plan = inference.ChunkPlan
        inference.ChunkPlan = lambda: original_plan(chunk_size=64, batch_size=1)
    """)


def _fixture(tmp_path: Path, variant: str, stage: str) -> tuple[list[str], dict[str, str], Path]:
    output = tmp_path / "output"
    output.mkdir()
    startup = tmp_path / "startup"
    startup.mkdir()
    (startup / "sitecustomize.py").write_text(_STARTUP, encoding="utf-8")
    source = tmp_path / "source.wav"
    with wave.open(str(source), "wb") as handle:
        handle.setparams((2, 2, 44100, 256, "NONE", "not compressed"))
        handle.writeframes(
            b"".join(struct.pack("<hh", index * 32, index * 16) for index in range(256))
        )
    repository = Path(__file__).resolve().parents[4]
    environment = dict(os.environ)
    environment.pop("FLITZIS_BS_INPUT_CONTRACT", None)
    environment.pop("FLITZIS_DEMUCS_ENCODER_CONTRACT", None)
    environment.update({
        "PYTHONPATH": os.pathsep.join((
            str(startup),
            str(repository / "src"),
            str(repository / "vendor/bs_roformer/src"),
        )),
        "FLITZIS_FIXTURE_OUTPUT": str(output),
        "FLITZIS_FIXTURE_VARIANT": variant,
        "FLITZIS_FIXTURE_STAGE": stage,
    })
    module = "flitzis_looper.demucs_streaming" if variant == "demucs" else "flitzis_bs_roformer"
    arguments = [sys.executable, "-m", module]
    if variant == "demucs":
        arguments.extend([
            "-n",
            "non-neural",
            "-d",
            "cpu",
            "--shifts",
            "0",
            "--segment",
            "1",
            "-o",
            str(output),
            str(source),
        ])
    else:
        arguments.extend([
            "--source",
            str(source),
            "--output",
            str(output),
            "--device",
            "cpu",
            "--checkpoint",
            str(output / "weights"),
            "--config",
            str(output / "config"),
        ])
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
) -> process.OwnedCommandResult:
    marker = temporary / "cancel"
    environment[CANCEL_PATH_ENV] = str(marker)
    return process.run_owned_command(
        arguments,
        cwd=temporary,
        env=environment,
        cancel=cancel,
        retired=retired,
        limits=WorkerLimits(timeout_seconds=40, reap_timeout_seconds=0.5),
        request_abort=partial(stem_process._request_abort, marker),
        abort_grace_seconds=0.4,
        receipt_prefixes=(ENCODER_RESULT.encode(), BS_INPUT_RESULT.encode()),
    )


@pytest.mark.skipif(
    not shutil.which("ffmpeg"), reason="actual input/output tools require FFmpeg PATH"
)
@pytest.mark.parametrize(
    ("variant", "stage"),
    [
        ("demucs", "before_model_stage"),
        ("demucs", "after_model_load"),
        ("demucs", "before_model_batch"),
        ("demucs", "output_pcm_block"),
        ("demucs", "output_verification"),
        ("bs", "before_model_stage"),
        ("bs", "model_artifact_block"),
        ("bs", "after_model_batch"),
        ("bs", "before_output_write"),
        ("bs", "output_verification"),
    ],
)
def test_real_entry_cooperates_at_named_stage_and_cleans_before_exit_130(
    tmp_path: Path,
    variant: str,
    stage: str,
) -> None:
    arguments, environment, output = _fixture(tmp_path, variant, stage)
    cancel, retired = Event(), Event()
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(_run, arguments, environment, tmp_path, cancel, retired)
        try:
            _wait_ready(output / "ready", future)
            assert not retired.is_set()
            if stage in {"output_pcm_block", "output_verification", "before_output_write"}:
                assert list(output.rglob("*.part")), (
                    "checkpoint must occur inside real output custody"
                )
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
    assert not list(output.rglob("*.part"))
    assert not list(output.glob(".decode-*"))
    assert not list(output.rglob("drums.wav"))
    assert retired.is_set()


@pytest.mark.skipif(
    os.name != "nt" or not shutil.which("ffmpeg"), reason="actual Windows own tree and tools"
)
@pytest.mark.parametrize("variant", ["demucs", "bs"])
def test_blocked_model_call_escalates_and_retires_actual_descendant_and_pipe(
    tmp_path: Path,
    variant: str,
) -> None:
    arguments, environment, output = _fixture(tmp_path, variant, "blocked_model_call")
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
    arguments, environment, output = _fixture(tmp_path, "bs", "after_model_batch")
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
