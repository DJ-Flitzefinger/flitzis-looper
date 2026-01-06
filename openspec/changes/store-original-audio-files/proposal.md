# store-original-audio-files Change Proposal

## Purpose
Change the sample caching strategy to store original audio files (MP3, FLAC, etc.) instead of resampled FP32 WAV files, and restore analysis results from project state instead of re-running analysis on startup.

## Motivation
- Decoding/resampling is computationally cheap compared to audio analysis
- FP32 WAV cache files are very large (4 bytes per sample)
- Original audio files are typically much smaller
- Running analysis on startup is expensive and unnecessary since results are already persisted

## Design Considerations
- Keep filename collision handling simple: append `_0`, `_1`, etc. when needed
- Remove blake3 hashing since we're not doing content-based caching anymore
- Ensure analysis results are restored from project state when samples are restored
- Maintain backward compatibility with existing cached WAV files during transition

## Scope
This change affects:
- Sample loading and caching logic in Rust (`rust/src/audio_engine/sample_loader.rs`)
- Project state model (`src/flitzis_looper/models.py`)
- Loader controller (`src/flitzis_looper/controller/loader.py`)
- Project persistence and restoration logic

## Capabilities
This change will:
1. Store original audio files in `./samples/` instead of resampled WAV files
2. Remove blake3 hasher dependency
3. Handle filename collisions with simple numeric suffixes
4. Restore analysis results from project state instead of re-running analysis on startup
5. Maintain all existing functionality for loading, unloading, and analysis

## Out of Scope
- Changing the analysis algorithm or library
- Adding new audio formats
- Changing the audio engine's in-memory sample format
