# Design: Store Original Audio Files

## Overview
This design changes the sample caching strategy from storing resampled FP32 WAV files to storing original audio files, and restores analysis results from project state instead of re-running analysis on startup.

## Cache File Strategy

### Filename Generation
1. Use the original filename stem as the base name
2. Use the original file extension (mp3, flac, etc.)
3. On collision, append `_0`, `_1`, etc. until a unique name is found

Examples:
- `my_loop.mp3` → `samples/my_loop.mp3`
- Collision → `samples/my_loop_0.mp3`, `samples/my_loop_1.mp3`, etc.

### Collision Handling Algorithm
```
base = project_samples_dir.join(format("{stem}{ext}"))
if base.exists():
    index = 0
    loop:
        candidate = project_samples_dir.join(format("{stem}_{index}{ext}"))
        if candidate.exists():
            index += 1
            continue
        else:
            use candidate
else:
    use base
```

## Analysis Restoration

### Current Flow (Problem)
1. Sample is loaded from original file
2. Sample is cached as WAV
3. Analysis is run
4. Results are stored in project state
5. On restart: WAV is loaded, then analysis is run again (expensive!)

### New Flow
1. Original file is copied to `./samples/`
2. Sample is decoded from the cached original file
3. Analysis is run
4. Results are stored in project state
5. On restart: Original file is loaded, analysis results are restored from project state

### Restoration Logic
When restoring a project:
- Load the sample from the cached original file
- If analysis results exist in `ProjectState.sample_analysis[id]`, use them
- Do not run automatic analysis after restoration
- Manual analysis can still be triggered to re-run detection

## Backward Compatibility

### Migration Strategy
We do not need to care about backward compatibility or old project folders.

### Project State Compatibility
The `ProjectState.sample_paths` field will now contain paths to original file names (inside `./samples` folder) instead of WAV files. We do not need to care about backward compatibility.

## Error Handling

### Collision Handling
- If all suffixes up to `_999` are taken, return an error
- This is extremely unlikely in practice

### Restoration Errors
- Missing cached file: ignore the pad assignment
- Unreadable cached file: ignore the pad assignment
- Unsupported file format: ignore the pad assignment
- All errors are non-fatal; the UI remains usable

## Performance Considerations

### Disk Space
- Original files are typically 3-10x smaller than FP32 WAV
- Example: 1 minute of stereo audio
  - MP3: ~1 MB
  - FP32 WAV: ~10 MB

### Startup Time
- No analysis runs on restoration
- Only decoding (cheap) instead of decoding + analysis (expensive)
- Analysis only runs when explicitly triggered or when loading new samples

### Load Time
- Decoding is still required (unchanged)
- Resampling is still required if sample rate doesn't match (unchanged)
- Analysis is skipped on restoration (significant improvement)

## Security Considerations
- Still validate that cached paths are under `./samples/`
- Still validate file format and sample rate on load
- No new attack vectors introduced

## Testing Strategy
- Unit tests for filename collision handling
- Integration tests for project restoration
- Tests for backward compatibility with existing WAV caches
- Performance tests to verify analysis isn't run on restoration
