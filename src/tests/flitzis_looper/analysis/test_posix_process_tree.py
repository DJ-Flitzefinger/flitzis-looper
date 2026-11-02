"""Real Linux supervision of the exact production adapter, without Native or models.

Only the unmodified production ClassDef and its real import bindings are loaded.
This isolates its operating-system behavior; it does not import the whole analysis
module or establish productive Linux application or Native admission acceptance.
"""

import ast
import ctypes
import hashlib
import json
import os
import select
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from contextlib import contextmanager, suppress
from pathlib import Path
from threading import Event, Thread
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from collections.abc import Iterator
    from typing import IO

    from flitzis_looper.analysis.process import _OwnedTreeProcess


def _adapter() -> tuple[type[_OwnedTreeProcess], dict[str, object]]:
    root = Path(__file__).resolve().parents[4]
    source_path = root / "src/flitzis_looper/analysis/process.py"
    source = source_path.read_bytes()
    module = ast.parse(source, filename=str(source_path))
    declaration = next(
        item
        for item in module.body
        if isinstance(item, ast.ClassDef) and item.name == "_OwnedTreeProcess"
    )
    required = {
        item.id
        for item in ast.walk(declaration)
        if isinstance(item, ast.Name) and isinstance(item.ctx, ast.Load)
    }
    imports = []
    for item in module.body:
        if isinstance(item, ast.Import):
            names = {alias.asname or alias.name.split(".")[0] for alias in item.names}
        elif isinstance(item, ast.ImportFrom):
            names = {alias.asname or alias.name for alias in item.names}
        else:
            continue
        if required.intersection(names):
            imports.append(item)
    namespace = {
        "__name__": "flitzis_looper.analysis.process",
        "__package__": "flitzis_looper.analysis",
        "__file__": str(source_path),
    }
    sys.path.insert(0, str(root / "src"))
    # Neither the declaration nor an imported binding is rewritten or mocked.
    compiled = ast.Module(body=[*imports, declaration], type_ignores=[])
    exec(compile(compiled, str(source_path), "exec"), namespace)
    return cast("type[_OwnedTreeProcess]", namespace["_OwnedTreeProcess"]), {
        "scope": "exact-production-class-body-real-linux",
        "source": str(source_path),
        "source_sha256": hashlib.sha256(source).hexdigest(),
        "class_ast_sha256": hashlib.sha256(ast.dump(declaration).encode()).hexdigest(),
        "imports": [ast.unparse(item) for item in imports],
        "native_imported": "flitzis_looper_audio" in sys.modules,
        "pydantic_imported": "pydantic" in sys.modules,
        "python": sys.version,
        "executable": sys.executable,
        "uname": list(os.uname()),
    }


def _exited(identity: int, timeout: float = 0.0) -> bool:
    return bool(select.select([identity], [], [], timeout)[0])


def _wait_file(path: Path) -> None:
    deadline = time.monotonic() + 5
    while not path.is_file() or not path.stat().st_size:
        if time.monotonic() >= deadline:
            message = "owned fixture did not reach its handshake"
            raise AssertionError(message)
        Event().wait(0.005)


def _subreaper(value: int | None = None) -> int:
    """Make only this test driver reap its deliberately orphaned fixture children."""
    library = ctypes.CDLL(None, use_errno=True)
    library.prctl.restype = ctypes.c_int
    previous = ctypes.c_int()
    if library.prctl(37, ctypes.byref(previous), 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), "PR_GET_CHILD_SUBREAPER")
    if value is not None and library.prctl(36, ctypes.c_ulong(value), 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), "PR_SET_CHILD_SUBREAPER")
    return previous.value


_WORKER = """
import json, os, subprocess, sys, time
from pathlib import Path
directory = Path(sys.argv[1])
shared = sys.argv[2] == 'shared'
child_code = '''
import json, os, time
from pathlib import Path
directory = Path({directory!r})
marker = directory.joinpath('child.tmp')
marker.write_text(json.dumps({{'pid': os.getpid()}}))
marker.replace(directory.joinpath('child'))
while True:
    time.sleep(0.01)
'''.format(directory=str(directory))
child = subprocess.Popen(
    [sys.executable, '-I', '-c', child_code],
    stdin=subprocess.DEVNULL,
    stdout=None if shared else subprocess.DEVNULL,
    stderr=None if shared else subprocess.DEVNULL,
)
while not directory.joinpath('child').exists():
    time.sleep(0.005)
marker = directory.joinpath('root.tmp')
marker.write_text(json.dumps({'pid': os.getpid()}))
marker.replace(directory.joinpath('root'))
while not directory.joinpath('release').exists():
    time.sleep(0.005)
print('supervised-output', flush=True)
"""


def _report(label: str, data: object) -> None:
    sys.stdout.write(label + " " + json.dumps(data) + "\n")
    sys.stdout.flush()


def _read_stream(stream: IO[bytes], output: bytearray, ended: Event) -> None:
    output.extend(stream.read())
    ended.set()


class _Fixture:
    def __init__(self) -> None:
        parent = Path(os.environ.get("FLITZIS_POSIX_TEST_ROOT", tempfile.gettempdir()))
        self.temporary = tempfile.TemporaryDirectory(prefix="owned-posix-", dir=parent)
        self.directory = Path(self.temporary.name)
        self.previous_subreaper = _subreaper(1)
        self.identities: dict[int, int] = {}
        self.births: dict[int, str] = {}
        self.processes: list[subprocess.Popen[bytes]] = []
        self.readers: list[Thread] = []
        self.stdout = bytearray()
        self.stderr = bytearray()
        self.stdout_end = Event()
        self.stderr_end = Event()

    def remember(self, pid: int) -> None:
        if pid not in self.identities:
            self.identities[pid] = os.pidfd_open(pid)
            self.births[pid] = (
                Path(f"/proc/{pid}/stat").read_text(encoding="ascii").rsplit(")", 1)[1].split()[19]
            )
        assert not _exited(self.identities[pid])

    def start(self, adapter: type[_OwnedTreeProcess], *, shared_output: bool) -> _OwnedTreeProcess:
        unrelated = subprocess.Popen(
            [sys.executable, "-I", "-c", "import time; time.sleep(60)"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        self.processes.append(unrelated)
        self.remember(unrelated.pid)
        worker = adapter(
            [
                sys.executable,
                "-I",
                "-c",
                _WORKER,
                str(self.directory),
                "shared" if shared_output else "quiet",
            ],
            cwd=self.directory,
            env=dict(os.environ),
            capture_stderr=True,
        )
        self.processes.append(worker)
        self.remember(worker.pid)
        assert worker._job is None
        assert worker.stdout is not None
        assert worker.stderr is not None
        self.add_reader(worker.stdout, self.stdout, self.stdout_end)
        self.add_reader(worker.stderr, self.stderr, self.stderr_end)
        worker.activate()
        return worker

    def add_reader(self, stream: IO[bytes], output: bytearray, ended: Event) -> None:
        reader = Thread(
            target=_read_stream, args=(stream, output, ended), name="scoped-posix-reader"
        )
        self.readers.append(reader)
        reader.start()

    def reap_orphans(self) -> None:
        """Reap only this private driver's fixture children, including setup failures."""
        deadline = time.monotonic() + 5
        while True:
            try:
                child, _ = os.waitpid(-1, os.WNOHANG)
            except ChildProcessError:
                return
            if child:
                continue
            children = (
                Path(f"/proc/self/task/{os.getpid()}/children").read_text(encoding="ascii").split()
            )
            for value in children:
                with suppress(ProcessLookupError):
                    identity = os.pidfd_open(int(value))
                    try:
                        signal.pidfd_send_signal(identity, signal.SIGKILL)
                    finally:
                        os.close(identity)
            assert time.monotonic() < deadline, "owned orphan did not reap"
            Event().wait(0.005)

    def close(self) -> None:
        # END precedes reader closure, temporary removal and restoring driver state.
        try:
            for identity in reversed(tuple(self.identities.values())):
                if not _exited(identity):
                    with suppress(ProcessLookupError):
                        signal.pidfd_send_signal(identity, signal.SIGKILL)
                assert _exited(identity, 5), "owned fixture process did not end"
            for process in self.processes:
                process.wait(timeout=5)
            self.reap_orphans()
            for reader in self.readers:
                reader.join(timeout=5)
                assert not reader.is_alive(), "actual output reader did not end"
            for process in self.processes:
                for stream in (process.stdout, process.stderr):
                    if stream is not None:
                        stream.close()
                if hasattr(process, "close_tree"):
                    process.close_tree()
            for identity in self.identities.values():
                os.close(identity)
            self.temporary.cleanup()
        finally:
            _subreaper(self.previous_subreaper)


@contextmanager
def _fixture() -> Iterator[_Fixture]:
    fixture = _Fixture()
    try:
        yield fixture
    finally:
        fixture.close()


def _scenario(*, shared_output: bool) -> None:
    adapter, lineage = _adapter()
    _report("POSIX_LINEAGE", lineage)
    with _fixture() as fixture:
        worker = fixture.start(adapter, shared_output=shared_output)
        _wait_file(fixture.directory / "root")
        child_pid = json.loads((fixture.directory / "child").read_text(encoding="utf-8"))["pid"]
        root_pid = json.loads((fixture.directory / "root").read_text(encoding="utf-8"))["pid"]
        fixture.remember(child_pid)
        fixture.remember(root_pid)
        fixture.directory.joinpath("release").touch()
        returncode = worker.wait(timeout=5)
        active = worker.tree_active()
        worker.kill()
        observed = {
            "shared_output": shared_output,
            "wrapper_pid": worker.pid,
            "payload_pid": root_pid,
            "child_pid": child_pid,
            "birth_ticks": dict(fixture.births),
            "returncode": returncode,
            "tree_active_after_root": active,
            "child_alive_after_kill": not _exited(fixture.identities[child_pid], 0.1),
            "stdout_ended_before_cleanup": fixture.stdout_end.wait(1.0),
            "stderr_ended_before_cleanup": fixture.stderr_end.wait(1.0),
            "unrelated_alive": not _exited(fixture.identities[fixture.processes[0].pid]),
        }
    observed["cleanup_completed"] = True
    observed["reader_threads_ended"] = all(not reader.is_alive() for reader in fixture.readers)
    observed["stdout_after_cleanup"] = bytes(fixture.stdout).decode("utf-8")
    observed["stderr_after_cleanup"] = bytes(fixture.stderr).decode("utf-8")
    _report("POSIX_OBSERVED", observed)
    assert observed["returncode"] == 0
    assert observed["unrelated_alive"]
    assert not observed["child_alive_after_kill"], "actual descendant remains alive"
    assert observed["stdout_ended_before_cleanup"], "actual stdout reader remains live"
    assert observed["stderr_ended_before_cleanup"], "actual stderr reader remains live"
    assert bytes(fixture.stdout) == b"supervised-output\n"
    assert bytes(fixture.stderr) == b""


@unittest.skipUnless(sys.platform == "linux", "actual Linux pidfd process supervision")
class LinuxOwnedProcessTests(unittest.TestCase):
    def test_silent_descendant_ends_with_its_original_supervisor(self) -> None:
        _scenario(shared_output=False)

    def test_inherited_pipe_descendant_ends_before_reader_release(self) -> None:
        _scenario(shared_output=True)


if __name__ == "__main__":
    names = unittest.defaultTestLoader.getTestCaseNames(LinuxOwnedProcessTests)
    _report("POSIX_TESTS", names)
    if sys.argv[1:] == ["--list"]:
        _, metadata = _adapter()
        _report("POSIX_LINEAGE", metadata)
    else:
        unittest.main()
