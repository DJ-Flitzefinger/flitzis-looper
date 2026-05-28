## 1. Specification And Planning
- [x] 1.1 Create the OpenSpec proposal, design, tasks, and spec delta for the Stage-6 alignment model.
- [x] 1.2 Document source-frame position, output-frame time, loop edits, prepared-stem alignment, and click-free follow-up sequencing.
- [x] 1.3 Run official strict OpenSpec validation for `clarify-loop-source-stem-alignment`.

## 2. Focused Tests
- [x] 2.1 Add Rust mixer coverage for live loop edits preserving source-frame position when the current frame remains inside the new loop.
- [x] 2.2 Add Rust mixer coverage for stem mask changes preserving source-frame position and loop-relative reading.

## 3. Validation
- [x] 3.1 Run focused uv-managed Rust tests for the changed mixer tests.
- [x] 3.2 Run broader uv-managed validation appropriate for this docs/spec/test-only slice.
