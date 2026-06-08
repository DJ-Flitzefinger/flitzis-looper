# Design: Realtime callback safety preparation

## Callback command budget

The audio callback processes at most a fixed number of control messages per invocation. Messages
that remain in the control-to-audio ring stay queued for later callbacks. This converts
producer-side UI/MIDI/parameter bursts from unbounded callback work into bounded per-callback work.

This stage does not add command prioritization or continuous-parameter coalescing. Those remain
the next command/parameter architecture stage.

## Oversized callback blocks

The mixer keeps its existing preallocated per-voice stretch buffers. The render entrypoint splits
larger output slices into chunks sized so the maximum supported tempo ratio still fits the
preallocated input buffer. Scheduled event splitting remains in `audio_stream.rs`; the mixer
chunking is an additional guard inside each scheduled segment.

## Buffer retirement

Sample and prepared-stem data continue to cross the control ring as shared immutable handles.
When the callback removes or rejects a handle, it moves the handle to a bounded retirement queue
drained by a non-audio worker thread. The worker performs the actual `Arc` drops outside the audio
callback.

If the retirement queue temporarily has pending backlog, the callback avoids popping additional
handle-retiring control messages when possible. The queue and backlog are fixed-size and
preallocated before the stream starts.

## Realtime constraints

The callback must still avoid disk I/O, JSON reads/writes, Python/GIL access, UI calls, blocking
locks or waits, logging, neural inference, plugin loading/scanning, unbounded loops, heavy
allocation, and long-running work.

## MIDI

MIDI remains architecturally acceptable for this stage: capture and mapping dispatch stay outside
the CPAL callback. This change only bounds how many resulting audio control messages the callback
handles per invocation.
