# Native dependency overlay

The Windows source and Portable builds select `vcpkg-overlay` explicitly with the
documented pinned vcpkg revision. The overlay retains libsamplerate 0.2.2 and its
verified upstream archive, BSD-2-Clause notice, converter quality and ABI.
RubberBand 4.0.0 continues to use SLEEF and libsamplerate.

`stereo-controls.patch` reuses the stereo converter's increment, scale and input
step within one processing call while the exact ratio stays unchanged. The first
calculation remains after the original input and termination checks. A changed
ratio recomputes the original expressions. Filter coefficients, tap order,
per-frame position updates, history, delay, reset and clone state are unchanged.
There is no unity-ratio bypass, new persistent cache or fast-math option.

Use the normal precise floating-point environment. The patch does not establish
equivalence under a rounding mode changed during processing, concurrent use of
one state, or an unqualified platform/toolchain. System-package Linux/macOS builds
retain their selected dependencies; Windows observations do not certify them.

The full overlay is tracked source and accompanies Portable source recovery.
Changing it invalidates the supplied Native source-commit compatibility check.
Installers build a fresh dependency version; do not overwrite libraries held by a
running application or reuse an earlier binary receipt for a changed patch.
