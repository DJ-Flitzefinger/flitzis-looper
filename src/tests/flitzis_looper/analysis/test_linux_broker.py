"""Real Linux broker and exact supervisor declarations, without Native/Pydantic.

This is bounded OS/supervisor integration, not full Linux application admission.
The two original qualified regressions remain byte-identical in their own file.
"""

import ast
import hashlib
import json
import os
import signal
import subprocess
import sys
import time
import types
import unittest
from contextlib import contextmanager
from pathlib import Path
from threading import Event
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper.analysis.linux_process import LinuxProcess
    from flitzis_looper.analysis.process import _OwnedTreeProcess

if sys.platform == "linux":
    sys.path.insert(0, str(Path(__file__).resolve().parents[4] / "src"))
    from tests.flitzis_looper.analysis.test_posix_process_tree import (
        _exited,
        _Fixture,
        _report,
        _wait_file,
    )

    class _LinuxFixture(_Fixture):
        """Cleanup uses test-owned pidfds even when product proof deliberately fails."""

        def __init__(self) -> None:
            super().__init__()
            self.owners: list[LinuxProcess] = []

        def close(self) -> None:
            try:
                super().close()
            finally:
                for owner in self.owners:
                    owner._control.close()
                    if owner._identity is not None:
                        os.close(owner._identity)
                        owner._identity = None


@contextmanager
def _expanded_fixture() -> Iterator[_LinuxFixture]:
    fixture = _LinuxFixture()
    try:
        yield fixture
    finally:
        fixture.close()


def _selected_declaration(item: ast.stmt, selected: set[str]) -> bool:
    if isinstance(item, ast.Import):
        return True
    if isinstance(item, ast.ImportFrom):
        return item.module not in {"flitzis_looper.analysis.contracts", "flitzis_looper_audio"}
    if isinstance(item, ast.ClassDef | ast.FunctionDef):
        return item.name in selected
    if isinstance(item, ast.Assign):
        return any(
            isinstance(target, ast.Name) and target.id in {"_POLL_SECONDS", "_READ_CHUNK_BYTES"}
            for target in item.targets
        )
    return False


def _supervisor() -> types.ModuleType:
    """Compile unchanged selected production declarations and genuine dependencies."""
    root = Path(__file__).resolve().parents[4]
    selected = {
        "ProcessResult",
        "OwnedCommandResult",
        "IndependentReceipts",
        "_OwnedTreeProcess",
        "_ResponseReader",
        "_CommandReader",
        "_OwnedProcess",
        "_resource_stop_reason",
        "_pending_stop_reason",
        "_completed_reason",
        "_monitor",
        "_finish",
        "_retire_later",
        "_command_readers",
        "_readers_for_command",
        "_ObserverSetupError",
        "_bind_command_observer",
        "_supervise_command",
        "run_owned_command",
        "_release_temporary",
    }
    module = types.ModuleType("linux_supervisor_declarations")
    sys.modules[module.__name__] = module
    contracts = root / "src/flitzis_looper/analysis/contracts.py"
    limits = next(
        item
        for item in ast.parse(contracts.read_bytes()).body
        if isinstance(item, ast.ClassDef) and item.name == "WorkerLimits"
    )
    dependency_imports = ast.parse("import math\nfrom dataclasses import dataclass").body
    exec(
        compile(
            ast.Module(body=[*dependency_imports, limits], type_ignores=[]), str(contracts), "exec"
        ),
        module.__dict__,
    )
    source_path = root / "src/flitzis_looper/analysis/process.py"
    source = source_path.read_bytes()
    tree = ast.parse(source)
    declarations = [item for item in tree.body if _selected_declaration(item, selected)]
    exec(
        compile(ast.Module(body=declarations, type_ignores=[]), str(source_path), "exec"),
        module.__dict__,
    )
    _report(
        "LINUX_SUPERVISOR_LINEAGE",
        {
            "scope": "exact-selected-supervisor-and-WorkerLimits-declarations-real-imports",
            "source_sha256": hashlib.sha256(source).hexdigest(),
            "declarations": sorted(selected),
            "limits_ast_sha256": hashlib.sha256(ast.dump(limits).encode()).hexdigest(),
            "native_imported": "flitzis_looper_audio" in sys.modules,
            "pydantic_imported": "pydantic" in sys.modules,
        },
    )
    return module


def _launch(module: types.ModuleType, fixture: _LinuxFixture, code: str) -> _OwnedTreeProcess:
    worker = module._OwnedTreeProcess(
        [sys.executable, "-I", "-c", code, str(fixture.directory)],
        cwd=fixture.directory,
        env=dict(os.environ),
        capture_stderr=True,
    )
    assert worker._linux is not None
    fixture.owners.append(worker._linux)
    fixture.processes.append(worker._linux._broker)
    fixture.remember(worker._linux.broker_pid)
    fixture.remember(worker.pid)
    assert worker.stdout is not None
    assert worker.stderr is not None
    fixture.add_reader(worker.stdout, fixture.stdout, fixture.stdout_end)
    fixture.add_reader(worker.stderr, fixture.stderr, fixture.stderr_end)
    return worker


def _settle(worker: _OwnedTreeProcess, fixture: _Fixture) -> int:
    worker.kill()
    result = worker.wait(timeout=5)
    deadline = time.monotonic() + 5
    while worker.tree_active():
        assert time.monotonic() < deadline, "private tree did not retire"
        Event().wait(0.005)
    assert fixture.stdout_end.wait(1)
    assert fixture.stderr_end.wait(1)
    assert all(_exited(identity, 1) for identity in fixture.identities.values())
    worker.close_tree()
    return result


@unittest.skipUnless(sys.platform == "linux", "actual Linux private ownership")
class LinuxBrokerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.supervisor = _supervisor()

    def test_gated_payload_identity_and_original_returncodes(self) -> None:
        with _expanded_fixture() as fixture:
            worker = _launch(
                self.supervisor,
                fixture,
                "import os,sys; from pathlib import Path; "
                "Path(sys.argv[1], 'ready').write_text(str(os.getpid())); "
                "print('payload-output'); sys.stderr.write('payload-error\\n'); "
                "sys.exit(7)",
            )
            assert worker._linux is not None
            assert worker.pid != worker._linux.broker_pid
            assert not fixture.directory.joinpath("ready").exists()
            assert worker.poll() is None
            assert not fixture.stdout_end.is_set()
            worker.activate()
            _wait_file(fixture.directory / "ready")
            assert int(fixture.directory.joinpath("ready").read_text()) == worker.pid
            assert worker.wait(timeout=5) == 7
            assert _settle(worker, fixture) == 7
        assert fixture.stdout == b"payload-output\n"
        assert fixture.stderr == b"payload-error\n"

    def test_setsid_double_fork_descendant_and_unrelated_survivor(self) -> None:
        code = """
import os, sys, time
from pathlib import Path
directory = Path(sys.argv[1])
if os.fork() == 0:
    os.setsid()
    if os.fork() != 0:
        os._exit(0)
    directory.joinpath('leaf').write_text(str(os.getpid()))
    while True:
        time.sleep(0.01)
while not directory.joinpath('release').exists():
    time.sleep(0.005)
print('escaped-output', flush=True)
"""
        with _expanded_fixture() as fixture:
            unrelated = subprocess.Popen(
                [sys.executable, "-I", "-c", "import time; time.sleep(60)"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
            try:
                worker = _launch(self.supervisor, fixture, code)
                worker.activate()
                _wait_file(fixture.directory / "leaf")
                fixture.remember(int(fixture.directory.joinpath("leaf").read_text()))
                fixture.directory.joinpath("release").touch()
                assert worker.wait(timeout=5) == 0
                assert _settle(worker, fixture) == 0
                assert unrelated.poll() is None
            finally:
                if unrelated.poll() is None:
                    unrelated.kill()
                unrelated.wait(timeout=5)
        assert fixture.stdout == b"escaped-output\n"
        assert fixture.stderr == b""

    def test_killed_gated_payload_keeps_its_pid_reserved_until_teardown(self) -> None:
        with _expanded_fixture() as fixture:
            worker = _launch(self.supervisor, fixture, "raise SystemExit(0)")
            payload = worker.pid
            signal.pidfd_send_signal(fixture.identities[payload], signal.SIGKILL)
            assert _exited(fixture.identities[payload], 1)
            # An unreaped direct child cannot recycle its observer's numeric PID.
            state = (
                Path(f"/proc/{payload}/stat")
                .read_text(encoding="ascii")
                .rsplit(")", 1)[1]
                .split()[0]
            )
            assert state == "Z"
            assert worker.poll() is None
            assert _settle(worker, fixture) == -signal.SIGKILL

    def test_stop_after_payload_exit_drains_before_tree_acknowledgement(self) -> None:
        with _expanded_fixture() as fixture:
            worker = _launch(self.supervisor, fixture, "raise SystemExit(0)")
            assert worker._linux is not None
            linux = worker._linux
            worker.activate()
            assert _exited(fixture.identities[worker.pid], 5)
            assert not _exited(fixture.identities[linux.broker_pid])
            # Queue a real teardown packet before consuming X/E, including when
            # the genuine ECHILD broker is already waiting for acknowledgement.
            assert linux._control.send(b"K") == 1
            assert worker.wait(timeout=5) == 0
            assert _settle(worker, fixture) == 0

    def test_real_supervisor_success_signal_and_missing_executable(self) -> None:
        commands = [
            ([sys.executable, "-I", "-c", "print('success')"], 0, "ok", b"success\n"),
            (
                [
                    sys.executable,
                    "-I",
                    "-c",
                    "import os,signal; os.kill(os.getpid(),signal.SIGTERM)",
                ],
                -signal.SIGTERM,
                "worker_crashed",
                b"",
            ),
            (["/flitzis-deliberately-missing-executable"], 127, "worker_crashed", b""),
        ]
        for arguments, status, reason, output in commands:
            with self.subTest(status=status), _expanded_fixture() as fixture:
                retired = Event()
                result = self.supervisor.run_owned_command(
                    arguments,
                    cwd=fixture.directory,
                    env=dict(os.environ),
                    cancel=Event(),
                    retired=retired,
                    limits=self.supervisor.WorkerLimits(reap_timeout_seconds=1),
                )
                assert retired.is_set()
                assert (result.returncode, result.reason, result.stdout, result.stderr) == (
                    status,
                    reason,
                    output,
                    b"",
                )

    def test_observer_refusal_and_pre_go_cancellation_prevent_exec(self) -> None:
        for refusal in (True, False):
            with self.subTest(refusal=refusal), _expanded_fixture() as fixture:
                cancel, retired = Event(), Event()

                def observer(
                    pid: int,
                    *,
                    refusal: bool = refusal,
                    cancel: Event = cancel,
                    fixture: _LinuxFixture = fixture,
                ) -> None:
                    fixture.remember(pid)
                    assert not fixture.directory.joinpath("executed").exists()
                    if refusal:
                        message = "deliberate observer refusal"
                        raise RuntimeError(message)
                    cancel.set()

                result = self.supervisor.run_owned_command(
                    [
                        sys.executable,
                        "-I",
                        "-c",
                        (
                            "from pathlib import Path; "
                            f"Path({str(fixture.directory / 'executed')!r}).touch()"
                        ),
                    ],
                    cwd=fixture.directory,
                    env=dict(os.environ),
                    cancel=cancel,
                    retired=retired,
                    on_process_started=observer,
                    limits=self.supervisor.WorkerLimits(reap_timeout_seconds=1),
                )
                assert result.reason == ("resource_unavailable" if refusal else "cancelled")
                assert retired.is_set()
                assert not fixture.directory.joinpath("executed").exists()
                assert all(_exited(identity, 1) for identity in fixture.identities.values())

    def test_timeout_and_cancel_retain_real_supervisor_cleanup(self) -> None:
        for cancelled in (True, False):
            with self.subTest(cancelled=cancelled), _expanded_fixture() as fixture:
                cancel, retired = Event(), Event()

                def observer(pid: int) -> None:
                    fixture.remember(pid)

                def stop_reason(
                    *,
                    cancelled: bool = cancelled,
                    cancel: Event = cancel,
                    fixture: _LinuxFixture = fixture,
                ) -> None:
                    if cancelled and fixture.directory.joinpath("ready").exists():
                        cancel.set()

                result = self.supervisor.run_owned_command(
                    [
                        sys.executable,
                        "-I",
                        "-c",
                        (
                            "import time; from pathlib import Path; "
                            f"Path({str(fixture.directory / 'ready')!r}).touch(); time.sleep(60)"
                        ),
                    ],
                    cwd=fixture.directory,
                    env=dict(os.environ),
                    cancel=cancel,
                    retired=retired,
                    on_process_started=observer,
                    stop_reason=stop_reason,
                    limits=self.supervisor.WorkerLimits(
                        timeout_seconds=0.25, reap_timeout_seconds=1
                    ),
                )
                assert result.reason == ("cancelled" if cancelled else "worker_timeout")
                assert retired.is_set()
                assert all(_exited(identity, 1) for identity in fixture.identities.values())

    def test_broker_loss_cannot_release_original_owner(self) -> None:
        self._lost_ownership(broker_lost=True)

    def test_control_loss_cleans_tree_without_fabricating_retirement(self) -> None:
        self._lost_ownership(broker_lost=False)

    def _lost_ownership(self, *, broker_lost: bool) -> None:
        # This standalone driver is the subreaper ONLY for deliberate fixture cleanup.
        with _expanded_fixture() as fixture:
            worker = _launch(self.supervisor, fixture, "import time; time.sleep(60)")
            assert worker._linux is not None
            linux = worker._linux
            retired = Event()
            owner = self.supervisor._OwnedProcess(worker, None, None, retired)
            worker.activate()
            if broker_lost:
                signal.pidfd_send_signal(fixture.identities[linux.broker_pid], signal.SIGKILL)
                assert linux._broker.wait(timeout=5) == -signal.SIGKILL
            else:
                linux._control.close()
            failed = False
            try:
                worker.poll()
            except OSError:
                failed = True
            assert failed, "lost proof must reject payload completion"
            assert worker.tree_active()
            assert not owner.release()
            assert not retired.is_set()
            if not broker_lost:
                assert linux._broker.wait(timeout=5) == 0
                assert _exited(fixture.identities[worker.pid], 1)
                assert fixture.stdout_end.wait(1)
                assert fixture.stderr_end.wait(1)
                assert worker.tree_active()
                assert not retired.is_set()


if __name__ == "__main__":
    names = unittest.defaultTestLoader.getTestCaseNames(LinuxBrokerTests)
    sys.stdout.write("LINUX_BROKER_TESTS " + json.dumps(names) + "\n")
    sys.stdout.flush()
    if sys.argv[1:] == ["--list"]:
        _supervisor()
    else:
        unittest.main()
