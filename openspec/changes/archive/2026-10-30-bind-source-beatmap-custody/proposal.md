## Why
The immutable SourceBeatmap parser validates geometry and internally consistent
decoder digests. It does not yet establish custody of a real current loaded source.

## What Changes
Add map-purpose validation to the existing offline job owner. Verify the exact
held decoder/original lease and original content/request, then deliver an immutable
map DTO through NativeOwned completion routing. Preserve ordinary analysis and
the shared admission/retirement boundary; no mono export is needed for map data.

## Non-goals
No musical annotation acceptance, scalar tempo replacement, project map selection,
automatic Loader adoption, durable Copy/restore, live variable-map rendering, new
job scheduler or Rust port. All parsing, hashing and retirement remain off realtime
and heavy work releases the GIL. The audio callback gains no work or allocations.
