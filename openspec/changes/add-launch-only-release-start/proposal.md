# Start an installed Release without rebuilding

## Why
The normal Windows start currently synchronizes dependencies and rebuilds the
native extension. The user needs an official starter for the existing Release.

## What Changes
- Add `start.bat` using the existing project Python environment directly.
- Check the actual loaded native build profile before starting the app; report
  missing, older or Debug builds with a concrete Release-build instruction.
- Preserve the Debug/Release build-and-start commands and add device-free
  `--check` and `--build-only` modes.
- Document the official normal start and test command routing and failures.

## Non-goals And Realtime Constraints
This change does not package an installer, create a second native installation,
launch an app in validation, or modify audio processing. Profile checking imports
the native module without constructing an engine or opening an audio device.
The realtime callback performs no launcher, filesystem, Python or build work.
