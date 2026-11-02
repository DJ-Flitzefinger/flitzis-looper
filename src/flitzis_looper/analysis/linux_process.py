"""Private Linux worker ownership; payload exit is independent of tree retirement."""

import array
import os
import select
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import IO, NoReturn


class LinuxProcess:
    """Keep a private subreaper alive until all of its descendants have been reaped.

    Only the broker forks. The multithreaded application never changes its own
    subreaper state or reaps unrelated children. Lost ownership fails closed.
    """

    def __init__(
        self, arguments: list[str], *, cwd: Path, env: dict[str, str], capture_stderr: bool
    ) -> None:
        if not arguments:
            message = "Linux worker requires a payload command"
            raise ValueError(message)
        self._control, child = socket.socketpair(socket.AF_UNIX, socket.SOCK_SEQPACKET)
        self._payload_pid: int | None = None
        self._identity: int | None = None
        self.returncode: int | None = None
        self._activated = False
        self._stopping = False
        self._ended = False
        self._lost = False
        self._closed = False
        try:
            self._control.settimeout(0.0)
            self._broker = subprocess.Popen(
                [
                    sys.executable,
                    "-I",
                    "-S",
                    str(Path(__file__).with_name("linux_broker.py")),
                    str(child.fileno()),
                    *arguments,
                ],
                pass_fds=(child.fileno(),),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE if capture_stderr else subprocess.DEVNULL,
                cwd=cwd,
                env=env,
            )
        except OSError:
            self._control.close()
            raise
        finally:
            child.close()

    @property
    def broker_pid(self) -> int:
        """Return the private supervisor identity, never the observed payload PID."""
        return self._broker.pid

    @property
    def pid(self) -> int:
        """Return the original payload identity while its private execution gate is held."""
        deadline = time.monotonic() + 5.0
        while self._payload_pid is None:
            self._receive()
            if time.monotonic() >= deadline:
                message = "Linux worker did not publish its gated identity"
                raise OSError(message)
            select.select([self._control], [], [], 0.02)
        if (
            not self._activated
            and not self._stopping
            and self._identity is not None
            and select.select([self._identity], [], [], 0)[0]
        ):
            self._fail()
        return self._payload_pid

    @property
    def stdout(self) -> IO[bytes] | None:
        """Return the original bounded-output pipe, also held until broker exit."""
        return self._broker.stdout

    @property
    def stderr(self) -> IO[bytes] | None:
        """Return the original diagnostic pipe independently of control messages."""
        return self._broker.stderr

    def _fail(self) -> NoReturn:
        self._lost = True
        # EOF asks a still-owning broker to clean up, but supplies no TreeEND proof.
        self._control.close()
        message = "Linux worker ownership was not proved; resources remain held"
        raise OSError(message)

    def _message(self, data: bytes, identities: list[int]) -> None:
        if data.startswith(b"R ") and self._payload_pid is None and len(identities) == 1:
            try:
                pid = int(data[2:])
            except ValueError:
                self._fail()
            if pid <= 0 or self.returncode is not None or self._ended:
                self._fail()
            self._payload_pid = pid
            self._identity = identities.pop()
        elif identities:
            self._fail()
        elif data.startswith(b"X ") and self._payload_pid is not None:
            if self.returncode is not None or self._ended:
                self._fail()
            try:
                value = int(data[2:])
            except ValueError:
                self._fail()
            if not -64 <= value <= 255:
                self._fail()
            self.returncode = value
        elif data == b"E" and self.returncode is not None and not self._ended:
            self._send(b"A")
            self._ended = True
        else:
            self._fail()

    def _packet(self, data: bytes, ancillary: list[tuple[int, int, bytes]], flags: int) -> None:
        identities: list[int] = []
        try:
            for level, kind, value in ancillary:
                if level != socket.SOL_SOCKET or kind != socket.SCM_RIGHTS:
                    self._fail()
                descriptors = array.array("i")
                descriptors.frombytes(value)
                identities.extend(descriptors)
            if flags & ~socket.MSG_CMSG_CLOEXEC or len(data) > 64:
                self._fail()
            self._message(data, identities)
        finally:
            for identity in identities:
                os.close(identity)

    def _receive(self) -> None:
        if self._lost:
            self._fail()
        if self._closed:
            return
        # There are exactly three packets per broker: READY, payload EXIT, TreeEND.
        for _ in range(4):
            try:
                data, ancillary, flags, _ = self._control.recvmsg(
                    65, socket.CMSG_SPACE(array.array("i").itemsize), socket.MSG_CMSG_CLOEXEC
                )
            except BlockingIOError:
                break
            except OSError:
                self._fail()
            if not data and not ancillary and not flags & ~socket.MSG_CMSG_CLOEXEC:
                if not self._ended:
                    self._fail()
                break
            self._packet(data, ancillary, flags)
        status = self._broker.poll()
        if status is not None and (status != 0 or not self._ended):
            self._fail()

    def _send(self, command: bytes) -> None:
        try:
            if self._control.send(command) != len(command):
                self._fail()
        except OSError:
            self._fail()

    def activate(self) -> None:
        """Open the payload gate once, after the caller has bound its observer."""
        _ = self.pid
        if self._activated or self._stopping or self._ended or self._identity is None:
            message = "Linux worker gate is no longer available"
            raise OSError(message)
        if select.select([self._identity], [], [], 0)[0]:
            self._fail()
        self._send(b"G")
        self._activated = True

    def kill(self) -> None:
        """Ask the still-owning broker to reap its complete private subtree."""
        self._receive()
        if self.returncode is None and not self._ended and not self._stopping:
            self._send(b"K")
            self._stopping = True

    def poll(self) -> int | None:
        """Return payload status, never a broker returncode or a tree-end inference."""
        self._receive()
        return self.returncode

    def wait(self, timeout: float | None = None) -> int:
        """Wait for payload exit; also join the broker if TreeEND was already accepted."""
        deadline = None if timeout is None else time.monotonic() + timeout
        while (status := self.poll()) is None:
            if timeout is not None and deadline is not None and time.monotonic() >= deadline:
                command = "Linux worker payload"
                raise subprocess.TimeoutExpired(command, timeout)
            select.select([self._control], [], [], 0.02)
        if self._ended:
            remaining = None if deadline is None else max(0.0, deadline - time.monotonic())
            if self._broker.wait(timeout=remaining) != 0:
                self._fail()
        return status

    def active(self) -> bool:
        """Require private ECHILD acknowledgement AND actual successful broker exit."""
        try:
            self._receive()
        except OSError:
            return True
        return not self._ended or self._broker.poll() != 0

    def close(self) -> None:
        """Release private descriptors only after genuine complete tree retirement."""
        if self.active():
            message = "Linux worker tree has not retired"
            raise OSError(message)
        if not self._closed:
            self._control.close()
            if self._identity is not None:
                os.close(self._identity)
                self._identity = None
            self._closed = True
