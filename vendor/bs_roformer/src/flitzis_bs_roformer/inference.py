"""Bounded adaptation of the release's reflection, batching and overlap-add."""

import ctypes
import sys
import wave
from contextlib import ExitStack
from dataclasses import dataclass
from typing import TYPE_CHECKING, BinaryIO

import torch
from torch import Tensor, nn
from torch.nn import functional

from flitzis_bs_roformer.audio import read_virtual_chunk
from flitzis_bs_roformer.model import (
    BATCH_SIZE,
    CHUNK_SIZE,
    MAX_TRANSIENT_PCM_BYTES,
    OVERLAP,
    SAMPLE_RATE,
    SOURCES,
)

if TYPE_CHECKING:
    from pathlib import Path


@dataclass(frozen=True)
class ChunkPlan:
    """Hold the bounded overlap-add shape; production uses the release defaults."""

    chunk_size: int = CHUNK_SIZE
    batch_size: int = BATCH_SIZE
    overlap: int = OVERLAP

    @property
    def step(self) -> int:
        """Return the number of source frames advanced per chunk."""
        return self.chunk_size // self.overlap

    @property
    def transient_pcm_bytes(self) -> int:
        """Conservatively count simultaneous PCM copies, excluding model activations.

        This overcounts source read/copy, stacked CPU/GPU inputs, CPU/GPU
        predictions and weighted predictions, rolling accumulators/counters,
        windows, and PCM16 conversion/byte copies. It is not process RSS, decoder
        internals, model weights, neural activations or an allocator-peak claim.
        """
        chunk = self.chunk_size
        batch = self.batch_size
        ring = chunk + (batch - 1) * self.step
        return (
            batch * 2 * chunk * 4 * 4
            + batch * len(SOURCES) * 2 * chunk * 4 * 3
            + (len(SOURCES) * 2 + 1) * ring * 4
            + 2 * chunk * 4 * 3
            + len(SOURCES) * 2 * chunk * 2 * 2
            + chunk * 4 * 3
        )

    def validate(self) -> None:
        """Reject unsupported shapes or PCM allocation budgets before decoding."""
        if self.chunk_size < 20 or self.overlap != 2 or not 1 <= self.batch_size <= BATCH_SIZE:
            msg = "Unsupported BS-RoFormer chunk plan"
            raise ValueError(msg)
        if self.transient_pcm_bytes > MAX_TRANSIENT_PCM_BYTES:
            msg = "BS-RoFormer transient PCM exceeds 1 GiB"
            raise ValueError(msg)


def _chunk_window(plan: ChunkPlan, start: int, total_frames: int) -> Tensor:
    fade = plan.chunk_size // 10
    window = torch.ones(plan.chunk_size, dtype=torch.float32)
    if start != 0:
        window[:fade] = torch.linspace(0, 1, fade)
    if start + plan.step < total_frames:
        window[-fade:] = torch.linspace(1, 0, fade)
    return window


def _read_batch(
    handle: BinaryIO,
    position: int,
    total_frames: int,
    source_frames: int,
    border: int,
    plan: ChunkPlan,
) -> tuple[Tensor, list[tuple[int, int]], int]:
    parts: list[Tensor] = []
    locations: list[tuple[int, int]] = []
    while position < total_frames and len(parts) < plan.batch_size:
        length = min(plan.chunk_size, total_frames - position)
        part = read_virtual_chunk(handle, position, length, source_frames, border)
        if length < plan.chunk_size:
            mode = "reflect" if length > plan.chunk_size // 2 + 1 else "constant"
            part = functional.pad(part, (0, plan.chunk_size - length), mode=mode)
        parts.append(part)
        locations.append((position, length))
        position += plan.step
    return torch.stack(parts), locations, position


def _write_pcm16(handle: wave.Wave_write, samples: Tensor) -> None:
    if not bool(torch.isfinite(samples).all()):
        msg = "BS-RoFormer produced non-finite PCM"
        raise ValueError(msg)
    interleaved = samples.transpose(0, 1).contiguous().clamp(-1, 1)
    integers = (interleaved * 32767).round().to(torch.int16)
    integers[interleaved <= -1] = -32768
    handle.writeframesraw(ctypes.string_at(integers.data_ptr(), integers.numel() * 2))


def _flush(
    handles: list[wave.Wave_write],
    result: Tensor,
    counter: Tensor,
    base: int,
    frames: int,
    border: int,
    source_frames: int,
) -> None:
    begin = max(0, border - base)
    end = min(frames, border + source_frames - base)
    if end <= begin:
        return
    denominator = counter[begin:end]
    if not bool((denominator > 0).all()):
        msg = "BS-RoFormer overlap-add left uncovered PCM"
        raise ValueError(msg)
    for index, handle in enumerate(handles):
        _write_pcm16(handle, result[index, :, begin:end] / denominator)


class _OverlapAdd:
    def __init__(
        self,
        handles: list[wave.Wave_write],
        source_frames: int,
        plan: ChunkPlan,
    ) -> None:
        self.handles = handles
        self.source_frames = source_frames
        self.plan = plan
        self.border = (
            plan.chunk_size - plan.step if source_frames > 2 * (plan.chunk_size - plan.step) else 0
        )
        self.total_frames = source_frames + 2 * self.border
        ring_frames = plan.chunk_size + (plan.batch_size - 1) * plan.step
        self.result = torch.zeros((len(SOURCES), 2, ring_frames), dtype=torch.float32)
        self.counter = torch.zeros(ring_frames, dtype=torch.float32)
        self.base = 0

    def add(self, prediction: Tensor, locations: list[tuple[int, int]]) -> None:
        expected_shape = (len(locations), len(SOURCES), 2, self.plan.chunk_size)
        if tuple(prediction.shape) != expected_shape:
            msg = "BS-RoFormer returned an incomplete four-source batch"
            raise ValueError(msg)
        prediction_cpu = prediction.to(device="cpu", dtype=torch.float32)
        if not bool(torch.isfinite(prediction_cpu).all()):
            msg = "BS-RoFormer produced non-finite PCM"
            raise ValueError(msg)
        for index, (start, length) in enumerate(locations):
            offset = start - self.base
            window = _chunk_window(self.plan, start, self.total_frames)[:length]
            self.result[..., offset : offset + length] += (
                prediction_cpu[index, ..., :length] * window
            )
            self.counter[offset : offset + length] += window

    def flush_after(self, position: int) -> None:
        flushed = min(position - self.base, self.total_frames - self.base)
        _flush(
            self.handles,
            self.result,
            self.counter,
            self.base,
            flushed,
            self.border,
            self.source_frames,
        )
        remaining = self.counter.numel() - flushed
        if remaining > 0:
            self.result[..., :remaining] = self.result[..., flushed:].clone()
            self.counter[:remaining] = self.counter[flushed:].clone()
        self.result[..., remaining:] = 0
        self.counter[remaining:] = 0
        self.base += flushed


def _infer_batches(model: nn.Module, source: BinaryIO, writer: _OverlapAdd, device: str) -> None:
    position = 0
    with torch.inference_mode(), torch.autocast("cuda", enabled=device == "cuda"):
        while position < writer.total_frames:
            batch, locations, position = _read_batch(
                source,
                position,
                writer.total_frames,
                writer.source_frames,
                writer.border,
                writer.plan,
            )
            prediction = model(batch.to(device))
            writer.add(prediction, locations)
            del prediction, batch
            writer.flush_after(position)


def _publish_outputs(temporary: list[Path], output: Path, source_frames: int) -> None:
    for path in temporary:
        with wave.open(str(path), "rb") as handle:
            shape = (
                handle.getnframes(),
                handle.getframerate(),
                handle.getnchannels(),
                handle.getsampwidth(),
            )
            if shape != (source_frames, SAMPLE_RATE, 2, 2):
                msg = "BS-RoFormer output shape is incomplete"
                raise ValueError(msg)
    for path, name in zip(temporary, SOURCES, strict=True):
        path.replace(output / f"{name}.wav")


def separate_pcm(
    model: nn.Module,
    decoded: Path,
    output: Path,
    source_frames: int,
    device: str,
    plan: ChunkPlan | None = None,
) -> None:
    """Write four complete PCM16 WAVs using a fixed-size CPU overlap-add ring.

    CUDA holds at most one inference batch. Finalized source regions leave the
    ring immediately. Reflection padding, tail padding and linear fades follow
    the upstream algorithm, with independent first/last windows per chunk.
    """
    actual_plan = plan if plan is not None else ChunkPlan()
    actual_plan.validate()
    if source_frames <= 0 or device not in {"cpu", "cuda"} or sys.byteorder != "little":
        msg = "Invalid BS-RoFormer inference input"
        raise ValueError(msg)
    output.mkdir(parents=True, exist_ok=True)
    temporary = [output / f".{source}.wav.part" for source in SOURCES]
    try:
        with ExitStack() as stack:
            handles = [stack.enter_context(wave.open(str(path), "wb")) for path in temporary]
            for handle in handles:
                handle.setnchannels(2)
                handle.setsampwidth(2)
                handle.setframerate(SAMPLE_RATE)
            source = stack.enter_context(decoded.open("rb"))
            writer = _OverlapAdd(handles, source_frames, actual_plan)
            _infer_batches(model, source, writer, device)
        _publish_outputs(temporary, output, source_frames)
    finally:
        for path in temporary:
            path.unlink(missing_ok=True)
