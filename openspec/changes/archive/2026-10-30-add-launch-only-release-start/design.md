# Windows start routing

The four root BAT files delegate to `scripts/start-app.bat`. Normal `start.bat`
uses launch mode and `.venv\Scripts\python.exe` directly, never `uv`, so no
environment synchronization, installation or compilation can occur implicitly.
The loaded module reports `debug` or `release` through a scalar native getter
derived from its compile configuration, without constructing an audio engine.
An absent getter is treated as an older unsupported build, not a Release guess.

The existing Debug and Release modes retain locked setup and Maturin installation,
then verify the actual installed profile with the same direct Python preflight.
`--check` and `--build-only` stop before the Python application entrypoint; failures
preserve the failed step's exit code and do not pause in these noninteractive modes.
Normal starts retain a visible paused diagnostic on failure. Repository-relative
paths are anchored through `pushd` to the shared BAT's own directory.

`build-release.bat` selects the shared build-only Release mode without exposing
options. It rejects arguments before directory setup, tools or Python execution.
It uses the same locked synchronization, Release build and actual installed-profile
preflight as `start-release.bat --build-only`; there is no copied build logic.
This interactive entrypoint reports completion and pauses on success as well as
failure. The shared path saves the failed step's exit code before the pause and
directory restoration, then returns that code. CLI `--build-only` and `--check`
remain noninteractive. None of these build-only/check paths enters the app or
constructs an audio engine. Launcher/build/profile work remains outside the
realtime callback, with no callback I/O, Python/GIL access or blocking.

Windows subprocess tests execute the real BAT files against a temporary real
Python environment, a harmless Python module exposing a controlled profile,
a harmless app receipt writer, and a logging `uv.cmd`. These establish routing,
working directory, visible result/pause and exact error propagation, without
claiming native build success, GUI startup, CPAL or hearing acceptance.
Actual profile checks and builds belong to the integration owner's device-free
validation.
