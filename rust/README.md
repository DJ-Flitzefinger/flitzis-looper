# Rust workspace

[The architecture overview](../docs/architecture.md) maps the Python/Native
boundary and links the technical chapters. Use the
[development guide](../docs/development.md) for toolchain setup, Native
dependencies and test commands.

| Crate | Responsibility |
| --- | --- |
| [`flitzis-looper`](crates/looper/) | PyO3 audio extension, Native input, media preparation, publication, render and lifecycle. |
| [`flitzis-looper-analysis`](crates/analysis/) | QM, scalar period assessment, KeyNet and optional Native Beat This worker. |
| [`flitzis-stem-conversion`](crates/stem-conversion/) | Bounded position-based conversion and instrumental summation used by the Native binding. |
| [`flitzis-stem-dsp`](crates/stem-dsp/) | Standalone spectral and candidate model DSP. Shipping separator selection is described in [Stems](../docs/architecture/stems.md). |

Find the callback, mixer and scheduler in
[`crates/looper/src/audio_engine/`](crates/looper/src/audio_engine/).
The Python-visible API is in the [type stub](../src/flitzis_looper_audio/__init__.pyi).
The [transport](../docs/architecture/runtime-and-transport.md) and
[media](../docs/architecture/media-and-pcm.md) chapters describe effect-time
checks, callback limits and retirement.

Run Windows tests through [`scripts/run-rust-tests.ps1`](../scripts/run-rust-tests.ps1)
from the repository root so test processes resolve the selected Python and DLLs.
Cargo features for diagnostics and isolated model workers are explicit in
[the crate manifests](crates/). Keep matching Rust, Python type stub and
[Portable preflight](../docs/portable-windows.md#installer-maintenance-and-validation)
contracts when changing the exposed API.
