"""Original CPU Torch arithmetic oracle for the isolated BS spectral core.

No model, weights, network inference or CUDA is initialized. Run under the existing
admitted offline diagnostic owner; outputs are synthetic reconstructible fixtures,
not application provenance or whole-model quality acceptance.
"""

import argparse
from pathlib import Path

import torch
from torch import Tensor

CASES = (("minimum", 1025), ("silence", 1323), ("fft", 2048), ("odd", 4099), ("release", 485100))
SPECTRUM_ATOL = 2e-4
SPECTRUM_RTOL = 2e-5
PCM_ATOL = 2e-5
PCM_RTOL = 2e-5


def reference(name: str, frames: int) -> tuple[Tensor, Tensor, Tensor, Tensor]:
    """Return planar PCM, nonidentity masks, original features and reconstruction."""
    if not 1025 <= frames <= 485100:
        msg = "Unsupported complete BS spectral fixture"
        raise ValueError(msg)
    t = torch.arange(frames, dtype=torch.float64, device="cpu")
    pcm = torch.stack([
        0.21 * torch.sin(t * 0.071) + 0.13 * torch.cos(t * 0.019),
        0.17 * torch.cos(t * 0.113) + 0.08 * (t.remainder(2) * 2 - 1),
    ]).float()
    pcm[:, 0] = torch.tensor([0.9, -0.8], dtype=torch.float32, device="cpu")
    pcm[:, -1] = torch.tensor([-0.7, 0.6], dtype=torch.float32, device="cpu")
    if name == "silence":
        pcm.zero_()
    window = torch.hann_window(2048, periodic=True, dtype=torch.float32, device="cpu")
    spectral = torch.stft(
        pcm,
        n_fft=2048,
        hop_length=441,
        win_length=2048,
        window=window,
        center=True,
        pad_mode="reflect",
        normalized=False,
        onesided=True,
        return_complex=True,
    )
    # Original b,s,f,t,c -> b,(f,s),t,c -> b,t,(f,s,c), for one batch.
    features = torch.view_as_real(spectral).permute(2, 1, 0, 3).contiguous()
    time = torch.arange(features.shape[0], device="cpu").reshape(-1, 1, 1)
    frequency = torch.arange(1025, device="cpu").reshape(1, -1, 1)
    channel = torch.arange(2, device="cpu").reshape(1, 1, -1)
    masks = torch.stack([
        torch.stack(
            [
                ((time + frequency * 3 + channel * 5 + stem * 7) % 17).float() * 0.025 + 0.1,
                ((time * 3 + frequency * 2 + channel * 7 + stem * 5) % 13 - 6).float() * 0.03,
            ],
            dim=-1,
        )
        for stem in range(4)
    ])
    # Invert the release model's f,s merge, retaining nonzero imaginary DC and
    # Nyquist masks so original real-irfft behavior is exercised, too.
    complex_masks = torch.view_as_complex(masks).permute(0, 3, 2, 1)
    masked = spectral.unsqueeze(0) * complex_masks
    output = torch.istft(
        masked.reshape(8, 1025, -1),
        n_fft=2048,
        hop_length=441,
        win_length=2048,
        window=window,
        center=True,
        normalized=False,
        onesided=True,
        length=frames,
        return_complex=False,
    ).reshape(4, 2, frames)
    return pcm, masks, features, output


def generate(output: Path) -> None:
    """Create complete exclusive synthetic float32 fixtures without model work."""
    output.mkdir(parents=False, exist_ok=False)
    torch.set_num_threads(1)
    torch.set_num_interop_threads(1)
    for name, frames in CASES:
        for suffix, value in zip(
            ("pcm", "masks", "features", "output"), reference(name, frames), strict=True
        ):
            with (output / f"{name}.{suffix}.f32").open("xb") as handle:
                handle.write(value.contiguous().numpy().astype("<f4", copy=False).tobytes())


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    generate(parser.parse_args().output)
