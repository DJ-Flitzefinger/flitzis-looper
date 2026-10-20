## Why
Persistent Windows sharing failures can starve later eligible material retirement.
Ordinary stem preparation also ignores its existing cancellation owner while
reading component ranges, doing avoidable I/O and registering canceled readers.

## What Changes
Rotate pending material work fairly through the existing bounded retirement
worker, and pass original cancellation into existing component range preparation.

## Non-goals
No new audio or scheduler path, deletion authority, private-data migration or DSP.

## Impact
Only background preparation/retirement. Callback work and live reader protections
remain unchanged; disk work, allocation and mutexes remain outside realtime.
