"""Single-thread Linux subreaper for one gated offline payload, using only stdlib.

Launched as an isolated script. No application imports, global App subreaper,
process-group assumptions or unrelated children are involved.
"""

import array
import ctypes
import errno
import os
import select
import signal
import socket
import sys
import time
from contextlib import suppress
from pathlib import Path
from typing import TextIO

_WAIT_ALL = 0x40000000  # Linux __WALL also covers clone children with a non-SIGCHLD exit.


def _subreaper() -> None:
    library = ctypes.CDLL(None, use_errno=True)
    library.prctl.restype = ctypes.c_int
    if library.prctl(36, ctypes.c_ulong(1), 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), "PR_SET_CHILD_SUBREAPER")
    # An exited, unreaped child keeps its PID reserved during pidfd acquisition.
    signal.signal(signal.SIGCHLD, signal.SIG_DFL)


def _children() -> list[int]:
    path = Path(f"/proc/self/task/{os.getpid()}/children")
    return [int(value) for value in path.read_text(encoding="ascii").split()]


def _terminate_children() -> None:
    # Single-threaded: no wait/reap can recycle any listed direct child's identity.
    # Killing parents reparents even setsid/double-fork descendants to this broker.
    for pid in _children():
        with suppress(ProcessLookupError):
            identity = os.pidfd_open(pid)
            try:
                signal.pidfd_send_signal(identity, signal.SIGKILL)
            finally:
                os.close(identity)


def _reap(payload: int, control: socket.socket, *, connected: bool) -> tuple[bool, bool]:
    exited = False
    while True:
        try:
            pid, status = os.waitpid(-1, os.WNOHANG | _WAIT_ALL)
        except ChildProcessError:
            return exited, not _children()
        if pid == 0:
            return exited, False
        if pid == payload:
            exited = True
            if connected:
                control.send(b"X " + str(os.waitstatus_to_exitcode(status)).encode("ascii"))


def _payload(arguments: list[str], control: socket.socket, gate: tuple[int, int]) -> None:
    control.close()
    os.close(gate[1])
    permission = os.read(gate[0], 1)
    os.close(gate[0])
    if permission == b"G":
        with suppress(OSError):
            os.execvpe(arguments[0], arguments, os.environ)
    os._exit(127)


def _close_output(stream: TextIO) -> None:
    descriptor = stream.fileno()
    stream.close()
    # CPython's standard streams use closefd=False. Their closed Python wrappers
    # alone do not end the broker's kernel pipe writers. No FD allocation/reuse
    # intervenes in this single-thread broker after complete child retirement.
    try:
        os.close(descriptor)
    except OSError as error:
        if error.errno != errno.EBADF:
            raise


def _acknowledge_end(control: socket.socket) -> None:
    """Drain racing teardown requests before closing the final proof channel.

    Closing AF_UNIX with an unread K can reset the peer and discard its queued
    TreeEND. The acknowledgement orders both directions without weakening ECHILD.
    """
    control.send(b"E")
    # All payload writers are genuinely gone. Their readers may reach real EOF
    # independently of the final control acknowledgement and actual broker exit.
    _close_output(sys.stdout)
    _close_output(sys.stderr)
    while (command := control.recv(2)) not in {b"A", b""}:
        if command != b"K":
            message = "Invalid Linux worker retirement acknowledgement"
            raise OSError(message)


def _run(control: socket.socket, payload: int, gate: int) -> None:
    stopping = False
    connected = True
    activated = False
    try:
        identity = os.pidfd_open(payload)
        try:
            control.sendmsg(
                [b"R " + str(payload).encode("ascii")],
                [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", [identity]))],
            )
        finally:
            os.close(identity)
        while True:
            if stopping:
                _terminate_children()
            # Do not reap a gated payload: its numeric observer identity must stay
            # reserved until admission has finished, even if it is killed early.
            exited, empty = (
                _reap(payload, control, connected=connected)
                if activated or stopping
                else (False, False)
            )
            stopping |= exited
            if empty:
                if connected:
                    _acknowledge_end(control)
                return
            if select.select([control], [], [], 0.01)[0]:
                command = control.recv(2)
                if command == b"G" and not activated and not stopping:
                    os.write(gate, b"G")
                    activated = True
                else:
                    stopping = True
                    connected &= bool(command)
    finally:
        os.close(gate)
        # Control loss or a local OS error must still clean this private subtree.
        # Never publish TreeEND from this fallback or before genuine ECHILD.
        while True:
            _terminate_children()
            _, empty = _reap(payload, control, connected=False)
            if empty:
                break
            time.sleep(0.01)


def main() -> None:
    """Own exactly one invocation and keep control out of the payload's descriptors."""
    control = socket.socket(fileno=int(sys.argv[1]))
    _subreaper()
    gate = os.pipe()
    payload = os.fork()
    if payload == 0:
        _payload(sys.argv[2:], control, gate)
    os.close(gate[0])
    try:
        _run(control, payload, gate[1])
    finally:
        control.close()


if __name__ == "__main__":
    try:
        main()
    except OSError, ValueError:
        # Never inject private supervisor diagnostics into payload output.
        sys.exit(1)
