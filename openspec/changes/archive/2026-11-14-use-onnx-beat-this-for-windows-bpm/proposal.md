# Use ONNX Runtime for Windows automatic BPM

## Why

Windows automatic BPM and Analyze can use the verified native implementation of
BeatThis with an isolated worker and a persistent CUDA kernel cache.

## Impact

- Both analysis entry points use the same pinned FP32 ONNX model and the existing
  complete-source timing and publication contracts.
- Source and Portable setup verify the native package and prepare CUDA for the
  installed device and driver. CPU uses the same graph.
- Temporary BPM admission follows checked allocation geometry and actual host
  resources. KeyNet and playback have independent admission rules.
