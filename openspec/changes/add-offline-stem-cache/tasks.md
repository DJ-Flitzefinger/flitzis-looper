## 1. Specification And Planning
- [x] 1.1 Create the OpenSpec proposal, design, tasks, and spec deltas for offline/cached stems.
- [x] 1.2 Update relevant planning documentation without implementing production feature code.
- [x] 1.3 Run `openspec validate add-offline-stem-cache --strict`.

## 2. Stem Cache Model
- [x] 2.1 Define a project-local stem cache layout and source-version identity.
- [x] 2.2 Represent the expected stem kinds: vocals, melody, bass, drums, and instrumental.
- [x] 2.3 Mark cached stems unavailable when the source pad is unloaded or replaced.
- [x] 2.4 Treat stale cache use for a replaced pad as a blocker before merge.

## 3. Background Stem Generation
- [x] 3.1 Add a manual per-pad background stem generation task.
- [x] 3.2 Reject or defer generation when the pad is currently playing, loading, unloading, or running a conflicting task.
- [x] 3.3 Ensure neural inference, disk I/O, temporary files, and heavy allocation run only outside the audio callback.
- [x] 3.4 Report progress and errors without freezing the UI.
- [x] 3.5 Add Python/controller tests for task gating, progress, failure, and stale-source behavior.
- [x] 3.6 Write deterministic aligned stem cache artifacts outside the audio callback without Rust publication or mixer changes.

## 4. Prepared Stem Publication
- [x] 4.1 Validate generated stem buffers for sample rate, channel layout, frame origin, and usable length before publication.
- [x] 4.2 Publish prepared immutable stem buffers to Rust by fixed-size control message and shared buffer handles.
- [x] 4.3 Reject stale generation results if the pad started playing or the source version changed before publication.
- [x] 4.4 Add Rust/Python tests for fixed-size publication, ring-buffer-full failure, and stale-generation rejection.

## 5. Prepared Stem Mixing
- [x] 5.1 Add fixed per-pad/per-stem storage in Rust audio-thread state.
- [x] 5.2 Mix prepared stems using the same voice playhead, loop region, transport timing, BPM-lock, and key-lock behavior as full-mix playback.
- [x] 5.3 Fall back to full-mix playback when stems are missing, stale, incomplete, or disabled.
- [x] 5.4 Treat any audio-thread disk I/O, Python/GIL access, logging, blocking operation, heap allocation, neural inference, or long-running work as a blocker.
- [x] 5.5 Add deterministic Rust mixer tests for synchronization, fallback, and real-time-safe state transitions.

## 6. Future UI And Persistence Follow-up
- [ ] 6.1 Design performer-facing stem availability indicators and controls in a separate OpenSpec slice.
- [ ] 6.2 Design project persistence for stem cache metadata after the cache identity is implemented.

## 7. Validation
- [x] 7.1 Run official OpenSpec validation for this change before implementation is considered complete.
- [x] 7.2 Run focused Rust and Python tests for any implementation slice that changes code.
- [x] 7.3 Run full uv-managed Rust/Python validation before merging production stem behavior.
