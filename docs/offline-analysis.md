# Offline analysis boundary

B1a/B1b provide an explicitly invoked diagnostic boundary and real optional Beat
This worker. Automatic loading, the normal manual Analyze action, the UI and
saved analysis still use their existing paths. The unconfigured adapter reports
`unavailable/missing_worker`. [Explicit setup](beat-this-setup.md) installs the
locked Windows CPU runtime and verified `final0` separately. Default cutover
requires B2 acceptance.

## Entry points and ownership

`flitzis_looper.analysis.jobs.OfflineAnalysisService.start(engine, pad_id,
workdir, model=BeatModelIdentity())` starts a diagnostic request for a loaded
pad. The caller supplies an absolute local scratch directory and retains the
service/job. Use `job.snapshot()` for nonblocking state, `job.cancel()` to
invalidate work and `service.shutdown()` to stop admission and request
cancellation. Shutdown reports current state; it does not join native inference.
`job.done` is set only after actual component and resource retirement.

The service obtains `AudioEngine.begin_offline_analysis(pad_id)` and an
`OfflineAnalysisJob` reservation. Native methods expose metadata, prepare a
temporary PCM export, run KeyNet, check cancellation, publish progress and finish
one validated envelope. After both branches actually settle, the supervisor calls
`retire_pcm()` to close native PCM owners before removing the job directory.
That call preserves admission until filesystem cleanup and `finish()` complete;
a refusal keeps files, admission and unpublished results in the retiring state.
`finish()` returns whether the request was accepted under
the native identity lock, so cancellation cannot leave a stale local success snapshot.
Heavy native calls release the GIL. No whole-track
Python array or viewport waveform reconstruction is involved. Loaded-source
generation and request IDs are separate; a new diagnostic request cannot make
an old loaded source appear to be a new source.

`analysis_pcm.rs` pins the loader's immutable interleaved `Arc<[f32]>` at its
actual loaded rate. It validates complete frames, rates from 8000 through
384000 Hz and 1 through 32 channels. One arithmetic channel mean, accumulated
in f64 and stored as f32, creates shared mono. Nonfinite input is rejected.
Frame zero, leading silence and the final frame remain part of the input.
Mixing and float32 little-endian export stream through bounded 4096-frame
chunks with cancellation checks; no complete loaded-rate mono Vec or key-input
copy is allocated. The transfer describes a complete mono file at the loaded
rate with origin `0.0`; JSON carries metadata, never PCM samples. Only a complete
flushed file with a retained readable native key handle becomes prepared. The
analysis pin on the loaded source then drops off-thread before key allocation.
Independent playback ownership keeps the original source alive and unchanged.

The native key branch reads that same staged file through its native handle in
bounded chunks and derives a complete 44100-Hz mono vector with the existing
Rubato FFT converter (`1024`, one subchunk/channel, `FixedSync::Input`). It reads
exact f32 samples without conversion at equal rates, flushes the delayed tail,
removes leading algorithmic delay once and returns
`ceil(source_frames * 44100 / loaded_rate)` frames. A dimension-derived finite
padding budget permits valid flush calls that emit zero frames while filling
the converter's internal block; cancellation is checked between those calls.
The worker independently
derives 22050-Hz input with the pinned upstream soxr HQ/log-mel frontend,
including the reference's rounded resampled length and centered STFT. No key
input is derived from downsampled beat input. KeyNet still receives the complete
vector with unchanged CQT/ONNX parameters; chunking does not shorten analysis or
reset the source-time origin. See the setup document for exact short-input and
exclusive-end conventions.

## Admission and retirement limits

| Resource | Diagnostic policy |
| --- | --- |
| Native jobs | One active or retiring request per engine; zero pending queue. |
| Service jobs | One active or retiring request per service; zero pending queue. |
| Beat worker trees | One active or retiring worker request globally, including launcher descendants; zero pending queue. |
| Windows tree members | At most 64 simultaneous processes; child admission stops before terminal capture/termination. |
| Native PCM staging | 512 MiB maximum simultaneous PCM ownership across the export and key stages; includes all live analysis source pins and PCM buffers. |
| Worker PCM | 512 MiB maximum complete exported input. |
| Checkpoint | 256 MiB maximum local file, checked in cancellable 1-MiB hash chunks. |
| Wire messages | 32 KiB request, 8 MiB worker response, 1 MiB final publication envelope. |
| Prediction arrays | At most 250000 positions or logits per bounded array. |
| Process timing | 120-second execution timeout; 5-second finite reap and output-reader join attempts. |

Native admission reserves the larger of two non-overlapping stages: retained
interleaved source plus bounded export buffers, or the complete key output
allocation plus bounded file-read/resampler PCM buffers. The final vector contains
exactly the ceiling-derived frame count; converter delay/tail handling uses
bounded output buffers instead of a full delayed-output allocation. The source
pin counts throughout export and can leave this accounting
only after the analysis owner actually releases it. The exported file is
independently capped at 512 MiB. Byte arithmetic and each required stage are
checked before allocation; oversized work fails explicitly without truncation.

These are engineering limits, not proof of complete resource acceptance. The
PCM cap excludes Rubato FFT scratch, CQT/ORT allocations and worker model memory;
it is not an RSS guarantee. Playback may continue owning the loaded source after
the analysis pin drops; that memory still counts in actual application/combined
RSS. Thread-count environment variables for common inference libraries and Torch
intra/inter-op counts are set to one.
[Reference evidence](beat-this-reference-evidence.md) records actual observations
and their limits; these are not full live-performance acceptance.

`OfflineAnalysisJob.staging_stats()` exposes the PCM reservation and stage
accounting for diagnostic evidence. `limit_bytes` and `export_file_bytes` report
the unchanged cap and complete file extent. `admitted_export_peak_bytes` and
`admitted_key_peak_bytes` bound the respective stages; `admitted_peak_bytes` is
their maximum. `observed_export_peak_bytes` records retained-source plus bounded
export-buffer bytes when export starts; `observed_key_peak_bytes` records actual
owned vector/adapter capacities plus the bounded read buffer after successful
key preparation. These start at zero before their respective stages are reached;
failed or cancelled key preparation may leave zero even after bounded temporary
allocations. They do not claim a complete failed-job allocation history.
`retained_source_bytes` tracks the remaining analysis source pin and becomes zero
after successful export or retirement. These are software-accounted PCM byte
capacities, not allocator profiling or process RSS measurements. Actual combined
RSS still requires independent sampling of the application and live worker tree.

Cancellation, unload, source replacement and engine shutdown invalidate the
native request. Preparation and rate conversion check cancellation between
controllable stages. A running CQT/ORT call remains non-preemptible and keeps
its native slot and PCM until it returns; its stale output cannot publish.
The UI never waits for that call.

The native key handle stays open for actual key readers and closes off-thread
before final file cleanup. The service retains the complete export and its job
directory until both key work and the owned beat tree have stopped reading.
Partial or unflushed exports never start either branch. Read/export failures
retire the associated resources; a key-only failure preserves valid beat output.

On Windows, the interpreter starts suspended, joins a non-inherited Job Object
and resumes only after containment. Cancellation, timeout and launcher exit
terminate remaining descendants off-thread. The supervisor confirms that the
job has no active members and retained member handles are signaled; job accounting
alone can precede full kernel teardown. A launcher PID or closed stdout is insufficient.
Process cancellation or timeout kills and reaps the isolated worker tree off-thread.
If finite reaping cannot confirm exit, a retirement owner retains the process
slot and the supervisor retains PCM until the reader actually stops. Temporary
file deletion failures likewise keep the request retiring. Neither beat-process
exit nor a cancellation flag alone means the whole request has finished.
There is no hard whole-request cancellation deadline for a stalled native key
call or unreleased OS resource.

## Local-only worker protocol and results

`BeatWorkerAdapter` is lazy. A separately supplied `WorkerConfiguration` names
local interpreter, script and checkpoint paths plus exact model identity.
Installed configurations also name their verified manifest. Preflight requires
matching model identity with a 64-character SHA-256 digest, validates installed
provenance, bounds and hashes the checkpoint, and rejects missing, unreadable or
mismatched artifacts before starting a process. The app-side adapter never
imports Beat This. The isolated worker loads only verified local bytes; neither
path resolves model shortnames or downloads packages/weights. Explicit setup is
the only acquisition path, and `small0` is never substituted.

Windows workers launch without a console, using isolated interpreter mode.
Request/response schema version 1 echoes pad/request/source/generation and
model/configuration identity. Responses must contain finite, strictly increasing
source-relative beat/downbeat seconds within the full source extent. Logit
counts and all message sizes are bounded. Logits are raw evidence, not
calibrated correctness probabilities.

The final diagnostic envelope is separate from the unchanged version-1 worker
wire response. Ready results use final schema version 2: `beat.predictions`
contains `encoding: "float64-le/base64"` and four strings named `beat_seconds`,
`downbeat_seconds`, `beat_logits` and `downbeat_logits`. Each string is canonical
padded standard Base64 of uncompressed IEEE-754 little-endian binary64 values.
Its decoded byte length divided by eight is the array count. Packing preserves
every validated value exactly, including signed zero; it neither downcasts
model evidence nor drops events or logits. There are no sidecar artifacts or
compressed-data allocations. Non-ready outcomes retain final schema version 1.

Both final versions are validated by Rust before enqueue. The packed validator
bounds text before decoding, rejects partial values, malformed/noncanonical
Base64, extra/missing prediction fields, unknown encodings, nonfinite values,
invalid source positions and unequal logit lengths. The 1-MiB envelope and
250000-value array limits remain hard bounds. Some complete results can still
exceed 1 MiB even when packed; they report an explicit beat publication failure
with the independent key outcome preserved. This is not a promise that every
allowed worker array count fits the final publication limit.

Diagnostic consumers use
`flitzis_looper.analysis.publication.decode_result(result_json, request)` for
either an event or job snapshot. It returns `PublishedAnalysisResult` with a
normal `BeatComponentResult` and full `BeatPredictions`. The retained request
provides expected identity/model and source duration; its PCM path need not
exist. Reading an exported envelope requires neither scratch files nor installed
models. Legacy final version-1 numeric arrays remain readable. This reader does
not restore or adopt project analysis: that behavior remains a later B2 gate.
Native publication remains authoritative for musical key spellings; the
diagnostic reader checks key component shape/status and preserves its outcome.

Beat and key attempts have separate `ready`, `unavailable`, `failed` or
`cancelled` statuses. Missing beat support can settle alongside a valid musical
key; key failure uses `unknown` without replacing valid beat output. The native
publication validator uses the same authoritative 24 KeyNet names as
the detector, including sharp spellings such as `G#m`. Legacy flat enharmonic
aliases remain valid and retain their submitted spelling. Invalid key values
remain rejected; a valid producer label must not leave the request retiring.
The supervisor emits one JSON envelope through the existing native loader-event
path only after resources settle. `offline_analysis_completed` is diagnostic
data: it is not adopted into project analysis, BPM, grids, loop markers or
manual maps, and it never relabels saved legacy analysis.

## Verification scope

Deterministic verification must cover source-pin release and independent playback
ownership, channel-mean bit parity, complete bounded export, first/last impulses,
silence and converter parity at 22050/44100/48000/96000 Hz, allocation limits,
bounded cancellation/admission, missing/corrupt artifacts, malformed/stale
responses, subprocess timeout/crash/output limits, a stalled key double,
source replacement and shutdown. Staged key conversion must match the prior
full-buffer output, complete length, origin and tail. Where the old converter
rejects a valid zero-output flush, compare with its explicitly zero-extended
input and retain only the original ceiling-length output; preserve the failed
case as evidence. File lifetime checks must
include failed preparation/reads, cancellation with a running key reader and
cleanup failure. Separate worker tests and B1b observations cover
reference frontend/model parity and real lifecycle behavior. None of these
certifies musical beat accuracy, callback deadlines or audible synchronization;
see [reference evidence](beat-this-reference-evidence.md) and the B2/B8 gates.
Full native T04/T05/R01 measurements, complete publication and natural retirement
are tracked separately in [acceptance evidence](beat-this-acceptance.md); streamed
staging or a worker-only result alone does not establish resource acceptance.

Focused checks from the repository root:

```powershell
.\scripts\run-rust-tests.ps1 analysis_pcm --lib
.\scripts\run-rust-tests.ps1 analysis_jobs --lib
uv run pytest src/tests/flitzis_looper/analysis
```

Run the full project checks in [Development](development.md) and official
strict validation of `adopt-beat-this-analysis` when changing this boundary.
