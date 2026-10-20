## Why
Derived BPM control failure can leave an already accepted override unwritten, or
persist a BPM Lock flag whose Native publication was refused.

## What Changes
Project and persist each admitted primary BPM change before derived publications,
and stage BPM Lock state until Native accepts its flag.

## Non-goals
No rollback of admitted Native state, automatic replay, new timing or audio path.

## Impact
Python control intent/persistence only. Native owns timing, effective audio and
existing source checks; realtime callback work remains unchanged.
