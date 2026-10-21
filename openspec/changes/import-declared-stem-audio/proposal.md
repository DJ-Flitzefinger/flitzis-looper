## Why

Loading a generated project stem through Load Audio currently rejects its declared WAV artifact as a non-original. External stem WAVs already import successfully. A performer must be able to use either as an independent PAD source, including replacement of a Re-Arrange copy.

## What Changes

- Permit explicit import of the five declared project stem WAV leaves through the existing source capture transaction.
- Keep the managed input pinned until capture finishes and publish only a canonical original with its own acknowledged assignment.
- Preserve strict restore and namespace rejection for metadata, PCM, unknown paths and unsafe aliases.

## Non-goals

No direct assignment of a stem generation, separator/model work, audio callback changes or broad artifact import. The separate reported transient PAD disappearance is not assumed to share this cause.

## Impact

The existing cold loader, ProjectAssets input pin, material capture, original delivery and callback ACK remain authoritative. Work stays off realtime. Existing direct-stem cleanup classification remains in place.
