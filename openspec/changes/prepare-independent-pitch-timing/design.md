## Status and ownership

Draft only, corresponding to B5 in `../../../docs/beatmap-sync-design.md`. Prepare the contract
and isolated diagnostics before selecting the future live
renderer. Production KEY intent remains zero; nonzero k is injected only by tests/examples.
Reuse the canonical Rust map evaluator, source reader and preparation boundaries. Do not add
another tempo controller or change the shared master clock to compensate pitch processing.

## Independent coordinates

For pad i, M(n) is master beat at absolute output frame n, B_i maps source seconds to beats,
S_i maps beats to source seconds and phi_i is intentional playback phase. With loaded source
rate Fs_i, b0=B_i(loop_start_seconds), L=B_i(loop_end_seconds)-b0>0 and Euclidean modulo:

```text
s_i(n) = Fs_i * S_i(b0 + mod(M(n) + phi_i - b0, L))  (loaded source frames)
r_i(n) = ds_i/dn              (source frames per output frame; outside explicit loop wraps)
h_i(n) = 2^(k_i(n)/12)        (KEY semitones, positive means upward)
```

k never enters M, phi, B/S, loop selection or source progression. KEYLOCK holds intended source
pitch plus k while tempo follows the source map/master; it does not undo deliberate k. The
source's detected/manual key remains metadata, not an instruction to change r or M.

Normalize both candidate paths to the same sample rate for the initial comparison. The mapped
reader advances at r; LiveShifter then uses pitch p=h/r. Full Stretcher consumes original source
with duration ratio q=1/r and independent pitch p=h. For unequal source/output rates distinguish
frame-rate conversion from physical duration ratio; do not blindly apply 1/r. Never apply the
same tempo warp both in source reads and again in Stretcher.

For future KEYLOCK-off behavior, propose native pitch p=h after varispeed, giving audible source
pitch multiplied by r*h. The corresponding full-Stretcher diagnostic uses q=1/r and pitch p=h*r
on original equal-rate source. k=0 preserves legacy varispeed. This is the later control contract's
proposal, not a new live mode here; the diagnostic covers it explicitly. KEY does not overwrite
global.speed, manual_key, manual BPM, detected key, original PCM or stem caches.

The later K1 feature must retain existing arbitrary nonempty manual_key strings rather than
treating this metadata setter as a transposition command. Relative k works when source key is
unknown. A target-key selector must define octave and tritone tie choices and preserve major/minor
mode; semitone shifting is not reharmonization. Under KEYLOCK off, varying r makes the audible
pitch vary, so an output-key label must not falsely advertise a fixed tempered key. Source
replacement resets future source-bound k to zero; restoring the same verified source retains its
saved k. These are forward constraints for K1, not a persistence/UI rollout in B5.

## Native bounds and transitions

Current `rubberband_backend.rs:75` clamps inverse-tempo pitch to [0.5,2], while current source
speed is [0.5,2]. An illustrative diagnostic k range [-12,+12] therefore requires LiveShifter
p in [0.25,4]. For r=0.5,k=+12 or r=2,k=-12, reusing the present clamp produces source pitch
instead of the requested octave shift. This diagnostic range is not a promised future UX range
or a verified native quality/realtime domain.

Validate the combined domain explicitly. For a certified native interval [p_min,p_max] and
one fixed r, accepted k lies within [12*log2(p_min*r),12*log2(p_max*r)]. Across a variable r
range [r_min,r_max], use the intersection
[12*log2(p_min*r_max),12*log2(p_max*r_min)]. Record unsupported requests and retain the previous
effective value. Do not silently clamp pitch or rate, alter map/phase, or display an accepted
combination that the renderer did not apply. API acceptance of positive pitch is not certification.

Current `stretch_processor.rs:128` gates wet processing on preserve_pitch and inverse pitch near
unity. Future k!=0 at r=1 needs pitch processing. Also p=h/r can cross one at nonunit r; native
bypass may be correct for carrier pitch yet wrong for latency/history. Do not reuse the existing
near-unity predicate as a synchronization policy. Compare delay-consistent processing with an
explicit prepared transition before selecting any bypass behavior.

Rubber Band's pitch API defines frequency scaling independently of duration. Offline Stretcher
fixes pitch after study begins, so static KEY edits need a new prepared result; time-varying
pitch needs a separately evaluated realtime path. Pitch changes must not run concurrently with
processing on the same handle. Pinned references:
[LiveShifter API](https://github.com/breakfastquay/rubberband/blob/v4.0.0/rubberband/RubberBandLiveShifter.h),
[Stretcher API](https://github.com/breakfastquay/rubberband/blob/v4.0.0/rubberband/RubberBandStretcher.h).

## Requested, pending, effective and rejected state

A diagnostic pitch request carries source/pad generation, k, pitch revision, intended output
frame and event sequence. Requested is intent; pending is validated intent awaiting preparation;
effective is what the rendered output provenance actually applies; rejected retains a reason
and the previous effective value. Never equate enqueue success with audible adoption.

Prepared state identity includes source/map/loop/tempo revisions, starting source/output phase,
pitch revision/trajectory, KEYLOCK mode, intended output range, renderer version/options,
channel/stem topology and native history.
A k change invalidates an old-pitch prepared result without invalidating raw PCM, beat analysis
or raw stem caches. Derived warped/pitched caches include k and their complete render identity.

If k changes while a launch is pending, keep the captured trigger T. Reprepare for T only if
ready. A late or superseded result is rejected; defer the KEY change with the previous effective
pitch or exercise the explicitly declared launch fallback. The diagnostic records this choice.
It must never silently move T, restart a voice, change phi or reanchor M to conceal unreadiness.
Queue failure, unload and newer revisions cannot adopt stale pitch state.

## Common output coordinates and audible limits

All old/new state comparisons use the same intended output n and canonical s(n). Native feed
position, buffered wet output and estimated audible device position are separate domains.
Source-aware preparation supplies the required native history, with reset/initial pitch work
outside rendering. Nominal getStartDelay alone does not certify a changing-pitch transition.
The pinned [native implementation](https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3LiveShifter.cpp#L173)
and [integration guide](https://breakfastquay.com/rubberband/integration.html) explain initial
measurement state and delayed output response.

Any future dual-state transition must align old and new output to the same musical coordinate,
use bounded prepared storage and retire native state off callback. Record crossfade interference,
attack loss and additional delay. Do not shift the master clock, independently shift other pads,
or infer equal acoustic attacks from equal source positions. A fixed latency envelope/render-ahead
is a candidate to measure, not a solution to intrinsic transient deformation or unavailable sound.

## Stems and diagnostics

One pad's selected stems share k, map, source trajectory, loop wrap and pitch schedule. Compare
selected-stem sum through one stereo processor with independently processed stems and full mix.
Shared settings or CHANNELS_TOGETHER do not prove spectral coherence or null-sum equality.
Separate separation residual from extra warp/pitch residual; mask changes must not alter logical
phase or require reset. Other pads retain independent k and identical timing when only one changes.

Freeze a staged diagnostic matrix before tuning:

- Arithmetic/static audio: r={0.5,0.8,1,1.25,2}, k={-12,-7,-1,0,+1,+7,+12}, rates 44.1/48/96 kHz.
  Include unit rate/nonzero k, both clamp corners, known tones/attacks and correlated stereo.
- Variable map: drift, ramps, abrupt slope, fractional BPM, loop seams and p=1 crossings from
  either changing r or changing k. Preserve accepted map interpolation between anchors.
- State: k events before/on/after captured launch, native block boundaries, pending preparation,
  pause/resume/seek/retrigger, masks, unload, queue failure and stale revision.
- Partition/multivoice: fixed 64/128/256/512 and [64,96,257,512,31,1], plus 1/4/8 pads with
  distinct maps/keys and one-pad edits. Export independent expected source/output traces.

Logical source/target/master traces must be invariant under changes to k. For the same pitch
trajectory and matched initial state, partition changes must retain deterministic continuation.
Audio tests separately measure frequency ratio, duration, attack/energy/peak times, seams and
stems. Do not compare raw PCM equality between different intentional keys. Freeze any new pitch
measurement budget before results; retain all original acoustic gates and failed evidence.
No live result follows from an offline pass; native allocations, deadlines, device delay and
the later long-session acceptance remain separate gates.

## Rollback

The preparation slice leaves production k=0 and no new performer controls. Failed diagnostics
remain evidence; discard only their disposable outputs. A later K1 UI/persistence/input change
must specify KEY range, requested/effective feedback and adoption policy separately.
