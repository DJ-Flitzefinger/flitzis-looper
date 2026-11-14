# Use ONNX Runtime for Windows HTDemucs

## Why

The Windows x64 HTDemucs selection needs a qualified standalone ONNX worker for the pinned
stock checkpoint. The existing separator defines the decoder, model and output reference.

## Impact

- Qualify both B1 decoder branches and the full-track host path on CPU and CUDA.
- Preserve the captured separator, device policy, shifts, overlap, and per-pad adoption.
- Stage derived resources for source, Release and Portable with a verified rollback path.
