# Independent optional key admission

## Why

Combined key preparation can exceed512MiB while complete-source primary BPM fits.
The current early admission then prevents both results. Omitting a key also breaks
the required result DTO, and key admission clears manual correction before its
optional work has succeeded.

## What changes

Admit mandatory complete-source BPM independently, keep optional key concurrency
within the same limit, report its component outcome and preserve key intent until
matching successful completion. Reuse current analysis/source/request owners.

## Non-goals

No estimator change, source cropping, budget increase, live SYNC, compatibility or
migration work, new scheduler, device session or private-data cleanup.
