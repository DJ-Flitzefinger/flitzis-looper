"""Actual artifact conversion beside the existing headless controller fixture.

The generated separator outputs are numeric fixtures, without neural inference.
Conversion call brackets include bridge overhead; completed blocks and worker CPU
progress establish advancing work, not a per-callback native CPU attribution.
"""

import hashlib
import math
import struct
import time
import wave
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch

from flitzis_looper.controller import stem_artifacts
from flitzis_looper.controller.stem_generation import AudioShape
from flitzis_looper.pcm16_wav import PcmWavReader

if TYPE_CHECKING:
    from concurrent.futures import Future

    from flitzis_looper_audio import StemConversionReservation

type SourceSpan = tuple[int, int, int, int, bytes]
type SourceSpans = tuple[SourceSpan, SourceSpan, SourceSpan, SourceSpan]
type Payloads = tuple[bytes, bytes, bytes, bytes, bytes]


def write_source(path: Path | str, rate: int, seconds: int, value: int | None = None) -> None:
    """Write the complete bounded stereo fixture, with no source cropping."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    block = b"".join(
        struct.pack("<hh", sample, sample)
        for frame in range(rate)
        for sample in [value if value is not None else round(8192 * math.sin(0.5 + frame * 0.037))]
    )
    with wave.open(str(path), "wb") as output:
        output.setnchannels(2)
        output.setsampwidth(2)
        output.setframerate(rate)
        for _ in range(seconds):
            output.writeframesraw(block)


class Conversion:
    """Own one actual guarded converter worker and all of its temporary readers."""

    def __init__(self, directory: str, converter: type[StemConversionReservation]) -> None:
        self.root = Path(directory)
        self.output = self.root / "model"
        self.cache = self.root / "generation"
        self.frames = 60 * 48000
        self.calls: list[tuple[int, int, int, int, int, int]] = []
        self.started_ns = 0
        self.ended_ns = 0
        self.cpu_ns = 0
        self.original = stem_artifacts.StemConversionReservation
        self.converter = converter
        self.executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="conversion-overlap")
        self.closed = False
        self.future: Future[None] | None = None
        for index, name in enumerate(("vocals", "drums", "bass", "other"), 1):
            write_source(self.output / "model" / "source" / f"{name}.wav", 48000, 60, index * 101)

    def start(self) -> None:
        """Begin productive artifact writing without a parked-work synchronization gate."""
        self.future = self.executor.submit(self._work)

    def _work(self) -> None:
        self.started_ns = time.perf_counter_ns()
        cpu = time.thread_time_ns()
        try:
            with patch.object(stem_artifacts, "StemConversionReservation", self._reserve):
                stem_artifacts.write_project_cache_artifacts(
                    output_root=self.output,
                    cache_dir=self.cache,
                    target_shape=AudioShape(48000, 2, self.frames),
                    progress=lambda *_: None,
                )
        finally:
            self.cpu_ns = time.thread_time_ns() - cpu
            self.ended_ns = time.perf_counter_ns()

    def _reserve(self, promised: int, limit: int) -> _Reservation:
        return _Reservation(self, self.converter(promised, limit))

    def snapshot(self) -> tuple[int, int, int, int]:
        """Return completed actual blocks and CPU measured on that worker alone."""
        completed = self.calls.copy()
        return (
            time.perf_counter_ns(),
            len(completed),
            sum(row[3] - row[2] for row in completed),
            sum(row[5] for row in completed),
        )

    def finish(self) -> None:
        """Join the real worker and verify every output frame against exact constants."""
        if self.closed:
            return
        try:
            if self.future is not None:
                self.future.result()
        finally:
            self.executor.shutdown(wait=True)
            self.closed = True
            assert not any(thread.is_alive() for thread in self.executor._threads)
            assert stem_artifacts.StemConversionReservation is self.original
        if self.future is None:
            return
        for name, value in (
            ("vocals", 101),
            ("drums", 202),
            ("bass", 303),
            ("melody", 404),
            ("instrumental", 909),
        ):
            path = self.cache / f"{name}.wav"
            with PcmWavReader(path) as reader:
                assert (reader.getframerate(), reader.getnchannels(), reader.getnframes()) == (
                    48000,
                    2,
                    self.frames,
                )
                remaining = self.frames
                while remaining:
                    count = min(4096, remaining)
                    payload = struct.pack("<hh", value, value) * count
                    assert reader.readframes(count) == payload
                    remaining -= count
                assert reader.readframes(1) == b""
                assert reader.verify() == hashlib.sha256(path.read_bytes()).hexdigest()


class _Reservation:
    """Observe the exact compiled converter, retaining its original admission."""

    def __init__(self, owner: Conversion, native: StemConversionReservation) -> None:
        self.owner = owner
        self.native = native

    def convert(
        self, sources: SourceSpans, target: tuple[int, int, int], start: int, frames: int
    ) -> Payloads:
        """Delegate complete immutable spans while recording actual call progress."""
        began = time.perf_counter_ns()
        cpu = time.thread_time_ns()
        result = self.native.convert(sources, target, start, frames)
        self.owner.calls.append((
            began,
            time.perf_counter_ns(),
            cpu,
            time.thread_time_ns(),
            start,
            frames,
        ))
        return result

    def close(self) -> None:
        """Release the original native reservation after synchronous conversion."""
        self.native.close()
