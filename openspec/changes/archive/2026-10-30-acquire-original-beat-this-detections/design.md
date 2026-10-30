## Authority and reuse

Use the existing `CompleteSourceReader`, source-map offline reservation, bounded
cold lane, `NativeAnalysisOwner`, private Raw artifact and single review lease.
The new acquisition is map-only intent: it does not advance audio preparation
epochs, select an accepted map or replace timing. Generation, content, placement,
cancellation and shutdown checks retain their existing authority.

The Native worker owns the complete staged float32-le mono file, its actual
digest/rate/frame count/origin and the readable original. Keep these owners until
the Beat This process, descendants, output readers and temporary owners have
really retired and immutable Raw publication has completed. No second native
analysis job, replacement reservation or independent completion owner is added.
Precharge retained source plus 336 MiB of parent-process Python wire/decoded/packed
objects, Native capture/codec/reparse and fixed I/O metadata under the existing
512-MiB job bound. Stream staged PCM to disk, separately bounded to512MiB, rather
than allocating a full mono vector. Preserve the separate worker process limits;
the parent heap allowance does not bound model subprocess RSS or disk usage.

## Worker boundary

Run a focused Python orchestration entry on the existing off-realtime cold worker.
It loads only an explicitly installed, verified `final0`/minimal configuration and
reuses `BeatWorkerAdapter`, its bounded process supervision and teardown. A private
read-only Native cancellation probe supplies the original request's cancellation,
source freshness and shutdown to the existing Event polling interface. It creates
no additional analysis owner or polling thread. Always observe actual adapter
retirement before returning, including failures and malformed responses.

Use the installation root documented as the workspace sibling
`analysis-runtime/beat-this`. App startup and analysis never invoke setup or
downloads. Missing, corrupt, mismatched or unavailable installation produces an
explicit failed/unavailable review attempt, with no QM fallback. Explicit developer
setup for validation remains separate from product analysis.

## Backend-aware complete capture

Introduce `RawCapture` with distinct QM and Beat This variants. Keep the existing
QM Raw envelope byte/schema contract; common accessors expose source identity,
event positions and inspection without pretending Beat This uses QM ODF indices.
Beat This Raw uses schema version2 and encoding
`source-beatmap-unreviewed-beat-this-raw-v2`. It retains all four arrays as exact
binary64 little-endian/base64 with strict bounded decoding, actual request/model
identity and actual Native staged-input provenance. Expected frontend identity and
50-Hz logits are not an invented per-run hash of internal 22050-Hz PCM or log-mel.
Downbeat timestamps remain independently retained; exact membership flags are
observations, not meter or every-fourth-event assumptions.

Each array retains the existing 250,000-event bound; the whole Raw envelope remains
at most16MiB. Verify byte/count bounds before allocating derived arrays. Preserve
full arrays on success; oversize or invalid results fail without truncation.

Accepted map V3, encoding `source-beatmap-pwa-v3`, embeds this complete Beat This
capture. V1/V2 retain strict prior dispatch and validation. The existing explicit
64-label/8-KiB interpretation may consume either genuine capture through common
source-second accessors; it still creates estimated anchors, uncertain intervals
and unknown meter. No automatic musical qualifier is fabricated by numbering
detections. Keep independent count, coverage, meter, phase, corrections and Undo.

## Application and modularity

The Source Map original-fullmix command selects the chosen Beat This acquisition;
the prior Native QM acquisition remains supported for compatibility. Normal pad
load/Analyze routing and the B2 default-cutover gate do not change in this block.
The UI truthfully identifies backend, estimated evidence and unavailable states.
Legacy QM ODF-specific inspection is shown only for QM; both retain source seconds.
Portable capability checks and typing include the new Native API/version.

Keep worker orchestration, backend capture/codec and map interpretation separate.
Engine/Mixer receive only necessary delegation. No generic plugin host, second
clock, duplicate scheduler or deepening of test-only include fixture chains.

## Verification limits

Protocol doubles test failure/cancel/retirement and exact arrays, never actual
model correctness. A separate opt-in, hardware-free generated-source run executes
the real pinned worker through the actual Native/app boundary. It proves that
composition and complete provenance, not musical annotations, perfect detection,
renderer timing, device/hearing or whole-SYNC acceptance. All full-Goal B2,
continuous rendering, onset/start-policy, resource and Human gates remain open.
