## Algorithm and ownership boundary

Use actual Signalsmith Stretch, not the historically named cute_dsp module.
Stretch revision `88061fd4eda56a2353c5d65c74aa3f4ccf76f9da` reports version1.3.2;
Linear0.6.6 resolves to `7bb1c8f36794d6980080ce817c91f8da8fe56a66`.
Vendor only Stretch's header and Linear's stft/fft headers plus both MIT licenses,
with exact upstream paths and SHA256 receipts. No optional FFT backend macros,
platform FFT dependency, bindgen, libclang or third-party Rust wrapper.

The nondefault `signalsmith-probe` feature enables the already locked cc1.2.63
compiler helper and one opaque C++ shim. Its build hook precedes all existing
RubberBand build-script early returns. The feature-off build retains the current
RubberBand dependency discovery and does not compile/link this C++ module.
MSVC and the Windows SDK are already required by Portable's existing checked
toolchain; ordinary locked install commands do not request the probe feature.

The Rust test owner is exclusive, non-Send/non-Sync and dropped off realtime.
Use fixed stereo, sample rate44100, fresh explicit mt19937 seed0 for every case,
one default preset with splitComputation=false, and fixed p1 or p2. Validate
all scalars, channel extents, finite samples and work/output bounds before FFI.
Never retain Rust PCM pointers. Catch all C++ exceptions inside the shim and
return failure; a failed processing owner cannot continue supplying accepted
audio. Destruction remains guaranteed through RAII. No production owner,
callback, public ABI, clock, source progression or adoption guard changes.

Primary source identities and API:

- https://github.com/Signalsmith-Audio/signalsmith-stretch/tree/88061fd4eda56a2353c5d65c74aa3f4ccf76f9da
- https://github.com/Signalsmith-Audio/linear/tree/7bb1c8f36794d6980080ce817c91f8da8fe56a66

## Predeclared natural output coordinate and ending

No seek, outputSeek, exact-length helper, fitted compensation, gain change or
cropping. Query block W, interval I, input latency Li and output latency Lo
before signal; require W5292/I1323/Li2646/Lo2646 under the frozen preset. Fresh
reset starts its processing position Li before the first input, while output
lags that position by Lo. Therefore the natural q1 raw-to-nominal translation is
D=Li+Lo=5292, determined before examining any output. P=0; no startup input is
removed. Keep the negative nominal prefix in Raw.

Process all canonical L frames exactly once followed by exactly Li genuine
zero input frames, using one continuous512-frame internal stager (including its
actual final remainder). Then make exactly one flush request of F=Lo+2W=13230
output frames with explicit playbackRate1. The pinned flush implementation
processes F-I zero input/output frames, then folds/tapers its final I outputs
and resets its state. Retain all streaming and flush samples separately and
together. The final I frames are labelled folded terminal output, not ordinary
streaming output. The exact returned extent is L+Li+F = L+D+2W.

The fixed nominal window is [D,D+L); the full extra suffix2W is retained. The
fold starts9261 frames after the nominal end, so it cannot alter nominal
samples. All energy denominators include the complete returned Raw. This API
has no terminal availability flag: the finite declared ending protocol is not
proof of unrequested output or internal tail exhaustion. Do not repeatedly
flush or claim RubberBand-style available==-1 semantics.

## Frozen inputs and criteria

Reuse original_source, dry_mapping, live_hard_negative_input and their hashes.
Source rate r=.5, desired h1, KEYLOCK ON; canonical warp already applied, so
q1/p2. Original events0/65536 become canonical0/131072; L262148/393220.
The interior first196608 canonical frames remain bit-identical to the old Live
hard negative. Both512 and irregular callers use the same internal blocks:
origin517*512+90, interior773*512+90. Compare complete Raw bitwise across callers.

Own unity uses fresh q1/p1,16391 frames, stereo impulse[1,-.73] at4096,
37*512+93 streaming input. Require the same coordinate/energy/peak/tail gates
and report actual maxPCM/RMS separately; no sample-exact unity promise.

Reuse nominal_metrics and strict_prerequisite without edits. Keep timing
q10/q50<=88frames/2ms, original peak, removed original energy<=.001, complete
nominal final20ms relative to both nominal and original energy<=1e-6, and
extra returned suffix<=1e-6. Retain actual final Raw20ms separately. Missing
complete windows cannot pass; diagnostic translation intervals never select
compensation. Surviving requires all four nominal AND strict kick cases plus
both own unity callers. A completed technical test can report acoustic REJECTED.

## Bounds and verification

Preserve 2M input/1M Raw frame caps and512-frame staging, with independent
process/caller/ending caps. Before allocating, conservatively account for all
simultaneously retained original/shifted/canonical/currentRaw/previousRaw,
staging, ending scratch, process metadata and shared scorer arrays. Their
sample/vector budget is256MiB; query and report actual capacities as well.
This does not cover allocator overhead, diagnostics, opaque C++ internal RAM,
process RSS, callback deadlines or resource216.

Run the new owner/control/cases and affected existing owner/scorer regressions,
format/Clippy, official strict OpenSpec, feature-off locked Release and relevant
Portable regression checks. Reuse compatible prior unchanged acoustic evidence;
do not compare different algorithms' waveforms as an equality oracle. One
independent final-diff review precedes the required coherent commit and normal
main push. No installed Native replacement is necessary for test-only code.

Even a surviving prerequisite closes no dynamic-pitch/FIFO, prepared Native
history/OwnACK/last-reader, S/E/T, start-mode, tone/cents,37rates/Total-23..+24,
automatic full map, B2, real stems, resources or musical/Human/device gate.
