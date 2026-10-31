"""Real pool/job cancellation with Native readers; asset retirement is recorded only."""

import threading
from pathlib import Path

from flitzis_looper.controller import stem_artifacts
from flitzis_looper.controller.stem_generation import AudioShape, StemGenerationRequest
from flitzis_looper.controller.stem_job import StemGenerationJob
from flitzis_looper.controller.stem_process import (
    StemCommandCancelledError,
    stem_command_scope,
)
from flitzis_looper.controller.stem_workers import StemWorkerPool
from flitzis_looper_audio import SealedProcessingFiles
from tests.flitzis_looper.controller.test_stem_artifacts import _outputs
from tests.flitzis_looper.controller.test_stem_job import _Assets


class Probe:
    def __init__(self, source, directory, converter, install):
        root = Path(directory)
        self.output, self.cache = root / "model", root / "generation"
        _outputs(self.output, frames=501)
        self.cache.mkdir()
        for name in ("vocals", "drums", "bass", "melody", "instrumental"):
            (self.cache / f"{name}.wav").write_bytes(b"previous-generation")
        self.before = {path.name: path.read_bytes() for path in self.cache.iterdir()}
        request = StemGenerationRequest(
            sample_id=0,
            source_path=Path(source.source_path),
            source_version=source.source_version,
            cache_dir=self.cache,
            target_shape=AudioShape(48000, 2, 501),
            model_cache_dir=root,
            original_bytes=source.original_bytes,
        )
        self.assets = _Assets()
        self.job = StemGenerationJob(request, source, self.assets)
        checkpoint = root / "owned-processing.bin"
        checkpoint.write_bytes(b"owned-numeric-fixture")
        self.job.processing_files = SealedProcessingFiles([str(checkpoint)])
        self.pool = StemWorkerPool()
        self.returned, self.dummy_release = threading.Event(), threading.Event()
        self.failure = []
        self.original_converter = stem_artifacts.StemConversionReservation
        stem_artifacts.StemConversionReservation = converter

        def work():
            assert self.job.begin() is not None
            install()
            try:
                with stem_command_scope(self.job.cancel_event):
                    stem_artifacts.write_project_cache_artifacts(
                        output_root=self.output,
                        cache_dir=self.cache,
                        target_shape=request.target_shape,
                        progress=lambda *_: None,
                    )
            except StemCommandCancelledError:
                self.failure.append("cancelled")
            finally:
                assert not self.job.finish_read()
                self.returned.set()

        self.pool(work)
        self.pool(lambda: self.dummy_release.wait(10))
        for _ in range(32):
            self.pool(lambda: None)

    def cancel_while_inside_native(self):
        self.job.cancel()
        assert self.job.running
        assert self.job.source is not None
        self.job.source.verify()
        assert self.job.processing_files is not None
        self.job.processing_files.verify()
        assert not self.assets.leases[0].released
        assert not self.assets.reservations[0].closed
        assert self.pool.try_reserve() is None
        assert not self.returned.is_set()
        actual = {
            path.name: path.read_bytes() for path in self.cache.iterdir() if path.suffix == ".wav"
        }
        assert actual == self.before

    def finish(self):
        try:
            assert self.returned.wait(10)
            assert self.failure == ["cancelled"]
            assert self.job.source is None
            assert self.job.processing_files is None
            assert not self.job.running
            assert self.assets.leases[0].released
            assert self.assets.reservations[0].closed
            assert {p.name: p.read_bytes() for p in self.cache.iterdir()} == self.before
        finally:
            self.dummy_release.set()
            self.pool.shutdown()
            # Test-only join proves the actual executor wrapper has returned
            # its permit after the inner job marker, with no owned worker left.
            self.pool._executor.shutdown(wait=True)
            permits = [self.pool.try_reserve() for _ in range(34)]
            assert all(permit is not None for permit in permits)
            assert self.pool.try_reserve() is None
            for permit in permits:
                permit.close()
            assert not any(thread.is_alive() for thread in self.pool._executor._threads)
            stem_artifacts.StemConversionReservation = self.original_converter


def start(source, directory, converter, install):
    return Probe(source, directory, converter, install)
