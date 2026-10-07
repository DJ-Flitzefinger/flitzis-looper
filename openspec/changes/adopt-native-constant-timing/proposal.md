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

G3b2f2 integrates source-specific native preparation in that productive path.
The existing worker retains an actual prepared Rubber Band handle with its
adapter FIFOs after 4096 active frames from pinned actual PCM/stems and copied
canonical playback/read-plan state, including rate smoothing. Current native
source/load/preparation/authority and full accepted projection permits protect
the job. Rendering splits at the exact captured output frame plus 4096 and
transactionally adopts only a current matching continuation after reserving
off-thread recycling. Pending, failed, stale, unready, late or saturated work
keeps the old effective native/FIFO owner and output. Active stem-selection
transitions defer preparation. Completion requires real productive ownership,
shifted-output and failure-path proof, rather than the separate test fixture.

G3b2g separately persists supported complete source-verified native QM raw evidence
and restores it through fresh native capture and actual callback acknowledgement.
G3b2h connects productive controller GLOBAL START/STOP, including mapped MIDI,
to one fixed-capacity native batch. Every affected pad uses the shared actual
current-source/authority resolver and full acknowledged accepted projection,
including exact period and signed origin. Native admission and scheduled execution
validate the complete batch and all required voice/retirement/playback-feedback capacity before any
loop, playback or bootstrap change. Terminal execution feedback preserves controller
restore intent on pending or rejected work; ordinary native messages own active/
paused truth. Active old-source pins cannot be relabeled by a replacement bank.

## Non-goals and realtime safety

G3b2f1 continuous history and G3b2f2 source-specific prepared native continuation
precede G3b2g accepted persistence/fresh loader work. The neutral warmed reserves
and test-only key_lock_source_preparation stay separate from productive source
priming. The 4096-frame worker horizon is a bounded catch-up contract, not an
audible delay or onset correction. No seamless mode transition is claimed.

This slice does not choose a musical acceptance policy or infer quarter labels,
switch the default analyzer, silently promote manual/TAP or legacy numbers, or
select later B5 audible crop/delay/transition compensation. That acoustic policy
is separate from the required G3b2f2 native ownership/adoption integration.
Supported source-verified SampleAnalysis/ProjectState persistence and fresh loader
adoption are implemented by G3b2g; source/accepted-bound GLOBAL START/STOP batches
are implemented by G3b2h. General explicit acceptance/derived loop/master refresh
orchestration remains follow-up work; loader-specific refresh alone does not close
that boundary. Musical/physical loop proof over
75/1000 cycles, fractional periods/rates/partitions/wrap and rendered/onset/device/
listening acceptance remain G3c. No PCM cache or Rust application-port planning is
included.

PCM scanning, QM analysis, hashing, fitting and evidence ownership remain outside
realtime processing. The callback handles only bounded source/permit checks and
fixed accepted timing metadata, actual borrowed-source feed and bounded adapter
storage, batch-capacity checks and exact adoption-boundary splitting; native DSP
construction/reset/loading, source-specific priming/catch-up and large-owner retirement stay outside
it. The existing worker owns prepared continuation, state recycling and warming.
Observed source digest plus loaded PCM identity does not prove immutable
copy-first decode lineage or defeat an original-file ABA replacement.
Current 96-native-handle setup/RAM and persistence integrity I/O/CPU costs remain
unmeasured; historical 64-handle measurements cannot establish current performance.
