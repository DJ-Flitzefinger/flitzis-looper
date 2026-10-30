## Existing authority and reason for this candidate

Reuse key_lock_stretcher_probe/hard_negative_tests.rs: original_source,
original_hash, dry_mapping and live_hard_negative_input. Its original source,
canonical finite interpolation and old Live input are already independently
compared. Keep every old offline owner assertion and negative result unchanged.

The new child module has its own exclusive non-Send realtime Native owner. It
uses the existing C-ABI declarations and bounded capture conventions, without
turning OfflineStretcher into a second processing mode or duplicating a product
source clock. A test-only consumer of canonical PCM is not productive adoption.

Pinned public options select ProcessRealTime, ThreadingNever, WindowStandard,
PitchHighConsistency, ChannelsTogether and EngineFiner: 0x34010001. Construct at
the actual target pitch. For the retained KEYLOCK-ON h=1/r=.5 prerequisite,
q=1 and p=h/r=2. Canonical PCM already contains the tempo warp; q=1/r would apply
it again. This differs from the earlier original-source offline q=1/r,p=h probe.

## Matrix and lifecycle

Reuse source-event0 and source-event65536. Their canonical output events are0
and131072, with262148/393220 canonical frames. Reconfirm that the interior first
196608 canonical frames exactly equal the old direct Live input. Both512 and
irregular callers stage the same stream into identical Native512-frame calls
plus the same remainder. Four prerequisite captures are declared before signal.
Add a separately declared finite stereo-impulse unity control under both callers.

Before signal processing, set maximum process size512, verify actual engine,
channels and q/p, and query P=getPreferredStartPad and N=getStartDelay on the
actual handle. Supply P zeros then every canonical sample exactly once. Do not
study, use a native keyframe map, warm at unity, borrow Live delay observations
or call the offline expected-duration API. Finalize exactly once. Drain every
available sample after submissions and retain complete output until terminal
available==-1 within explicit finite input/capture/call bounds. Zero availability
before finalization is not EOF. Do not fabricate padding or truncate capture.

Record actual loaded DLL identity. Engine3 alone is not a library-version proof.
The expected pinned Standard/HC startup values are P2048 and N1024 at p2, and
P2048/N2048 at p1. These are source-grounded pre-signal expectations, not values
inferred from the measured response; actual API observations remain visible.

## Fixed measurement axes and criteria

Declare nominal output index raw_n-N before wet observation. This API's padding
contract is separate from Live's H/D/F and adapter511; do not transplant N+511.
Keep all raw negative-nominal samples and full original energy/peak denominators.

Evaluate actual nominal PCM raw[N,N+canonical_frames) against the independent dry
canonical input if the entire fixed window is available. Recompute its quantiles;
require q10/q50 error<=2ms (88 integer frames at44.1kHz), original raw peak retention,
discarded full raw energy<=.001, fixed nominal final20ms and extra raw suffix
energy fractions<=1e-6. The fixed final20ms uses the actual nominal-output energy
denominator, as in the existing emitted-output criterion; disclose and check its
full-original-energy fraction as well. Missing response or incomplete nominal windows fail.
Disclose the actual drained final20ms separately. A longer capture cannot change
the declared nominal end window or dilute an original energy denominator.

Also preserve the existing diagnose() strict prerequisite calculation using a
dry reference with exactly N leading zeros and event N+canonical_event. Report
its unchanged full-original-energy retention and q10/q50 intersection separately
from actual nominal-output metrics. A candidate survives only if the unchanged
strict prerequisite and actual complete nominal-output criteria both pass in all
four cases and the unity coordinate control passes. A negative acoustic result is
a real candidate rejection; technical capture assertions must not be weakened to
hide it. Diagnostic translation intervals never select a fitted compensation.

The unity control uses a predeclared finite impulse, actual p1 startup observations,
the same complete capture protocol and nominal-axis budgets. It reports peak
offset and PCM error explicitly. Its failure blocks interpreting this mapping;
it cannot be repaired by fitting N or changing the crop.

## Evidence and scope

No surviving candidate automatically grants extreme pitch, tones, dynamic ratios,
continuous source ownership, callback safety, public SYNC or musical acceptance.
Only a surviving prerequisite justifies expanding to the known full matrix.
Negative results and actual limits remain first-class evidence.

Primary pinned references:

- [C ABI and flags](https://github.com/breakfastquay/rubberband/blob/v4.0.0/rubberband/rubberband-c.h)
- [Realtime padding, dynamic pitch and duration contract](https://github.com/breakfastquay/rubberband/blob/v4.0.0/rubberband/RubberBandStretcher.h)
- [R3 construction, startup queries and bounded process interface](https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3Stretcher.cpp)
- [HC resampling and window selection](https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3Stretcher.h)
- [Multiwindow guide and transient processing](https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/Guide.h)
