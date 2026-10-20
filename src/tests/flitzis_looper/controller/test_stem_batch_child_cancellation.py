"""Confirmed batch tokens reach real owned stock-entrypoint cancellation.

The actual controller, two-worker pool, shared job, command scope, process tree
and output readers run here. Existing explicit fixtures substitute Native source,
asset leases and producer inputs, and the stock child uses a non-neural network.
The backend adapter only selects that child fixture; production command supervision
and its finite cancellation grace are unchanged. No neural, RSS, device or musical
acceptance is inferred, nor is the Native file-deletion implementation exercised.
"""

import os
import shutil
import subprocess
import sys
import time
from contextlib import contextmanager
from dataclasses import dataclass, field
from threading import Event
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.analysis import process
from flitzis_looper.controller import stem_process
from flitzis_looper.controller.performance_confirmation import StemBatchConfirmation
from flitzis_looper.controller.stem_job import DIRECT_STEM_INTEREST
from flitzis_looper.controller.stem_workers import StemWorkerPool
from tests.flitzis_looper.analysis.test_owned_command import _record_processes
from tests.flitzis_looper.controller import test_shared_stem_material as shared_fixtures
from tests.flitzis_looper.controller.test_stem_batch_integration import _loaded_metadata
from tests.flitzis_looper.controller.test_stem_child_cancellation import _fixture
from tests.flitzis_looper.controller.test_stem_pending_admission import _production_pool
from tests.flitzis_looper.controller.test_stem_process import _wait_file

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_batch import StemBatchSummary
    from flitzis_looper.controller.stem_generation import (
        StemGenerationRequest,
        StemGenerationResult,
        StemProgressCallback,
    )
    from flitzis_looper.controller.stem_job import StemGenerationJob
    from tests.flitzis_looper.conftest import FakeProjectAssetLease, FakeStemGenerationBackend

# The existing declared producer fixture seals small owned inputs. It does not
# attest the installed release weights, or turn the non-neural child into one.
_declared_shared_producer = shared_fixtures._declared_shared_producer


@dataclass(slots=True)
class _StockEntry:
    variant: str
    output: Path | None = None
    returned: Event = field(default_factory=Event)

    def generate(
        self, request: StemGenerationRequest, _progress: StemProgressCallback
    ) -> StemGenerationResult:
        try:
            request.cache_dir.mkdir(parents=True, exist_ok=True)
            stage = "output_pcm_block" if self.variant == "demucs" else "before_output_write"
            arguments, environment, self.output = _fixture(request.cache_dir, self.variant, stage)
            # Read the same original held by the actual controller job, rather
            # than _fixture's separate input. Partial output and private startup
            # inputs also stay inside the actual job's held directory.
            source_index = -1 if self.variant == "demucs" else arguments.index("--source") + 1
            arguments[source_index] = str(request.source_path)
            stem_process.run_stem_command(arguments, cwd=request.cache_dir, env=environment)
            pytest.fail("the gated stock entrypoint returned without cancellation")
        finally:
            self.returned.set()

    def ready(self) -> bool:
        return self.output is not None and (self.output / "ready").is_file()


@dataclass(slots=True)
class _ReadEnd:
    reached: Event = field(default_factory=Event)
    release: Event = field(default_factory=Event)

    def install(
        self, monkeypatch: pytest.MonkeyPatch, launches: list[process._OwnedTreeProcess]
    ) -> None:
        original = process._CommandReader._read

        def read(reader: process._CommandReader) -> None:
            original(reader)
            if launches and reader.stream is launches[0].stdout:
                # The real pipe reached EOF, but its reader has not returned.
                # Keep the production retirement lane
                # responsible for joining it, including after finite reap.
                self.reached.set()
                assert self.release.wait(15)

        monkeypatch.setattr(process._CommandReader, "_read", read)


def _drive_until(controller: AppController, condition: Callable[[], bool]) -> None:
    deadline = time.monotonic() + 30
    while not condition():
        controller.on_frame_render()
        assert time.monotonic() < deadline, "actual batch/child transition did not settle"
        Event().wait(0.005)
    controller.on_frame_render()


@contextmanager
def _foreign_process(directory: Path) -> Iterator[subprocess.Popen[bytes]]:
    ready, release = directory / "foreign-ready", directory / "foreign-release"
    body = (
        "import time\nfrom pathlib import Path\n"
        f"Path({str(ready)!r}).write_text('ready', encoding='utf-8', newline='\\n')\n"
        f"while not Path({str(release)!r}).exists(): time.sleep(.005)\n"
    )
    with subprocess.Popen(
        [sys.executable, "-I", "-c", body],
        cwd=directory,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    ) as child:
        try:
            _wait_file(ready)
            yield child
        finally:
            # This process is deliberately outside the owned command's job.
            # Let it exit normally; a broad cancellation must not have killed it.
            release.write_text("release", encoding="utf-8", newline="\n")
            child.wait(timeout=10)


@dataclass(slots=True)
class _BatchChild:
    controller: AppController
    audio: Mock
    entry: _StockEntry
    job: StemGenerationJob
    child: process._OwnedTreeProcess
    foreign: subprocess.Popen[bytes]
    read_end: _ReadEnd

    @property
    def summary(self) -> StemBatchSummary:
        summary = self.controller.stem_batch.summary
        assert summary is not None
        return summary

    def assert_custody(self) -> None:
        assert self.job.running
        assert not cast("FakeProjectAssetLease", self.job._source_lease).released
        assert not cast("FakeProjectAssetLease", self.job._generation_lease).released
        assert self.job.request.source_path.is_file()
        assert self.job.request.cache_dir.is_dir()
        assert not any(
            call.args[0] == str(self.job.request.cache_dir)
            for call in self.audio.retire_project_asset.call_args_list
        )
        assert self.foreign.poll() is None

    def wait_for_read_end(self) -> None:
        assert self.read_end.reached.wait(10)
        self.child.wait(timeout=5)
        assert self.child.poll() is not None, "the actual stock child must have exited"
        assert not self.entry.returned.is_set(), "the real output reader still owns the worker"
        assert self.child.stdout is not None
        assert not self.child.stdout.closed
        channels = list(self.job.request.cache_dir.glob(".stem-command-*/cancel"))
        assert len(channels) == 1
        assert channels[0].read_bytes() == b"cancel"
        self.assert_custody()
        self.controller.on_frame_render()

    def settle(self) -> None:
        self.read_end.release.set()
        _drive_until(
            self.controller,
            lambda: (
                not self.job.running
                and self.job.request.cache_dir not in self.controller.stems._jobs
            ),
        )
        assert self.entry.returned.is_set()
        assert self.child.stdout is not None
        assert self.child.stdout.closed
        assert self.child.stderr is not None
        assert self.child.stderr.closed
        assert cast("FakeProjectAssetLease", self.job._source_lease).released
        assert cast("FakeProjectAssetLease", self.job._generation_lease).released
        assert not list(self.job.request.cache_dir.glob(".stem-command-*"))
        self.audio.retire_project_asset.assert_any_call(
            str(self.job.request.cache_dir), recursive=True
        )
        assert self.job.request.cache_dir not in self.controller.stems._jobs
        assert self.job.request.source_path.is_file()
        assert self.foreign.poll() is None


@contextmanager
def _running_batch_child(
    controller: AppController,
    audio: Mock,
    backend: FakeStemGenerationBackend,
    temporary: Path,
    monkeypatch: pytest.MonkeyPatch,
    variant: str,
) -> Iterator[_BatchChild]:
    pool, entry, read_end = StemWorkerPool(), _StockEntry(variant), _ReadEnd()
    launches = _record_processes(monkeypatch)
    read_end.install(monkeypatch, launches)
    _production_pool(controller, audio, temporary, pool)
    _loaded_metadata(controller, audio, (0, 215))
    controller.project.stem_separator = (
        "demucs:htdemucs" if variant == "demucs" else "bs-roformer:musdb18hq"
    )
    monkeypatch.setattr(backend, "generate", entry.generate)
    try:
        with _foreign_process(temporary) as foreign:
            warning = controller.performance_confirmation
            warning.request_all_loaded_stems()
            intent = warning.pending
            assert isinstance(intent, StemBatchConfirmation)
            assert sum(item is not None for item in intent.inventory.items) == 2
            warning.accept(intent)
            _drive_until(controller, lambda: entry.ready() or entry.returned.is_set())
            assert entry.ready(), "the actual stock child failed before its output checkpoint"
            assert len(launches) == len(controller.stems._jobs) == 1
            job = next(iter(controller.stems._jobs.values()))
            assert {row.sample_id for row in job.subscribers()} == {0, 215}
            assert entry.output is not None
            assert list(entry.output.rglob("*.part")), "checkpoint must hold actual partial output"
            rig = _BatchChild(controller, audio, entry, job, launches[0], foreign, read_end)
            assert rig.summary.running == 2
            rig.assert_custody()
            yield rig
    finally:
        read_end.release.set()
        controller.stems.shut_down()
        # The production shutdown is deliberately nonblocking. The test boundary
        # joins this one owned pool after cancellation so no child/reader survives.
        pool._executor.shutdown(wait=True)


@pytest.mark.skipif(
    os.name != "nt" or not shutil.which("ffmpeg"), reason="actual Windows owned tree and FFmpeg"
)
@pytest.mark.parametrize("variant", ["demucs", "bs"])
def test_final_batch_cancel_waits_for_stock_child_exit_and_actual_reader_end(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    variant: str,
) -> None:
    with _running_batch_child(
        controller, audio_engine_mock, stem_backend, tmp_path, monkeypatch, variant
    ) as rig:
        controller.stem_batch.cancel()
        assert rig.job.cancel_event.is_set()
        assert not rig.job.subscribers()
        assert (rig.summary.cancelled, rig.summary.cancelling, rig.summary.active) == (1, 1, True)
        rig.wait_for_read_end()
        assert (rig.summary.cancelled, rig.summary.cancelling, rig.summary.active) == (1, 1, True)
        controller.stem_batch.dismiss_result()
        assert rig.summary.active

        rig.settle()
        terminal = rig.summary
        assert (terminal.cancelled, terminal.cancelling, terminal.active) == (2, 0, False)
        assert terminal.done == terminal.failed == 0
        controller.on_frame_render()
        controller.stem_batch.cancel()
        assert rig.summary == terminal, "terminal original results remain visible"


@pytest.mark.skipif(
    os.name != "nt" or not shutil.which("ffmpeg"), reason="actual Windows owned tree and FFmpeg"
)
@pytest.mark.parametrize("variant", ["demucs", "bs"])
def test_shared_direct_survives_batch_cancel_then_its_last_token_reaps_stock_child(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    variant: str,
) -> None:
    with _running_batch_child(
        controller, audio_engine_mock, stem_backend, tmp_path, monkeypatch, variant
    ) as rig:
        owner = next(row.interest_owner for row in rig.job.subscribers() if row.sample_id == 0)
        assert controller.stems.generate_stems_async(0)
        assert len(owner.current) == 2
        controller.stem_batch.cancel()
        terminal = rig.summary
        assert (terminal.cancelled, terminal.cancelling, terminal.active) == (2, 0, False)
        assert owner.current == frozenset({DIRECT_STEM_INTEREST})
        assert not rig.job.cancel_event.is_set()
        assert not rig.entry.returned.wait(0.1)
        assert rig.child.poll() is None
        assert not list(rig.job.request.cache_dir.glob(".stem-command-*/cancel"))
        rig.assert_custody()

        cancellation = controller.stems.cancel_generation_interest(owner, DIRECT_STEM_INTEREST)
        assert cancellation.disposition == "detached"
        assert cancellation.jobs == (rig.job,)
        assert rig.job.cancel_event.is_set()
        assert controller.stems.generation_cancellation_is_pending(cancellation)
        rig.wait_for_read_end()
        assert controller.stems.generation_cancellation_is_pending(cancellation)
        assert rig.summary == terminal, "later DIRECT reads do not reattach the cancelled batch"

        rig.settle()
        assert not controller.stems.generation_cancellation_is_pending(cancellation)
        assert rig.summary == terminal
