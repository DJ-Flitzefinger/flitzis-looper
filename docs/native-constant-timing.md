# Native accepted timing adoption

G3b2a connects the G3a accepted record to actual loaded-pad ownership and the live
native SourceGrid. This is an explicit control API. Normal loading and analysis,
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

## Remaining shared-period and loop work

This completes native accepted adoption and SourceGrid only. Editor lines,
snapping and automatic endpoints still derive their scalar period from effective
BPM. Transport/master/output-clock timing and BPMLOCK still use legacy binary32
BPM/rates. They must consume the same accepted period/origin/revision, with one
ratio and preserved fractional source epochs, in subsequent G3b2 work.

MIDI signatures, pending prepared source/stem state and productive Key Lock
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
