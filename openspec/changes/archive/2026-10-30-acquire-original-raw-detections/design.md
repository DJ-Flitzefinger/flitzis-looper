## Existing authority and typed result

Extend MapInput/AsyncMapInput with AcquireRaw and close prepared results over
Accepted and Raw variants. Keep accepted completion and qualification APIs strict.
The dedicated source_beatmap_raw_completed event carries the original private
loader owner. No mutable DTO, digest, equal bytes or candidate revision can replace
that original admission.

Public control API: begin_source_beatmap_raw_async(sample_id,
expected_source_generation=...) returns the original OfflineAnalysisJob;
resolve_source_beatmap_raw_completion(original_delivery) returns an opaque,
nonconstructible SourceBeatmapRawCandidate. Candidate info() and
inspect(start_index,max_events=128) are control-only queries. No map(), receipt,
import, restore or accepted/durable ACK is exposed.

Use existing SourceBeatmap purpose: its begin_input snapshots the source request
without advancing it, and each original job has its own cancellation Arc.
Replacing OfflineJobs.current's weak addressing entry does not revoke an older
effective accepted delivery. Raw control must not call accepted _admit/_cancel or
mutate selected intent. Actual source/analysis epoch changes still revoke stale
work. Move follows original content placement; queries with placement pending
return no rows, and stale/cancelled candidates cannot expose current rows.

## Complete signal and immutable evidence

Reuse CompleteSourceReader original custody, full stream_mono and the existing
cancellable Rubato converter to 44100 Hz, then exact f32-to-f64 promotion and one
analyze_bpm_raw call. Preserve frame zero, silence and the complete partial tail,
including original data outside a finite resident window and independent of the
currently selected stems. Hash the actual streamed mono f32 bytes and actual
analyzer f64 bytes. Record source descriptor/identity/rate/extent/zero, conversion
revision, detector/code/configuration and actual integer ODF hop.

The actual f64 channel mean followed by f32 storage differs from parts of normal
legacy f32 channel arithmetic; record the executed rule honestly. This exposes
unverified legacy QM evidence, not a new default or completion of gated Beat This
quality/resource work. Preserve original binary64 ODF frames and downbeat raw-index
associations. Physical-domain classification does not establish musical coverage,
quarter-note allocation, meter or calibrated timing uncertainty. Retain outside-
source detections with their classification instead of cropping or snapping them.

## Staged budget and retirement

Use a 512-MiB job-peak bound with checked arithmetic before large allocations:
retained PCM/view, stream scratch/mono, converter capacities and actual FFT
workspace, f64 analyzer and default_bpm_working_bytes, raw arrays and typed
serialization/sealed reparse. Drop completed mono/analyzer stages on the worker
before later stages. Bound raw encoding at 16 MiB and each raw/index array at 250,000
entries. This is a bounded job peak, not a process-RAM or long-track acceptance
claim. Do not weaken existing guards or crop the input to fit.

Reuse the existing exclusive ArtifactBirth/store/sync/sealed reopen/hash/FileID/
pin/deferred reader owner through typed raw payloads. Accepted parsers reject raw
encoding. Raw is never acknowledge_export-ed or made durable. A sole Native raw
review lease begins at successful JobState capture, covering arm/submit/drop
failures too. Job cleanup relinquishes its lease copy after actual source/staged
reader release; artifact readers retain it through real deferred data/file/guard/
pin release. The final lease clears busy before publishing its retired receipt.
No candidatewrapper/JobState/retirement owner cycle is permitted.

The existing source_beatmap_async_status exposes raw_review_retired separately
for a raw job; no second status API is introduced. Existing terminal
cleanup_done AND pool_retired proves temporary job/lane retirement; it does not
prove final raw-reader release or exact-birth disk cleanup. UI retains cancelled
work until terminal and its attempt's raw_review_retired are both true. Disk
cleanup can separately retry only the original known birth. Synchronous QM checks
cancellation around its run, retaining all admissions/readers until actual return.

## One transient review in the existing controller and UI

Keep accepted _pending/_effective/status/errors/effective/playback_delivery and
ProjectState untouched. Add one independent transient raw attempt inside the same
SourceBeatmapSelection owner, with original job/delivery/candidate, content/source
identity and a monotonic attempt token. This token fences identical-digest
reacquisition gestures. Copy, Bank and restart do not inherit raw review state;
Move/Swap follows its original content. Removed content cancels/prunes only its
own attempt, retaining retiring handles to actual terminal. Shutdown cancels via
the existing loader shutdown owner.

Analyze refuses while accepted preparation for that content is busy or another
raw review owns the slot. Accepted edits remain available beside an unreviewed
immutable candidate; they cannot modify or promote it. Discard/reacquire is
explicit, with no automatic raw retry/restore or implicit accepted-map selection.

If placement becomes pending after the initial loader lookup, retain that same
original completion event in the existing bounded delivery queue. A temporarily
unreadable source-generation capture is not evidence of a changed source. Retry
the original private resolution on a later poll; a genuinely different generation
still cancels only its own raw attempt, and unrelated errors remain visible.

The English Source Map panel adds Analyze original full mix, truthful raw state,
an unreviewed warning, at most 128 original events per page and discard/reacquire.
Queries/actions revalidate original Native currentness and source/content/attempt
fences. Event indices are detector indices, observed downbeats are detector flags;
neither becomes a quarter beat, bar, sample anchor, snap target or waveform grid.
Existing grid/bar/SYNC behavior continues to use accepted selection only.

## Verification and full scope

Prove actual complete versus finite-source acquisition, original bits/provenance,
staged overflow/budget refusal, original completion identity, source/placement
ABA and cancellation/terminal/last-reader boundaries. Test accepted selection and
audio invariance through raw success/failure/cancel/discard, independent Copy and
content-following Move/Swap, removed content/Bank/shutdown, bounded query/stale
gestures and identical-digest attempt ABA. Regress existing map artifact/custody/
async/editor/selection and affected portable/API paths. No isolated software proof
closes musical interpretation, durable raw integration, Native SYNC or final gates.
