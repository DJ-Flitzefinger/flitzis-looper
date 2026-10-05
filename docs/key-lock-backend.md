# Key Lock Backend

This document records the current Rubber Band based Key Lock implementation.

Key Lock is one bounded part of the Rust audio/DSP foundation. It does not imply
plugin hosting, a separate FX graph, or realtime stem generation.

## Runtime Modules

The active backend is implemented behind:

```text
rust/crates/looper/src/audio_engine/stretch_processor.rs
rust/crates/looper/src/audio_engine/rubberband_backend.rs
rust/crates/looper/src/audio_engine/key_lock_preparation.rs
rust/crates/looper/src/audio_engine/source_playback.rs
rust/crates/looper/src/audio_engine/source_reader.rs
```

`RtMixer` owns tempo-ratio target selection, per-pad Key Lock state and per-voice
source/processor calls. `source_reader.rs` centralizes
effective loop regions, explicit seek progression, full-mix/stem compatibility,
integer source addressing and source-selection crossfades. It borrows accepted
buffers and can be reused by background preparation without importing the mixer
or native processor. `source_playback.rs` owns scalar fractional source epochs
and active-output-frame ratio smoothing. The mixer uses that cursor with the
reader's linear interpolation taps and advances stem transition ramps by
fractional source distance.
`VoiceSlot` owns the per-voice source playback state, `StretchProcessor` and
explicit seek mode. The mixer retains one preparation worker for its 32 voice lanes.

## Playback Semantics

- Per-pad Key Lock off: playback is varispeed, so tempo and pitch move
  together.
- Per-pad Key Lock on: source-frame tempo progression remains active, and the
  varispeed block is processed through a per-voice Rubber Band LiveShifter with
  pitch scale derived from `1.0 / tempo_ratio`.
- The global Key Lock control overwrites currently loaded pads' per-pad Key
  Lock values. A later per-pad toggle changes only that loaded pad, and unloaded
  pads remain disabled.
- BPM Lock off: the active tempo ratio is the global speed multiplier.
- BPM Lock on with valid master and pad BPM metadata: the active tempo ratio is
  `master_bpm / pad_bpm`.
- Pads without valid BPM metadata use the global speed multiplier.
- Full-mix and prepared-stem playback share the same source addressing and Key
  Lock path.

## Source Timing And Resampling

Scheduled render segments carry absolute output-frame positions from
`audio_stream.rs` into `RtMixer::render_rt_at_output_frame(...)`.

Every playback mode uses `SourcePlayback` to derive fractional source progress
from a scalar source epoch plus active output-frame count times the actual native
`f32` tempo ratio promoted to `f64`. Reads linearly interpolate two integer
neighbors through the shared half-open loop/seek policy. The lookahead tap wraps
at loop end; explicit intro seeks play into the loop, and tail seeks play to
track end before wrapping into the loop. Both channels and prepared-stem
selections use the same addresses and fractional source transition progress.

Rate changes rebase from the current fractional position instead of independently
rounding source consumption per segment. Pause freezes source and smoothing
progress; resume continues them. In-range loop edits retain fractional carry,
while out-of-range edits retain the existing loop-start clamp. Explicit seeks
and retriggers start a new source epoch. Integer telemetry floors the next
source cursor. Ordinary wrapping does not redefine the Rust master output
timeline, Rubber Band state, editor source grid or prepared-stem alignment.

The same source feed supplies dry varispeed output and the Rubber Band adapter.
`StretchProcessor` no longer interpolates render-segment endpoints. This makes
source/native input independent of fixed, irregular and one-frame partitions
when source state and accepted control events are equivalent.

The separate `source_grid.rs` foundation derives source beat/bar and loop-cycle
phase from the editor's signed origin. Transport retains complete beat position
across master-BPM changes and can bootstrap once from the selected BPMLOCK
reference. These operations preserve voice read positions and Rubber Band
ownership. The master-to-source mapping helper is internal and tested; normal
starts still read the effective loop beginning. Neither that helper nor the
one-time bootstrap compensates audible Rubber Band/device delay.
Bootstrap queries a copy of the canonical source cursor configured against the
current effective loop. It maps the integer source frame plus its fractional
remainder to beats, matching the position rendering will read after loop/seek
normalization without advancing the live cursor. A changed loop cannot anchor
the master to a source position that will be clamped before the next read.
These changes do not reconstruct Rubber Band state.

## Rubber Band Processing

### Preparation Ownership

Stream setup constructs 64 unique native handles: one current handle and one
reserve for each of 32 voices. Every handle is warmed with neutral silent
blocks before callback rendering. Fixed block buffers, channel pointer arrays,
and bounded FIFOs are also allocated before rendering. Preparation failures,
including failure to start the worker, reject stream setup.

The two-handle pool has a measurable startup/memory cost. Cold standalone
Windows measurements for 32 stereo voices observed:

| Output rate | Pool setup | Working-set increase | Private-committed increase |
| --- | --- | --- | --- |
| 48 kHz | 151.347 ms | 137.477 MiB | 146.668 MiB |
| 96 kHz | 270.090 ms | 205.965 MiB | 230.719 MiB |

These are single-run process deltas on the measured system, not portable memory
limits or live callback costs. The fixed reserve preserves quality/options and bounds
ownership exchange; its resource cost remains part of later profiling.

The pinned Rubber Band 4.0.0 source audit found that native `reset()` and a
pitch change before the first `shift()` call `measureResamplerDelay()`, which
creates two temporary `std::vector<float>` buffers. The probe's calling-thread
Rust allocator counter does not observe these C++ allocations. Construction,
native reset, cold pitch setup, and silent warming therefore run only during
setup or on the preparation worker. The callback applies pitch changes to an
already warmed uniquely owned
handle. See the [pinned native implementation](https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3LiveShifter.cpp).

Start/retrigger, stop, seek, and leaving wet processing clear only the adapter's
own bounded storage and mark used native state dirty. On the next wet render,
the voice exchanges that state for its warmed reserve through two bounded SPSC
queues. The worker resets and warms the returned handle. The exchange reserves
return capacity first and never destroys native state, waits, or prepares DSP
inside rendering. Teardown releases the worker after voice rendering stops.

When no reserve is ready or the recycle lane is full, wet rendering returns
silence for that segment and retries later; the source timeline continues.
Dry varispeed and the approximately neutral ratio remain immediate. Pause/resume
retain the current native state. Stem mode/mask transitions retain native
history and use the existing source crossfade instead of resetting Rubber Band.

### Adapter And Measured Delay

The tested SHORT + CHANNELS_TOGETHER backend uses 512-frame blocks. Wet
activation seeds the output FIFO with `block_size - 1` silent frames, so the
adapter adds a fixed 511-frame lead (10.646 ms at 48 kHz), independent of callback
partitioning. This removes the previous growing offset from underflow silence;
the lead is currently uncompensated. Missing shifted output still uses bounded
silence, with no refill spin.

Native nominal delay, measured transient onset/peak, adapter lead, and device
buffering are separate quantities. The initial optimized offline baseline used
cold ratio-specific native state and isolated impulses at output-domain frame
8192:

| Output rate | Tempo ratio | API delay frames | Impulse peak delay frames |
| --- | --- | --- | --- |
| 48 kHz | 0.5 | 2909 | 2909 |
| 48 kHz | 2.0 | 3678 | 3260 |
| 96 kHz | 2.0 | 7774 | 6844 |

These baseline values are not correction constants for music. In the final
warmed-pool probe, the 48 kHz ratio-2 adapter impulse peak was 3771 frames after
its reference: a 3260-frame native peak plus the fixed 511-frame adapter lead.
Fixed 64/128/256/512-frame callbacks and the irregular
`[64,96,257,512,31,1]` pattern produced the same peak for this fixture. The
nominal native API delay was still 3678 frames. The independent fixed-lead FIFO
model reported no underflow across 2048 calls per tested pattern.

Final optimized 48 kHz ratio-2 measurements, using 24 repetitions, observed:

| Operation | Median us | p95 us | Maximum us |
| --- | --- | --- | --- |
| Native preparation for reuse | 1250.25 | 1578.50 | 1742.60 |
| Adapter first activation and processing | 215.90 | 531.30 | 534.50 |
| Adapter Rust-only reset | 0.90 | 1.30 | 2.60 |

The baseline adapter reset median was 105.35 us and first activation median was
800.40 us. Moving preparation changes where work happens; it does not remove
the native preparation cost. Calling-thread Rust allocation counts were zero
for measured first activation, reset, reactivation, and warm processing; worker
and C/C++ allocations are outside that counter.

A handle warmed at neutral pitch retains different startup history when its
ratio changes. The API delay getter remains nominal; it does not prove the
output transient's location for that history or a changing ratio. Startup and
settled responses need separate measurement. The local baseline and final
prepared results live in workspace `scratch/`, including
`slice3-key-lock-latency-findings.md` and
`slice3-key-lock-latency-{baseline,prepared}.csv`; generated CSVs and logs are
not repository artifacts. Reproduce the probe using
[the development guide](development.md#offline-key-lock-measurement).

The adapter partition test supplies identical already-varispeed samples. The
fractional source foundation also compares immutable nonconstant sources,
loop/intro/tail boundaries and prepared-stem sums through the common source
path at 44.1/48/96 kHz. Rate changes and pause/resume use active-frame progress,
so their source feed does not change with render partition sizes. This proves
the source-to-adapter prerequisite independently of the adapter FIFO property;
it does not prove source priming, transient compensation or audible hardware
alignment. The live mixer and non-live preparation proof now call the same
`SourceReadPlan::fill_fractional_buffers` helper. Live prepared-state activation
and audible compensation remain pending.

This preparation and adapter safety stage (slice 3a) does not perform track
pre-roll, delay discarding, or a separate DSP feed-ahead cursor. Source playheads,
persisted markers, the shared clock, and launch scheduling keep their existing
meaning. Source-aware prepared handover, audible phase compensation, and short
wet/bypass transitions, including ratio 1.0 and global/per-pad toggles, remain
slice 3b work. Current mode changes can still switch between delayed wet output
and immediate dry output without that transition compensation.

Output-clock snapshots estimate device buffering from CPAL callback timestamps.
They do not include native/adapter signal delay or unknown latency after the
device buffer. Offline impulse measurements and Rust allocation telemetry do
not establish hardware onset precision or live callback deadlines. Captured-input
nearest-grid diagnostics remain separate from current launch execution.

### Non-live Exact-source Proof

Native tests compile `key_lock_source_preparation.rs` and its independent
reference tests. This fixture borrows already accepted immutable source/stem
buffers; production builds and the live mixer do not include it. Both copied
cursors use an explicit constant ratio. Live smoothing and asynchronous control
changes are outside this proof. The caller's fractional clock and pending rate
target remain unchanged.

Exact initial inverse pitch uses the same `f32` conversion as live processing.
It is set before native reset and the first source-content shift. Reset initializes
the previous native output hop from that pitch. The fixture advances only future
feed during preparation and retains native state plus fixed input/output FIFOs.
For block size B and experimental discard D, it feeds
`P = ceil((D + B - 1) / B) * B` frames and retains raw output `[D..P]`.
Initial occupancy Q=P-D is at least B-1. After n rendered frames, input backlog
is n mod B and output occupancy is Q minus that backlog. Logical playback advances
by n; feed advances by P+n. It inserts no second silent adapter lead.

The separate oracle resamples immutable source samples algebraically and shifts
them through a raw native handle. It compares every retained and continued stereo
sample, source cursor and FIFO count across 44.1/48/96 kHz, fractional ratios,
loop/intro/tail positions, selected stems and unequal partitions. Explicit discard
endpoints verify that D is honored independently of nominal native delay.

The release CSV reports the uncropped response translated by D, startup-peak
clipping and discarded energy, alongside the retained response. Negative residuals
and a discarded original peak must remain visible: cropped output alone can hide
the lost first transient. This is evidence for selecting a later compensation
strategy, not audible acceptance. Reproduce the proof and CSV using
[the development guide](development.md#offline-key-lock-measurement).

The optimized Windows proof passed 984 reference comparisons, including 180
impulse cases with identical metrics in 45 rate/ratio/marker groups across four
partitions. In this exact-pitch/reset fixture, nominal discard clipped the startup
peak at ratios 1.37 and 2.0 at all three rates. At 48 kHz/ratio 2, discard 3678
removed 96.499% of the marker-zero response energy. Its uncropped translated
onset/1%-onset/peak residuals were -3044/-2144/-420 frames; cropped metrics were
0/0/+96. For the settled marker at frame 8192, the corresponding residuals were
-3055/-2187/-418. These describe this native fixture, excluding the live adapter
lead and device delay. They reject nominal discard alone as a startup rule and
motivate explicit source history plus a separately justified musical timing
criterion. Local evidence is `scratch/slice3d-{source-preparation.csv,validation-summary.json}`.

### Explicit Source History And Musical Criterion

The test-only request now accepts a declared earlier source epoch and history H
in output frames. Forward progression at the exact requested ratio must reach
the logical frame, fraction and seek mode. This preserves intro/tail provenance
after entering the loop without guessing a backward path. Positive history
rejects active stem-selection ramps because their earlier state is unavailable;
zero history preserves the original fixture. The path declares hypothetical
prepared content, not an actual record of previously played controls.

Native processing starts at that history origin. Raw discard D remains an
independent experiment; effective translation is D-H. With raw transient time R
and independently resampled dry time T, both measured from the history origin,
the residual is R-D-(T-H). The logical cursor begins at H while feed begins at
the processed P. Some diagnostic requests allow P<H; live source alignment is
a separate gate. Ready FIFO/native continuation accounting remains unchanged.

The bounded sweep uses H=0/8192/16384, phases 0/17/511, impulse, 8-ms source tone
and damped percussion bursts at the existing five ratios and all three rates.
Translations are API-2B, API-B, API and API+B; each compares all four render
partitions to the same independent raw suffix. Isolated responses contain no
overlapping loop transient. Separate nonconstant source tests cover loops,
intro/tail and full-mix/subset/all-component stems with fractional origins.

Before measurement, the exploratory engineering budget is set to two ms for
both q10 attack and q50 body timing: pooled stereo squared energy, smoothed by
a centered 0.5-ms box, defines cumulative-energy q10/q50/q90. Signed envelope
centers before frame zero preserve startup-reference energy without adding
causal filter delay. Candidates must retain the original response peak, discard
at most 0.1% response energy and leave at most 0.0001% in the capture's final
20 ms. These are engineering budgets, not universal perception thresholds.
Both attack and body constrain alignment without correlating different dry/wet
carrier pitches. Uncropped onset/1%-of-original-peak onset, peak and q90 remain
visible, including negative residuals. Retained onset uses that same original
peak threshold. Cut-to-silence and ready-FIFO/native-continuation jumps expose
cut/join continuity; they do not prove click-safe mode transitions.

The exported summary intersects integer timing and exact energy/peak retention
bounds across all nine phase/signal fixtures for each fixed rate/ratio/history.
An empty interval rejects constant translation under this declared criterion,
including untested integers; it does not rule out a different content/onset
policy. Three sampled phases are diagnostic coverage, not exhaustive phase
acceptance. Reproduction commands and both CSV exports are in the development
guide. No compensation is selected by the fixture.

The optimized Windows sweep passes 6480 exact-suffix comparisons; all 1620
candidate/fixture groups have identical metrics across four partitions. None
of the four tested translations passes all nine fixtures at any fixed
rate/ratio/history. The interval check also rejects every integer translation
for all 36 nonneutral groups under the declared criterion. Only the nine
ratio-1 groups have feasible unswept intervals; live ratio-1 already bypasses
shifted processing. Twenty groups fail timing alone, and all captures meet the
tail bound. For 48 kHz/ratio 2/H16384, common timing bounds [3240,2867] are
already empty, and retention limits C to at most 2080. Nominal C3678 clips the
original impulse peak and discards 96.714% energy; q10/q50 residuals are
-907/-420 frames. Source history therefore establishes coherent provenance,
but cannot by itself make an intrinsically spread transient fit both timing
budgets. Resolve the onset/content policy before choosing compensation or live
adoption. Local measurements are `scratch/slice3e-source-history{,.summary}.csv`;
these isolated fixtures do not measure busy musical context or a device.

Live source/generation/loop/seek/stem/ratio identity, fixed handover frames,
stale/late rejection, off-thread retirement and source-aligned mode transitions
remain pending. No transport, marker, launch policy or live fallback changes in
this proof gate, and no device measurement is claimed.

## Continuous Onset And Content Feasibility

A test-only gate separates native envelope deformation from a launch cut. If
`a_p = raw_qp - dry_qp`, then `a50-a10` and `a90-a50` do not depend on translation.
For integer output-frame times, the best unconstrained scalar minimax error is
`ceil((max(a)-min(a))/2)`. With a permitted integer interval `[L,U]`, the minimum is
`max(ceil((max(a)-min(a))/2), max(a)-U, L-min(a))`, provided `L <= U`.
The uncropped original peak and 0.1% energy budget impose the retention upper bound;
the reference is never recomputed after cropping. q90 remains diagnostic, outside the
existing q10/q50 acceptance criterion.

The unchanged isolated sweep contains 405 unique fixtures, 324 at nonneutral ratios.
The q10/q50 scalar lower bound exceeds the original two-ms budget in 82 of these
324 fixtures; preserving peak/energy raises that count to 234. At 48 kHz/ratio2/
H16384/marker0, the impulse displacement centers are 2771/3258/3317 frames. The
q10/q50 integer bound is 244 frames (5.083 ms), and retention requires C<=2080.
These are bounds for the measured responses, not a claim about every musical signal.

The longer-history experiment holds target-relative source content and native block
phase constant across H32768/H65536, with silent or nonzero periodic stereo history.
Varied tonal and percussive attacks probe exact ratios 0.5/1/2 at 44.1/48/96 kHz.
An attack-minus-background output is a nonlinear sensitivity diagnostic; it can
include changes to subsequent background processing. Compare it with the isolated
native attack and report actual mixture metrics too. Whole-history energy quantiles
can be dominated by background and are not an attack acceptance criterion.

One fixed offline candidate starts with canonical dry varispeed for 2 ms and then
uses a 5-ms raised-cosine transition into nominally translated continuous wet output.
Both channels share the coefficient at each absolute output frame, and the wet suffix
after the transition must remain exact. At ratios2/0.5 the dry portion changes pitch
by +12/-12 semitones; a timing result cannot certify Key Lock quality. Retain raw wet
clipping/energy evidence even if the added dry content makes a cropped envelope look
better. This is a falsification experiment, with unchanged budgets and no live policy
selected. It starts no device and changes no source grid, clock, marker, backend option
or runtime ownership. Task7.2 remains pending until an acceptable content policy is
justified; identity/adoption and synchronized Quantize must wait.

The release matrix contains 432 actual fixtures and passes 864 exact nominal-suffix
comparisons. Every nonneutral common-translation group remains infeasible. At ratio2
all 72 isolated cases fail nominal timing/retention; the bridge improves timing alone
in 46 cases but passes the complete criterion in none. Nominal cut loss reaches 99.9825%
of isolated response energy. At ratio0.5 only 10 of 72 isolated bridge cases pass the
complete criterion. Neither nonneutral background paired group has an individual full
pass. Ratio1 gives the passing neutral control in both scopes.

Isolated PCM, energy times and deformation are identical across the two histories.
Matching raw output windows from H-2048 through H+delay+capture differ for the actual
nonneutral mixture, with relative L2 up to 1.95859/1.88705 at ratios0.5/2. Paired
diagnostic q times change by up to 10339/20321 frames at ratios0.5/2, with relative
nonadditivity L2 up to 13.3664. Such long-tail context sensitivity cannot establish a
converged attack metric or be reduced to additive target energy. Duration, frequency
and phase are varied jointly; no marker0 or exhaustive musical content is covered.
The declared markers17/511 are mapped to rounded source frames (at ratio0.5 those
source coordinates correspond to output18/512). The independently read dry reference
supplies actual timing, including interpolation and the attack envelope.
These results require actual target-local continuous musical references and a pitch-preserving
policy; neither more scalar fitting nor this dry bridge is sufficient.

### Fixed Unity-Source Attack Candidate

The separate `key_lock_source_pitch_probe.rs` test module evaluates the same two-ms hold and
five-ms raised-cosine transition with a constant ratio-1 source branch. It copies the exact
logical fractional source phase and uses the existing reader; independent algebraic taps
verify its samples, including loop/intro/tail and prepared-stem addressing. Every fixture uses
its own continuous raw-native reference with matched origin, history, exact ratio and initial
pitch/reset order. H32768 and H65536 remain separate contexts, not interchangeable references.
The original wet suffix must remain bit-exact after the finite transition.

This branch preserves source carrier pitch by reading p+n, but the canonical tempo path reads
p+r*n. At 48 kHz the seven-ms endpoint is 336 output frames: ratio0.5 is 168 source frames ahead
(3.5 ms of source, seven ms of canonical output); ratio2 is 336 source frames behind (seven ms
of source, 3.5 ms of canonical output). No compensating rate ramp hides that discrepancy.
The mixed transition can still interfere, alter an attack envelope or miss a later target.
Mathematical source-rate preservation alone cannot establish perceived mixed-output pitch.

The nonzero-history matrix covers ratios0.5/1/2, 44.1/48/96 kHz, markers0/17/511 and the six
existing tone/percussion variations. Launch-local and independently source-mapped target-local
seven/40-ms windows report actual mixture energy/envelope differences and q10/q50 displacement
against matched wet and source references. Known event coordinates include source rounding.
Background can dominate a nonzero window; a launch window excluding the attack is not attack
evidence. Weighted source/wet energy and their signed cross-term expose interference. These
are diagnostics with no newly invented passing threshold. A short envelope also retains carrier
phase; a two-ms source excerpt cannot support a general spectral pitch estimate.

The earlier native q10/q50 two-ms, peak and 0.1%-energy bounds remain failed and unchanged.
Added source content cannot count as retained discarded native content. No live compensation,
identity/adoption, transition or synchronized Quantize policy follows from this offline candidate.
Reproduction is documented in the development guide; generated evidence stays in workspace scratch.

The release probe passes 648 independent unity-reader and 648 exact native-suffix comparisons
across 324 actual mixtures; all later wet suffixes remain exact. In the finite target40-ms
windows, native-relative q50 differences reach 15.927 ms at ratio0.5 and 28.027 ms at ratio2.
At ratio2 the target7-ms candidate/native energy ratio ranges from 0.227 to 271.319. These
background-containing, independently normalized window diagnostics describe substantial local
replacement/interference, not isolated attack timing or a newly defined pass/fail budget.
Seventy-two targets lie after the fixed bridge; exact target-window output there proves only
unchanged wet continuation. The unity-source bridge is not accepted as a general onset policy.

### Causal Scheduling Limit

The prior isolated raw retention bounds also define a necessary scheduling condition. With
nominal translation C and maximum translation U retaining the original peak and at least 99.9%
of raw target-response energy, preserving that content needs at least max(0,C-U) frames before
the nominal musical anchor. This is not the first infinitesimal onset and selects no correction.
For the 48 kHz/ratio2/H16384 impulse, C3678 and U2080 require 1598 frames (33.292 ms).
At 120 BPM a nearest future 1/64 boundary offers at most 15.625 ms, before device/control lead;
many future targets offer less and past targets offer none. Across the prior matrices, maximum
necessary headroom is 36.875 ms (slice3e) and 34.286 ms (slice3f). These bounds use isolated raw
responses, not background-dominated mixture energy or nonlinear subtraction.

Emitting before a quantized target would change the audible-start contract, and even sufficient
headroom does not remove the failed intrinsic q10/q50 timing intervals. Resolve controllable
pre-target content, musical anchoring and explicit missed-content accounting for authorized late
catch-up before selecting a live policy. Another fitted bridge or lower retention budget cannot
resolve this causal limit. The unchanged strict criterion remains unaccepted.

## Settings Contract

Project persistence stores the global `key_lock` boolean as global-control
intent and `pad_key_lock` as bounded loaded-pad intent. Unloaded pads are saved
and restored with disabled per-pad Key Lock values. It does not store Rubber
Band handles, DLL/shared-library paths, runtime buffers, measured latency,
algorithmic delay, or callback-internal backend state.

The performer Settings UI exposes no Rubber Band backend tuning surface. Rust
uses a maximum tempo-ratio step of `0.05` every `512` active output frames in
dry and Key Lock modes. A newly accepted target initiates its first step
immediately. Rendering splits at later step boundaries so source ratios remain
independent of callback partitions; paused output does not consume an interval.
Native pitch-update order also matches with equivalent initialized native/preparation
state and reserve availability. Missing reserves retain bounded silence without
changing the canonical source feed. These values are not persisted or user-tunable.

## Realtime Constraints

The audio callback must not:

- allocate or resize DSP buffers,
- read files or decode audio,
- load plugins or models,
- log,
- block on locks or waits,
- acquire the Python GIL,
- run neural inference or stem separation,
- spin while waiting for Rubber Band output.

The callback updates scalar mode/ratio state, reads bounded per-pad Key Lock
state, reads prepared source buffers, uses fixed Rubber Band staging storage,
consumes or produces bounded FIFO data, and mixes the resulting output through
Gain/Trim, DSP, metering, and master volume.

## Native Dependency

The backend requires a Rubber Band C API that exports `rubberband_live_*`
symbols. The Windows vcpkg Rubber Band 4.0.0 package satisfies that requirement.
Ubuntu 24.04 `librubberband-dev` 3.3.0 does not provide the required LiveShifter
C API.

Build discovery uses documented platform mechanisms and explicit environment
overrides:

- Linux: `pkg-config` for a Rubber Band package with LiveShifter C API support,
  or explicit `RUBBERBAND_LIB_DIR` and `RUBBERBAND_INCLUDE_DIR` overrides.
- Windows: `RUBBERBAND_LIB_DIR`, `VCPKG_ROOT`, or the documented
  `%LOCALAPPDATA%\vcpkg` development location.
- Runtime DLL/shared-library availability is established before the audio engine
  enters realtime callback rendering.

The Windows source-run helper registers Rubber Band DLL directories before
loading the native extension. Standalone Rust test binaries do not import that
Python wrapper, so Windows Rust validation should either run with the Rubber
Band runtime directory already on `PATH` or use `scripts/run-rust-tests.ps1`,
which discovers uv's selected Python runtime plus the same documented Rubber
Band runtime locations and prepends them to `PATH` before invoking Cargo.
Packaging should provide the required runtime libraries with the application
artifact and account for Rubber Band licensing before binary distribution.

Reference URLs:

- https://breakfastquay.com/rubberband/integration.html
