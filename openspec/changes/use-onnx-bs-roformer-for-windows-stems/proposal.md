# Use ONNX Runtime for Windows BS-RoFormer MUSDB18HQ

## Why

Windows needs a native inference worker for the selected BS-RoFormer MUSDB18HQ
separator. The existing Looper implementation defines the comparison reference
for model weights, decoded source, chunking, reconstruction and stem alignment.

## Impact

- The captured BS-RoFormer selection uses an external ONNX worker on Windows
  x64 after CPU and CUDA qualification. HTDemucs and its settings are independent.
- The job keeps the existing source lease, cancellation, private generation,
  output alignment, persistence and playback adoption contracts.
- Setup derives model artifacts from the pinned upstream release and stages a
  verified worker and runtime. Existing release resources support rollback.
