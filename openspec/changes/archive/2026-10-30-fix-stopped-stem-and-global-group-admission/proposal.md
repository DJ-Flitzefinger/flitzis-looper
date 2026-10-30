## Why

A stopped PAD with an admitted V1 stem selection rejects its own V2 candidate
because preflight requires an outgoing ramp reader that stopped adoption never
uses. Prepared global starts also leave terminal feedback, voice and retirement
capacity failures pending instead of rejecting the complete group.

## What Changes

- Validate stopped stem adoption against the actual transition-free target.
- Reject global capacity failure once the complete original group can retire.
- Retain zero-capacity custody and distinguish timely recovery from late refusal.

## Non-goals

No weaker source, candidate, launch, identity, timing or reader guards; no new
scheduler, callback allocation, blocking, I/O or Native preparation.
