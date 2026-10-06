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

## Non-goals and realtime safety

This is a bounded part of G3b2. It does not choose a musical acceptance policy or
infer quarter labels, switch the default analyzer, silently promote manual/TAP or
legacy numbers, or complete productive StretchProcessor/voice/DSP-history binding.
The warmed Key Lock pool stays source-neutral and key_lock_source_preparation is
test-only. Accepted source-verified SampleAnalysis/ProjectState persistence and
loader schema with fresh runtime adoption, source/accepted-bound controller global
START/STOP batch launch including MIDI, and explicit acceptance/derived loop/master
refresh orchestration remain follow-up work. Musical/physical loop proof over
75/1000 cycles, fractional periods/rates/partitions/wrap and rendered/onset/device/
listening acceptance remain G3c. No PCM cache or Rust application-port planning is
included.

PCM scanning, QM analysis, hashing, fitting and evidence ownership remain outside
realtime processing. The callback handles only bounded source/permit checks and
fixed accepted timing metadata; large owners retire through non-realtime paths.
Observed source digest plus loaded PCM identity does not prove immutable
copy-first decode lineage or defeat an original-file ABA replacement.
