## Why

Analysis already persists complete beat lists, but the editor and engine consume scalar BPM
and one origin. Variable-tempo material needs an explicit, validated source-to-beat map before
either UI or rendering can use it. Beat This! is now the selected replacement analyzer; its
estimated beat positions still need validation/correction before becoming authoritative maps.
The legacy downbeat repair is comparator work, not a dependency of this foundation.

## What Changes

- Define versioned offline beatmap records, provenance, uncertainty and checked inverse mapping.
- Reuse existing analysis, project persistence and Rust source-grid boundaries.
- Preserve legacy scalar grids and original analysis; no automatic conversion to trusted maps.
- Expose a Rust control-side evaluator for diagnostics and later editor integration.

Status: planned, not implemented. Backend transition belongs to adopt-beat-this-analysis.
See `../../../docs/beatmap-sync-design.md` for selected direction/dependencies and the linked
research report for primary evidence. Source maps remain independent of future pad transposition.

## Non-goals

No live map publication, Quantize/SYNC activation, model download, backend replacement, new meter
support in the master transport, plugin hosting or application rewrite. No automatic rewriting
of existing manual offsets, loop markers or analysis caches.

## Realtime Constraints

Analysis, map construction, validation, serialization and bulk evaluation stay outside the audio
callback without GIL access from audio. This change does not add callback vector traversal,
allocation, locks, file I/O or inference.

## Impact

New `source-beatmaps` capability; later implementation affects the analysis result boundary,
project persistence, native API types and a focused map module. Existing scalar runtime remains
the rollback path. Update architecture/development docs when that implementation exists.
