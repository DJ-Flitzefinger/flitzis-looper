## Why

The Human requests fresh defaults of1/16 trigger step, FLAC24/compression8 and
the existing BS-RoFormer MUSDB18HQ separator.

## What Changes

Change only the three authoritative model defaults. Quantization stays disabled
initially; FLAC24/8 already exists. Preserve explicit saved choices, other settings
and all current source/recording/stem ownership. Add no migration or runtime work.

## Impact

Fresh ProjectState/RecordingOptions factories and missing-field defaults use the
new choices. Existing default/roundtrip/settings tests verify them. Fixtures with
WAV- or Demucs-specific assertions declare that input explicitly. Native1EB/API3,
encoder implementations, model assets and inference paths remain unchanged.
