## Implementation

- [x] Add a focused constant-memory background PCM activity detector and native
  load-success metadata in the loaded frame domain; cover channel/rate/quiet/fade cases.
- [x] Use valid candidates for genuinely new assignments through shared loop
  initialization; preserve restore, manual markers, grid/BPM and stale-result guards.
- [x] Add meaningful Rust and Python regressions and update maintained docs.
- [x] Run full builds/Rust/Python/lint/type checks and official strict validation;
  verify release startup and a leading-silence load without musical-accuracy claims.
