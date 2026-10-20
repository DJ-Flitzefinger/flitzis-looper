## Why

Startup currently admits asynchronous restored sources before applying saved
runtime controls. Saved manual or legacy BPM and grid initialization then advances
the same source epoch captured by an unarmed cold worker. Depending on worker
timing, reopening an unchanged project can therefore cancel its own restore and
leave the PAD without a current complete source.

## What Changes

Finish the saved runtime control and timing initialization before admitting any
ordinary cold restore jobs. Each restore retains its original immutable request,
initial pitch, resolved Key Lock and resident selection. Keep fresh Automatic
timing verification after genuine source acknowledgement.

## Non-goals

No relaxed epoch/source/request/proposal cancellation, retries of stale requests,
new persistence or restoration queue, private-data reset, device work or realtime
I/O. This changes the existing control-thread startup order only.
