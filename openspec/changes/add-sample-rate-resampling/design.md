## Design: Sample Rate Resampling

## Context
- The audio engine currently requires all audio files to match the output device's sample rate exactly
- Sample rate mismatches result in load errors, which is user-unfriendly
- Resampling must happen on the Python thread (non-real-time) to maintain audio thread safety
- The `rubato` crate provides high-quality, real-time-safe resampling algorithms

## Goals / Non-Goals
- **Goals**: 
  - Support loading audio files with any sample rate
  - Maintain high audio quality through the resampling process
  - Keep the audio thread real-time safe (no allocations or blocking)
  - Minimize CPU usage during resampling
- **Non-Goals**:
  - Real-time sample rate conversion during playback
  - Sample rate conversion for the audio output device
  - Supporting sample rate changes after loading

## Decisions
- **Resampler choice**: Use `rubato::FftFixedInOut` for synchronous resampling since:
  - It provides excellent quality
  - It's significantly faster than sinc resamplers for fixed ratio conversion
  - The resampling ratio is fixed (file_sample_rate → output_sample_rate)
  - We can pre-calculate exact buffer sizes
- **Processing location**: Perform resampling in `decode_audio_file_to_sample_buffer` on the Python thread, so:
  - No impact on real-time audio processing
  - Can allocate memory freely during conversion
  - Errors can be propagated to Python gracefully
- **Chunk size**: Use a chunk size of 1024 frames for the resampler, which provides:
  - Good balance between memory usage and efficiency
  - Works well for typical audio file sizes
- **Channel mapping**: Perform channel mapping after resampling to avoid unnecessary operations

## Risks / Trade-offs
- **CPU usage**: Resampling adds CPU overhead during file loading, but:
  - Loading is already a non-real-time operation
  - Modern CPUs can resample audio much faster than real-time
  - This is a one-time cost per file load
- **Memory usage**: Resampling requires temporary buffers, but:
  - Buffers are allocated on the heap during loading (not in audio thread)
  - Memory is freed after conversion completes
  - Total memory usage is bounded by the file size
- **Quality**: FFT resampling provides very high quality, suitable for professional audio applications

## Open Questions
- None currently identified
