## Pinned mechanism and reuse

Use the existing linked RubberBand C ABI and exclusive non-Send/non-Sync RAII
test owner. Reidentify its actual loaded file/binding; engine3 is not library
semver. The retained DLL is298496B/SHA4a8d323cc8c40d16413bbffe459811f93ebb5f7249b9e830bf4d11c62e4feb6d,
with library semver unavailable in the existing identity receipt. No additional
library load, install or production path is needed. The pinned v4.0.0 source is
the mechanism reference, not a claim that this DLL's exact source is proven.
R3Stretcher.h404–453 chooses before-resampling for HighSpeed p=2; HC always
chooses after-resampling, and HQ at p=2 chooses the same after mechanism. This
is a predeclared signal-order experiment, not a success prediction or delay fit.
Primary sources:
- https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3Stretcher.h#L404-L453
- https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3Stretcher.cpp#L470-L500
- https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3Stretcher.cpp#L743-L797
- https://github.com/breakfastquay/rubberband/blob/v4.0.0/rubberband/RubberBandStretcher.h#L331-L353

Add a small private profile configuration at constructor/capture inputs in
realtime_mapped_prerequisite.rs. Preserve existing HC wrappers, test bodies,
labels, criteria and OfflineStretcher. Put substantial new HS test bodies in a
focused child module. Shared scorer/Native ownership logic remains one authority.

## Exact predeclared input and startup

HighSpeed flags are RealTime|ThreadingNever|WindowStandard|ChannelsTogether|
EngineFiner=0x30010001, with no HC/HQ/formant flag. Reuse original_source,
original_hash, dry_mapping and live_hard_negative_input: exact44.1-kHz kick,
source rate.5/h1/KEYLOCK ON. Canonical source warp is already applied, so the
Stretcher receives q=1,p=2; never q=2. Original events0/65536 become canonical
0/131072, with canonical lengths262148/393220. Interior first196608 frames must
remain bit-exact to the old Live input. Both512 and irregular callers are required.

Query actual engine3/channels2/q/p/P/N before any signal. The pinned source
predicts target P4096/N2048; require that actual result without adapting the
nominal axis to improve scores. Feed P real zeros and each canonical sample once,
using the existing512 stager. Expected block sequences are520*512+4 and776*512+4.
Finalize once on the final real input block and retain every raw sample until
actual available==-1; available==0 is not EOF. No Study, expected duration,
keyframe map, pitch retarget, output padding/cropping, gain/peak fitting or Live511.

Use a fresh HS q1/p1 handle for its own finite16391-frame stereo impulse
[1,-.73] at4096, under both callers. Expected P=N2048 and36*512+7. Earlier HC unity
waveforms are not HS expected output. Measure timing, original peak/energy/tail,
complete caller parity and visible maxPCM/RMS errors; a timing pass promises no
sample-exact unity waveform.

## Unchanged scoring and rejection

Reuse nominal_metrics and strict_prerequisite. Retain full original Raw including
negative nominal time and its energy denominator/peak. Actual window
[N,N+canonical_frames) must be complete. Require q10/q50 errors<=88 frames2ms,
original Raw peak retained, all removed original energy<=.001, fixed nominal last
20ms energy relative to BOTH actual nominal and original Raw energy<=1e-6,
extra Raw suffix<=1e-6. Report actual drained-end20ms separately. Old strict
diagnostic with N zeros before unchanged dry/eventN+event remains additional.
No end-window padding or unavailable-as-pass. A candidate survives only when
ALL four joint kick cases and both own unity callers pass. Technical execution
can pass while acoustic rejection remains an explicit result. No matrix expansion
or productive permission follows a failed prerequisite.

## Compatibility, resources and limits

Reproduce old HC complete Raw/canonical hashes, coordinates and metric rows
against retained realtime-first.log from probe-realtime-mapped-stretcher. Shared
infrastructure refactoring cannot alter that evidence or rewrite old results.
Full HS/HC audio equality is not required. Preserve bounded process/retrieve/drain
and input caps:2M input frames,1M Raw frames,3907 process calls,1M retrieves and16
final drains. Stager and drain scratch each4096B; count complete input/Raw/compare/
energy arrays separately and report a conservative peak for the synthetic run.
These bounds do not qualify Native internal RAM or callback admission.

HighSpeed's fixed-pitch semantics do not qualify arbitrary dynamic pitch changes.
Even a surviving prerequisite closes no Tone/Cents/timbre, both KEYLOCK states,
37 rates/Total-23…+24, actual retainedFuture/FIFO/Mixer, S/E/T/deadline/start-mode,
automatic map, resource216 or Human/device gate. Full program scope remains;
stop before full Rust-port/Slice8.
