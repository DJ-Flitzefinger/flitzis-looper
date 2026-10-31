"""Original vendored CPU Torch track-arithmetic oracle, without learned inference.

Uses the existing source reader, batching, windows, OLA and PCM16 writer. The
deterministic arithmetic predictor depends on chunk, stem, stereo and local frame;
it is deliberately not a separator, weight load, neural network or CUDA probe.
"""

import argparse
from contextlib import ExitStack
from pathlib import Path

import torch
from flitzis_bs_roformer import inference
from flitzis_bs_roformer.model import SOURCES
from torch import Tensor, nn

from flitzis_looper.pcm16_wav import Pcm16WavWriter

CASES = (1, 1025, 242550, 242551, 242552, 485100, 485101, 485102, 485103, 1455307)
PCM_ATOL = 1e-7
PCM_RTOL = 1e-6


class ArithmeticPredictor(nn.Module):
    """Exercise original orchestration with four distinct non-neural outputs."""

    def __init__(self) -> None:
        super().__init__()
        self.ordinal = 0

    def forward(self, batch: Tensor) -> Tensor:
        """Return complete CPU release-shape predictions, preserving batch order."""
        local = torch.arange(batch.shape[-1], dtype=torch.int64, device="cpu")
        result = torch.empty((len(batch), 4, 2, batch.shape[-1]), dtype=torch.float32)
        for b in range(len(batch)):
            for stem, gain in enumerate((0.25, -0.5, 0.75, 1.25)):
                for channel in range(2):
                    code = (
                        (local * 3 + (self.ordinal + b) * 7 + stem * 11 + channel * 13) % 101
                    ).float()
                    result[b, stem, channel] = batch[b, channel] * gain + (code - 50) * 0.0001
        self.ordinal += len(batch)
        return result


def generate_case(output: Path, frames: int) -> None:
    """Stream synthetic source and original float/PCM16 results to exclusive files."""
    source = output / f"{frames}.pcm.f32"
    with source.open("xb") as handle:
        for start in range(0, frames, 65536):
            t = torch.arange(start, min(frames, start + 65536), dtype=torch.float64)
            left = 0.21 * torch.sin(t * 0.071) + 0.13 * torch.cos(t * 0.019)
            right = 0.17 * torch.cos(t * 0.113) + 0.08 * (t.remainder(2) * 2 - 1)
            pcm = torch.stack((left, right), dim=1).float()
            if start == 0:
                pcm[0] = torch.tensor((0.9, -0.8))
            if start + len(pcm) == frames:
                pcm[-1] = torch.tensor((-0.7, 0.6))
            handle.write(pcm.numpy().astype("<f4", copy=False).tobytes())
    with ExitStack() as stack:
        original = stack.enter_context((output / f"{frames}.output.f32").open("xb"))
        handles = [
            stack.enter_context(
                Pcm16WavWriter(
                    output / f"{frames}.{stem}.wav", sample_rate_hz=44100, channels=2, frames=frames
                )
            )
            for stem in SOURCES
        ]
        source_handle = stack.enter_context(source.open("rb"))
        blocks: list[Tensor] = []
        original_write = inference._write_pcm16

        def capture(handle: Pcm16WavWriter, samples: Tensor, checkpoint: object = None) -> None:
            del checkpoint
            blocks.append(samples)
            original_write(handle, samples)
            if len(blocks) == 4:
                # Match native diagnostic frame-major [time, stem, stereo].
                block = torch.stack(blocks).permute(2, 0, 1).contiguous()
                original.write(block.numpy().astype("<f4", copy=False).tobytes())
                blocks.clear()

        inference._write_pcm16 = capture
        try:
            writer = inference._OverlapAdd(handles, frames, inference.ChunkPlan())
            inference._infer_batches(ArithmeticPredictor(), source_handle, writer, "cpu")
            if blocks:
                msg = "Incomplete original four-source flush"
                raise ValueError(msg)
        finally:
            inference._write_pcm16 = original_write


def generate(output: Path) -> None:
    """Create reproducible fixtures under an existing admitted diagnostic owner."""
    output.mkdir(parents=False, exist_ok=False)
    torch.set_num_threads(1)
    torch.set_num_interop_threads(1)
    for frames in CASES:
        generate_case(output, frames)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    generate(parser.parse_args().output)
