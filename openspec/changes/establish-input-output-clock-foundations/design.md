## Context

The permanent output-frame transport and bounded scheduler already exist. MIDI records time in
a private epoch created after stream startup, then loses that field at direct dispatch; Python
fallback ignores it. UI launch paths are unstamped. CPAL callback metadata is currently ignored.

## Goals / Non-Goals

Goals are coherent time provenance, preserved launch timestamps and testable estimated device
mapping with nearest-target diagnostics. Current launch execution stays unchanged. Source phase,
signed manual grids, session bootstrap, audible Key-Lock preparation and launch activation belong
to the following bounded slices. No project format or dependencies change.

## Decisions

### One engine epoch and transient command metadata

Create the Rust epoch before stream creation. Share it with native MIDI and expose capture through
the UI action facade. Keep two-argument native playback callers compatible; stamped callers use
the optional keyword. Retain the stamp in scheduled events even though current execution does not
use it to choose timing. Python monotonic time is unsuitable because it has a different origin.

### Bounded estimated device mapping

Observe shared-epoch time at callback entry. Add the checked CPAL playback-minus-callback duration
to estimate the audible time of its first output frame. Publish frame/time/rate/grid fields through
coherent bounded atomics, with no callback retries. Control readers try once. Reject implausible
delay, staleness and discontinuity. Never change the transport to conceal device buffering.

This uses driver timestamps and host callback observation; jitter and driver precision remain
measurement limits. Actual impulse-loopback alignment and DSP delay accounting are later acceptance
work. A wall-clock scheduler or Python-owned device clock would duplicate realtime authority.

Specifically, CPAL 0.17 WASAPI estimates playback time using available buffer frames and documents
unknown latency after the buffer. It does not expose an actual audible cursor through this API.
The estimate must be checked against audible/loopback measurements before synchronized-launch
activation; this foundation cannot establish precise pending-device-frame ownership by itself.
See the [CPAL backend source](https://github.com/RustAudio/cpal/blob/v0.17.0/src/host/wasapi/stream.rs).

### Nearest-grid diagnostics, current launches

The focused Rust timing module maps a captured timestamp to an output frame and chooses the nearest
grid boundary, with midpoint ties toward the future. Equivalent fresh observations of the same
steady timeline produce the same answer independent of processing delay/callback partition.
Missing or invalid data returns no result rather than a newly captured input time.

Diagnostics exercise the future timing policy without enabling partially prepared phase/DSP work.
Current future-only scheduling and loop-start playback continue to satisfy existing specs.

### Intended synchronized launch contract for subsequent slices

Activation will replace the existing future-only/always-loop-start contract in an explicit delta:

- Captured input chooses the nearest `1/16`, `1/32` or `1/64` boundary; ties go future. Already
  rendered/buffered frames are immutable, so a missed target uses the earliest controllable audible
  frame and preserves the original musical target.
- Quantize + BPMLOCK with valid metadata chooses target and source phase together using the SAME
  manual BPM, signed Adjust Loop grid origin and effective loop region. Source at audible entry is
  the phase at target plus musical progress to entry, wrapped through the effective loop. Future
  boundaries can also require a source position other than loop start.
- With a valid master grid but missing pad phase/BPMLOCK, target selection is unchanged; source at
  target is loop start, and late entry advances at actual playback rate. Unequal tempos need
  BPMLOCK for lasting synchronization. Invalid master timing uses immediate loop-start fallback.
- Quantize off starts at effective loop beginning as soon as technically controllable, without
  added grid waiting or phase catch-up. Device and DSP delay remain unavoidable until accounted for.
- A one-time session/master bootstrap reuses selected-pad/BPMLOCK reference semantics. Silence,
  other-pad metadata, normal triggers/stops, loop wraps and stem masks never reanchor the clock.
  BPM changes preserve musical phase. All stems share source addressing and audible-delay handling.
- Persisted markers do not change. Prepared constant-tempo material and compatible loops are the
  scope; live correlation, variable-tempo warping and a new SYNC control are excluded.

Signed phase/bootstrap and pending starts across BPM/metadata changes must be specified/tested in
slice 2; safe Rubber Band pre-roll and measured audible delays in slice 3 precede activation in 4.

The subsequent causal content proof distinguishes the selected musical target T, earliest
controllable estimated audible frame E (including preparation readiness), and first permitted
emission S. They are not interchangeable: strict first-sound gating uses max(E,T), while preserving
some continuous native pre-target content requires S<T. Earlier audible emission is an unresolved
product decision, not an authorization implied by captured timestamps or late phase catch-up.
Insufficient headroom cannot silently move T to a later boundary. See the Key-Lock preparation
change and backend guide for the fixed-case evidence. Foundation diagnostics/execution stay intact.

## Risks / Trade-offs

- Driver timestamp jitter or discontinuity → validity/freshness checks, diagnostics labelled
  estimated, and later audible hardware acceptance.
- UI can only observe accepted inputs at its frame boundary → capture there before controller work;
  never imply an earlier OS timestamp is available.
- Clock outage → unavailable diagnostic without rerounding or affecting current playback.
- Direct MIDI queue failure → original timestamp survives Python fallback and successful direct
  events stay deduplicated.
- Large existing audio modules → a focused timing module; retain established scheduler ownership.

## Migration Plan

Audit and record a reproducible installed-release baseline. Introduce timestamp propagation and
clock diagnostics, extend meaningful regression tests, update maintained docs and pass official
strict OpenSpec plus full Rust/Python checks. Keep generated audio/results local. Rollback removes
the transient timing fields/APIs without project migration. This change does not deploy or commit.

## Open Questions

Audible hardware precision and DSP delay are intentionally unverified here. Subsequent slices must
resolve them before synchronized-launch activation; passing source/clock tests alone is insufficient.
