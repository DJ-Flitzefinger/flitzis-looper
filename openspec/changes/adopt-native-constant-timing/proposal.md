# Adopt accepted timing under native pad ownership

## Why

G3a's immutable accepted record and adoption guard have no live caller. A matching
caller snapshot cannot protect a pad after source, request or timing intent has
changed. The live SourceGrid still reconstructs its period from binary32 BPM.

## What changes

G3b2a connects explicit accepted constant timing to the actual native loaded-pad
owner. An opaque preparation ticket captures complete native PCM and lossless QM
evidence; publication recomputes acceptance from explicit independent quarter
counts, timing-error provenance, origin and acceptance assertions. Real ownership
checks protect preparation, enqueue and bounded mixer adoption. Callback feedback
distinguishes pending publication from actual acceptance. The live SourceGrid uses
the accepted binary64 period and signed origin with the complete accepted revision.

G3b2b adds current-pad resolution from retained native records and fixed callback
acknowledgement epochs, including authority revocation across Manual/Tap/Legacy
roundtrips. Native transport, output clock and BPMLOCK consume the acknowledged
period directly. SourcePlayback target/ramp/ratio and the Rubber Band pitch ABI
use binary64, sharing one source trajectory and retaining fractional epochs.

G3b2c connects Python grid/loop/editor and global controls to one current timing
snapshot. G3b2d binds productive MIDI runtime, direct triggers and failed-direct
fallback to actual native source ownership and complete acknowledged timing.
Loop intent and launch travel as one guarded effect; quantized execution rechecks
the same binding before changing audio state.

G3b2e binds productive prepared-source permits, PreparedStemSet admission and
source-reader rendering to actual current native source/authority and the full
acknowledged accepted revision, exact period and signed origin. Already admitted
same-source stems retain their PCM and one SourcePlayback trajectory; successful
native adoption/clear refreshes only their fixed effective timing projection.
Pending/rejected timing and failed stem admission retain previously effective audio.

G3b2f1 binds continuous productive voice ownership and actual StretchProcessor native/FIFO
history to the source that supplies its canonical fractional feed and that
source's complete effective accepted revision, exact period and signed origin.
The processor fills its own fixed feed through SourceReadPlan/SourcePlayback;
an expected next fractional position detects discontinuities before new feed
enters old state. Same-source timing/rate refresh preserves chronological history.
An old active voice retains its pinned source and effective timing after bank
replacement; retrigger adopts the current bank owner through existing retirement.
Productive per-pad EQ/isolator filter history uses the same source/projection/
trajectory binding. Foreign-source or discontinuous output clears only bounded
fixed Rust filter storage before reuse; continuous timing/rate changes retain it.
New voice adoption also requires bounded agreement between the current native
control-source fence and effective callback bank. Loading unavailable or a
replacement control PCM ahead of bank adoption rejects new starts while ongoing
old effective source/timing/history remains available.

## Non-goals and realtime safety

G3b2f1 completes continuous productive history ownership only. Required NEXT
G3b2f2 remains absent: source-specific worker priming, retained prepared native/FIFO
ownership, full source/current-revision/rate/epoch permits and timed transactional
adoption with catch-up. It precedes G3b2g accepted persistence/fresh loader work.
The warmed Key Lock pool stays source-neutral and key_lock_source_preparation is
test-only; neither completes that required native integration.

This slice does not choose a musical acceptance policy or infer quarter labels,
switch the default analyzer, silently promote manual/TAP or legacy numbers, or
select later B5 audible crop/delay/transition compensation. That acoustic policy
is separate from the required G3b2f2 native ownership/adoption integration.
Accepted source-verified SampleAnalysis/ProjectState persistence and
loader schema with fresh runtime adoption, source/accepted-bound controller global
START/STOP batch launch including MIDI, and explicit acceptance/derived loop/master
refresh orchestration remain follow-up work. Musical/physical loop proof over
75/1000 cycles, fractional periods/rates/partitions/wrap and rendered/onset/device/
listening acceptance remain G3c. No PCM cache or Rust application-port planning is
included.

PCM scanning, QM analysis, hashing, fitting and evidence ownership remain outside
realtime processing. The callback handles only bounded source/permit checks and
fixed accepted timing metadata, actual borrowed-source feed and bounded adapter
storage; native DSP construction/reset/loading and large-owner retirement stay
outside it. The existing worker owns native state recycling and warming.
Observed source digest plus loaded PCM identity does not prove immutable
copy-first decode lineage or defeat an original-file ABA replacement.
