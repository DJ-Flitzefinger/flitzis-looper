"""Run the private portable runtime; import checks never start the GUI/device."""

# Standalone entrypoint copied into each runtime.
import argparse
import ctypes
import importlib
import json
import os
import runpy
import subprocess
import sys
from pathlib import Path


def physical_path(path: Path) -> Path:
    """Compare private origins without Windows extended-path spelling differences."""
    value = str(path.resolve())
    if os.name == "nt":
        if value.startswith("\\\\?\\UNC\\"):
            value = "\\\\" + value[8:]
        elif value.startswith("\\\\?\\"):
            value = value[4:]
    return Path(value)


def configure(root: Path, checkout: Path, runtime: Path | None = None) -> list[object]:
    """Select one checkout and its private interpreter before importing the app."""
    runtime = runtime or Path(sys.executable).absolute().parent.parent
    sys.path.insert(0, str(checkout / "src"))
    os.environ["PYTHONPATH"] = str(checkout / "src")
    os.chdir(checkout)
    os.environ["FLITZIS_MODEL_CACHE_DIR"] = str(root / "models/torch/hub/checkpoints")
    os.environ["TORCH_HOME"] = str(root / "models/torch")
    os.environ["HF_HOME"] = str(root / "models/huggingface")
    os.environ["XDG_CACHE_HOME"] = str(root / "cache")
    os.environ["DORA_DIR"] = str(root / "cache/dora")
    os.environ["TMP"] = os.environ["TEMP"] = str(root / "cache/temp")
    ffmpeg = runtime / "runtime/ffmpeg"
    os.environ["FLITZIS_FFMPEG_DIR"] = str(ffmpeg)
    os.environ["PATH"] = os.pathsep.join(
        [str(ffmpeg), str(runtime / "python"), os.environ.get("PATH", "")],
    )
    # Keep loader directory handles alive until the GUI and workers terminate.
    return [
        os.add_dll_directory(str(path)) for path in [checkout / "src/flitzis_looper_audio", ffmpeg]
    ]


def private_dlls(runtime: Path, checkout: Path) -> dict[str, str]:
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
        dll = physical_path(Path(buffer.value))
        assert any(
            dll.is_relative_to(physical_path(directory))
            for directory in [runtime, checkout / "src/flitzis_looper_audio"]
        ), f"Nonportable DLL resolution: {dll}"
        dlls[name] = str(dll)
    return dlls


def check_runtime(root: Path, checkout: Path, runtime: Path) -> None:
    # Runtime imports must follow private-path/DLL setup, before any GUI/device opens.
    native_module = importlib.import_module("flitzis_looper_audio")
    native = physical_path(Path(native_module.flitzis_looper_audio.__file__))
    assert native.is_relative_to(physical_path(checkout / "src/flitzis_looper_audio")), native
    assert native_module.native_build_profile() == "release"
    assert physical_path(Path(sys.executable)).is_relative_to(physical_path(runtime / "python"))
    customization = importlib.import_module("sitecustomize")
    assert physical_path(Path(customization.__file__)).is_relative_to(
        physical_path(runtime / "python")
    )
    for name in ["imgui_bundle", "torch", "torchaudio", "torchcodec"]:
        module = importlib.import_module(name)
        assert physical_path(Path(module.__file__)).is_relative_to(
            physical_path(runtime / "python")
        ), module
    assert importlib.import_module("flitzis_looper.controller.stems").StemController
    assert importlib.import_module("flitzis_looper.ui.render.sidebar_left")
    generation = importlib.import_module("flitzis_looper.controller.stem_generation")
    assert generation.default_demucs_model_cache_dir() == root / "models/torch/hub/checkpoints"
    assert physical_path(Path.cwd()) == physical_path(checkout)
    assert (checkout / "assets/models/keynet.onnx").is_file()
    app_module = importlib.import_module("flitzis_looper")
    assert physical_path(Path(app_module.__file__)).is_relative_to(physical_path(checkout / "src"))
    dlls = private_dlls(runtime, checkout)
    child_env = os.environ | {"PYTHONPATH": str(checkout / "src"), "PYTHONDONTWRITEBYTECODE": "1"}
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
            [str(runtime / "runtime/ffmpeg" / tool), "-version"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    sys.stdout.write(
        json.dumps({
            "portable_preflight": "passed",
            "native": str(native),
            "checkout": str(physical_path(checkout)),
            "dlls": dlls,
        })
        + "\n"
    )
    sys.stdout.flush()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--checkout", type=Path)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--check-runtime", action="store_true")
    args = parser.parse_args()
    # Preserve the starter's short operational path; use resolved paths only
    # when proving that imports/DLLs belong to this private runtime.
    runtime = args.runtime or Path(__file__).absolute().parent
    root = args.root.resolve()
    checkout = args.checkout or root / "flitzis-looper"
    dll_handles = configure(root, checkout, runtime)
    assert dll_handles
    if args.check or args.check_runtime:
        check_runtime(root, checkout, runtime)
    else:
        runpy.run_module("flitzis_looper", run_name="__main__")


if __name__ == "__main__":
    main()
