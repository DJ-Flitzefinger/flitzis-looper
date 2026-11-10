"""Opt-in standalone Native protocol/lifecycle check, without learned inference."""

import hashlib
import json
import os
import struct
from pathlib import Path
from threading import Event

import pytest

from flitzis_looper.analysis import process
from flitzis_looper.stem_cancellation import CANCEL_ACK_PREFIX, CANCEL_PATH_ENV


def _assert_pipe_readers_end(readers) -> None:
    assert len(readers) == 2
    for reader in readers:
        assert reader.thread.ident is not None
        assert not reader.thread.is_alive()
        assert reader.stream.closed
        assert not reader.failed.is_set()


def _control_command(executable: Path, tmp_path: Path, directory: Path) -> list[str]:
    directory.mkdir()
    soxr = Path(os.environ["FLITZIS_BEATTHIS_SOXR"]).resolve(strict=True)
    source = tmp_path / "mono.f32"
    source.write_bytes(struct.pack("<24000f", *([0.25, -0.25] * 12000)))
    return [
        str(executable),
        "--cancellation-control",
        str(soxr),
        hashlib.sha256(soxr.read_bytes()).hexdigest(),
        str(source),
        "48000",
        str(directory / "result.json"),
    ]


@pytest.mark.skipif(
    os.name != "nt"
    or not os.environ.get("FLITZIS_BEATTHIS_CANCELLATION_EXE")
    or not os.environ.get("FLITZIS_BEATTHIS_SOXR"),
    reason="requires the explicitly built standalone Windows BeatThis diagnostic",
)
def test_native_cancellation_settles_private_files_tree_and_both_pipe_readers(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, record_property
) -> None:
    executable = Path(os.environ["FLITZIS_BEATTHIS_CANCELLATION_EXE"]).resolve(strict=True)
    marker, directory = tmp_path / "own-cancel", tmp_path / "private"
    foreign = tmp_path / "foreign-cancel"
    foreign.write_bytes(b"cancel")
    cancelled, retired = Event(), Event()
    observed = []
    tree_end = []
    pids = []
    readers_for_command = process._readers_for_command
    wait_tree = process._OwnedProcess.wait_tree

    def readers(*args, **kwargs):
        result = readers_for_command(*args, **kwargs)
        observed.extend(result)
        return result

    def wait(owned, timeout):
        result = wait_tree(owned, timeout)
        tree_end.append(result and not owned.process.tree_active())
        return result

    def stop():
        if observed and b"BeatThis cancellation control ready:" in observed[0].data:
            assert not marker.exists()
            assert any(path.stat().st_size for path in directory.iterdir())
            cancelled.set()

    def request_abort():
        with marker.open("xb") as stream:
            stream.write(b"cancel")

    monkeypatch.setattr(process, "_readers_for_command", readers)
    monkeypatch.setattr(process._OwnedProcess, "wait_tree", wait)
    result = process.run_owned_command(
        _control_command(executable, tmp_path, directory),
        cwd=tmp_path,
        env=dict(os.environ, **{CANCEL_PATH_ENV: str(marker)}),
        cancel=cancelled,
        retired=retired,
        limits=process.WorkerLimits(
            timeout_seconds=20, reap_timeout_seconds=2, max_response_bytes=65536
        ),
        request_abort=request_abort,
        abort_grace_seconds=1,
        stop_reason=stop,
        on_process_started=pids.append,
    )
    assert result.reason == "cancelled"
    assert result.returncode == 130, (result.stdout, result.stderr)
    assert result.stderr == b""
    prefix = CANCEL_ACK_PREFIX.encode("ascii")
    acks = [line for line in result.stdout.splitlines() if line.startswith(prefix)]
    assert len(acks) == 1
    assert json.loads(acks[0][len(prefix) :]) == {"stage": "control.frontend"}
    assert b'"trial_only"' not in result.stdout
    assert retired.is_set()
    assert tree_end
    assert all(tree_end)
    assert len(pids) == 1
    _assert_pipe_readers_end(observed)
    assert directory.is_dir()
    assert not list(directory.iterdir())
    assert foreign.read_bytes() == b"cancel"
    record_property("native_pid", pids[0])
    record_property("native_stdout", result.stdout.decode("ascii"))
    record_property("native_returncode", result.returncode)
    record_property("tree_end", tree_end)
    record_property("pipe_readers_end", 2)
