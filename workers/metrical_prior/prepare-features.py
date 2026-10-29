#!/usr/bin/env python3
"""Isolated TempoCNN frontend preparation, without a model or audio decoder.

Adapted from TempoCNN feature.py/std_normalizer at the pinned commit below.
The unmodified author sources and their AGPL-3.0 license are in upstream/.
This artifact is not a production selector or source/phase authority.
"""

from __future__ import annotations

import argparse
import ast
import ctypes
import hashlib
import importlib
import importlib.metadata
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import sys
import time
from types import SimpleNamespace


HERE = Path(__file__).resolve().parent
AUTHOR_COMMIT = "482c64ad2e01b61d39bc161b796e3c2a1afaf67e"
AUTHOR_HASHES = {
    "feature.py": "30ca9aef9ebafb06c790d5ae63a52608b12076625a7907cbea0d19847336fbf0",
    "classifier.py": "516a874c59f7c6d9a4de818ea6fac479e9db6e742e9cd5c2b0736c004f23c374",
    "code-LICENSE": "76a97c878c9c7a8321bb395c2b44d3fe2f8d81314d219b20138ed0e2dddd5182",
    "weights-LICENSE": "76a97c878c9c7a8321bb395c2b44d3fe2f8d81314d219b20138ed0e2dddd5182",
}
MODEL_HASHES = {
    "model.json": "f93693c5a913b69aa378438665937600c09286f57714359155f2763496aba184",
    "group1-shard1of1.bin": "f7688e8868706813cd74610442f9923fca51162c1ebde6cf7fecfd62ebd24a15",
}
VERSIONS = {
    "numpy": "2.2.6",
    "librosa": "0.11.0",
    "scipy": "1.14.1",
    "numba": "0.61.2",
    "llvmlite": "0.44.0",
    "soundfile": "0.13.1",
    "soxr": "0.5.0.post1",
}
RATE = 11025
FFT = 1024
SPECTRAL_HOP = 512
MELS = 40
PATCH = 256
PATCH_HOP = 128
MEMORY_LIMIT = 512 * 1024 * 1024
WORKSPACE_RESERVE = 64 * 1024 * 1024
IO_CHUNK = 1024 * 1024


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(IO_CHUNK), b""):
            digest.update(block)
    return digest.hexdigest()


def _sha256_array(array) -> str:
    data = memoryview(array).cast("B")
    digest = hashlib.sha256()
    for offset in range(0, len(data), IO_CHUNK):
        digest.update(data[offset : offset + IO_CHUNK])
    return digest.hexdigest()


def _write_features(path: Path, features) -> dict:
    data = memoryview(features).cast("B")
    with path.open("xb") as stream:
        for start in range(0, len(data), IO_CHUNK):
            stream.write(data[start : start + IO_CHUNK])
    return {"path": path.name, "dtype": "float32-le", "shape": list(features.shape),
            "bytes": features.nbytes, "sha256": _sha256_array(features)}


def _json_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"Duplicate JSON key: {key}")
        result[key] = value
    return result


def _read_json(path: Path):
    if path.stat().st_size > IO_CHUNK:
        raise ValueError("JSON input exceeds one MiB")
    return json.loads(
        path.read_text(encoding="utf-8"),
        object_pairs_hook=_json_pairs,
        parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)),
    )


def _write_json(path: Path, value) -> None:
    with path.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2, allow_nan=False)
        stream.write("\n")


def _memory_snapshot() -> dict:
    """Observe process memory; the separate ledger admits managed allocations."""
    if os.name == "nt":
        class Counters(ctypes.Structure):
            _fields_ = [
                ("cb", ctypes.c_ulong), ("PageFaultCount", ctypes.c_ulong),
                ("PeakWorkingSetSize", ctypes.c_size_t), ("WorkingSetSize", ctypes.c_size_t),
                ("QuotaPeakPagedPoolUsage", ctypes.c_size_t), ("QuotaPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t), ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                ("PagefileUsage", ctypes.c_size_t), ("PeakPagefileUsage", ctypes.c_size_t),
                ("PrivateUsage", ctypes.c_size_t),
            ]
        counters = Counters()
        counters.cb = ctypes.sizeof(counters)
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.GetCurrentProcess.restype = ctypes.c_void_p
        psapi = ctypes.WinDLL("psapi", use_last_error=True)
        psapi.GetProcessMemoryInfo.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
        if not psapi.GetProcessMemoryInfo(kernel.GetCurrentProcess(), ctypes.byref(counters), counters.cb):
            raise OSError(ctypes.get_last_error(), "GetProcessMemoryInfo failed")
        return {
            "current_bytes": max(counters.WorkingSetSize, counters.PrivateUsage),
            "peak_bytes": max(counters.PeakWorkingSetSize, counters.PeakPagefileUsage),
            "measurement": "Windows working set/private commit",
        }
    status = Path("/proc/self/status")
    if not status.is_file():
        raise ValueError("This runner requires Windows or Linux process-memory observations")
    values = {}
    for line in status.read_text(encoding="ascii").splitlines():
        if line.startswith(("VmRSS:", "VmHWM:")):
            key, value, _unit = line.split()
            values[key] = int(value) * 1024
    return {"current_bytes": values["VmRSS:"], "peak_bytes": values["VmHWM:"], "measurement": "Linux RSS/HWM"}


def _observe_memory(stage: str, observations: list) -> None:
    observation = {"stage": stage, **_memory_snapshot()}
    observations.append(observation)
    if observation["peak_bytes"] > MEMORY_LIMIT:
        raise MemoryError(f"Observed process memory exceeds 512 MiB at {stage}")


def _runtime(dependency_root: Path, output: Path):
    if sys.version_info[:2] != (3, 12):
        raise ValueError("The frozen runtime requires Python 3.12")
    if sys.byteorder != "little":
        raise ValueError("The frozen f32le runtime requires a little-endian host")
    dependency_root = dependency_root.resolve(strict=True)
    sys.path.insert(0, str(dependency_root))
    # These settings affect this process only; no shared worker/package mutation.
    for name in ("OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS", "NUMBA_NUM_THREADS"):
        os.environ[name] = "1"
    os.environ["NUMBA_CACHE_DIR"] = str(output / "numba-cache")
    os.environ["LIBROSA_CACHE_DIR"] = str(output / "librosa-cache")
    os.environ["LIBROSA_CACHE_LEVEL"] = "0"
    packages = {}
    modules = {}
    for name, expected in VERSIONS.items():
        module = importlib.import_module(name)
        actual = importlib.metadata.version(name)
        path = Path(module.__file__).resolve(strict=True)
        if actual != expected or not path.is_relative_to(dependency_root):
            raise ValueError(f"{name} must be {expected} from the isolated dependency root")
        packages[name] = {"version": actual, "module_path": str(path), "module_sha256": _sha256_file(path)}
        modules[name] = module
    # Resolve lazy frontend modules before recording the runtime baseline.
    for name in ("librosa.core.audio", "librosa.core.spectrum", "librosa.feature.spectral", "librosa.filters"):
        module = importlib.import_module(name)
        path = Path(module.__file__).resolve(strict=True)
        packages[name] = {"module_path": str(path), "module_sha256": _sha256_file(path)}
    return SimpleNamespace(np=modules["numpy"], librosa=modules["librosa"], sf=modules["soundfile"]), {
        "python": platform.python_version(),
        "python_executable": sys.executable,
        "python_executable_sha256": _sha256_file(Path(sys.executable)),
        "platform": platform.platform(),
        "dependency_root": str(dependency_root),
        "packages": packages,
        "requirements_lock_sha256": _sha256_file(HERE / "requirements.lock"),
        "thread_settings": {name: os.environ[name] for name in ("OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS", "NUMBA_NUM_THREADS")},
    }


def _pins() -> dict:
    author = {}
    model = {}
    for name, expected in AUTHOR_HASHES.items():
        path = HERE / "upstream" / name
        actual = _sha256_file(path)
        if actual != expected:
            raise ValueError(f"Pinned author artifact differs: {name}")
        author[name] = {"sha256": actual, "bytes": path.stat().st_size}
    for name, expected in MODEL_HASHES.items():
        path = HERE / "upstream" / "deeptemp-k16-3-tfjs" / name
        actual = _sha256_file(path)
        if actual != expected:
            raise ValueError(f"Pinned model artifact differs: {name}")
        model[name] = {"sha256": actual, "bytes": path.stat().st_size}
    return {"author_commit": AUTHOR_COMMIT, "author": author, "model": model, "runner_sha256": _sha256_file(Path(__file__))}


def _ledger(source_frames: int, source_rate: int, baseline: int) -> dict:
    analyzer_frames = (source_frames * RATE + source_rate - 1) // source_rate
    mel_frames = 1 + analyzer_frames // SPECTRAL_HOP
    padded_frames = max(PATCH, mel_frames)
    windows = 1 + (padded_frames - PATCH) // PATCH_HOP
    elements = windows * MELS * PATCH
    pcm_bytes = source_frames * 4
    analyzer_bytes = analyzer_frames * 4
    mel_bytes = MELS * mel_frames * 4
    patch_bytes = elements * 4
    # Count full complex/magnitude spectra, although librosa internally blocks FFT work.
    spectral_work = (FFT // 2 + 1) * mel_frames * (8 + 4)
    phases = {
        "resample": pcm_bytes + analyzer_bytes,
        "mel": analyzer_bytes + spectral_work + mel_bytes,
        "patch": mel_bytes + MELS * padded_frames * 4 + patch_bytes,
        # f64 source, std scratch, subtract/divide temporaries and final f16/f32.
        "normalize": patch_bytes + elements * (8 * 4 + 2 + 4),
    }
    admitted_peak = baseline + max(phases.values()) + WORKSPACE_RESERVE + 2 * IO_CHUNK
    result = {
        "limit_bytes": MEMORY_LIMIT,
        "runtime_baseline_bytes": baseline,
        "native_runtime_workspace_reserve_bytes": WORKSPACE_RESERVE,
        "io_reserve_bytes": 2 * IO_CHUNK,
        "managed_phase_bytes": phases,
        "admitted_peak_bytes": admitted_peak,
        "analyzer_frames": analyzer_frames,
        "mel_frames": mel_frames,
        "padded_mel_frames": padded_frames,
        "patch_count": windows,
        "feature_shape": [windows, MELS, PATCH, 1],
        "scope": "Managed-array admission plus declared workspace reserve; observed process peak is checked separately. No OS hard-memory limit is claimed.",
    }
    if admitted_peak > MEMORY_LIMIT:
        raise MemoryError(f"Frontend memory admission exceeds 512 MiB: {admitted_peak}")
    return result


def _resample(pcm, sample_rate: int, rt):
    output = rt.librosa.resample(
        pcm, orig_sr=sample_rate, target_sr=RATE, res_type="soxr_hq", fix=True, scale=False, axis=-1,
    )
    output = rt.np.ascontiguousarray(output, dtype=rt.np.float32)
    if rt.np.shares_memory(output, pcm):
        output = output.copy()
    expected = (len(pcm) * RATE + sample_rate - 1) // sample_rate
    if output.ndim != 1 or len(output) != expected or not rt.np.isfinite(output).all():
        raise ValueError("Resampling did not preserve the declared complete finite extent")
    return output


def _patches(pcm_11025, rt):
    data = rt.librosa.feature.melspectrogram(
        y=pcm_11025, sr=RATE, n_fft=FFT, hop_length=SPECTRAL_HOP,
        win_length=FFT, window="hann", center=True, pad_mode="constant", power=1,
        n_mels=MELS, fmin=20, fmax=5000, htk=False, norm="slaney", dtype=rt.np.float32,
    )
    original_frames = data.shape[1]
    if original_frames < PATCH:
        padded = rt.np.zeros((MELS, PATCH), dtype=data.dtype)
        padded[:, :original_frames] = data
        data = padded
    count = 1 + (data.shape[1] - PATCH) // PATCH_HOP
    result = rt.np.empty((count, MELS, PATCH, 1), dtype=data.dtype)
    for index in range(count):
        start = index * PATCH_HOP
        result[index, :, :, 0] = data[:, start : start + PATCH]
    return result, {
        "mel_frame_count": original_frames,
        "short_padding_frames": max(0, PATCH - original_frames),
        "unpatched_tail_frames": data.shape[1] - ((count - 1) * PATCH_HOP + PATCH),
        "patch_starts_mel_frames": [index * PATCH_HOP for index in range(count)],
    }


def _normalize(patches, rt):
    data = patches.astype(rt.np.float64)
    mean = float(rt.np.mean(data))
    std = float(rt.np.std(data))
    if not math.isfinite(mean) or not math.isfinite(std) or std <= 0:
        raise ValueError("Global overlapping-tensor variance must be finite and positive")
    normalized = ((data - mean) / std).astype(rt.np.float16)
    if not rt.np.isfinite(normalized).all():
        raise ValueError("Float16 frontend features are nonfinite")
    return normalized.astype("<f4"), {
        "scope": "entire-overlapping-window-tensor", "precision": "float64",
        "roundtrip": "float16-to-float32", "mean": mean, "std": std, "ddof": 0,
        "population": "including short-input zeros and repeated overlap frames",
        "quantization": "float64 standardized -> float16 -> little-endian float32",
    }


def _frontend_receipt() -> dict:
    return {
        "revision": "tempocnn-author-482c64ad-librosa011-global-std-f16-v1",
        "sample_rate_hz": RATE,
        "resampler": {"implementation": "librosa.resample/soxr_hq", "fix": True, "scale": False, "axis": -1},
        "spectrogram": {"n_fft": FFT, "hop_length": SPECTRAL_HOP, "win_length": FFT, "window": "hann", "center": True, "pad_mode": "constant", "power": 1},
        "mel": {"n_mels": MELS, "fmin": 20, "fmax": 5000, "htk": False, "norm": "slaney", "dtype": "float32"},
        "patch": {"frames": PATCH, "hop": PATCH_HOP, "extra_front_back_padding": False, "short_padding": "right zero-pad to 256 mel frames", "tail": "author full-window rule; incomplete final patch is not emitted"},
        "graph_input_signature": [-1, 40, -1, 1],
        "emitted_layout": "C-order Bx40x256x1",
        "output_dtype": "little-endian float32 after float16 roundtrip",
    }


def _author_functions(rt):
    spec = importlib.util.spec_from_file_location("pinned_tempocnn_feature", HERE / "upstream" / "feature.py")
    if spec is None or spec.loader is None:
        raise ValueError("Cannot load the pinned author feature module")
    feature = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(feature)
    source = (HERE / "upstream" / "classifier.py").read_text(encoding="utf-8")
    tree = ast.parse(source)
    nodes = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "std_normalizer"]
    if len(nodes) != 1:
        raise ValueError("Expected exactly one pinned author std_normalizer")
    # Only this exact function executes; classifier imports/classes (TensorFlow) do not.
    module = ast.Module(body=nodes, type_ignores=[])
    namespace = {"np": rt.np}
    exec(compile(module, str(HERE / "upstream" / "classifier.py"), "exec"), namespace)
    extracted = ast.get_source_segment(source, nodes[0])
    return feature.read_features, namespace["std_normalizer"], hashlib.sha256(extracted.encode("utf-8")).hexdigest()


def _smoke(output: Path, rt, baseline: int, observations: list) -> dict:
    author_features, author_normalize, function_hash = _author_functions(rt)
    cases = [("short", 11025, 1025), ("tail", 11025, 512 * 401 + 137), ("resampled_tail", 48000, 48000 * 19 + 113)]
    reports = []
    for name, rate, frames in cases:
        ledger = _ledger(frames, rate, baseline)
        t = rt.np.arange(frames, dtype=rt.np.float64) / rate
        # Amplitude variation makes accidental per-patch normalization detectable.
        pcm = ((0.2 + 0.1 * rt.np.sin(2 * rt.np.pi * 0.17 * t)) * rt.np.sin(2 * rt.np.pi * 317 * t)
               + 0.05 * rt.np.cos(2 * rt.np.pi * 911 * t)).astype("<f4")
        pcm[-1] += rt.np.float32(0.125)
        wav = output / f"{name}.wav"
        with wav.open("xb") as stream:
            rt.sf.write(stream, pcm, rate, format="WAV", subtype="FLOAT")
        resampled = _resample(pcm, rate, rt)
        actual_patches, geometry = _patches(resampled, rt)
        literal_patches = author_features(str(wav), frames=PATCH, hop_length=PATCH_HOP, zero_pad=False)
        if (actual_patches.shape != literal_patches.shape
                or actual_patches.dtype != literal_patches.dtype
                or actual_patches.dtype != rt.np.dtype("<f4")
                or not rt.np.array_equal(actual_patches.view("<u4"), literal_patches.view("<u4"))):
            raise ValueError(f"Author mel/patch parity failed: {name}")
        actual, statistics = _normalize(actual_patches, rt)
        literal = author_normalize(literal_patches).astype("<f4")
        if (actual.shape != literal.shape or actual.dtype != literal.dtype
                or actual.dtype != rt.np.dtype("<f4")
                or not rt.np.array_equal(actual.view("<u4"), literal.view("<u4"))):
            raise ValueError(f"Author global normalization parity failed: {name}")
        expected_mel_frames = 1 + len(resampled) // SPECTRAL_HOP
        expected_count = 1 + (max(PATCH, expected_mel_frames) - PATCH) // PATCH_HOP
        if list(actual.shape) != [expected_count, MELS, PATCH, 1] or geometry["mel_frame_count"] != expected_mel_frames:
            raise ValueError(f"Full extent/window geometry failed: {name}")
        if name == "short" and geometry["short_padding_frames"] <= 0:
            raise ValueError("Short-input smoke did not exercise padding")
        if name.endswith("tail") and not 0 < geometry["unpatched_tail_frames"] < PATCH_HOP:
            raise ValueError("Tail smoke did not exercise incomplete-patch handling")
        if len(actual) > 1:
            local = rt.np.concatenate([author_normalize(patch[None, ...]).astype("<f4") for patch in actual_patches], axis=0)
            if rt.np.array_equal(actual, local):
                raise ValueError("Smoke did not distinguish global from per-patch normalization")
            del local
        feature_file = _write_features(output / f"{name}.features.f32le", actual)
        reports.append({"case": name, "source_sha256": _sha256_file(wav), "source_rate_hz": rate, "source_frames": frames,
                        "source_pcm_sha256": _sha256_array(pcm), "wav_sha256": _sha256_file(wav),
                        "resampled_frames": len(resampled), "resampled_pcm_sha256": _sha256_array(resampled),
                        "shape": list(actual.shape), "features_sha256": _sha256_array(actual),
                        "author_patch_bit_parity": True, "author_normalized_bit_parity": True,
                        "features": feature_file, "geometry": geometry, "statistics": statistics, "memory_ledger": ledger})
        del t, pcm, resampled, actual_patches, literal_patches, actual, literal
        _observe_memory(f"smoke-{name}", observations)
    zeros = rt.np.zeros(RATE, dtype=rt.np.float32)
    zero_patches, _geometry = _patches(zeros, rt)
    if not rt.np.array_equal(author_normalize(zero_patches), rt.np.zeros_like(zero_patches, dtype=rt.np.float16)):
        raise ValueError("Unexpected pinned author zero-input behavior")
    for name, patches in (("zero_variance", zero_patches), ("nonfinite_variance", rt.np.full_like(zero_patches, rt.np.inf))):
        try:
            _normalize(patches, rt)
        except ValueError:
            reports.append({"case": name, "refused": True})
        else:
            raise ValueError(f"Expected frontend refusal: {name}")
    if any(name == "tensorflow" or name.startswith("tensorflow.") for name in sys.modules):
        raise ValueError("Smoke unexpectedly imported TensorFlow")
    return {"mode": "deterministic_smoke", "author_std_normalizer_source_sha256": function_hash,
            "cases": reports, "cohort_audio_opened": False, "tensorflow_imported": False}


def _manifest(path: Path) -> tuple[dict, Path, Path]:
    value = _read_json(path)
    fields = {"schema_version", "source_sha256", "source_sample_rate_hz", "source_frame_count", "source_channels", "origin_seconds",
              "mono_f32le_path", "mono_f32le_sha256", "mono_downmix_revision", "source_receipt_path", "source_receipt_sha256"}
    if not isinstance(value, dict) or set(value) != fields or type(value["schema_version"]) is not int or value["schema_version"] != 1:
        raise ValueError("Unexpected full-PCM input-manifest fields/version")
    for name, lower, upper in (("source_sample_rate_hz", 1, 384000), ("source_frame_count", 1, 2**53 - 1), ("source_channels", 1, 32)):
        if type(value[name]) is not int or not lower <= value[name] <= upper:
            raise ValueError(f"Invalid input geometry: {name}")
    if type(value["origin_seconds"]) not in (int, float) or value["origin_seconds"] != 0:
        raise ValueError("Only a complete source with origin zero is admitted")
    for name in ("source_sha256", "mono_f32le_sha256", "source_receipt_sha256"):
        digest = value[name]
        if not isinstance(digest, str) or len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest):
            raise ValueError(f"Invalid lowercase SHA256: {name}")
    if not isinstance(value["mono_downmix_revision"], str) or not 1 <= len(value["mono_downmix_revision"]) <= 256:
        raise ValueError("A bounded actual downmix revision is required")
    resolved = []
    for name in ("mono_f32le_path", "source_receipt_path"):
        if not isinstance(value[name], str) or not value[name]:
            raise ValueError(f"Missing input path: {name}")
        item = Path(value[name])
        resolved.append((item if item.is_absolute() else path.parent / item).resolve(strict=True))
    pcm, receipt = resolved
    if pcm.stat().st_size != value["source_frame_count"] * 4:
        raise ValueError("Mono f32le byte count differs from the complete source extent")
    if receipt.stat().st_size > IO_CHUNK or _sha256_file(receipt) != value["source_receipt_sha256"]:
        raise ValueError("Bounded source-custody receipt hash differs")
    return value, pcm, receipt


def _prepare(manifest_path: Path, output: Path, rt, baseline: int, observations: list) -> dict:
    manifest_path = manifest_path.resolve(strict=True)
    value, pcm_path, receipt_path = _manifest(manifest_path)
    ledger = _ledger(value["source_frame_count"], value["source_sample_rate_hz"], baseline)
    if _sha256_file(pcm_path) != value["mono_f32le_sha256"]:
        raise ValueError("Actual complete mono PCM hash differs")
    pcm = rt.np.memmap(pcm_path, mode="r", dtype="<f4", shape=(value["source_frame_count"],))
    try:
        for start in range(0, len(pcm), IO_CHUNK // 4):
            if not rt.np.isfinite(pcm[start : start + IO_CHUNK // 4]).all():
                raise ValueError("Actual mono PCM contains nonfinite samples")
        resampled = _resample(pcm, value["source_sample_rate_hz"], rt)
        if _sha256_file(pcm_path) != value["mono_f32le_sha256"]:
            raise ValueError("Input PCM changed while preparing the complete resampled extent")
    finally:
        pcm._mmap.close()
    del pcm
    _observe_memory("resampled-full-source", observations)
    analyzer = {"sample_rate_hz": RATE, "frame_count": len(resampled), "origin_seconds": 0,
                "duration_seconds": len(resampled) / RATE, "f32le_sha256": _sha256_array(resampled)}
    patches, geometry = _patches(resampled, rt)
    del resampled
    _observe_memory("complete-patches", observations)
    features, statistics = _normalize(patches, rt)
    del patches
    if list(features.shape) != ledger["feature_shape"]:
        raise ValueError("Actual patch geometry differs from admitted full-source geometry")
    _observe_memory("normalized-features", observations)
    feature_path = output / "features.f32le"
    feature_file = _write_features(feature_path, features)
    return {"mode": "pinned_complete_mono_pcm", "input_manifest": value,
            "source_sha256": value["source_sha256"],
            "standardized_source_pcm": {"format": "mono-f32le", "sha256": value["mono_f32le_sha256"],
                                        "sample_rate_hz": value["source_sample_rate_hz"], "frame_count": value["source_frame_count"],
                                        "origin_seconds": 0, "downmix_revision": value["mono_downmix_revision"]},
            "input_manifest_sha256": _sha256_file(manifest_path),
            "source_custody_receipt": {"path": str(receipt_path), "sha256": value["source_receipt_sha256"], "meaning": "Opaque acquired custody link; this runner does not authenticate original decoding or grant source/phase authority."},
            "source_extent": {"sample_rate_hz": value["source_sample_rate_hz"], "frame_count": value["source_frame_count"], "channels_before_mono": value["source_channels"], "origin_seconds": 0, "duration_seconds": value["source_frame_count"] / value["source_sample_rate_hz"]},
            "resampled_pcm": analyzer, "geometry": geometry, "global_normalization": statistics,
            "features": feature_file,
            "memory_ledger": ledger}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--smoke", action="store_true", help="Only deterministic owned WAV fixtures; no cohort input")
    mode.add_argument("--manifest", type=Path, help="Pinned full actual-mono f32le geometry/hash manifest")
    parser.add_argument("--dependency-root", "--packages-dir", dest="dependency_root", type=Path, required=True, help="Root-owned isolated Python package target")
    parser.add_argument("--out", type=Path, required=True, help="New output directory; existing paths are refused")
    args = parser.parse_args()
    output = args.out.resolve()
    created = False
    started = time.monotonic()
    try:
        output.mkdir(parents=False, exist_ok=False)
        created = True
        pins = _pins()
        rt, runtime = _runtime(args.dependency_root, output)
        observations = []
        _observe_memory("isolated-runtime-loaded", observations)
        baseline = observations[-1]["current_bytes"]
        result = _smoke(output, rt, baseline, observations) if args.smoke else _prepare(args.manifest, output, rt, baseline, observations)
        _observe_memory("finished", observations)
        receipt = {"schema_version": 1, "scope": "Technical frontend only; no model inference, acceptance or automatic selection.",
                   "pins": pins, "runtime": runtime, "frontend": _frontend_receipt(),
                   "result": result, "memory_observations": observations, "elapsed_seconds": time.monotonic() - started}
        _write_json(output / "frontend-receipt.json", receipt)
        print(json.dumps({"status": "complete", "output": str(output), "receipt": "frontend-receipt.json"}))
        return 0
    except (OSError, ValueError, MemoryError, ImportError) as error:
        refusal = {"status": "refused", "error_type": type(error).__name__, "detail": str(error), "elapsed_seconds": time.monotonic() - started}
        if created:
            _write_json(output / "refusal.json", refusal)
        print(json.dumps(refusal), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
