# Key + Transposition on independently playable content

## Why

Current manual_key is display metadata, current speed controls tempo, and current
Rubber Band clamp/bypass does not establish creative transposition support. The
requested workflow needs separate source correction, audible base and additional
semitones, playable persistent menus and immutable Learn SET/SET_RETRIGGER actions.

## What changes

Define numeric base_shift and extra_shift, correction epochs, neutral legacy
migration, one pitch pass, and the desired total -23..+24 plus actual r(n) proof.
Capture accepted selected-content actions and apply pitch/retrigger atomically in
the existing scheduler. Preserve prepared-event individuality and existing routing.
Use shared state for two stay-open menus and persistent independent musical intent.

## Delivery and lineage

K-META now implements only the pure durable key policy and source/request-bound
metadata lifecycle. Its saved numeric shifts are neutral intent; they issue no
audible pitch or retrigger command and add no new GUI or MIDI action.

R0 introduced the contract revision. The [extended program](../../../docs/pad-owned-pcm-program.md)
and [coverage](../../../docs/pre-rust-program-coverage.json) own all56+9/seven groups,
38 IDs/48 edges. K-META supplies neutral policy/storage; B5 expanded diagnostics,
B6-K-AUDIO/B7-K-ATOMIC native ownership/events; K1a/b guarded controls; V0 combined
software; H-LIVE/SLICE7/H-FINAL/C-FINAL real final gates. R0-CLOSURE precedes J0/P1.
`prepare-independent-pitch-timing` retains test-only k in B5; this later change
owns production activation. Existing mapping/clock/source/RT contracts are extended,
not reimplemented. Musical mask durability is owned by the revised PCM change.

## Non-goals and realtime constraints

R0 introduced contracts only. K-META performs metadata/persistence work and
hardware-free software validation, with no app/GUI/device/hearing/model operation.
No audible major/minor conversion, harmony recommendations, automatic scale/pad
compensation, second scheduler/cache/pitch cascade or full-source pitch PCM.
Preparation, validation, persistence and large retirement stay off callback;
bounded Rust guards/handles apply only prepared events with reserved feedback and
retirement. No I/O/JSON/Python/GIL/UI/locks/logging/inference/heavy allocation or
unbounded work in the callback. STOP before full Rust-app port planning/Slice8.
