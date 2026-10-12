## Source-main installation

- [x] 1. Replace ordinary local/release-channel installation with the standalone
  BAT bootstrap of current remote `main`; keep bundle packaging only as an
  advanced maintainer utility.
- [x] 2. Check/acquire verified pinned Git, uv, CPython 3.14.2, Rust 1.95.0,
  vcpkg/Rubber Band and FFmpeg in installation-owned tools/caches; check/install
  missing Microsoft C++ Build Tools/Windows SDK with elevation and restart
  handling before activation.
- [x] 3. Build an exact source snapshot into a private non-editable Release
  runtime; retain serialized owned staging, archive containment, private
  preflight, atomic activation and old runtimes/persistent data on failure.
- [x] 4. Preserve simple root starters, installed-directory relocation and
  private data/model/cache paths; avoid rebuilds for unchanged healthy `main`.
- [x] 5. Add focused missing-tool, unchanged-main, changed-main, failure/data
  retention and relocation regressions; run relevant checks and record their
  actual results, strict OpenSpec validation and independent final review.
- [x] 6. Update beginner documentation and repository maintenance rules for
  installer/BAT/helper/test/docs adaptation in the same prerequisite/ABI/layout
  change; explain initial downloads, compilation, disk use and shared Microsoft
  system build components accurately.

## Actual-machine gates

- [ ] 7. Verify first-install bootstrap on clean compatible Windows x64 without
  preinstalled Git/Python/Rust, including missing Microsoft toolchain and
  applicable elevation/restart paths. Configured-machine tests do not pass this
  gate.
- [ ] 8. Human portable GUI/audio/device/listening test and any reported
  corrections. Do not infer acceptance from import/preflight tests.

Project binary Release publication is not a prerequisite or delivery gate for
this source-main installer and is not authorized by this change.
