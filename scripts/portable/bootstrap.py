"""Run the private portable runtime; import checks never start the GUI/device."""

# Standalone entrypoint copied into each runtime, not an importable package.
# ruff: noqa: INP001
import argparse
import ctypes
import importlib
import json
import os
import runpy
import subprocess
import sys
from pathlib import Path


def configure(root: Path, version: Path) -> list[object]:
    sys.path.insert(0, str(version / "app"))
    os.chdir(root / "data")
    os.environ["FLITZIS_MODEL_CACHE_DIR"] = str(root / "models/torch/hub/checkpoints")
    os.environ["TORCH_HOME"] = str(root / "models/torch")
    os.environ["HF_HOME"] = str(root / "models/huggingface")
    os.environ["XDG_CACHE_HOME"] = str(root / "cache")
    os.environ["DORA_DIR"] = str(root / "cache/dora")
    os.environ["TMP"] = os.environ["TEMP"] = str(root / "cache/temp")
    ffmpeg = version / "runtime/ffmpeg"
    os.environ["FLITZIS_FFMPEG_DIR"] = str(ffmpeg)
    os.environ["PATH"] = os.pathsep.join(
        [str(ffmpeg), str(version / "python"), os.environ.get("PATH", "")],
    )
    # Keep loader directory handles alive until the GUI and workers terminate.
    return [
        os.add_dll_directory(str(path)) for path in [version / "app/flitzis_looper_audio", ffmpeg]
    ]


def private_dlls(version: Path) -> dict[str, str]:
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GetModuleHandleW.argtypes = [ctypes.c_wchar_p]
    kernel.GetModuleHandleW.restype = ctypes.c_void_p
    kernel.GetModuleFileNameW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_uint]
    dlls = {}
    for name in [
        "rubberband-3.dll",
        "samplerate.dll",
        "sleef.dll",
        "sleefdft.dll",
        "DirectML.dll",
        "MSVCP140.dll",
        "MSVCP140_1.dll",
        "VCOMP140.dll",
        "VCRUNTIME140.dll",
        "VCRUNTIME140_1.dll",
    ]:
        handle = kernel.GetModuleHandleW(name)
        assert handle, f"Runtime DLL not loaded: {name}"
        buffer = ctypes.create_unicode_buffer(32768)
        assert kernel.GetModuleFileNameW(handle, buffer, len(buffer))
        dll = Path(buffer.value).resolve()
        assert dll.is_relative_to(version), f"Nonportable DLL resolution: {dll}"
        dlls[name] = str(dll)
    return dlls


def check_runtime(root: Path, version: Path) -> None:
    # Runtime imports must follow private-path/DLL setup, before any GUI/device opens.
    native_module = importlib.import_module("flitzis_looper_audio")
    native = Path(native_module.flitzis_looper_audio.__file__).resolve()
    assert native.is_relative_to(version / "app"), native
    assert native_module.native_build_profile() == "release"
    assert Path(sys.executable).resolve().is_relative_to(version / "python")
    for name in ["imgui_bundle", "torch", "torchaudio", "torchcodec"]:
        module = importlib.import_module(name)
        assert Path(module.__file__).resolve().is_relative_to(version / "python"), module
    assert importlib.import_module("flitzis_looper.controller.stems").StemController
    assert importlib.import_module("flitzis_looper.ui.render.sidebar_left")
    generation = importlib.import_module("flitzis_looper.controller.stem_generation")
    assert generation.default_demucs_model_cache_dir() == root / "models/torch/hub/checkpoints"
    assert (version / "python/keynet.onnx").is_file()
    dlls = private_dlls(version)
    child_env = os.environ | {"PYTHONPATH": str(version / "app"), "PYTHONDONTWRITEBYTECODE": "1"}
    subprocess.run(
        [
            sys.executable,
            "-s",
            "-B",
            "-c",
            (
                "import torchcodec.encoders; import flitzis_looper.demucs_streaming; "
                "import flitzis_bs_roformer.__main__"
            ),
        ],
        env=child_env,
        check=True,
    )
    for tool in ["ffmpeg.exe", "ffprobe.exe"]:
        subprocess.run(
            [str(version / "runtime/ffmpeg" / tool), "-version"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    sys.stdout.write(
        json.dumps({"portable_preflight": "passed", "native": str(native), "dlls": dlls}) + "\n"
    )
    sys.stdout.flush()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    version = Path(__file__).resolve().parent
    root = args.root.resolve()
    dll_handles = configure(root, version)
    assert dll_handles
    if args.check:
        check_runtime(root, version)
    else:
        runpy.run_module("flitzis_looper", run_name="__main__")


if __name__ == "__main__":
    main()
