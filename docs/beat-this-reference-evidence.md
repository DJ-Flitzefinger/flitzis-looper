# Beat This B1b reference evidence

Date: 2026-10-05. This is diagnostic engineering evidence for the selected
optional worker. B2 musical-quality/default-cutover acceptance and B8 audible
live-performance acceptance remain separate gates.

## Reproducible identities

- Beat This 1.1.0 wheel SHA-256:
  `3f2b2d1e027c6dac380bf80c71555e3c28a4036a7f1af20129a945915a72a645`.
- Accepted `final0` SHA-256:
  `8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331`.
- Worker lock SHA-256:
  `e7d180bd53a02736693e0054dfb62a868da77478922b8ca3027c07fe12f98a75`.
- Native release extension SHA-256:
  `bfae73f3d43f9dcade1eac21e4df21ac1a5abd7d03c81dd5d5e806af4c82bcfb`.

The six loaded upstream source modules (`inference`, `preprocessing`,
`beat_tracker`, `postprocessor`, `roformer`, `utils`) are byte-identical across
the installed wheel and inspected commit
`b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c`. The release tag resolves to
`ad7974846029835307ba19a3d5cefbf40b243041`; version names alone were not used as
proof of source equivalence. Per-file hashes remain in local
`scratch/b1b/source-parity.json`. See [setup](beat-this-setup.md) for immutable
primary links, exact runtime and installation provenance.

## Reference checks

The complete first/last-impulse, silence and fractional-frequency tone fixtures
at 22050/44100/48000 Hz match upstream resampled PCM and log-mel tensors exactly.
soxr's half-up output-length rounding is preserved, including odd 44100-Hz
frame counts. Centered Hann STFT uses FFT 1024, hop 441 and 128 Slaney mel bands.
No offset fitting, leading-silence removal or end trimming is performed.

The real-checkpoint test compares both raw logit arrays and minimal-postprocessor
positions directly with upstream. The non-round 4-second fixture and the
32-second fixture exercise short and overlapping 1500-frame chunk paths. All
reference logits remain present; only detections outside the original exclusive
source end are excluded. Clips with at most 512 resampled frames report failure
because the reference reflect padding cannot process them.

## Resource observation method

The local harness uses a Ryzen 9 9955HX3D (16 cores/32 logical processors),
33,483,366,400 bytes physical RAM and the recorded native release build. It loads
private audio once through Rust, starts a muted playing voice, and invokes
`OfflineAnalysisService` on the immutable loaded PCM. Private audio, full raw
predictions, telemetry and fault-injection outputs stay outside Git in
`scratch/b1b/`.

Each analysis starts a fresh isolated model process. Report first and repeat
fresh-process timings, not cold-disk versus a retained warm model. Checkpoint
preflight already reads file pages before inference. Measure the launcher and
actual descendant worker separately: Windows venv launchers do not carry the
model's memory footprint themselves. Windows peak working set is recorded per
process; private memory is sampled and parent peak is cumulative for that
measurement process. There is no CUDA execution; this does not measure other
applications' GPU usage.

The playback probe records playheads, output-clock progression and control ping
latency. It does not expose callback-duration percentiles or driver underrun
counters, and muted telemetry is not an acoustic recording. No claim of zero
deadline misses, audible synchronization, or multi-pad KEYLOCK/STEMS acceptance
follows from this run.

## Full-track observations

The private reference is a complete 170.630875-second stereo PCM16 source at
48000 Hz. Rust loads it at the active 96000-Hz rate: 16,380,564 complete frames.
The worker consumes that loaded PCM at origin zero, not a separate file decode.
Both runs returned 300 beats, 75 downbeats and 8532 logits per channel; native
KeyNet independently returned `ready/Gm`. These counts/key labels are model
outputs, not manually certified annotations.

| Measurement | First fresh process | Repeat fresh process |
| --- | ---: | ---: |
| Complete diagnostic job | 13.005 s | 13.345 s |
| Actual model-process peak working set | 580.13 MiB | 579.78 MiB |
| Parent-process peak working set | 924.30 MiB | 924.50 MiB |
| Sampled simultaneous combined working set | 1090.49 MiB | 1092.30 MiB |
| Baseline control polling gap p99 | 5.593 ms | 5.588 ms |
| During-analysis control polling gap p99 | 5.754 ms | 5.699 ms |
| During-analysis maximum polling gap | 8.308 ms | 9.931 ms |

Full raw predictions and exported loaded-PCM hashes were identical between the
two runs. Original-source PCM hash equality is not claimed: the native loader
resampled 48 to 96 kHz, so its playback-buffer domain differs from the original
file. Reference fixtures separately prove frontend parity at all three stated
input rates.

There were zero invalid/stale clock observations or unexpected stop events,
with 130/133 during-analysis playhead observations. The requested callback size
was 512 frames, while observed output-clock increments were 960 frames (10 ms
at 96 kHz). This is observed clock cadence, not a callback-duration measurement.
The native API exposes the default endpoint rather than its device name; the
recorded sound-device inventory does not prove which endpoint was selected.

A natural 100-ms tone completed real beat inference in 3.045 seconds while the
native key branch returned `failed/unknown` with `InsufficientData`. This
demonstrates independent outcomes without a substituted model or injected key
exception. It makes no quality claim for beat estimates from such short audio.

## Lifecycle and offline checks

All nine final integration cases completed with actual resource retirement,
removed request/PCM scratch and successful subsequent native admission:

- First/repeat full-track runs returned identical ready outputs.
- The 100-ms case returned ready beat output alongside natural key insufficiency.
- Missing and corrupt checkpoint copies returned `unavailable/missing_checkpoint`
  and `unavailable/checkpoint_hash_mismatch`; no worker started and KeyNet succeeded.
- A deliberately altered identity in a completed real worker response was rejected
  as `invalid_worker_response`; successful key output remained independent.
- Cancellation and unload suppressed the completion event. Observed complete
  retirement after the action was 62.1 ms and 95.3 ms respectively; the API calls
  themselves took approximately 0.03 ms. These are observations at about three
  seconds into the job, not guaranteed deadlines for arbitrary native key work.
- Forced worker crash settled as failure and retired in 62.5 ms. The final
  observed worker trees had no running descendant at `job.done`.

Inference uses verified local files with worker Python networking denied;
missing artifacts never trigger acquisition. Separate regression tests exercise
offline setup policy, wrong hashes, altered installation/source metadata,
timeouts, output bounds, stalled native key work and Windows process ownership.
Eight real Windows lifecycle tests cover a venv launcher plus a silent child,
failed assignment/resume, blocked child creation during termination and retry
after a transient termination failure. Job accounting reaching zero was observed
before process handles signaled; retirement now waits for both.

Raw final evidence is `exports/b1b-reference-20261005.json` and
`scratch/b1b/final-*`; earlier exploratory runs are excluded from the report.
The aggregate records fault injection explicitly.

Full project validation passed: `uv sync`, debug and locked release Maturin
builds, Cargo check, 450 Rust tests (one ignored doc test), 904 application
Python tests, 35 separate worker tests with real checkpoint parity, Ruff,
mypy (108 source files), formatting and whitespace checks. Both affected
OpenSpec changes pass the official strict validator. Editor-change archival
still has a pre-existing base-spec structural warning; no archival was attempted.

## Installed footprint

Installed logical file size is 3,362,965,600 bytes (3.13 GiB), comprising
3,281,865,915 bytes in the isolated environment, 81,058,141 checkpoint bytes and
small project/receipt files. This excludes shared uv-managed CPython and uv cache
outside the workspace, other installations and the separate 3,290,658,137-byte
developer test environment. These are file-length sums, not NTFS allocation or
download sizes. The separately copied existing KeyNet asset is 1,852,747 bytes.
Optional worker installation is therefore not evidence of a small base app.

## Remaining gates

Freeze private annotations, count/downbeat error limits, correction burden and
resource criteria before B2 tuning/cutover. Source-generation identity is still
engine-local; persisted source content identity and cached new-analysis restore
belong to subsequent adoption work. The known legacy downbeat-unit defect must
be addressed if its output is used as a comparator.

The native staging limit excludes FFT/CQT/ORT and model workspace. A 120-second
worker timeout, 8-MiB response bound, 250000-logit bound and 1-MiB final envelope
can reject long material; they never authorize silent truncation. The last
envelope bound is stricter than the raw worker wire bound, so B2 must include
long-track publication in resource acceptance. Native KeyNet calls remain
non-preemptible; cancelling beat work does not impose a whole-request deadline.

No diagnostic result is adopted into saved grids or playback. Variable-map
editor/BPM display, continuous SYNC and per-pad pitch remain later stages.
