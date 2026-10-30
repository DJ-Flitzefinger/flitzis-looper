## Why

With MultiLoop OFF, START/STOP can restore an old PAD group. Turning MultiLoop
OFF leaves peers running and recreates the BPM Lock reference from selection.
The performer needs one actual playback target and stable independent Lock.

## What Changes

- Keep one source-bound last effective explicit PAD target across ordinary STOP.
- Make OFF START/STOP singleton while preserving global recording and STOP ALL.
- Admit runtime OFF, peer stops and old-launch fences as one Native transaction.
- Keep BPM Lock independently available and retain its qualified master owner.
- Require matching Native API3 before App/device startup.

No source-SYNC, second clock, compatibility migration or private-data cleanup.

## Impact

Existing scheduler, source/content, ordinary start/resume, runtime-image and
qualified BPM Lock owners remain authoritative. GUI, MIDI, source replacement,
Move/Swap, recording and pending/cancel/retirement paths receive targeted checks.
