# Design

## Published seams and responsibilities

In the current source reconciliation (2026-10-20),
`controller/transport/global_playback.py` selects the remembered stopped group
or current active-minus-paused group, captures current source/accepted timing
and each effective loop region, and waits on existing residency. The native
`global_playback_batch.rs` binds current source/timing/launch revisions;
productive starts proceed through `pitch_runtime::accept_group` and the final
`mixer/ordinary_global.rs::execute_ordinary_global_start` whole-group claim.
The existing prepared path owns replanning, the actual effective output frame,
complete group voice/retirement/feedback admission and original participant
validation. Later atomic recording must join that final claim and effective
frame, not the original target, enqueue time or a second independent command.
This is the authority to extend. `MAX_VOICES = 32` remains.

`audio_stream::render_scheduled_audio_tracking_pads` splits productive rendering
at native execution frames; `render_mixer_segment` renders exact frame slices.
`render_scheduled_audio` is now only a test wrapper. Mix output already
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

Ordinary audio telemetry is best effort when its queue is full and therefore
cannot be the recording terminal authority. Reserve a session-owned sticky
terminal observation independently of data and telemetry capacity. Preserve
separate effective capture/end acknowledgement, submitted extent, writer extent
and closed/error observations; an acknowledged stop alone is not a closed spool.
Stale session commands cannot stop or complete a newer take. Use the existing
`AudioCallbackState`/`CallbackCloseOwner` off-realtime image retirement for final
held blocks and stream teardown; no last heavy owner may deallocate on callback.

Capture records `[first_output_frame, end_output_frame)` with absolute u64 frame
indices. Segment copying begins only after effective start and stops before
effective end; the worker verifies contiguous extents/session/geometry while
draining. All captured memory and in-flight blocks have a fixed admitted byte
bound independent of take duration. Actual allocation/held-block peaks and
writer throughput must be measured. Duration grows disk spool, not a full-take
RAM array. Ring overflow, nonfinite mix, writer/disk or stream failure marks a
failed take, stops capture safely, keeps playback running and preserves the
valid prefix as explicitly incomplete rather than a successful gapped file.

The delivered `MixCaptureTicket` is an opaque original-engine/session observation;
all start/stop/status operations must keep that same ticket. A queued start or
stop is not an effective-frame acknowledgement. FORMAT keeps one Finalizing owner
until its actual worker, file verification and publication finish: Native alone
permits a new prepare after RT release and writer join, which retires the old
ticket's status access. Before encoding, require the original live status to have
terminal decision, RT seal/release, worker close/join, no failure, a nonempty
successful spool and mutually matching final extents. `sealed_candidate` metadata
alone cannot grant this permission, including after a final metadata I/O failure.
The inspector checks headers/length only; the caller must hold stable file identity
and independently scan the complete finite float payload before publication.

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

The final capture start authorization uses START_CLAIMED=0x40 in the existing
AtomicU8 decision word, alongside FINAL_DECISION=0x80 and failure codes masked by
0x3f. It is a private permission bit, never a new clock, registry or effective
ACK. Geometry/readiness checks precede one start CAS; failure before that CAS
rejects, while failure after it remains a runtime failure without rolling back
accepted playback. Control failure preserves the permission/final flags with at
most two strong CAS attempts; offRT worker failure and the final writer decision
preserve both flags. The permission capability exclusively borrows its original
CaptureRt until its consuming publication. A different owner, intervening RT
mutation or repeated publication is unrepresentable in safe Rust; a debug-only
precondition or a new fallible post-effect check is insufficient. Existing
standalone start wraps claim plus infallible publication. Combined group commit
must claim capture last after member claims,
abort all members if that fails, then publish voice stores and capture fields
infallibly, with shared.started Release last. Spool headers keep ordinary failure
codes and no permission bit. The separate group-event/stop reservation and
attachment/cancellation integration remain required before LEFT acceptance.

The optional original capture publication travels in the existing captured global
batch and transfers only on the same successful FIFO admission. Readiness pending
retains both original owners. The control lock order is capture owner, pad request
IDs, then producer. A transferred duplicate batch retires inert; it cannot reject
an accepted receipt or cancel an original pending/active capture. Installed
association uses the original capture session and existing runtime group slot and
action sequence, not session equality alone.

One existing PitchAction carries the group and logically reserves its own stop
slot. No extra CaptureStart event or clock is introduced. Replanning retains the
original input order and any already admitted stop S. Before mutating member plans,
S before proposed common E rejects the whole empty capture/group; S equal to E
starts the group before its own stop and yields an empty capture; S after E stays
unchanged. The scheduler's retained order_sequence establishes the equal-frame
order even after reinsertion. A stop encountering an uncommitted group settles
that exact group and empty capture instead of losing its endpoint. Retirement
pressure retains terminal owners and cannot defer that due stop.

All rejection, cancellation and teardown paths settle both original owners and
preserve foreign events. Pre-install settlement may need two heavy retirement
units; without them the original message remains held. A lifetime-bound capture
claim may borrow the identical Box temporarily outside the mixer slot while
existing disjoint voice effects apply, provided every normal path restores it and
no heavy owner drops on RT. Restoring that custody pointer is not an audio effect;
the effective ACK still follows every combined audio-effect store.

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

The existing Native capture start/stop control messages carry a scalar explicit
frame or NextAdmissible target. Python optional target_frame=None expresses the
latter; explicit integers retain their old meaning. OffRT request ownership is
recorded only after successful FIFO insertion. The original stop mode/frame is
immutable for idempotent retry; changing explicit/next mode cannot retarget it.
Callback placement, deferred-command and capture-retirement gates retain the
unresolved choice. Only actual admission resolves NextAdmissible to the callback's
first unrendered frame F, without +1. Existing scheduler/reserved-stop/CaptureRt
still use u64 frames; effective first/end ACKs can differ from the requested F
when ordinary due-event deferral requires later execution. This component does
not provide the separate atomic left group transaction or controller/UI wiring.
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
settings diagnostic while preserving unrelated project/pad state. Recording choices
are one immutable `RecordingOptions` value in `ProjectState`; Settings replaces
that value through its existing controller and dirty callback. A prepared take
retains its original value. Selective field recovery retains valid sibling choices
and reports through a nonblocking persistence notice, the existing console sink
and Settings. This notice is separate from the existing unsafe-metadata `load_error`
save fence; recording recovery neither creates nor clears that fence.

The float32 spool preserves the exact mixed values, including overrange peaks.
Integer/lossy formats apply the explicitly documented fixed full-scale
quantization/saturation required by that format, with peak/clipped-frame counts
and a warning; they never normalize the take. Verify the actual codec, precision
or bitrate, rate, channels, duration and successful close. MP3 delay/padding and
gapless trim metadata are explicit verification inputs; lossy samples are not
described as bit-identical PCM. WAV uses RF64 or an equivalently verified large
WAV container before the RIFF size limit; no artificial 4-GiB/duration cap.

Conversion reads/writes fixed-size blocks, never a whole take array. Float32 WAV
retains the input payload bits, including signed zero. For integer precision B
(16 or 24), clamp each finite input to [-1, 1], multiply by 2^(B-1), round ties to
even and clamp to the signed B-bit range; no dither or per-take scaling is applied.
MP3 receives that fixed 24-bit PCM input before its explicit lossy encoder. Count
original samples/frames strictly outside full scale separately from exact endpoint
quantization; report their peak and integer/lossy saturation. Float32 output keeps
those overrange values. Nonfinite input or any partial/extra frame fails the take.

The existing owned Windows Job supervisor remains unchanged. An owned Python
child can open the exclusive seekable media file and pass it to FFmpeg as `fd:`;
outer stdout/stderr remain bounded diagnostics. Actual selected codec/geometry
preflight and final reopened verification must check the requested output rate,
channels and options, without automatic downmix/resampling/fallback. The finite
Windows prototype demonstrated MP3 trim metadata with seekable `fd:` output and
its absence with the ordinary `pipe:1` control; it does not establish complete
recording, payload verification or actual large-file acceptance.

Every encoded file and verification sidecar keeps its original producing handle
against independent Windows write/delete opens while the encoder/probe inherits
that same handle. Capture FileID and digest before closing the producing handle;
the subsequent retained read pin must match both before any evidence is consumed.
Keep that pin through the complete stream/packet/decode/payload verification.
Choosing a new identity by reopening a pathname after verification is insufficient.
Exclusive namespace holding protects the original ancestors, attempt and owner
record while worker-created leaves are opened; it does not itself pin their bytes.

Preparation and codec wrappers each own retirement from their first local file
work through their last read/pin release. Compose private child-tree retirement
with that outer lifetime; tree exit alone is not wrapper retirement. Explicitly
release locally owned Native references in finally even when an exception or
traceback remains retained. Caller-owned immutable codec image witnesses remain
with the prepared take. Native terminal status revokes writer_ready; complete
sealed/joined capture agreement requires that actual False projection rather than
fabricating an admission-ready terminal writer.

The performer controller owns one serial operation through an actual per-take
worker return and join, including all outer namespace, readiness-image, live-file
and exception-held references. Private preflight/codec/finalizer retired events
are local facts and cannot alone permit another take. Worker results contain
metadata, not retained exceptions, tracebacks or custody references. Failed can
remain busy; acknowledging its diagnostic cannot discard an unresolved owner.
An ordinary completed take returns to Idle with retained completion metadata.

From writer-ready, retain its original read/FileID witness while sharing Native
writer access and denying delete; retain it through finalizer's stronger closed
spool pin and all outer cleanup. The initial witness does not authenticate bytes:
the unchanged original writer SHA and complete final finite scan remain required.
No default or no-op live witness is allowed. Main owns original press-time left
intent admission and native start/stop requests; the worker cannot recapture that
gesture or restart playback when readiness arrives. Shutdown polls recorder work
before unrelated short-circuit readiness checks and keeps audio alive for actual
terminal extent/finalization. Deadline cancellation never fabricates join, and
dead-stream recovery requires actual engine/callback quiescence.

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


## Capture terminal decision and recovery checkpoint authority

The existing shared failure word also holds the writer terminal-decision bit.
After the sealed extent is drained and payload flush/sync finishes, the worker
atomically freezes control cancellation against that decision. A control failure
accepted before it remains the first failure; one arriving later reports refusal.
Worker I/O or panic may still add the first failure after the decision. The
terminal decision, actual file close, worker join and RT pool release remain
separate observations. No callback lock or second scheduler is introduced.

The fixed dual header declares a `sealed_candidate`, never publication success.
A last header write can leave valid bytes even when its subsequent sync/seek
fails; unavailable storage cannot be assumed able to save its own diagnostic.
The live session retains WriterIo and cannot claim successful spool completion.
Read-only recovery inspection always requires independent payload/format
verification and grants no recording publication authority. FORMAT owns that
verification and final-file publication. Normal engine shutdown's quiesced
callback fallback preserves incomplete data; the later CONTROL graceful shutdown
still needs its stop/drain/finalize ordering before ordinary audio teardown.

The existing off-realtime writer also hashes each complete successful payload
write under its original exclusive writer handle. After contiguous payload
flush/sync and terminal decision it publishes one fixed 32-byte immutable witness
in the original live session. This changes fixed Shared/worker memory, not ring
or scratch dimensions; the exact allocation profile is refreshed. A later final
header failure may retain the byte witness diagnostically, but live failure
still prohibits success. No payload digest or source authority is added to the
recovery checkpoint. Finalization requires complete live terminal agreement and
compares its full finite source scan with this original writer witness before
encoding, rejecting same-FileID finite edits in the close-to-pin interval.

The successful nonreplace link is the final publication decision. All namespace,
source and encoded verification precedes it; no subsequent stat/read can revoke
that observed commit. If custody release then reports failure, typed outcomes
retain the committed path/FileID and verified completion, while outer retirement
remains explicitly uncertain. The future high-level recorder must preserve that
published outcome and pending custody rather than reporting a missing failed
take or admitting a new owner. Unknown data is never deleted to resolve it.
The same uncertainty applies when an actual Windows namespace pin close reports
an error before commit. It is classified at the close operation, separately from
ordinary validation or I/O rejection after successful custody release. Such a
pre-commit error retains incomplete evidence and leaves the public retirement
signal unset; it does not fabricate either publication or completed retirement.

Startup discovery is a read-only streaming observation seam for the future same
serial recording worker. It considers immediate .incomplete-<uuid> entries only,
reads bounded owner metadata under existing ordinary-path/Windows pin checks and
releases each entry's pins before yielding. Exact canonical owner bytes/options/
observed FileIDs establish present consistency only: unsigned metadata can be
changed in place and cannot authenticate the original capture after restart.
An encoded/final same-FileID pair is an observed unverified publication; different
identities are a preserved conflict. No ticket, completion, publish or deletion
capability is reconstructed. Discovery does not encode, hash whole media, repair
or clear custody uncertainty. Explicit selected-content verification, recovery
actions, controller startup/shutdown and visible feedback remain integration work.
The root and ancestor pins remain held with the scanner, closing on exhaustion,
explicit close, cancellation or failure. Windows directory handles request the
same GENERIC_READ access used by Native directory_guards, with READ|WRITE sharing
and no DELETE sharing; an attributes-only handle did not prevent an empty-root
rename in the actual regression. No child-file handle is assumed to protect an
otherwise unpinned root before the first entry is examined.
