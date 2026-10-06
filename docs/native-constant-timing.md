# Native accepted timing adoption

G3b2a/b/c/d connects the G3a accepted record to actual loaded-pad ownership, current
acknowledged authority, native SourceGrid, transport/output clock and playback
rate. This is an explicit control API. Normal loading and analysis,
manual/TAP controllers and saved legacy projects retain their existing routing;
their numerical values are not promoted to accepted evidence.

## Explicit preparation and acceptance

`AudioEngine.set_pad_timing_intent(sample_id, intent)` accepts `automatic`, `manual`,
`tap` or `legacy`. Automatic preparation requires explicit Automatic intent.
Returning to that intent after another edit cannot revive a prior ticket.

`prepare_constant_timing(sample_id, timing_error_halfwidth_seconds,
timing_error_provenance)` captures the actual loaded source and current native
request, digest, generation and loaded rate. It creates verified complete
channel-mean PCM and executes the shared native lossless QM pipeline off the audio
callback. Its opaque `ConstantTimingTicket` cannot be constructed from Python
metadata or persisted. `metadata()` exposes source/PCM/job/timebase and complete
raw evidence for independent count assessment. Preparation does not accept a
musical interpretation or change the default estimator.

`publish_constant_timing(ticket, hypotheses_json, origin_seconds,
origin_provenance, acceptance_policy_version, acceptance_provenance)` recomputes
the supported raw fit and constructs `AcceptedConstantTiming`. The hypotheses
JSON is a list of objects with `id`, `provenance`, `verification` (`verified` or
`unverified`), `quarter_note_denominator` and `quarter_counts` (integer numerators
or null for explicit exclusions). Counts correspond to every original raw event;
the API does not derive them from array indices or insert a missing source-zero
attack. The denominator expresses the chosen musical interpretation explicitly.
The same G3a support, ambiguity and uncertainty rules apply.

Verification and acceptance are explicit caller assertions with named provenance.
Their consistency is checked; their independent musical truth is not established
by a good fit. This API supplies no new musical labels, general refinement policy
or automatic acceptance threshold. Timing-error provenance and selected signed
origin remain separate from the fitted diagnostic intercept and source zero.

## Publication ownership and feedback

Native current source/request/intent ownership is checked after preparation and
again during publication under the request owner. Matching stale caller snapshots
do not establish current-pad validity. The production caller uses
`TimingAdoptionGuard` and retains complete evidence outside realtime processing.
Existing request/epoch ownership invalidates pending work on load, unload, new
analysis requests, cancellation and successful timing edits, including same-value
edits. Explicit Manual, Tap and Legacy prevent automatic publication.

Ordinary Analyze admission also advances the shared request/epoch. Late normal
analysis results cannot replay legacy timing after a newer request. A still-current
source's load completion retains path/duration/loading bookkeeping while marking
retired timing with `timing_stale`; Python then skips old analysis, automatic grid/
loop initialization and legacy restore replay. Retired analysis completions settle
their matching task without clearing a newer task. Replaced-source events are
discarded and polling continues past retired events. Preparation rejects pads
that are still loading or have active native tasks.

Checked counters and queue reservations precede state mutation. Full rings and
invalid/stale inputs retain previously effective timing. The callback uses bounded
source-pointer and atomic-permit checks. Its payload retains exact binary64
period/origin and fixed bytes of the complete accepted revision. Large sample
pins and evidence retire outside the callback through existing retirement paths.
Earlier queued legacy parameter updates cannot erase a newer accepted projection;
a successful later legacy edit revokes it.

`ticket.publication_status()` distinguishes `captured`, `pending`, `accepted` and
`rejected`. Enqueue is pending, not effective availability.
`ticket.accepted_metadata()` becomes available only after actual mixer acceptance.
It describes that ticket's historical adoption; it is not a current-pad polling
authority after later source or timing edits. Polling observes acknowledgement
and never drives audio progression.

`AudioEngine.current_constant_timing(sample_id)` resolves current metadata from
the actual loaded source Arc/digest/generation/rate, Automatic timing authority
and fixed callback acknowledgement. It includes the complete accepted revision,
accepted request identity, original/mono content identity, source zero, exact
period, independent signed origin and acceptance provenance. The native retained
record owns no PCM allocation, including weak array ownership. A non-dereferenced
source address/extent is checked with native monotonic loaded generation, digest
and rate, so recycled addresses cannot substitute for current source ownership.
Historical `ticket.accepted_metadata()` can remain available after this resolver
returns `None`. New requests and pending replacement retain the previous effective
revision until actual adoption. A changing acknowledgement during a lookup returns
unavailable for that poll. Successful Manual/Tap/Legacy edits revoke authority even
while the callback clear waits; returning to Automatic cannot revive old timing.
Full queues and failed edits preserve the previous current result. Retention and
pruning occur outside realtime processing.

The mixer creates SourceGrid directly from the accepted period multiplied by the
actual loaded rate and the independent signed origin. It does not derive the
period through binary32 BPM. Publication leaves source zero, stored physical loop
endpoints and active fractional source progression intact.

## Source verification limits

The complete mono PCM digest verifies the actual loaded sample bytes under the
existing arithmetic-channel-mean rule. Arc, generation and loaded timebase checks
bind that PCM to the current native pad. The original digest remains the loader's
observed file-content association. Decode-before-copy and path replacement/ABA
limits are unchanged: this does not prove an immutable original was the exact
decoder input. Copy-first ownership and a full versioned PCM cache remain C1.
The loaded buffer is resampled/channel-mapped at the existing engine rate;
original, loaded and QM analyzer timebases remain distinct.

## Native shared-period and rate consumers

Native source timing prefers the acknowledged accepted period over legacy BPM.
Transport retains authoritative binary64 output seconds per quarter. Its bootstrap,
phase anchor, beat progression and quantization use that period directly. The shared
OutputClock stores exact period bits; `output_clock_snapshot()` exposes
`master_period_seconds` and derives its compatibility `master_bpm` for presentation.
Legacy public speed, master BPM and pad BPM retain binary64 through messages.

BPMLOCK derives one target as `source_period / master_output_period`; start and
render share that resolver. Global speed applies when unlocked or timing is
unavailable. Supported speed clipping cannot redefine the requested master period.
SourcePlayback rate, target and smoothing preserve binary64 and rebase from the
existing fractional source position. The same cursor supplies full mix and stems.
Rubber Band receives a named inverse-rate conversion through its native `c_double`
pitch ABI. Its existing near-unity and update-threshold behavior remains; this
does not establish sub-threshold pitch application, prepared DSP-history revision
binding or audible SYNC. Physical integer-loop wrapping is unchanged.

Regression tests exercise actual callback drains and rendering/bootstrap, including
legacy BPM deliberately disagreeing with accepted timing, replacement while playing,
rate clipping, precise long source epochs and a quantization boundary where a
binary32 BPM projection would choose the wrong output frame.

## Python current consumers and global controls

`BpmController.current_timing()` resolves a frozen source period/origin/loaded-rate
snapshot with full accepted revision and current source/adoption identity and
provenance. Manual project intent takes priority, followed by native current
acknowledgement and ordinary Legacy analysis. It never reads historical ticket
metadata. `AudioEngine.pad_timing_intent()` exposes declared authority without
claiming acknowledgement; Automatic with no current record stays unavailable.

Scalar grid, waveform editor lines/view margin/loop-relative labels and loop
snap/clamp/bar duration/maximum-auto/effective endpoints use the exact period.
One operation carries the same resolved snapshot, including unavailable `None`.
Signed accepted origins retain their fractional source position. Physical marker
conversion rounds each absolute boundary once at the actual loaded rate.
ALL, maximum auto-loop length and waveform navigation/query clipping use the
accepted full frame extent rather than stale saved duration. The UI waveform
cache/view key includes native source identity/generation/digests/extent/rate;
timing-only revisions of the same source preserve navigation. This bounded UI
cache binding does not implement the future C0-C3 PCM cache/residency design.

Passive restore/refresh/BPMLOCK activation skip legacy BPM and origin writes for
Automatic authority. Unavailable automatic restore preserves native loop/master
state. Intentional Manual/Tap edits retire acceptance, and final native intent
follows legacy numeric/origin publication. Clearing manual or editing a legacy
grid offset resumes existing Legacy analysis/grid policy without caching accepted
evidence; accepted origin is not copied into saved legacy base intent. Failed
first numeric admission preserves the saved BPM/offset intent. Validated load,
unload/reset and ordinary analysis completions restore Legacy before derived
callbacks; pending or retired timing_stale work cannot revoke current acceptance.

Accepted master control publishes `current_source_period / speed` directly through
`AudioEngine.set_master_period()`. This parameter and compatibility BPM share one
coalesced lane with last-admitted ordering. Session master period/revision records
the last successful publication, while display BPM is derived from period. Speed,
BPM target and nudge reuse one source snapshot; unavailable Automatic anchors
reject explicit speed changes before saved speed or native speed is changed.
`set_speed_and_master_period()` admits an accepted locked speed change as one
bounded parameter effect, so a full ring cannot admit only half the update.

Accepted publication remains an explicit native control API. The integration
caller refreshes derived Python loop/master controls after acknowledgement via
`on_pad_bpm_changed()`. An acknowledged replacement alone does not automatically
republish physical auto-loop endpoints/master intent; read-only UI polling never
drives correction. A normal analyzer/acceptance orchestrator is not added here.

## MIDI current source and authority

`current_input_runtime_pad_binding(sample_id)` captures an opaque native source
and authority snapshot outside the callback. Its metadata names the actual source
generation, digest, loaded rate, full extent and channels, declared timing intent
and authority revision. Its optional `accepted_timing` is resolved through the
same current acknowledged record as `current_constant_timing`, with complete
accepted revision, period, signed origin and provenance. It owns no PCM buffer;
caller metadata cannot construct or persist this binding.

Python carries one frozen `CurrentPadTiming` through effective loop calculation
and signature comparison, checks it against that native binding and publishes the
binding with runtime loop intent. Equal BPM/endpoints do not hide a different
source, full accepted revision or same-value authority edit. An unavailable or
inconsistent Automatic snapshot disables the pad's direct runtime until a fresh
matching publication; it does not replay saved analysis or clear the live loop.

Direct MIDI loop intent and launch are one fixed guarded command. Native refresh
revalidates current ownership before replacing the runtime. Callback/scheduled
execution rechecks source identity, declared authority and acknowledged accepted
projection before applying loop intent or exclusive playback. A newer effective
runtime loop invalidates old queued/scheduled loop intent. Successful source or
timing edits invalidate old bindings immediately, including Manual/Automatic
roundtrips. New preparation requests and pending/rejected replacements retain the
previous effective accepted record. No PCM scans, evidence allocations, locks,
Python or I/O enter the callback.
Scheduler capacity is allocated once on the heap before callback construction;
the larger guarded event does not create a large inline startup stack temporary.
Scheduling and execution reuse that fixed storage without callback allocation.

Failed direct pad-trigger events refresh current runtime and retry through
`trigger_input_runtime_pad`, retaining the captured timestamp and guarded
all-or-nothing semantics. They do not use the ordinary unguarded Python loop/play
sequence. A full queue, unavailable Automatic state or stale binding admits no
partial loop or launch. Enqueue feedback still reports admission rather than
audible or scheduled-execution acceptance; a subsequently retired trigger is
discarded at execution. MIDI stop-all remains a global action.

Runtime publication changes dormant input intent; polling does not publish accepted
timing, refresh master controls or change a live loop. Explicit accepted adoption
and subsequent caller-owned loop/master refresh remain the orchestration gate.
Controller-owned global START/STOP (including its MIDI mapping) retains ordinary
Python batch launch semantics; it does not consume RuntimePadState bindings. A
source/accepted-bound global batch launch and its refresh/adoption orchestration
remain explicit follow-up consumers. This slice covers runtime pad triggers and
their fallback, and does not claim all controller launch paths are guarded.

## Remaining shared-period and loop work

Pending prepared source/stem state and productive Key Lock
history still need full accepted revision binding. The generic warmed Key Lock
pool is source-neutral; test-only source preparation is not a production consumer.
Accepted persistence needs source verification and explicit manual/TAP/legacy
policy; no opaque ticket is a saved identity and no saved legacy BPM is evidence.

G3c remains separate: musical period versus rounded physical loops over 75/1000
cycles, nonintegral periods/rates and callback partitions/wrap. Numerical grid
tests do not pass rendered DSP, onset, device or sustained audible SYNC gates.

See [accepted record identity](accepted-constant-timing.md),
[prepared stem ownership](prepared-stem-publication.md) and
[source coordinates](scalar-source-coordinates.md).
