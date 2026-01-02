# Remove Old `load_sample` Method

## Overview

This change proposal removes the old synchronous `load_sample` method from the Rust AudioEngine and all related Python code, replacing it with the new asynchronous `load_sample_async` implementation.

## Background

The audio engine previously provided both synchronous (`load_sample`) and asynchronous (`load_sample_async`) methods for loading audio samples. The synchronous version has been deprecated in favor of the asynchronous approach which provides better responsiveness and prevents blocking the main thread during file I/O operations.

## Requirements

### ADDED
- No new functionality is being added, but existing functionality will be simplified by removing redundant code paths

### MODIFIED
- Rust `AudioEngine` struct: Remove `load_sample` method implementation
- Python type stub (`__init__.pyi`): Remove `load_sample` signature from `AudioEngine` class
- Test files: Update to use only `load_sample_async`

### REMOVED
- Rust `load_sample` method implementation in `src/audio_engine/mod.rs`
- Rust `sample_loader` module (since it's not used by the new async approach)
- Python code that references the old `load_sample` method

## Implementation Plan

1. Remove `load_sample` method from Rust AudioEngine
2. Remove `sample_loader` module since it's no longer needed
3. Update Python type stub to remove `load_sample` signature
4. Update all test files and usage examples that reference the old method
5. Verify that existing functionality using `load_sample_async` remains intact