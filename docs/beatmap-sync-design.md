# Beat This migration and independent pad pitch: implementation design

Decision date: 2026-10-05. Status: planned; no production implementation in this revision.
This document adopts the user's selected direction: **Beat This! 1.1.0, final0, minimal
postprocessing** replaces the normal beat/downbeat analyzer after the stated acceptance gates.
It is no longer an open model shortlist. `small0` is a later explicit footprint option.
The [research evidence](beatmap-sync-research.md) remains relevant; the sequence here supersedes
its earlier comparator-first recommendation.

The resulting engine must support variable-tempo MULTILOOP with independent SYNC and KEYLOCK,
and be ready for a later per-pad musical transposition control. That future control changes
pitch without changing source timing, master time or another pad. This plan prepares its contract
and tests now; it does not implement the future KEY UI or a full application Rust port.

## Fixed architectural decisions

1. Rust owns one permanent output clock and complete master musical position M(n).
2. Each source has one accepted, versioned, monotone beatmap B and inverse S. Editor, preparation,
   source reader and diagnostics use that same implementation and interpolation revision.
3. Quantize chooses entry intent; SYNC determines ongoing source-time trajectory. BPMLOCK cannot
   multiply a second tempo ratio into a SYNC-owned trajectory.
4. Per-pad transposition k is a separate musical parameter. It must never enter M, B/S, loop
   addressing, beat count, launch target or creative phase calculations.
5. KEYLOCK holds the intentionally selected pitch while tempo changes. With future k=+3, it
   holds source pitch +3 semitones, rather than forcing the source back to its original pitch.
6. Offline analysis and heavy audio/state preparation run outside the callback. Readiness,
   latency and audible alignment are proven separately from correct source coordinates.

## What sample accuracy means here

Beat This normally emits positions on a 20-ms grid. This is approximately 960 samples at
48 kHz; writing such a position as an integer sample does not make its musical estimate exact.
The front end and peak selection are inspectable in the
[preprocessor](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/beat_this/preprocessing.py)
and [postprocessor](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/beat_this/model/postprocessor.py).

The plan therefore distinguishes four measurable properties:

| Property | Required evidence |
| --- | --- |
| Musically correct beat/downbeat | Reference annotations, count/meter checks, uncertainty and manual corrections. No perfect automatic detection claim. |
| Exact accepted source anchor | Manual/refined anchors retain the chosen source sample index and its sample rate, or precise source seconds plus that edit provenance. A grid line can be placed/edited at one-sample resolution. |
| Shared numerical map and schedule | B/S round trips, output-frame scheduling and loop arithmetic agree within declared frame-rounding limits; no cumulative integration/partition drift. |
| Audible inter-pad synchronization | Rendered and recorded output measurements with actual time/pitch DSP, state transitions and device conditions. It is not inferred from grid lines. |

Display pixels may aggregate many samples when zoomed out. Grid drawing resolution cannot change
stored anchors. Persist stable source-time coordinates and derive loaded-frame coordinates once;
keep fractional positions internally where sample-rate conversion requires them. Round physical
markers at the final loaded-frame boundary, never repeatedly round every beat/callback/loop.
Numerical tests target <=1 loaded frame of mapping/round-trip error at representable anchors,
with exact equality for integer fixtures. That is not a one-sample beat-detection guarantee.

Beat This raw detections remain immutable evidence. A separate accepted map records beat count,
quarter-note interpretation, downbeat/meter, coverage, confidence/review state and corrections.
First compare the unrefined model to its reference implementation. Later optional local refinement
may search a bounded region for convincing onset evidence with preserved raw/corrected coordinates,
method/version and displacement. Ambiguous candidates stay uncertain. Never move every beat to
the nearest transient: a valid beat can lie in silence or under a soft/syncopated event. Missing
beats, false beats and half/double tempo are count problems, not sample-position problems.
Manual sample-domain anchors and explicit count correction remain available and authoritative.

## Analysis migration

Use the existing loaded immutable PCM and source/request generation, not the waveform envelope
or another file decoder. A full-track non-realtime PCM snapshot/export boundary must be added;
the existing viewport waveform API is not such a boundary. Share canonical mono preparation,
then derive Beat This's 22,050-Hz input and the existing key detector's 44,100-Hz input separately.
Preserve origin/tail and record preprocessing identity; do not cascade through the other model's
sample rate or repair unexplained offsets with a fitted constant.

Reuse current request IDs, progress events and stale-result rejection. Existing rejection of a
completed stale job is not cancellation of its computation: B1a must add bounded cancellation,
timeout/process teardown and off-thread PCM cleanup. A bounded lazy offline worker owns model
loading/inference and can use read-only mapped PCM plus small messages. Version-pin
its environment separately if the app's Python >=3.14 environment is incompatible. CPU operation
is required; GPU is optional. No Python inference or PCM export occurs in the audio callback.
Worker count, Torch thread count, queued bytes and retained snapshots need explicit limits.

The bounded termination deadline applies to the isolated beat job. Existing Rust KeyNet/ORT
calls are not assumed interruptible: cancel queued key work, check between cancellable stages,
and track any in-flight native call as retiring until it actually ends. Bound key execution/
retirement slots and retained PCM bytes, applying backpressure rather than creating replacement
threads without limits. Shared PCM stays alive until its last reader releases it. A whole request
is terminal only after both branches settle; beat-process exit alone cannot certify complete
analysis cancellation or resource release. B1a tests this distinction with a stalled key double.

Acquire `final0` through explicit setup into local model storage, verify actual SHA-256 and
save a manifest with package/checkpoint/configuration/license/source URL and byte length.
The author host previously advertised 81,058,141 bytes; this is not a verified checksum.
Normal startup/load/analysis never downloads. Fail before upstream loader invocation if the local
file is absent because upstream accepts names/URLs that can trigger downloads.
[Upstream loader](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/beat_this/inference.py).

Keep Beat This inference optional to playback and legacy cached analysis. The base app currently
still has mandatory Torch/Demucs dependencies; a lazy optional beat worker alone does not remove
those. The existing separator/dependency slice must complete that reduction. KeyNet remains the
key detector; Beat This does not determine musical key.

After worker/provenance/quality acceptance, new automatic and requested analyses use Beat This
by default. Old projects retain saved grids/manual corrections without forced reanalysis.
Missing model or failed inference produces explicit beat-analysis status while loaded audio and
independent key results remain usable. No silent qm fallback may be labeled Beat This output.
The reproduced qm downbeat defect remains a bounded comparator/legacy repair, not a prerequisite
that postpones designing or integrating the selected backend. Do not compare against its known
broken downbeat output as if it were a valid baseline.

## Timing and transposition are independent

For a supported loop, B maps source seconds to beats and S maps beats to source seconds.
Define b0=B(loop_start_seconds), L=B(loop_end_seconds)-b0>0, loaded source rate Fs and
intentional phase phi:

```text
s(n) = Fs * S(b0 + euclidean_mod(M(n) + phi - b0, L))  (loaded source frames)
r(n) = ds/dn  (source frames per output frame, excluding loop wraps)
h(k) = 2^(k/12), where k is signed semitones
```

Changing k cannot change s(n), r(n), M(n), phi, B/S or the scheduled output-frame range.
Different pads use different maps and k values but the same master/output frame. Stems of one
pad share that pad's source trajectory, pitch intent, wrap decisions and preparation identity.

| Pipeline, equal source/output rate | KEYLOCK on | Proposed future KEYLOCK off behavior |
| --- | --- | --- |
| Current varispeed reader + pitch shifter | Native pitch scale p=h/r; resulting intended pitch h. | p=h; resulting pitch follows h times varispeed r. |
| Full Stretcher on original source | Duration ratio q=1/r, pitch scale h. | Duration ratio q=1/r, pitch scale h*r to retain varispeed semantics. |

At unequal frame rates, duration ratio is `(source_rate/output_rate)/r`; preprocess experiments
to the same rate. For time-varying r, these are desired trajectory relationships, not permission
to apply the current output frame's p to arbitrary future native input. The adapter must associate
rate/pitch segments with the same source and intended output ranges through native buffering.

The KEYLOCK-off column is a complete proposed extension, not existing behavior: transposition
remains intentional while tempo-related pitch follows varispeed. With k=0, legacy semantics are
preserved. The future feature specification must confirm that UX before enabling its control.
Uniform semitone transposition preserves mode; converting minor to major or reharmonizing notes
is a different feature and is not promised by a KEY selector.

### Current implementation traps that the plan must remove

- `PadController.set_manual_key` and `ProjectState.manual_key` correct/display source-key
  metadata; they send no audio transposition command. Preserve that meaning. Reserve a separate
  future `pad_pitch_shift_semitones` intent, default 0, plus a derived output-key label.
  Preserve legacy arbitrary key strings; unknown source key still permits relative semitone
  changes. Target-key UI must define octave/tritone choices; KEYLOCK-off with varying r cannot
  display an unqualified fixed output key. Source replacement resets source-bound k; restoring
  the same verified source retains saved k.
- The existing global Pitch/Speed control changes speed/BPM. It is not a semitone controller.
- `pitch_scale_for_tempo_ratio` currently clamps compensation to 0.5..2. Combining k=+/-12
  with r=0.5..2 requires p=0.25..4 on the current pipeline. This is a diagnostic stress range,
  not a promised product range or evidence of native quality/safety. Reject unsupported pairs
  explicitly rather than clipping pitch or changing r to make them fit.
- Pitch processing may be required at r=1 when k is nonzero. Conversely p=h/r can cross 1 at
  nonneutral tempo (e.g. +12 semitones and r=2). Existing unity/dry bypass assumptions can change
  latency or destroy state. Test these crossings as real mode transitions, never shortcuts.
- KEYLOCK toggles and future key changes must preserve the chosen source timeline. A pitch
  glide/crossfade may change timbre during its declared transition, but cannot restart, seek,
  duplicate/drop source time or move another pad to hide latency.

## Rendering and state ownership

Evaluate current map-reader/LiveShifter, offline Stretcher keyframes and, only if needed, a full
realtime Stretcher adapter on identical map/rate/pitch trajectories. Preserve Rubber Band quality
and compare one versus two DSP passes explicitly. Offline warping for a fixed tempo is a useful
reference but cannot by itself satisfy arbitrary live master-tempo and per-pad KEY changes.
Choosing a cache route must include a measured update/preparation strategy or reject that route
for the live requirement. Do not freeze future key changes into a design that needs full-track
reanalysis/separation or destructive source rewrites.
Offline Stretcher pitch must be set before its study/processing phase; it is not a live pitch
automation API. Static-k offline results are references, while variable-k trajectories need a
separately evaluated realtime route. See the [Stretcher contract](https://github.com/breakfastquay/rubberband/blob/v4.0.0/rubberband/RubberBandStretcher.h).

Represent prepared output/state by source generation, map/interpolation/loop revision, master
tempo trajectory epoch, creative phase, pitch trajectory revision, KEYLOCK mode, renderer
version/options, channel layout and output range. Raw beatmaps and raw stem caches are independent
of k; pitch-rendered caches/prepared state are not. A changed KEY request invalidates only affected
pad processing/preparation, not the original analysis or other pads.

Distinguish requested, prepared and effective pitch/mode revisions. Prepare cold state/history
off-thread. The old and new processors must represent the same intended output-frame range at
handover; compare their actual native delay/history, not just source cursor or nominal delay.
Reject a stale prepared launch if its key/map/source revision changed. A latest-request coalescer
is bounded, ordered at the authoritative apply frame and cannot adopt an older request later.

Use a common output timeline and explicit per-path latency/readiness accounting, including dry,
pitch-only and stretched pads. A bounded session headroom reservation may cover the measured
supported operating envelope; it is not a scalar cure for native transient deformation. A new
pitch request that needs more preparation stays visibly pending or is rejected while existing
audio continues. Do not delay one audible pad unannounced, shift the master, reset another voice
or enlarge global audible latency mid-performance to conceal an unready path.

No callback model work, reset/construction, allocation, blocking, logging, GIL access, file I/O,
final large-buffer destruction or unbounded feed/retrieve loop is allowed. Use existing bounded
publication and off-thread retirement. Source-specific state must be ready before claiming lock.
500-ms supervision compares matching output-time domains and subtracts creative phase; it never
performs periodic seeks/resets or changes source coordinates based on KEY.

Retain the existing causal/retention failures, including the 33.292-ms fixture. Neither transposition,
offline preparation nor a more accurate map recovers already elapsed sound. Launch policy and
per-path acoustic fidelity remain separate acceptance gates.

## Ordered implementation slices

Each row is a bounded implementation/validation handoff, not a single broad rewrite. Do not mark
a row complete from documentation or mocked inference alone. Run full project checks for each
implemented Rust/audio/persistence/shared-contract slice, and strict validation of its affected
changes. Keep generated/private evidence outside the repository.

| Slice | Concrete output and dependencies | Acceptance and rollback |
| --- | --- | --- |
| B1a: offline boundary | Selected Beat This adapter/job DTO, immutable full-track non-RT PCM boundary, optional worker discovery, cancellation/stale-ID handling, local-checkpoint-only guard. No default switch. | Missing model/runtime cannot break loading/playback; tested source origin, worker limits, shutdown and stale completion. Existing analyzer remains until cutover. |
| B1b: real reference | Explicit final0 acquisition with hash manifest, pinned Windows CPU worker, whole-track oracle output from shared PCM. Raw logits/predictions retained locally. | Actual inference, not mocks; resampler/chunk-boundary parity, offline rerun, duration/origin, cold/warm RAM/time, cancellation. Failed setup produces a precise repair task. |
| B2: accepted new default | Freeze local annotation/quality/resource criteria before tuning; measure raw Beat This on all available benchmark classes, repair qm comparator only as needed; switch NEW automatic/manual analysis after gates. | Published quality/error/uncertainty report, no unflagged count errors in accepted regions, corrected-grid effort and critical downbeats assessed; old projects untouched. Rollback selects legacy backend explicitly and preserves provenance. |
| B3: shared map | Versioned raw/accepted records, precise anchors, monotone B/S with bounded lookup, explicit gaps/beat units, legacy scalar mode and persistence. Can be built against exact fixtures in parallel with B1. | Identity/restore/round-trip/long-loop/rate tests; no silent variable playback activation. Scalar mode remains intact. |
| B4: editor and correction | Same B/S for dynamic grid, snap/auto-loop and display; source-sample manual anchors, count/downbeat repair, optional separately evaluated refinement, revision/uncertainty display. Depends on B3. | Editor and engine diagnostic coordinates match; old markers/master remain stable. Displayed map is explicitly not yet live. |
| B5: independent time/pitch proof | Implement diagnostic/core contract for r and k, test-only nonzero k; compare Rubber Band routes and state transitions on exact and accepted maps. Depends on map semantics, not completion of B4 UI. | Matrix below and original acoustic gates; document supported joint envelope, measured readiness and chosen render architecture. No production KEY UI or live SYNC yet. |
| B6: live mapped ownership | Prepared map/trajectory/state publication, bounded source/DSP progression, pitch-ready revisions, per-path output alignment, rollback and deferred/rejected updates. Depends on B5, then device evidence. | No stale state, callback violations, accumulating phase or concealed per-pad delay; full-mix/stems and mode transitions pass. Keep activation guarded until evidence passes. |
| B7: unified entry and SYNC | One captured-input launch policy plus independent continuous SYNC; all four Quantize/SYNC combinations, preserved phi, tempo changes, stop/pause/seek/retrigger/cancel. Depends on B6 and explicit remaining launch UX choices. | Source and audible phase tests across MIDI/native/fallback/keyboard/mouse, headroom/lateness/loops; no 500-ms reset. Old launch path remains available until accepted. |
| B8: performance release | Existing optimization and live release slices 5/6, including 30-minute recording and future-k test injection; docs/UI truthfulness and portability measurements. | Device/driver/voices/callback conditions and all failures recorded. Only supported reviewed maps and joint ranges advertised as locked. |
| K1: later user feature | Actual per-pad KEY controls, mappings, persistence and output-key display, using already proven k contract. Not implemented by this planning request. | Confirm range/KEYLOCK-off/formant UX, test all B5 transitions through real controls; never reuse metadata-key setter or speed control. |
| Existing remaining program | Separator replacement/optional inference slice 7, then full Rust-port PLAN slice 8 remain after live acceptance. | Beat analysis native export is a separate parity task. Full application port still needs the user's explicit post-bugfix authorization. |

The immediate next implementation slice is B1a. Model choice is settled; do not reopen the general
survey or let the known legacy detector defect become a prerequisite for the new worker boundary.
Acquisition/inference B1b is explicit setup work and cannot happen incidentally during B1a tests.
Later launch tolerance and future KEY UI choices do not block B1-B6 contract/feasibility work.

## Required joint validation matrix

Freeze expected coordinates and original acoustic gates before running candidates. At minimum:

- 44.1/48/96 kHz, fixed/irregular/one-frame partitions; constant tempo, vinyl-like drift, ramps,
  abrupt accepted/rejected changes, signed origins, fractional BPM, short/whole-bar loops.
- r=0.5/1/2 and trajectories crossing unity; k=-12/-7/0/+7/+12 as diagnostic stress values,
  plus user-range edges once selected. Test r=1 with k!=0 and p=1 at r!=1 explicitly.
- One, four and eight pads with distinct maps/k values on the same M; full mix, prepared stems,
  masks and stereo/mono. Same-source stems share one map; separately warped stems require
  additional coherence tests rather than assuming they sum like a single processed mix.
- KEY change while playing, queued quantized launch, paused/resuming, looping, preparing,
  restoring; rapid opposing KEY requests; KEYLOCK toggles; source/map/tempo/mask replacement.
- Numerical invariant: for identical time intent, every k sequence produces identical canonical
  source/output coordinates and other-pad state. No phase drift over at least 30 minutes of
  deterministic simulated output. Accepted integer target frames remain identical.
- Pitch evidence: settled tonal fixtures measure expected h under KEYLOCK with a declared cents
  tolerance fixed before testing; transition windows are scored against the intended glide, not
  hidden as steady-state success. Unpitched impulses measure timing, not transposition accuracy.
- Audible evidence: output-tagged markers/attack envelopes, unchanged original onset/peak/energy
  gates, pitch-conditioned latency, crossfade/seam behavior and musical listening/loopback.
  A source-coordinate pass does not certify audible sample accuracy or phase-coherent waveforms.
- Native+Rust allocation/deadline measurements include cold preparation, ready handover, unity
  crossings and extreme combined pitch. Failures shrink the supported envelope or block adoption;
  do not silently clip, weaken gates or mask a failed path with clock changes.

## OpenSpec and evidence ownership

The active planned changes are `adopt-beat-this-analysis`, `prepare-versioned-source-beatmaps`,
`share-editor-beatmap-coordinates`, `evaluate-variable-tempo-rendering`, and
`prepare-independent-pitch-timing`. Their implementation tasks remain unchecked. Later live
ownership and Quantize/SYNC activation need their own focused deltas after B5 measurements;
future KEY UI needs its own user-facing delta. Current runtime docs remain accurate until an
implementation slice changes behavior, at which point that slice updates them too.

Public evidence establishes model/API feasibility, not successful execution here. No model has
been acquired or run, no new native render performed and no production behavior changed in this
planning revision. The selected design is deliberately testable before any claim of perfect
musical grids or audibly sample-exact independent transposition is made.
