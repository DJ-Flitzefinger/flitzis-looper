# Portable Looper for Windows x64

Copy [`INSTALL-LOOPER-PORTABLE.bat`](../INSTALL-LOOPER-PORTABLE.bat) into
the chosen parent folder and double-click it. It creates `Flitzis-Looper`
beside the BAT and installs one exact commit from remote `main`. It acquires
pinned tools over the network and builds from source. The initial install needs
download time and disk space. Shared Microsoft C++ Build Tools and the Windows
SDK can require elevation or a restart. Rerun the BAT afterward.

## Start, update and repair

- Start with `Flitzis-Looper/START-LOOPER.bat`. The installed `start.bat` and
  `start-release.bat` aliases use the same private Release runtime. Normal
  start uses the installed runtime with no build or update.
- `START-LOOPER.bat --check` runs runtime import, DLL, codec and worker checks
  before App or audio-device acquisition. It reports CUDA cache readiness
  without running the model.
- Close the App and rerun the current installer BAT to update or repair.
  The launch and update paths share a lock held for the actual App process
  lifetime. Older active private Python processes also fence updates.
- Keep the whole installation directory together when moving it. Use the
  installed starters to keep the runtime, model and cache bindings local.
  The automation form accepts an optional target directory and `--no-pause`.

The [user guide](user-guide.md#audio-output-and-startup) describes output
selection and Apply. Native ASIO uses a vendor driver already installed and
registered on the system. WASAPI works without ASIO. Portable supplies the App
runtime and does not install GPU or audio-device drivers.

## Layout and runtime resources

| Path below `Flitzis-Looper` | Contents |
| --- | --- |
| `flitzis-looper/` | Full Git checkout and Native extension. `samples/`, `config/input/` and `record/` hold user data. |
| `Runtime/` | Private interpreter, packages, DLLs, FFmpeg/FFprobe, launch support and the Release Beat This executable. |
| `analysis-runtime/beat-this/` | Pinned ONNX worker assets, verified active pointer and installation-owned CUDA cache. |
| `models/` | Independent stem model cache. |
| `tools/`, `cache/`, `b/`, `logs/` | Bootstrap tools, retained downloads and diagnostics, short Native build path, and logs. UI layout is installation-local in `cache/ui/`. |

[The installer](../scripts/portable/install-main.ps1) selects verified Git, uv,
CPython, Rust, vcpkg, Rubber Band and codecs. The Native build uses the tracked
[vcpkg overlay](../scripts/native/vcpkg-overlay/) and MSVC ASIO bridge. Driver
access and realtime behavior are described in the
[audio runtime](architecture/runtime-and-transport.md#output-selection-and-apply).

[`prepare_analysis.py`](../scripts/portable/prepare_analysis.py) stages the
Release Beat This worker and verifies the selected package. Auto-BPM and Analyze
use its pinned dynamic FP32 graph and ONNX Runtime 1.24.4. The installer
provisions CPU on Windows x64 and the pinned CUDA 12.8.1 and cuDNN 9.10.2
libraries when it detects a usable NVIDIA device. The same graph runs on CPU
when CUDA is unavailable or a CUDA failure qualifies for a CPU retry.
[Beat This model and runtime notices](../assets/models/beat-this/README.md)
describe the pinned sources.

The installer prepares the CUDA disk cache with three synthetic inputs in the
order 30.25 s, 162 s and 8 s. It reports CUDA readiness only after each worker
confirms CUDA execution and exits. The cache survives restarts and can be reused
on unchanged-main repair. The selected GPU, NVIDIA driver release, worker,
model and package determine which cache is usable. After a driver change or
cache loss, repair or the App's background analysis path can prepare the new
cache. If a CPU-only installation later gains a usable NVIDIA device, rerun
the installer to acquire the CUDA package. If the installer cannot complete
CUDA preparation for a detected device, it reports the failure and keeps
the previous installation selected. A system without usable CUDA can install
the CPU worker.

[`prepare_models.py`](../scripts/portable/prepare_models.py) independently
verifies the pinned BS-RoFormer and Demucs files and loads each model on CPU in
its own process before activation. Unchanged-main worker and model repair
can reuse a healthy Runtime without rebuilding Native.
[Worker and asset details](model-worker-setup.md) describe both environments.

Acquisition resumes retained partial downloads and checks full pins before use.
The installer binds child processes to its own runtime and ignores inherited
personal bindings. Temporary drive mappings protect long build/runtime paths
and last for the owning process. Starts do not depend on the installation-time
drive letter.

## Update and recovery

Candidate source, runtime, required model resources and file custody are
verified before activation. A durable transaction journal covers checkout and
Runtime swaps, data relocation and worker selection. After interruption rerun
the same BAT to recover or finish the verified operation. The starter refuses
an unfinished transaction.

Project material and recording files retain their identities. The
[relocation adapter](../scripts/portable/relocate_data.py) uses Native custody
and journal validation for current assignments, Copy/Pair histories and the
recognized older `versions/main-…` plus `data` layout. Legacy sample moves
require same-volume Windows FileIDs and directory alternate data streams.
External audio retains its external path. Unknown ownership, unsafe/reparse
paths and inconsistent receipts stop recovery. A recognized old checkout with
unregistered extras can be preserved intact in its reported backup path while
a clean successor activates. Changed registered source or Runtime bytes need
diagnosis. The tracked generated-extension exception preserves the old checkout
and builds a clean candidate without importing the changed binary.

## Installer maintenance and validation

Review the BAT, [Portable scripts](../scripts/portable/), and focused tests when
changing package locks, ABI, toolchains, Native/codec DLLs, layout or model
resolution. Keep Native relocation authoritative. Preflight must use the
candidate's actual private interpreter and verified DLLs and reflect the
matching Native APIs without opening a device. Exercise the affected bootstrap,
deep-path, unchanged-main repair, update/rollback, legacy data custody or
lifetime path. Clean-machine setup, device use and listening have separate
evidence from import and worker readiness.

The advanced [bundle builder](../scripts/portable/build_bundle.py) packages a
matching Native wheel and tracked source snapshot from a prepared Windows x64
CPython 3.14 environment. It builds the matching Beat This executable and
stages both provider packages for its runtime check. It accepts
`--native-wheel`, `--native-source-commit`,
`--ffmpeg-dir`, declared `--dll-dir` and `--license-file NAME=PATH` inputs,
and `--output` outside the repository. Its output is a separate development
bundle with no Git checkout metadata, private project data, Stem models or
machine-specific CUDA cache.
Extract it in an empty directory and use its `START-LOOPER.bat --check`.
The ordinary BAT remains the source install entry point.
