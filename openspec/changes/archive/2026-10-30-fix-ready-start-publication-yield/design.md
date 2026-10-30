## Existing authority and bounded change

PadPlaybackController captures one original CapturedPadStart before residency.
ResidencyController retains its original native dependency and asks the existing
Native resident preparation path to create or join an exact window member.
That path can return a ticket whose genuine publication status is already
accepted. The controller currently yields unconditionally after this operation.

Remove only that unconditional yield. Once preparation installs its non-optional
Native ticket, re-enter the existing poll path once during the same call. Because
that ticket is now present, this bounded re-entry cannot prepare again. The
existing terminal, deadline, assignment and attempt checks run before publication.
The existing status check must still
reject terminal windows, retain preparing/pending/adopting windows, and publish
only an accepted exact ticket with the same original capture. There is no new
readiness flag, copied numeric authorization, scheduler or asynchronous owner.

Preparation and publication each consume one existing admission attempt. The
same-call path must check the remaining total budget before publication so an
eighth successful preparation cannot cause an unauthorized ninth attempt.
The original deadline, source/assignment checks, Native final publication
guards, cancellation and requested/effective reporting remain authoritative.

## Evidence and limits

Focused Python regressions prove same-call publication for an accepted ticket,
continued waiting for real ACK, unchanged owner/ticket reuse, a preparation that
crosses its original deadline, and successful eighth versus rejected ninth
admission attempts. Existing controller tests cover
independent dependencies, Stop, stale owners, queue retries and feedback state.
These mocks establish controller routing only. Native PCM, actual T/E timing,
first-attack completeness and Human audio acceptance remain independent gates.
