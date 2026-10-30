## Implementation
- [x] Add the official launch-only Release starter and native profile preflight.
- [x] Preserve build-and-start modes and add device-free check/build-only options.
- [x] Document start modes and add Windows command-routing regression tests.

## Validation
- [x] Run the Windows launcher regressions and current lint/type checks.
- [x] Verify the actual loaded Debug/Release profile and launch-only check without an app or device.
- [x] Run official strict validation for this change.

## Double-click Release build
- [x] Add `build-release.bat` through the shared locked Release build/profile pipeline, pausing after success or failure and preserving the exit code.
- [x] Document the no-argument double-click builder and preserve the existing starter and noninteractive CLI contracts in OpenSpec.
- [x] Extend real Windows BAT regressions for success, setup/build/profile failures, invalid arguments, visible waiting and exact exit codes without an app or device.
- [x] Run focused launcher, lint/format/type checks and official strict validation for this change.
- [x] Execute the actual new builder and installed-Release `start.bat --check` without an app or device, recording exit codes and profile identity.
- [x] Complete independent nonauthor review of the final launcher code, tests, docs and spec; repair any findings and rerun affected checks.
