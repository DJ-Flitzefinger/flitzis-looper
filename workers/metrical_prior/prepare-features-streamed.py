#!/usr/bin/env python3
"""Artifact-only, byte-parity-gated streaming pilot for the pinned TempoCNN frontend.

The E4 reference runner and all previous features/distributions remain immutable.
Adapted author frontend: TempoCNN commit482c64ad, AGPL-3.0; see upstream/code-LICENSE.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import sys
import time


HERE = Path(__file__).resolve().parent
REFERENCE_SHA256 = "e4f8ba95983a74624692a044c18301493443a08af1a04833b589c7a1959022ba"
LIMIT = 512 * 1024 * 1024
OPAQUE_RESERVE = 64 * 1024 * 1024
IO_RESERVE = 2 * 1024 * 1024
STFT_SCRATCH = 2 * 1024 * 1024
SOURCE_CHUNK_FRAMES = 4096
STFT_CHUNK_FRAMES = 64


def _reference_helpers():
    path = HERE / "prepare-features.py"
    if hashlib.sha256(path.read_bytes()).hexdigest() != REFERENCE_SHA256:
        raise ValueError("The immutable E4 reference runner hash differs")
    spec = importlib.util.spec_from_file_location("immutable_e4_frontend", path)
    if spec is None or spec.loader is None:
        raise ValueError("Cannot load the pinned E4 helpers")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Budget:
    """Combined declared parent charge and observed child memory, without an OS cap."""

    def __init__(self, helper, held_bytes: int):
        if not 0 <= held_bytes <= LIMIT:
            raise ValueError("Held Native/QP/bridge bytes must be in0..536870912")
        self.helper = helper
        self.held_bytes = held_bytes
        self.observations = {}
        self.baseline = 0

    def start(self) -> None:
        self.observe("startup")
        self.baseline = self.observations["startup"]["max_current_bytes"]

    def observe(self, stage: str) -> None:
        actual = self.helper._memory_snapshot()
        row = self.observations.setdefault(stage, {
            "checks": 0, "max_current_bytes": 0, "max_peak_bytes": 0,
            "measurement": actual["measurement"],
        })
        row["checks"] += 1
        row["max_current_bytes"] = max(row["max_current_bytes"], actual["current_bytes"])
        row["max_peak_bytes"] = max(row["max_peak_bytes"], actual["peak_bytes"])
        row["combined_peak_bytes"] = self.held_bytes + row["max_peak_bytes"]
        if row["combined_peak_bytes"] > LIMIT:
            raise MemoryError(f"Held parent plus observed child peak exceeds512MiB at {stage}")

    def admit(self, source_frames: int, source_rate: int) -> dict:
        e4 = self.helper
        target = (source_frames * e4.RATE + source_rate - 1) // source_rate
        columns = 1 + target // e4.SPECTRAL_HOP
        padded = max(e4.PATCH, columns)
        patches = 1 + (padded - e4.PATCH) // e4.PATCH_HOP
        elements = patches * e4.MELS * e4.PATCH
        pcm_bytes = target * 4
        magnitude_bytes = (e4.FFT // 2 + 1) * columns * 4
        mel_bytes = e4.MELS * columns * 4
        filter_bytes = e4.MELS * (e4.FFT // 2 + 1) * 4
        # A rate-dependent output allowance also prevents pathological upsampling
        # from escaping admission. The extra65536 samples cover bounded flush output.
        output_chunk_frames = (SOURCE_CHUNK_FRAMES * e4.RATE + source_rate - 1) // source_rate + 65536
        phases = {
            "stream_resample": SOURCE_CHUNK_FRAMES * 4 + output_chunk_frames * 4,
            # Charge every page of BOTH simultaneously mapped full files.
            "global_lattice_stft": pcm_bytes + magnitude_bytes + STFT_SCRATCH,
            # Whole einsum may make one complete magnitude BLAS copy. Include
            # output and a second output-sized layout allocation conservatively.
            "whole_mel": 2 * magnitude_bytes + 2 * mel_bytes + filter_bytes,
            "patch": mel_bytes + e4.MELS * padded * 4 + elements * 4,
            "global_normalize": elements * (4 + 8 * 4 + 2 + 4),
            "feature_serialization": elements * 4,
        }
        child = self.baseline + max(phases.values()) + OPAQUE_RESERVE + IO_RESERVE
        combined = self.held_bytes + child
        ledger = {
            "limit_bytes": LIMIT, "held_native_qp_bridge_bytes": self.held_bytes,
            "runtime_baseline_bytes": self.baseline, "managed_phase_bytes": phases,
            "opaque_workspace_reserve_bytes": OPAQUE_RESERVE, "io_reserve_bytes": IO_RESERVE,
            "stft_scratch_bytes": STFT_SCRATCH, "admitted_child_peak_bytes": child,
            "admitted_combined_peak_bytes": combined, "analyzer_frames": target,
            "mel_frames": columns, "magnitude_shape": [e4.FFT // 2 + 1, columns],
            "feature_shape": [patches, e4.MELS, e4.PATCH, 1],
            "maximum_resampler_output_chunk_frames": output_chunk_frames,
            "scope": "Complete mapped extents plus phase maximum; explicit unmaps precede the next phase. Admission and observed peaks are not an OS hard cap.",
        }
        self.observe("preflight-before-audio-allocation")
        if combined > LIMIT:
            raise MemoryError(f"Held parent plus admitted complete frontend exceeds512MiB: {combined}")
        return ledger

    def receipt(self) -> dict:
        return {
            "held_native_qp_bridge_bytes": self.held_bytes,
            "aggregate_scope": "declared-held-parent-plus-child" if self.held_bytes else "child-only equivalence; no aggregate Native coexistence claim",
            "observations": self.observations,
        }


def _sync_file(path: Path) -> None:
    with path.open("r+b") as stream:
        stream.flush()
        os.fsync(stream.fileno())


def _write_json(path: Path, value) -> None:
    with path.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2, allow_nan=False)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())


def _write_array(path: Path, array, e4) -> dict:
    data = memoryview(array).cast("B")
    with path.open("xb") as stream:
        for offset in range(0, len(data), e4.IO_CHUNK):
            stream.write(data[offset : offset + e4.IO_CHUNK])
        stream.flush()
        os.fsync(stream.fileno())
    return {"path": path.name, "dtype": "float32-le", "shape": list(array.shape),
            "bytes": array.nbytes, "sha256": e4._sha256_file(path), "synced": True}


def _create_extent(path: Path, size: int) -> None:
    with path.open("xb") as stream:
        stream.truncate(size)
        stream.flush()
        os.fsync(stream.fileno())


def _stream_resample(source: Path, value: dict, output: Path, ledger: dict, rt, e4, budget: Budget) -> dict:
    expected = ledger["analyzer_frames"]
    expected_bytes = value["source_frame_count"] * 4
    if source.stat().st_size != expected_bytes or e4._sha256_file(source) != value["mono_f32le_sha256"]:
        raise ValueError("Complete source PCM size/pre-hash differs")
    engine = None
    if value["source_sample_rate_hz"] != e4.RATE:
        soxr = sys.modules["soxr"]
        engine = soxr.ResampleStream(value["source_sample_rate_hz"], e4.RATE, 1, dtype="float32", quality="HQ")
    digest = hashlib.sha256()
    consumed = 0
    produced = 0
    flushes = 0
    max_delay = 0.0
    try:
        with source.open("rb") as incoming, output.open("x+b") as outgoing:
            while consumed < value["source_frame_count"]:
                wanted = min(SOURCE_CHUNK_FRAMES, value["source_frame_count"] - consumed)
                block = incoming.read(wanted * 4)
                if len(block) != wanted * 4:
                    raise ValueError("Source PCM ended before its complete declared extent")
                digest.update(block)
                samples = rt.np.frombuffer(block, dtype="<f4")
                if not rt.np.isfinite(samples).all():
                    raise ValueError("Source PCM contains nonfinite samples")
                consumed += wanted
                budget.observe("resample-input-chunk-before-use")
                if engine is None:
                    converted = samples
                else:
                    last = consumed == value["source_frame_count"]
                    converted = engine.resample_chunk(samples, last=last)
                    flushes += int(last)
                    delay = float(engine.delay())
                    if not math.isfinite(delay) or delay < 0:
                        raise ValueError("Invalid streaming resampler delay")
                    max_delay = max(max_delay, delay)
                budget.observe("resample-output-chunk-before-use")
                if (converted.ndim != 1 or converted.dtype != rt.np.dtype("<f4")
                        or len(converted) > ledger["maximum_resampler_output_chunk_frames"]
                        or not rt.np.isfinite(converted).all()):
                    raise ValueError("Streaming resampler exceeded its finite f32 output geometry")
                outgoing.write(memoryview(converted).cast("B"))
                produced += len(converted)
                del converted, samples, block
            if incoming.read(1) or digest.hexdigest() != value["mono_f32le_sha256"]:
                raise ValueError("Source PCM EOF/streamed hash differs")
            if engine is not None and flushes != 1:
                raise ValueError("The continuous resampler must flush exactly once")
            if abs(produced - expected) > 1:
                raise ValueError("Streaming resampler extent differs from its complete rounded output")
            # librosa fix=True: right-trim or right-zero-pad to the exact ceil.
            if produced < expected:
                outgoing.write(b"\0" * ((expected - produced) * 4))
            outgoing.truncate(expected * 4)
            outgoing.flush()
            os.fsync(outgoing.fileno())
    finally:
        if engine is not None:
            engine.clear()
        del engine
    if source.stat().st_size != expected_bytes or e4._sha256_file(source) != value["mono_f32le_sha256"]:
        raise ValueError("Source PCM size/post-hash differs")
    budget.observe("resampler-closed-complete-pcm-synced")
    return {"path": output.name, "sample_rate_hz": e4.RATE, "frame_count": expected,
            "origin_seconds": 0, "duration_seconds": expected / e4.RATE,
            "f32le_sha256": e4._sha256_file(output), "bytes": output.stat().st_size,
            "implementation": "identity-copy" if value["source_sample_rate_hz"] == e4.RATE else "soxr.ResampleStream/HQ/float32",
            "source_chunk_frames": SOURCE_CHUNK_FRAMES, "source_frames_consumed": consumed,
            "raw_output_frames": produced, "right_length_correction_frames": expected - produced,
            "final_flush_count": flushes, "maximum_pending_output_samples": max_delay,
            "input_pre_stream_post_hash_equal": True, "source_eof_verified": True,
            "state_closed": True, "synced": True}


def _global_stft(pcm_path: Path, magnitude_path: Path, ledger: dict, rt, e4, budget: Budget) -> dict:
    bins, columns = ledger["magnitude_shape"]
    frames = ledger["analyzer_frames"]
    _create_extent(magnitude_path, bins * columns * 4)
    pcm = rt.np.memmap(pcm_path, mode="r", dtype="<f4", shape=(frames,))
    magnitude = None
    try:
        magnitude = rt.np.memmap(magnitude_path, mode="r+", dtype="<f4", shape=(bins, columns), order="F")
        budget.observe("both-complete-stft-maps-open")
        for start in range(0, columns, STFT_CHUNK_FRAMES):
            count = min(STFT_CHUNK_FRAMES, columns - start)
            left = start * e4.SPECTRAL_HOP - e4.FFT // 2
            length = e4.FFT + (count - 1) * e4.SPECTRAL_HOP
            samples = rt.np.zeros(length, dtype=rt.np.float32)
            first = max(0, left)
            end = min(frames, left + length)
            if end > first:
                samples[first - left : end - left] = pcm[first:end]
            budget.observe("global-lattice-input-before-stft")
            spectrum = rt.librosa.stft(samples, n_fft=e4.FFT, hop_length=e4.SPECTRAL_HOP,
                                       win_length=e4.FFT, window="hann", center=False, dtype=rt.np.complex64)
            block = rt.np.abs(spectrum) ** 1
            budget.observe("bounded-stft-output-before-use")
            if (spectrum.shape != (bins, count) or spectrum.dtype != rt.np.dtype("complex64")
                    or block.dtype != rt.np.dtype("<f4") or not rt.np.isfinite(block).all()):
                raise ValueError("Bounded STFT differs from the global finite magnitude geometry")
            magnitude[:, start : start + count] = block
            del block, spectrum, samples
        magnitude.flush()
    finally:
        if magnitude is not None:
            magnitude._mmap.close()
        pcm._mmap.close()
    del magnitude, pcm
    _sync_file(magnitude_path)
    budget.observe("pcm-and-writable-magnitude-unmapped")
    return {"path": magnitude_path.name, "shape": [bins, columns], "dtype": "float32-le", "order": "F",
            "bytes": magnitude_path.stat().st_size, "sha256": e4._sha256_file(magnitude_path),
            "global_frame_centers": "j*512", "input_start": "j*512-512",
            "maximum_block_frames": STFT_CHUNK_FRAMES, "end_padding": "only outside true complete source bounds",
            "pcm_map_closed_before_whole_mel": True, "writable_map_closed": True, "synced": True}


def _whole_mel(path: Path, ledger: dict, rt, e4, budget: Budget):
    magnitude = rt.np.memmap(path, mode="r", dtype="<f4", shape=tuple(ledger["magnitude_shape"]), order="F")
    try:
        basis = rt.librosa.filters.mel(sr=e4.RATE, n_fft=e4.FFT, n_mels=e4.MELS, fmin=20, fmax=5000,
                                      htk=False, norm="slaney", dtype=rt.np.float32)
        budget.observe("whole-mel-before-einsum")
        mel = rt.np.einsum("...ft,mf->...mt", magnitude, basis, optimize=True)
        budget.observe("whole-mel-output-before-use")
        if (mel.shape != (e4.MELS, ledger["mel_frames"]) or mel.dtype != rt.np.dtype("<f4")
                or rt.np.shares_memory(mel, magnitude) or not rt.np.isfinite(mel).all()):
            raise ValueError("Whole mel transform returned unexpected finite ownership/geometry")
        del basis
    finally:
        magnitude._mmap.close()
    del magnitude
    budget.observe("magnitude-unmapped-before-patches")
    return mel


def _patches_from_mel(data, rt, e4):
    original = data.shape[1]
    if original < e4.PATCH:
        padded = rt.np.zeros((e4.MELS, e4.PATCH), dtype=data.dtype)
        padded[:, :original] = data
        data = padded
    count = 1 + (data.shape[1] - e4.PATCH) // e4.PATCH_HOP
    patches = rt.np.empty((count, e4.MELS, e4.PATCH, 1), dtype=data.dtype)
    for index in range(count):
        start = index * e4.PATCH_HOP
        patches[index, :, :, 0] = data[:, start : start + e4.PATCH]
    return patches, {"mel_frame_count": original, "short_padding_frames": max(0, e4.PATCH - original),
                     "unpatched_tail_frames": data.shape[1] - ((count - 1) * e4.PATCH_HOP + e4.PATCH),
                     "patch_starts_mel_frames": [index * e4.PATCH_HOP for index in range(count)]}


def _compare_reference(path: Path, result: dict, e4) -> dict:
    reference = e4._read_json(path)
    if (not isinstance(reference, dict) or reference.get("schema_version") != 1
            or not isinstance(reference.get("pins"), dict)
            or reference["pins"].get("runner_sha256") != REFERENCE_SHA256):
        raise ValueError("Comparison receipt is not from the immutable E4 runner")
    previous = reference.get("result", {})
    if not isinstance(previous, dict):
        raise ValueError("Missing E4 result object")
    for key in ("source_sha256", "standardized_source_pcm", "source_extent", "input_manifest_sha256", "geometry", "global_normalization"):
        if previous.get(key) != result[key]:
            raise ValueError(f"Exact E4 reference provenance/math differs: {key}")
    old_pcm = previous.get("resampled_pcm", {})
    if not isinstance(old_pcm, dict):
        raise ValueError("Missing E4 complete11025PCM receipt")
    for key in ("sample_rate_hz", "frame_count", "origin_seconds", "f32le_sha256"):
        if old_pcm.get(key) != result["resampled_pcm"][key]:
            raise ValueError(f"Exact E4 complete11025PCM differs: {key}")
    old_features = previous.get("features", {})
    if not isinstance(old_features, dict):
        raise ValueError("Missing E4 feature-file receipt")
    for key in ("dtype", "shape", "bytes", "sha256"):
        if old_features.get(key) != result["features"][key]:
            raise ValueError(f"Exact E4 normalized feature bytes differ: {key}")
    old_file = (path.parent / old_features["path"]).resolve(strict=True)
    if old_file.stat().st_size != old_features["bytes"] or e4._sha256_file(old_file) != old_features["sha256"]:
        raise ValueError("The immutable E4 reference feature file no longer matches its receipt")
    return {"receipt_path": str(path), "receipt_sha256": e4._sha256_file(path),
            "reference_runner_sha256": REFERENCE_SHA256, "complete11025_pcm_hash_exact": True,
            "normalized_feature_file_hash_exact": True, "normalization_and_provenance_exact": True,
            "model_inference_run": False, "cdf_reuse_authorized_by_this_runner": False}


def _prepare(manifest_path: Path, output: Path, reference_path: Path | None, rt, e4, budget: Budget) -> dict:
    manifest_path = manifest_path.resolve(strict=True)
    value, pcm, custody = e4._manifest(manifest_path)
    ledger = budget.admit(value["source_frame_count"], value["source_sample_rate_hz"])
    analyzer = _stream_resample(pcm, value, output / "complete11025.f32le", ledger, rt, e4, budget)
    magnitude = _global_stft(output / analyzer["path"], output / "magnitude.f32le", ledger, rt, e4, budget)
    mel = _whole_mel(output / magnitude["path"], ledger, rt, e4, budget)
    patches, geometry = _patches_from_mel(mel, rt, e4)
    del mel
    budget.observe("patches-before-global-normalization")
    features, statistics = e4._normalize(patches, rt)
    del patches
    budget.observe("global-normalization-output-before-use")
    if list(features.shape) != ledger["feature_shape"]:
        raise ValueError("Complete feature shape differs from the admitted geometry")
    feature_file = _write_array(output / "features.f32le", features, e4)
    del features
    budget.observe("feature-array-released-and-file-synced")
    result = {
        "mode": "pinned_complete_mono_pcm_streamed", "input_manifest": value,
        "source_sha256": value["source_sha256"],
        "standardized_source_pcm": {"format": "mono-f32le", "sha256": value["mono_f32le_sha256"],
                                    "sample_rate_hz": value["source_sample_rate_hz"], "frame_count": value["source_frame_count"],
                                    "origin_seconds": 0, "downmix_revision": value["mono_downmix_revision"]},
        "input_manifest_sha256": e4._sha256_file(manifest_path),
        "source_custody_receipt": {"path": str(custody), "sha256": value["source_receipt_sha256"],
                                   "meaning": "Opaque acquired custody link; no new decoding, source/phase/ACK authority."},
        "source_extent": {"sample_rate_hz": value["source_sample_rate_hz"], "frame_count": value["source_frame_count"],
                          "channels_before_mono": value["source_channels"], "origin_seconds": 0,
                          "duration_seconds": value["source_frame_count"] / value["source_sample_rate_hz"]},
        "resampled_pcm": analyzer, "magnitude": magnitude, "geometry": geometry,
        "global_normalization": statistics, "features": feature_file, "memory_ledger": ledger,
    }
    if reference_path is not None:
        result["e4_reference_comparison"] = _compare_reference(reference_path.resolve(strict=True), result, e4)
    return result


def _smoke(output: Path, rt, e4, budget: Budget) -> dict:
    author_features, author_normalize, normalizer_hash = e4._author_functions(rt)
    cases = [("short", 11025, 1025), ("tail", 11025, 512 * 401 + 137),
             ("resampled_tail", 48000, 48000 * 19 + 113), ("block64_boundary", 11025, 512 * 64)]
    results = []
    for name, rate, frames in cases:
        budget.admit(frames, rate)
        owned = output / name
        owned.mkdir()
        t = rt.np.arange(frames, dtype=rt.np.float64) / rate
        pcm = ((0.2 + 0.1 * rt.np.sin(2 * rt.np.pi * 0.17 * t)) * rt.np.sin(2 * rt.np.pi * 317 * t)
               + 0.05 * rt.np.cos(2 * rt.np.pi * 911 * t)).astype("<f4")
        pcm[-1] += rt.np.float32(0.125)
        mono = _write_array(owned / "input.f32le", pcm, e4)
        wav = owned / "source.wav"
        with wav.open("xb") as stream:
            rt.sf.write(stream, pcm, rate, format="WAV", subtype="FLOAT")
            stream.flush()
            os.fsync(stream.fileno())
        source_hash = e4._sha256_file(wav)
        custody = owned / "synthetic-custody.json"
        _write_json(custody, {"scope": "Owned deterministic mono fixture; no cohort source", "source_sha256": source_hash,
                              "mono_pcm_sha256": mono["sha256"], "sample_rate_hz": rate, "frame_count": frames})
        manifest = {"schema_version": 1, "source_sha256": source_hash, "source_sample_rate_hz": rate,
                    "source_frame_count": frames, "source_channels": 1, "origin_seconds": 0,
                    "mono_f32le_path": "input.f32le", "mono_f32le_sha256": mono["sha256"],
                    "mono_downmix_revision": "technical-fixture-mono-identity-f32-v1",
                    "source_receipt_path": custody.name, "source_receipt_sha256": e4._sha256_file(custody)}
        manifest_path = owned / "input.json"
        _write_json(manifest_path, manifest)
        del t, pcm
        actual = _prepare(manifest_path, owned, None, rt, e4, budget)
        # Only tiny owned fixtures invoke full reference/author transforms.
        # Actual cohort comparisons read immutable E4 receipts and hashes only.
        source_pcm = rt.np.fromfile(owned / "input.f32le", dtype="<f4")
        reference_pcm = e4._resample(source_pcm, rate, rt)
        budget.observe("smoke-e4-reference-pcm-before-use")
        if e4._sha256_array(reference_pcm) != actual["resampled_pcm"]["f32le_sha256"]:
            raise ValueError(f"Streaming complete11025PCM differs from E4: {name}")
        author_pcm, author_rate = rt.librosa.load(str(wav), sr=e4.RATE)
        budget.observe("smoke-author-reference-pcm-before-use")
        if author_rate != e4.RATE or e4._sha256_array(author_pcm) != actual["resampled_pcm"]["f32le_sha256"]:
            raise ValueError(f"Streaming complete11025PCM differs from the author WAV load: {name}")
        del author_pcm
        reference_patches, reference_geometry = e4._patches(reference_pcm, rt)
        reference_features, reference_stats = e4._normalize(reference_patches, rt)
        literal_patches = author_features(str(wav), frames=e4.PATCH, hop_length=e4.PATCH_HOP, zero_pad=False)
        literal_features = author_normalize(literal_patches).astype("<f4")
        budget.observe("smoke-author-reference-features-before-use")
        if (reference_geometry != actual["geometry"] or reference_stats != actual["global_normalization"]
                or list(reference_features.shape) != actual["features"]["shape"]
                or e4._sha256_array(reference_features) != actual["features"]["sha256"]
                or e4._sha256_array(literal_features) != actual["features"]["sha256"]
                or reference_features.dtype != rt.np.dtype("<f4") or literal_features.dtype != rt.np.dtype("<f4")):
            raise ValueError(f"Streaming normalized FEATUREBYTES differ from E4/author: {name}")
        if name == "block64_boundary" and actual["magnitude"]["shape"][1] != 65:
            raise ValueError("The technical fixture did not cross the64-frame block boundary")
        results.append({"case": name, "result": actual, "complete11025_pcm_e4_hash_exact": True,
                        "normalized_features_e4_author_hash_exact": True})
        del source_pcm, reference_pcm, reference_patches, reference_features, literal_patches, literal_features
        budget.observe("smoke-reference-arrays-released")
    zero = rt.np.zeros((1, e4.MELS, e4.PATCH, 1), dtype=rt.np.float32)
    for name, patches in (("zero_variance", zero), ("nonfinite_variance", rt.np.full_like(zero, rt.np.inf))):
        try:
            with rt.np.errstate(invalid="ignore", over="ignore"):
                e4._normalize(patches, rt)
        except ValueError:
            results.append({"case": name, "refused": True})
        else:
            raise ValueError(f"Expected unchanged global-normalizer refusal: {name}")
    return {"mode": "deterministic_streamed_smoke", "cases": results,
            "author_std_normalizer_source_sha256": normalizer_hash,
            "cohort_audio_opened": False, "model_inference_run": False}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--manifest", type=Path)
    mode.add_argument("--smoke", action="store_true")
    parser.add_argument("--packages-dir", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--held-native-bytes", type=int, required=True, help="Actual retained Native+QP+bridge byte charge;0 gives child-only equivalence")
    parser.add_argument("--reference-receipt", type=Path)
    args = parser.parse_args()
    output = args.out.resolve()
    created = False
    budget = None
    started = time.monotonic()
    try:
        if args.smoke and args.reference_receipt is not None:
            raise ValueError("Smoke uses actual pinned author/E4 functions, not an external cohort receipt")
        if not 0 <= args.held_native_bytes <= LIMIT:
            raise ValueError("Invalid held Native/QP/bridge byte charge")
        output.mkdir(exist_ok=False)
        created = True
        e4 = _reference_helpers()
        pins = e4._pins()
        pins["reference_runner_sha256"] = pins.pop("runner_sha256")
        pins["runner_sha256"] = e4._sha256_file(Path(__file__))
        rt, runtime = e4._runtime(args.packages_dir, output)
        budget = Budget(e4, args.held_native_bytes)
        budget.start()
        result = _smoke(output, rt, e4, budget) if args.smoke else _prepare(args.manifest, output, args.reference_receipt, rt, e4, budget)
        budget.observe("before-receipt-serialization")
        if any(name == "tensorflow" or name.startswith("tensorflow.") for name in sys.modules):
            raise ValueError("The artifact unexpectedly imported TensorFlow")
        frontend = e4._frontend_receipt()
        frontend["storage_implementation"] = {"revision": "streamed-soxr-global-stft-full-mel-v1", "pcm_chunk_frames": SOURCE_CHUNK_FRAMES,
                                              "stft_block_frames": STFT_CHUNK_FRAMES, "magnitude_order": "F",
                                              "mel": "single whole np.einsum(...ft,mf->...mt,optimize=True)",
                                              "normalizer": "exact immutable E4 helper"}
        receipt = {"schema_version": 1, "scope": "Technical streaming pilot only; exact parity and Root review precede any cached-CDF reuse or product proposal.",
                   "pins": pins, "runtime": runtime, "frontend": frontend, "result": result,
                   "resource_receipt": budget.receipt(), "elapsed_seconds": time.monotonic() - started}
        budget.observe("receipt-ready-before-write")
        _write_json(output / "frontend-receipt.json", receipt)
        budget.observe("receipt-synced")
        _write_json(output / "completion.json", {"status": "complete", "receipt_sha256": e4._sha256_file(output / "frontend-receipt.json"),
                                                 "resource_receipt": budget.receipt()})
        budget.observe("completion-synced-before-final-use")
        print(json.dumps({"status": "complete", "output": str(output), "receipt": "frontend-receipt.json"}))
        return 0
    except (OSError, ValueError, MemoryError, ImportError, RuntimeError) as error:
        refusal = {"status": "refused", "error_type": type(error).__name__, "detail": str(error),
                   "elapsed_seconds": time.monotonic() - started,
                   "resource_receipt": budget.receipt() if budget is not None else None,
                   "partial_owned_outputs_preserved": True}
        if created:
            _write_json(output / "refusal.json", refusal)
        print(json.dumps(refusal), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
