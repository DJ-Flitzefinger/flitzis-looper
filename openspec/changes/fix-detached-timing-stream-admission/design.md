# Design

Revalidate the captured stream identity at the existing final control admission boundary. Reject completion when that producer is absent or belongs to an earlier stream; do not replay or recapture the operation against a new stream. This check precedes epoch, ticket, evidence registry and queue mutation. Work already admitted before shutdown retains its existing semantics.

Deterministic device-free public tests hold actual accepted and saved timing work at existing detached barriers, shut down before admission, then detect any late old-queue push or pending record. Additional replacement-stream and unchanged-stream controls distinguish stream identity from mere producer availability.
