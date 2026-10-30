# Commit resident controls before changing live DSP

Resident loop and seek controls can meet a placement writer or cancellation after callback entry. Their current reservation step exchanges live Native handles and resets history before the transaction owns its placement claim. A deferred or rejected control can therefore alter audible playback.

Move all destructive DSP exchanges and resets behind the existing successful placement and publication claim. Preserve bounded callback work, source guards, deferred payload identity and off-thread Native preparation and retirement.

## Non-goals

No new scheduler, preparation worker, DSP algorithm or live crossfade is introduced.
