# Complete PCM cache and finite loop residency

## Why

At the C0 audit, every restore decoded a complete source into a device-format buffer.
The loader hashed a path before decode and copied it afterwards, which could not
prove the decoder consumed the bytes of the retained original under replacement
or ABA. Short saved loops still retain complete-track playback PCM. Existing
editor, explicit seek, ALL, analysis, stems and accepted-timing consumers depend
on the complete source and cannot safely receive an unlabelled cropped buffer.

## What changes

Specify copy-first immutable input, byte-exact project originals and complete
versioned disk PCM with validated reuse. Preserve decoder/source rate and channel
metadata; derive separately identified full playback PCM for the current device
format, with independently identified analyzer transforms. Add finite resident
loop descriptors, proved reader/DSP context, explicit readiness, guarded atomic
publication and last-owner cleanup. Keep full-source editor/analysis and
full-track seek/ALL exceptions.

C1a delivers bounded Windows copy-first complete cold artifacts and native ACK
publication. C1b adds complete fresh immutable warm validation, shared preparation
and assignment/read ownership, immutable stem generations and contained bounded
last-reader cleanup, plus preliminary real integrity/save costs. Residency and
measured startup/RAM acceptance remain the later slices.

C0 delivers the contract audit and maintained design in
`docs/pcm-cache-residency.md`, plus this focused change. C2/C3 implementation,
integration and measurements remain unchecked in tasks.md. The MODIFIED deltas
resolve existing unconditional decode/immediate-region/delete wording and the
old bank-replacement scenario against the already specified pinned-voice behavior.

## Non-goals and realtime constraints

No resident-window implementation in C1a/C1b, continuous streaming system, callback disk
access, forced 48-kHz conversion, analyzer-default cutover, new timing evidence,
new pitch/FX backend, acoustic-delay compensation or full application Rust-port
planning/implementation. Actual device/hearing acceptance stays open and occurs
at the end of the authorized pre-port program. Performance improvement is a
hypothesis until C3, including current 96-native-handle and save-integrity costs.

All copying, hashing, validation, decoding, resampling, waveform/analysis reads,
large allocation and deletion stay off the callback. The callback consumes
prebuilt immutable handles with bounded guards/ACK and reserved retirement
capacity; no GIL, locks, JSON, logging, inference, plugin work or unbounded loops.
