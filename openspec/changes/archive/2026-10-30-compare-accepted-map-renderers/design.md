## Existing authority and exact gap

SourceBeatmap owns the checked piecewise-affine seconds/quarter-note mapping.
SourcePlayback and SourceReadPlan own fractional PCM addressing; the existing
NativeAdapterState owns LiveShifter/FIFO processing. The existing test-only
OfflineStretcher owns study/process/retrieval and destruction. Its present two
keyframes are constrained to one ratio and do not exercise variable timing.

The diagnostic reuses these components. It does not turn a parsed map or a
historical asset lease into current source/preparation/audio permission.

## Frozen first comparison

At 48 kHz, output spans are 32768 frames. Accepted source anchors are at frames
0,16384,49152,114688,163840 with quarter coordinates0,2,4,6,8. Thus the exact
source-frame/output-frame rates are0.5,1,2,1.5 under one constant master clock.
An independent integer/affine oracle checks the actual shared inverse at every
address used. PCM geometry and complete hashes bind the synthetic map input;
synthetic consistency is explicitly distinct from live producer custody.

Declare output-domain creative phase offsets0 and17 frames before measurement.
The latter puts map-rate boundaries inside Native blocks and starts at a
fractional source coordinate. Retain any resulting partition/pitch-assignment
failure; block-aligned results alone cannot establish arbitrary-boundary support.

Compare neutral,+7 and total endpoints-23,+24, each Key Lock state, complete
isolated attacks and settled stereo tonal input. Native pitch is h/r ON and h
OFF; original-source Stretcher pitch is h ON. Offline Stretcher does not support
variable pitch automation, so OFF with changing r must be explicitly unsupported
there rather than mislabeled as equivalent. Canonical coordinates remain the
same for every pitch request. Use existing512 and irregular caller partitions,
canonical native blocks, true exclusive endpoints, no redundant0->0 R3 keyframe.

Loop-address/seam and parent-map stem comparisons must retain their own scope:
same source positions do not imply identical independently processed stem sums.
Coverage refusal must occur before starting a candidate renderer. Unknown meter
alone must not erase trusted quarter-note geometry; gaps cannot be interpolated
into eligibility or hidden behind a scalar fallback.

## Evidence and limits

Retain every frame of the declared capture, including32768zero-fed Native frames.
Full internal Native-tail exhaustion remains unproven; low energy at this finite
capture boundary alone does not prove it. Keep the original2ms timing,0.001 discard
and1e-6 capture-tail energy criteria, plus the existing15-cent settled-pitch criterion.
Retain the original tonal evidence guards as well: finite samples, mean energy
strictly greater than0.001, more than8positive crossings and absolute error strictly
below15cents. Insufficient cycles in the frozen window means missing evidence,
not a pitch pass; optional measured frequency remains diagnostic only.
Report raw natural-clock timing separately from any declared nominal Native
delay; never fit alignment to observed peaks. Failed acoustic predicates are
diagnostic results, not a successful production-policy selection. Report actual
native profiles, partitions, source/map/output identities and processing costs.
The component comparison uses four synthetic gain-derived inputs and independent
serial Native instances, with raw component-sum error reported separately. It does
not exercise actual PreparedStemSet masks, generation or simultaneous lifetimes.
No device, callback deadline, C++ allocation, full matrix or Human proof follows.
