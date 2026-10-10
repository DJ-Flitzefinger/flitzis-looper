# Design

## Published seams and responsibilities

At baseline `694cd61e50a3fc81bc6a41d88269f6458c4ae269`,
`controller/transport/global_playback.py` selects the remembered stopped group
or current active-minus-paused group, captures current source/accepted timing
and each effective loop region, and waits on existing residency. The native
`global_playback_batch.rs` binds current source/timing/launch revisions;
`audio_stream.execute_global_playback_batch` reserves complete group voice,
retirement and feedback capacity before mutation. This is the authority to
extend, not logic to duplicate in a record renderer. `MAX_VOICES = 32` remains.

`audio_stream.render_scheduled_audio` splits rendering at native execution
frames; `render_mixer_segment` renders exact frame slices. Mix output already
includes selected stems, rates/KEYLOCK, per-pad Gain/Trim/DSP/velocity, summing,
Master Volume and momentary mute. Capture copies that float mix after these
operations and before the device sample-format conversion. It records output
rate/channels rather than source-file rate/channels. Tapping a whole callback
after rendering would incorrectly include prelaunch frames when launch is
quantized inside that buffer and is insufficient.

Python owns settings, take intent, path/preflight and UI projection; Rust owns
effective capture start/end/session and scheduler execution. A focused recorder
controller/worker and focused native capture module prevent unrelated growth of
`AppController`/`RtMixer`. No generalized job or transport framework is needed.
Reuse suitable FFmpeg discovery from `controller/stem_generation.py`, existing
settings/persistence and UiContext patterns. Diagnostic capture helpers can
provide independent file/frame oracles, not a second realtime owner.

## Capture and writer ownership

A control-side prepared take owns a unique session ID, immutable selected
format/quality, output geometry, exclusive output path, preallocated fixed block
pool/SPSC data ring and writer-ready permit. Prepare worker/open float32 spool
and verify required selected encoder/container support before launch admission.
Large handles and blocks retire outside the callback using the existing
retirement boundary. Queue/control/terminal-feedback capacity is reserved; a
full data ring cannot hide the failure notification.

Capture records `[first_output_frame, end_output_frame)` with absolute u64 frame
indices. Segment copying begins only after effective start and stops before
effective end; the worker verifies contiguous extents/session/geometry while
draining. All captured memory and in-flight blocks have a fixed admitted byte
bound independent of take duration. Actual allocation/held-block peaks and
writer throughput must be measured. Duration grows disk spool, not a full-take
RAM array. Ring overflow, nonfinite mix, writer/disk or stream failure marks a
failed take, stops capture safely, keeps playback running and preserves the
valid prefix as explicitly incomplete rather than a successful gapped file.

Stop acknowledgement precedes drain, actual file close, encode, codec/frame
verification and atomic final publication. Finalizing is observable; success
requires the closed verified file. Startup recovery finds only recorder-owned
unique incomplete spool/metadata under `record/`, offers explicit recovery or
removal and never claims they are completed takes or overwrites any file. During
shutdown request capture stop, receive/quiesce the terminal extent, drain and
finalize before audio teardown; a failure/deadline preserves recoverable
incomplete data and reports it. Worker waits happen outside realtime. Unknown
files are untouched. `/record/` is added to `.gitignore` with implementation.

## Atomic left launch and gesture states

Idle left click prepares the writer before a single source/intent/session-bound
native transaction. It reserves all existing global-group resources plus capture
state/feedback and admits both at one Rust-selected execution frame. Every pad
starts at its freshly validated effective loop start, with existing stem masks,
rates and DSP intent; this group behavior works with MULTI LOOP ON or OFF. A
source/loop/restore revision change, missing readiness, queue/voice pressure,
writer failure or stale permit admits neither partial group nor take. Deferred
preparation cannot substitute new content or an unrelated newer group silently.
No extra voice slots or implicit STEM generation are introduced.

The small shared button layout/interaction helper retains START/STOP behavior
but RECORD recognizes one left/right press edge, never held-right repetition.
Left and right gestures on one frame cannot double-dispatch; use one explicit
priority (right stop/toggle) and test it. RECORD states are Idle, Preparing,
Recording, Finalizing and Failed with native effective/pending/error distinction.

- Idle right click prepares/adopts capture only at the native next admissible
  output frame, including silence when no pad is active. It never restarts pads.
- Recording right click requests capture stop then finalization only.
- Recording left click requests capture stop then finalization only; it does not
  restart the group or create another take. This is the bounded unspecified
  repeat-left choice made without a Human question.
- Preparing/Finalizing left or right clicks report busy and admit no extra take
  or playback change. A failed take retains error/recovery evidence; a new take
  requires an explicit subsequent gesture after failure is acknowledged.
- Empty left group reports no active/remembered group and starts no take; zero
  captured frames on a free right take report empty and create no successful file.
- Standard START/STOP remains independent while recording/finalizing. Its stop
  does not finalize the take; capture continues with the resulting mix/silence
  until a recording stop, shutdown or failure.

## Persisted formats and conversion policy

Use the actual engine output rate/channels and retain them when supported by the
selected codec. Defaults: WAV IEEE float32 (RF64-capable), FLAC 24-bit with lossless
compression level 8, MP3 CBR 320 kbit/s. Supported settings explicitly distinguish
WAV PCM16/PCM24/float32, FLAC 16/24-bit precision and compression 0..8, and MP3
CBR 128/192/256/320 kbit/s. FLAC compression is not audio fidelity. No hidden
normalization, attenuation, source-rate substitution or implicit format fallback.
Unsupported geometry/encoder fails writer readiness before an atomic left
launch. Missing/invalid recording-only fields use these defaults with a visible
settings diagnostic while preserving unrelated project/pad state.

The float32 spool preserves the exact mixed values, including overrange peaks.
Integer/lossy formats apply the explicitly documented fixed full-scale
quantization/saturation required by that format, with peak/clipped-frame counts
and a warning; they never normalize the take. Verify the actual codec, precision
or bitrate, rate, channels, duration and successful close. MP3 delay/padding and
gapless trim metadata are explicit verification inputs; lossy samples are not
described as bit-identical PCM. WAV uses RF64 or an equivalently verified large
WAV container before the RIFF size limit; no artificial 4-GiB/duration cap.

## Bounded deliveries and real acceptance gates

| Delivery | Complete bounded result and prerequisites | Fresh evidence required |
| --- | --- | --- |
| X11-RECORD-CAPTURE | Native session/frame-bound segmented mix capture plus non-RT float spool/drain/retirement and shutdown API. Uses existing clock/scheduler/mix and accepted group seams; user-facing atomic group integration remains CONTROL. | Hardware-free productive native render with irregular callbacks, quantized starts/stops inside callbacks, exact first/exclusive end and independent PCM oracle for gains/DSP/master/stems/KEYLOCK; allocation/block/queue peak, overflow/slow writer/disk/stream/session-stale/teardown proofs with unchanged playback. |
| X11-RECORD-FORMAT | After CAPTURE, persisted format/quality and writer-ready preflight, real encoders/container verification, unique record outputs and recovery/ignore lifecycle. | Actual independently probed WAV/FLAC/MP3 files, precision/bitrate/rate/channels, capture-vs-decoded extent and MP3 trim handling, malformed settings, unavailable/broken encoder, overrange, disk failure, stop/drain/shutdown/reopen and no overwrite; actual long RF64 spool/container path with bounded RAM. |
| X11-RECORD-CONTROL | After CAPTURE and FORMAT, RECORD layout/single-edge states and one transaction capture+existing active/remembered group at effective loop starts. | Productive controller/native ticket/scheduler tests for MULTI LOOP ON/OFF, remembered/current groups, existing stem masks, current/old source permits, pending residency/queue/voice/stale races, no partial effect, empty/repeat gestures, independent START/STOP and actual first recorded output frame equal to group start. |

Each delivery needs focused checks appropriate to its real Rust/audio/persistence
impact, docs, strict OpenSpec validation and independent nonauthor source review.
Later R2/B7 review rechecks integration with broader control/atomicity changes;
there is no dependency on unfinished whole R2/B7 before using the already
productive global batch. V0/H-LIVE/H-FINAL/C-FINAL include the complete feature.
Actual Human GUI/device/recording/listening remains final and cannot be replaced
by synthetic output, a hash or a headless encoder success. No such run is made
or accepted in this plan-only step.
