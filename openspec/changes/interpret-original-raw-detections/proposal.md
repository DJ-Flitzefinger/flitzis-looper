## Why

The complete-original Raw inspector now preserves genuine detector observations,
but the accepted map editor cannot yet use them to construct its initial variable
quarter-note geometry. The existing interval editor needs real accepted anchors.
Raw detection order alone does not establish quarter-note count, meter or coverage.
The version-1 accepted format also cannot preserve the complete Raw capture,
including observations outside the physical source, without losing provenance.

## What Changes

- Add an additive version-2 accepted record embedding the complete existing Raw
  capture, while retaining the strict unchanged version-1 decoding contract.
- Add one Native interpretation builder and original-candidate asynchronous
  admission through the existing worker, artifact, completion and selection path.
- Let the Raw inspector collect explicit source-bound quarter labels and build a
  durable map whose initial intervals remain uncertain and whose meter/phase are
  unknown. Reuse existing independent interval and position corrections afterward.
- Preserve the entire capture through editing, undo, durable references, Copy,
  Bank and fresh source-verified restore without another model invocation.

## Non-goals

Explicit review is the first client of the shared builder, not a replacement for
the full Goal's qualified automatic full-track map. No ordinal raw-index-as-quarter
rule, inferred default 4/4, uncertainty-as-accuracy claim, verified JSON flag, model
default change or B2 bypass. No new persistent Raw-file reference, worker pool,
playback pipeline, Native output alignment or musical/device/Human acceptance.

## Realtime

Parsing, hashing, verification, serialization and artifact retirement stay on the
existing non-realtime owners. No new callback input, I/O, GIL, lock, allocation or
heavy final-owner destruction is introduced.
