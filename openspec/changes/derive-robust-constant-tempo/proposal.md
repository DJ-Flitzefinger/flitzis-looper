# Robust constant-tempo evidence

## Why

The current BPM interval mean telescopes to two detector endpoints. Quantized
endpoint errors therefore become a persistent scalar-grid slope. Replacing it
with a good linear fit alone cannot establish musical beat counts, resolve
half/double tempo or distinguish real variation from outliers.

## What changes

G2a introduces a backend-independent offline Rust evidence/count/fit API. It
retains complete raw positions and source identity, assesses explicit quarter-note
hypotheses, compares distant regions and reports residuals and conditional period
uncertainty. Its supported output is a numerical candidate, not accepted timing.

G2b then supplies lossless backend adapters and signal-supported PCM refinement.
The actual exact-WAV measured-span frame gate must pass before G2 is complete.
G3 separately publishes accepted source timing to every consumer.

## Non-goals and realtime safety

No default-backend cutover, musical acceptance, integer-BPM snapping, origin edit,
manual/TAP replacement, persistence migration, live period/rate/wrap change,
variable beatmap, SYNC, GPU installation or full Rust port is included. All fit
allocations and bounded searches occur outside the callback; no callback path
calls this API. Existing frozen B2 acceptance limits remain unchanged.
