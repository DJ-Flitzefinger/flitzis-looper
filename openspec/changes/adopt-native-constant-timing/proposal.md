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

## Non-goals and realtime safety

This is a bounded part of G3b2. It does not choose a musical acceptance policy or
infer quarter labels, switch the default analyzer, silently promote manual/TAP or
legacy numbers, or complete editor/snap/auto-loop, transport/master/BPMLOCK,
MIDI, prepared Key Lock/stem revision integration or accepted persistence.
Those consumers remain explicit follow-up work. Musical/physical loop proof and
audible DSP/device acceptance remain G3c. No PCM cache or Rust application-port
planning is included.

PCM scanning, QM analysis, hashing, fitting and evidence ownership remain outside
realtime processing. The callback handles only bounded source/permit checks and
fixed accepted timing metadata; large owners retire through non-realtime paths.
Observed source digest plus loaded PCM identity does not prove immutable
copy-first decode lineage or defeat an original-file ABA replacement.
