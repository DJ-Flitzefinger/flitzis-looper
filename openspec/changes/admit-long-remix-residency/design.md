## Existing authority and precise gap

`CompleteSourceReader` and sealed `VerifiedStemPair` already provide bounded
positioned reads of immutable complete disk assets. Live `SampleBuffer` still
holds contiguous resident PCM. A finite loop currently admits its entire physical
range; selected vocal playback retains four components and its reference range
for existing atomic switches. This change does not pretend that complete disk
streaming is already advancing-window realtime playback.

`resident_accounting` owns scope-specific actual view witnesses, deduplicated
backings, pending allocation promises and transfer to readers. Extend this owner;
do not create a second resident manager. Existing reservations continue to
protect active, paused, history, capture, pending and retiring readers.

## Admission

Automatic mode has no duration, bar, 512-MiB or 1-GiB live-resident ceiling.
Native budget value zero denotes this mode; positive supported budgets remain
explicit manual limits. Host admission applies in either mode. Query Windows
caller committable headroom, independent system commit headroom and available
physical memory outside realtime. Reserve an operating margin and outstanding
not-yet-settled allocation promises before approving incremental physical PCM.
Use checked arithmetic and a shared promise boundary across engine scopes, so
simultaneous workers cannot independently spend the same observed headroom.
Observations cannot guarantee a future OS allocation. Returned admission and I/O
errors remain recoverable through the existing transaction. The existing direct
`Arc::new_uninit_slice` allocation is infallible at the Rust API boundary: a real
allocator OOM can abort the process. The host margin reduces admission pressure;
it is not a guarantee of recovery from allocator exhaustion or an OS-memory race.

Existing PCM already occupies OS memory and must not be subtracted again as new
allocation. Scope-local ownership costs and incremental physical allocation costs
are distinct, including a first foreign-scope adoption of shared immutable PCM.
Zero-allocation reuse does not require spare RAM or a fresh OS observation.
Retain the original owner/capacity and scoped charging rules.

Resolve exact source/component reuse before prospective component charges.
All live sealed-reader paths must honor the same reservation, including cold,
warm, relocation, component preparation and committed-set reacquisition. A legacy
whole-conversion branch retains its genuine bounded temporary PCM check. Stream
buffers remain bounded; final immutable arrays are not called scratch.

## Persistence and feedback

Persist a separate automatic-mode setting beside the existing numeric manual
allowance. Missing mode with the old default numeric allowance selects automatic
admission. A saved nondefault manual allowance remains manual. Preserve the
numeric allowance when switching automatic mode on/off. Apply Native policy
before mutating/persisting settings and before startup admission.

The manual slider uses whole GiB steps. Existing precise MiB values, including the
Human's9925MiB, remain exact until an edit and are shown as their actual GiB value;
rendering or opening Settings must not round a persisted allowance.

Settings distinguishes automatic machine admission, optional manual allowance,
exact held/pending PCM and any host observation. No byte counter is labeled RSS,
and no runtime readiness or machine snapshot is persisted.

## Verification boundaries

Test large declared geometry around the former cutoff without multi-GB buffers;
injected sufficient/insufficient/unavailable host snapshots; pending promises,
overflow, cancellation, zero-cost sharing and manual caps; productive small
sealed source/component range preparation and existing retirement regressions.
Run affected Rust/Python integration, static checks, strict OpenSpec and a real
Release build/import in an isolated owned path while the Human Release remains
open. Preserve negative results and independently review the actual final diff.
No bounded test substitutes for physical multi-GB or Human listening acceptance.

## Bounded implementation evidence

The final affected Release selection passes337 tests. Python passes723 with2
unchanged device skips against the separately built Native protocol2. Twelve
actual static Native contract groups, automatic/manual9925 switching, real host
status and Release launcher preflight pass in an isolated import path. The open
Human Native is unchanged. Rust fmt, explicit changed-file fmt, Ruff, seven-file
mypy, strict OpenSpec and final diff checks pass; Clippy completes with inherited
warnings. One continuous independent nonauthor diff review has no open findings.
Actual commit/push/clean-remote and owned cleanup receipts belong in existing local
logs at delivery. The large-extent regression is geometry-only with tiny arrays;
productive small fixtures separately cover reads, own ACK, pressure and retirement.
No actual multi-GB allocation, full process/RAM acceptance, device listening,
advancing-window playback, one-shot EOF or live SYNC acceptance is claimed.
