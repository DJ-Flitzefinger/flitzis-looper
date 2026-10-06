# G3b2a/b native adoption, current authority and precise consumers

## Native authority

Preparation captures the engine's actual loaded source pointer, content digest,
source generation, loaded rate and current request under its publication owner.
The complete channel-mean PCM is verified and the existing lossless QM pipeline
executes off the audio callback. Caller-provided stale matching metadata cannot
replace this capture. Heavy work is followed by another current-owner check.

The ticket exposes diagnostic source/PCM/timebase and raw positions for an
explicit independent count assertion. It supplies neither a musical quarter
interpretation nor automatic acceptance. Publication constructs fresh
AcceptedConstantTiming with full evidence and uses TimingAdoptionGuard under
native ownership. New requests, load/unload, cancellation and timing edits revoke
pending authority. Explicit Manual, Tap and Legacy intent rejects automatic
acceptance; returning to Automatic cannot revive prior tickets.

Queue capacity and checked identities are validated before publication mutates
authority. Failed inputs, stale ownership, exhausted counters and full queues
preserve the previously effective state. Existing request/preparation freshness
and acknowledgement patterns are reused. Callback acceptance verifies the actual
loaded source and current permit with bounded work; evidence and sample owners
must retire outside the callback. Feedback, rather than enqueue success, declares
availability.

## Exact metadata and consumption

The accepted revision is the complete G3a canonical identity, not the raw revision
or a request/preparation counter. The realtime payload carries fixed revision
bytes, the binary64 seconds-per-quarter and independently chosen signed origin.
SourceGrid consumes the period directly through its existing from_period path.
No binary32 BPM roundtrip, source-zero rewrite, physical marker rewrite or
playhead reset participates in this publication.

Ordered and parameter-ring updates must not let an older pending legacy update
override a later accepted publication. Tests exercise this real callback path.
Successful later legacy timing edits clear accepted grid authority. New analysis
requests invalidate pending adoption without making already accepted source timing
depend on control polling.

## Current acknowledgement and native consumers

Current-pad resolution uses native loaded-source ownership, explicit Automatic
intent and fixed callback acknowledgement epochs to select retained current
metadata with complete accepted revision identity. Ticket metadata remains
historical. New preparation/requests
do not erase the previous effective record; pending replacements do not become
current until callback adoption. Successful intent/source/legacy edits revoke
current authority; returning to Automatic cannot revive a revoked record.
The metadata registry owns neither strong nor weak PCM array references. Its
non-dereferenced address/extent plus checked loaded generation/digest/rate reject
stale/recycled source addresses. Full evidence and source owners remain in the
existing off-realtime ticket/retirement paths; registry pruning also occurs off RT.

Mixer source periods prefer the accepted projection over legacy parameters.
Transport stores authoritative binary64 output seconds per quarter; bootstrap,
anchor, frame-per-beat arithmetic and quantization consume it directly. Shared
OutputClock stores the exact period bits; public BPM is derived presentation.
Legacy public BPM/speed messages preserve binary64. Master admission converts BPM
once, while BPMLOCK target is source period divided by master output period.
Start/render use the same rate resolver, including existing supported clipping;
clipping does not redefine the requested master. SourcePlayback ratio/target/ramp
are binary64 and rate changes retain fractional source epochs. LiveShifter pitch
uses its native c_double ABI through a named inverse-rate conversion. Its existing
near-unity/update threshold policy remains; no sub-threshold pitch accuracy or
audible SYNC is claimed. No new physical wrap policy or DSP-history ownership is
introduced.

## Limits and remaining G3b2 consumers

This slice activates a precise native grid only through explicit control API
acceptance. Normal loading, manual/TAP controllers and project restore retain
their existing behavior. Source content is an observed loader digest; the
complete mono digest and Arc/timebase checks verify the actually loaded PCM, not
an immutable historical original-to-decoder relationship.

Editor/snap/automatic markers and presentation/controller state still need the
same current accepted period/origin/revision.
MIDI signatures, prepared source/stem identities, productive Key Lock history and
source-verified persistence still need the full accepted revision. The existing
Key Lock warmed-state pool is source-neutral; proof-only source preparation is
not productive integration. G3c retains 75/1000-cycle, fractional wrap,
callback-partition and rendered/device gates. This slice claims no audible SYNC.
