#!/usr/bin/env python3
"""Generate librosa CQT reference data for exact parameters (105 bins, fmin=65, bpo=24, sr=44100, hop=8820)."""

import json
from pathlib import Path

import librosa
import numpy as np

SCRIPT_DIR = Path(__file__).parent
ROOT_DIR = SCRIPT_DIR.parent

N_BINS = 105
FMIN = 65
BINS_PER_OCTAVE = 24
SAMPLE_RATE = 44100
HOP_LENGTH = 8820


def main():
    # Load test audio
    mp3_path = (
        ROOT_DIR / "rust" / "crates" / "analysis" / "tests" / "fixtures" / "test_120bpm.mp3"
    )
    y, sr = librosa.load(str(mp3_path), sr=SAMPLE_RATE, mono=True, dtype=np.float64)
    print(f"Audio: {len(y)} samples at {sr} Hz ({len(y) / sr:.2f}s)")

    # Compute CQT
    cqt = librosa.cqt(
        y,
        sr=SAMPLE_RATE,
        hop_length=HOP_LENGTH,
        n_bins=N_BINS,
        bins_per_octave=BINS_PER_OCTAVE,
        fmin=FMIN,
    )
    mag = np.abs(cqt)
    print(f"CQT shape: {cqt.shape} (n_bins={cqt.shape[0]}, n_frames={cqt.shape[1]})")

    # CQT frequencies
    freqs = librosa.cqt_frequencies(
        n_bins=N_BINS, fmin=FMIN, bins_per_octave=BINS_PER_OCTAVE, tuning=0.0
    )
    print(f"Frequencies: {len(freqs)} values")
    print(f"  range: [{freqs.min():.2f}, {freqs.max():.2f}] Hz")

    # Magnitude stats
    print(f"Magnitude: min={mag.min():.6f}, max={mag.max():.6f}, mean={mag.mean():.6f}")

    # Per-bin mean energy
    bin_means = mag.mean(axis=1)
    peak_bin = int(np.argmax(bin_means))
    print(f"Peak energy bin: {peak_bin} ({freqs[peak_bin]:.2f} Hz), mean={bin_means[peak_bin]:.6f}")

    # Sample columns for testing (first, middle, last)
    n_frames = cqt.shape[1]
    sample_cols = [0, n_frames // 2, n_frames - 1]

    # Build reference data
    ref = {
        "sr": SAMPLE_RATE,
        "hop_length": HOP_LENGTH,
        "n_bins": N_BINS,
        "bins_per_octave": BINS_PER_OCTAVE,
        "fmin": FMIN,
        "n_frames": n_frames,
        "frequencies": freqs.tolist(),
        "magnitude_stats": {
            "min": float(mag.min()),
            "max": float(mag.max()),
            "mean": float(mag.mean()),
        },
        "sample_cols": sample_cols,
        "magnitude_samples": [mag[:, col].tolist() for col in sample_cols],
    }

    # Write reference JSON
    out_path = (
        ROOT_DIR / "rust" / "crates" / "analysis" / "tests" / "references" / "cqt_105bins.json"
    )
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(ref, indent=2) + "\n")
    print(f"Wrote reference to {out_path} ({out_path.stat().st_size / 1024:.0f} KB)")


if __name__ == "__main__":
    main()
