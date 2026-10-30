# Start an installed Release and build explicitly

## Why
The normal Windows start must use the existing Release without implicit setup
or rebuilding. The user also needs a Release build that works by double-click
without shortcut arguments and leaves success or errors visible.

## What Changes
- Add `start.bat` using the existing project Python environment directly.
- Check the actual loaded native build profile before starting the app; report
  missing, older or Debug builds with a concrete Release-build instruction.
- Preserve the Debug/Release build-and-start commands and add device-free
  `--check` and `--build-only` modes.
- Add `build-release.bat` as a no-argument, build-only Release entrypoint through
  the shared locked build pipeline, with actual installed-profile verification
  and a visible result that preserves the exit code after waiting for a key.
- Document the official normal start and test command routing and failures.

## Non-goals And Realtime Constraints
This change does not package an installer, create a second native installation,
add shortcut configuration, launch an app in build-only validation, or modify
audio processing. Profile checking imports the native module without
constructing an engine or opening an audio device.
The realtime callback performs no launcher, filesystem, Python or build work.
