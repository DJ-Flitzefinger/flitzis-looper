## Why

The user selected Beat This! as the planned replacement for new beat/downbeat analysis.
The existing qm-dsp-derived detector does not provide the intended model-based foundation,
and its downbeat timebase defect also limits its value as a comparison reference. Model
selection is settled: Beat This! 1.1.0, `final0`, minimal postprocessing. Windows compatibility,
quality and resource measurements remain acceptance work, not an open-ended model contest.

## What Changes

- Add a diagnostic PCM/job boundary and lazy local worker adapter behind the existing request
  lifecycle; real Beat This frontend/inference follows in B1b.
- Reuse decoded PCM and one source origin; derive 22.05-kHz beat and 44.1-kHz KeyNet inputs
  from shared mono audio without re-decoding or cascading the two analysis resamplers.
- Define explicit setup, verified model provenance, offline loading and independent beat/key
  outcomes. A missing model must not fail sample playback or silently run qm-dsp instead.
- Preserve saved legacy grids, manual corrections and loop markers. Change the default for
  new analysis only in a separate, finite acceptance/cutover task.
- Retain raw detections and uncertainty; a 20-ms prediction interval is not sample accuracy.

Status: B1a's explicitly invoked diagnostic boundary is implemented and validated.
There is no UI/default-routing or saved-analysis adoption change.
No Beat This runtime, inference script, accepted checkpoint manifest or weights are installed.
The unconfigured adapter reports beat-unavailable while native KeyNet can finish independently.
Real setup/inference is B1b, default cutover is B2, and versioned trusted-map storage remains
coordinated with `prepare-versioned-source-beatmaps`.

## Non-goals

No Quantize/SYNC activation, live warp, automatic transient snapping, reanalysis of saved
projects, key-model replacement, hidden `small0` substitution, plugin hosting or Rust app port.
No removal of current mandatory Torch/Demucs dependencies: optional Beat This packaging alone
does not slim the current base installation; separator/dependency work remains program slice 7.

## Realtime Constraints

PCM export, process management, resampling, inference, validation, cache writes and object
retirement occur outside the audio callback. No callback allocation, blocking, GIL access,
disk I/O or model loading is introduced. The worker uses bounded jobs/resources and existing
request identities; live audio receives only existing safe metadata/state boundaries.

## Impact

Modify `audio-analysis` and the shared-preprocessing/parallelism requirements in
`musical-key-cnn`; scope `qm-dsp-bpm-detection` to explicitly selected legacy/diagnostic
use; add optional analyzer setup requirements to `distribution-setup`.
The current implementation adds focused Rust PCM/job modules and Python contracts, process
supervision and diagnostic result events. Normal analysis routing and persistence remain intact.
Future stages add real inference/setup, result adoption and provenance storage. KeyNet stays in Rust.
See [research decision](../../../docs/beatmap-sync-research.md) and the pinned sources in design.
