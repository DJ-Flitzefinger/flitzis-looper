"""Allocated owned child for the real headless Native cold-START regression."""

import os
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from threading import Event

from flitzis_looper.controller import stem_process
from flitzis_looper.controller.stem_resources import StemResourceGuard


class AllocatedChild:
    def __init__(self, budget_type: type, directory: str) -> None:
        self.directory = Path(directory)
        self.previous_budget = stem_process.OfflineChildMemoryReservation
        stem_process.OfflineChildMemoryReservation = budget_type
        self.executor = ThreadPoolExecutor(max_workers=1)
        self.future = self.executor.submit(self.run)

    def run(self) -> None:
        ready, release = self.directory / "allocated", self.directory / "release"
        body = (
            "import time\nfrom pathlib import Path\n"
            "payload=bytearray(2048*1024*1024)\n"
            "for index in range(0,len(payload),4096): payload[index]=1\n"
            f"Path({str(ready)!r}).write_text('2 GiB committed and touched')\n"
            f"while not Path({str(release)!r}).exists(): time.sleep(.005)\n"
            "assert payload[-4096] == 1\n"
        )
        with stem_process.stem_command_scope(Event(), resources=StemResourceGuard()):
            result = stem_process.run_stem_command(
                [os.environ.get("PYO3_PYTHON", sys.executable), "-I", "-c", body],
                cwd=self.directory,
                env=dict(os.environ),
            )
        assert result.reason == "ok", result.stderr

    def wait(self) -> None:
        deadline = time.monotonic() + 15
        while not (self.directory / "allocated").exists():
            if self.future.done():
                self.future.result()
            assert time.monotonic() < deadline
            Event().wait(0.005)

    def assert_running(self) -> None:
        assert not self.future.done()

    def finish(self) -> None:
        (self.directory / "release").touch()
        try:
            self.future.result(15)
        finally:
            self.executor.shutdown(wait=True)
            stem_process.OfflineChildMemoryReservation = self.previous_budget


def start(budget_type: type, directory: str) -> AllocatedChild:
    session = AllocatedChild(budget_type, directory)
    try:
        session.wait()
    except AssertionError, OSError, RuntimeError, ValueError:
        session.finish()
        raise
    return session
