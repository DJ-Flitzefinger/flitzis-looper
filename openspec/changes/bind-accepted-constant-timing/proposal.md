# Source-bound accepted constant timing

## Why

G2 supplies complete source-bound numerical candidates. Its raw revision does not
identify the chosen musical count interpretation, fitted period, uncertainty or
independent grid origin. Retained-request equality also cannot reject a result
after current source or manual timing intent has changed.

## What changes

G3a introduces an immutable, explicitly accepted constant-timing record in the
offline Rust analysis crate. Construction recomputes the existing evidence/fit
path and requires a uniquely supported independently verified interpretation plus
an explicit named acceptance assertion. A canonical versioned revision binds
complete evidence, count/fit/feature policies, period/error diagnostics and origin.
A separate control-only adoption guard rejects stale source/request/intent tickets.

This is the first bounded G3 slice. It has no production caller: runtime consumer
integration follows in G3b, and physical-versus-musical loop proof follows in G3c.

## Non-goals and realtime safety

No automatic analyzer cutover or numerical acceptance policy is inferred from a
fit. No automatic BPM repair, manual/TAP/legacy migration, callback/ring/transport
change, persistence migration, storage cache, variable beatmap, live wrapping,
audible SYNC acceptance or full Rust-port planning is included. Hashing, scans,
allocation, fitting and adoption-state ownership remain outside realtime audio.
Frozen B2 musical/resource gates and G2 uncertainty limits remain unchanged.
