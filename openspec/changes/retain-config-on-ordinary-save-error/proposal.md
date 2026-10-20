## Why
An ordinary debounced Config save can abort the GUI when atomic replacement raises
an operating-system error. The previous Config and dirty intent already survive,
but the optional save boundary does not retain a usable retry and diagnostic.

## What Changes
Keep optional saves usable after an operating-system write failure, retain current
dirty intent, retry automatically after ten seconds and report the deferred save
through the existing control-thread console sink.

## Non-goals
No Config reset, file-permission changes, new writer, worker, audio path or cause
claim for a Windows replacement failure. No change to Native ACKs, timing
verification, transaction fences or raw commit failure propagation.

## Impact
Only the existing Python optional-save wrappers and console observation change.
All disk work remains outside the realtime callback; no callback allocation,
blocking operation, Python/GIL access or retry loop is introduced.
