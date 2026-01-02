# Change Proposal: Async Sample Loading with Background Thread and Progress Updates

**Change ID**: async-sample-loading  
**Status**: Proposed  
**Authors**: [Your Name]  
**Date**: 2026-01-02  

## Summary
Currently, `load_sample` executes on the Python thread, performing disk I/O, decoding, and FFT resampling sequentially. This blocks the Global Interpreter Lock (GIL) and freezes the ImGui UI (`imgui_bundle`). To eliminate UI freezing, we propose moving the heavy loading work to a background worker thread while maintaining real-time responsiveness through event notifications back to Python.

## Motivation
- **UI Freeze**: The current synchronous `load_sample` call blocks the main Python thread, causing UI unresponsiveness.
- **User Experience**: Users expect to see progress indicators and remain able to interact with the UI during sample loading.
- **Scalability**: Background loading enables better utilization of multi-core systems and prepares the system for future features like BPM analysis.

## Goals
1. **Decouple Loading from UI Thread**: Move disk I/O, decoding, and resampling to a dedicated worker thread.
2. **Progress Reporting**: Provide status updates (e.g., "Progress: 50%") to the UI.
3. **Non‑blocking API**: `load_sample_async(id, path)` returns immediately.
4. **Thread‑Safe Communication**: Use a channel to push `LoaderEvent` messages from Rust to Python.
5. **Integration with Audio Engine**: Loaded samples are pushed to the audio ring buffer for playback.

## Out of Scope
- **Real‑time Constraints**: Detailed real‑time scheduling or guarantee of sub‑millisecond latency (future work may address this).
- **Advanced Analytics**: BPM analysis or other post‑processing steps are mentioned but not required for the initial implementation.
- **Thread Pool Management**: Using a custom thread pool instead of `std::thread` is deferred.

## Assumptions
- The `AudioEngine` struct can be extended with a `Sender<LoaderEvent>` to broadcast events.
- Python can periodically poll a `poll_loader_events()` method without blocking.
- Existing audio pipeline (ring buffer, mixer) remains unchanged.

## Dependencies
- **Rust**: Standard library concurrency primitives (`std::thread`, `std::sync::mpsc`).
- **Python**: New method `poll_loader_events()` on the `AudioEngine` PyO3 wrapper.
- **UI Layer**: ImGui code to call `poll_loader_events()` each frame and update UI accordingly.

## Risks
- **Thread Safety**: Ensuring that data moved into the worker thread is `Send` and that shared state is properly synchronized.
- **Error Propagation**: Errors during loading must be communicated back to Python to avoid silent failures.
- **Performance Overhead**: Minimal overhead from channel operations; measured during implementation.

## Implementation Approach (High‑Level)
1. **Define `LoaderEvent` enum** in Rust for UI updates (Started, Progress, Success, Error).
2. **Extend `AudioEngine`** with a `loader_tx` (Sender) and `loader_rx` (Receiver) pair.
3. **Add `load_sample_async`** method that spawns a background thread to perform loading steps.
4. **In Worker Thread**:
   - Send `Started` event.
   - Perform disk read, decode, resample.
   - Push resulting sample to audio ring buffer.
   - Send `Success` or `Error` event.
5. **Python UI Loop**: Call `audio_engine.poll_loader_events()` each frame to retrieve events and update UI (e.g., enable Play button, update progress spinner).
6. **Update Documentation** to reflect new API and concurrency model.

## Stakeholder Impact
- **End Users**: smoother UI experience, responsive controls during sample loading.
- **Developers**: New concurrency pattern to maintain; existing `load_sample` usage unchanged except for async variant.
- **Testing**: Additional integration tests for background loading and event handling.