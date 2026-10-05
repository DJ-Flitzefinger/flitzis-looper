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
one validated envelope. `finish()` returns whether the request was accepted under
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
Mixing, copying and float32 little-endian export check cancellation between
4096-frame chunks. The transfer describes a complete mono file at the loaded
rate with origin `0.0`; JSON carries metadata, never PCM samples.

The native key branch derives 44100-Hz mono directly from shared mono with the
existing Rubato FFT converter. It skips conversion at equal rates, flushes the
delayed tail, removes leading algorithmic delay once and returns
`ceil(source_frames * 44100 / loaded_rate)` frames. The worker independently
derives 22050-Hz input with the pinned upstream soxr HQ/log-mel frontend,
including the reference's rounded resampled length and centered STFT. No key
input is derived from downsampled beat input; see the setup document for exact
short-input and exclusive-end conventions.

## Admission and retirement limits

| Resource | Diagnostic policy |
| --- | --- |
| Native jobs | One active or retiring request per engine; zero pending queue. |
| Service jobs | One active or retiring request per service; zero pending queue. |
| Beat worker trees | One active or retiring worker request globally, including launcher descendants; zero pending queue. |
| Windows tree members | At most 64 simultaneous processes; child admission stops before terminal capture/termination. |
| Native PCM staging | 512 MiB admission cap for retained source, shared mono, loaded-rate key copy and padded key output. |
| Worker PCM | 512 MiB maximum complete exported input. |
| Checkpoint | 256 MiB maximum local file, checked in cancellable 1-MiB hash chunks. |
| Wire messages | 32 KiB request, 8 MiB worker response, 1 MiB final publication envelope. |
| Prediction arrays | At most 250000 positions or logits per bounded array. |
| Process timing | 120-second execution timeout; 5-second finite reap and output-reader join attempts. |

These are provisional engineering limits, not measured Beat This acceptance
limits. The PCM cap excludes Rubato FFT scratch, CQT/ORT allocations and worker
model memory; it is not an RSS guarantee. Thread-count environment variables
for common inference libraries and Torch intra/inter-op counts are set to one.
[Reference evidence](beat-this-reference-evidence.md) records actual observations
and their limits; these are not full live-performance acceptance.

Cancellation, unload, source replacement and engine shutdown invalidate the
native request. Preparation and rate conversion check cancellation between
controllable stages. A running CQT/ORT call remains non-preemptible and keeps
its native slot and PCM until it returns; its stale output cannot publish.
The UI never waits for that call.

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

Beat and key attempts have separate `ready`, `unavailable`, `failed` or
`cancelled` statuses. Missing beat support can settle alongside a valid musical
key; key failure uses `unknown` without replacing valid beat output. The
supervisor emits one JSON envelope through the existing native loader-event
path only after resources settle. `offline_analysis_completed` is diagnostic
data: it is not adopted into project analysis, BPM, grids, loop markers or
manual maps, and it never relabels saved legacy analysis.

## Verification scope

Deterministic tests cover source ownership, channel mean, full-track export,
first/last impulses and silence at 22050/44100/48000 Hz, key resampling,
bounded cancellation/admission, missing/corrupt artifacts, malformed/stale
responses, subprocess timeout/crash/output limits, a stalled key double,
source replacement and shutdown. Existing resampling fixtures continue to
exercise the shared converter. Separate worker tests and B1b observations cover
reference frontend/model parity and real lifecycle behavior. None of these
certifies musical beat accuracy, callback deadlines or audible synchronization;
see [reference evidence](beat-this-reference-evidence.md) and the B2/B8 gates.

Focused checks from the repository root:

```powershell
.\scripts\run-rust-tests.ps1 analysis_pcm --lib
.\scripts\run-rust-tests.ps1 analysis_jobs --lib
uv run pytest src/tests/flitzis_looper/analysis
```

Run the full project checks in [Development](development.md) and official
strict validation of `adopt-beat-this-analysis` when changing this boundary.
