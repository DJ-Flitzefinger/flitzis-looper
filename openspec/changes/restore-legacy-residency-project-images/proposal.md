## Why

Adding the persisted automatic PCM admission flag made the strict arrangement
project-image reader reject older otherwise complete images. Three settled Pair
journals and one settled Copy journal in the reported test project omit only
this new field. Startup consequently treated completed history as unresolved
recovery, retained the shared config-writer fence and prevented normal Close.

## What Changes

- Recognize the explicitly supported historical omissions for source-map intent
  and automatic PCM admission, including their combination.
- Reuse the ProjectState migration: an absent mode with 512 MiB means automatic;
  an absent mode with a nondefault allowance preserves manual admission.
- Preserve original sealed journal bytes and digests and all other strict image,
  recovery, ownership and shutdown checks.

## Non-goals

No permissive loading of arbitrary incomplete or unknown images, journal rewrite,
forced settlement, config-fence bypass, general unknown-recovery Close redesign,
audio behavior change, SYNC implementation or claim of device acceptance.

## Realtime

Image validation remains in Python startup/control persistence. No callback,
Native DSP, allocation, file access or GIL boundary changes.
