## Policy and authority
Share one controller-owned guard across resolve, inference and Pair preparation.
Keep immutable waiting metadata in existing owners. Before the six-lane arbiter
reserves capacity, require both caller and system available commit >=2 GiB and
physical available >=512 MiB. A blocked/query-failed guard requires >=3 GiB and
>=768 MiB to recover. These are application safety margins, not model predictions.
Query at most every250 ms, with a fresh observation at actual worker/command entry.
Admission query failure defers; queued entry query failure terminates its original
attempt through the normal collector. Unsupported platforms are explicit and keep
the previous bounded-pool behavior. No sample history/background thread is added.

Active commands stop at <=512 MiB caller OR system commit or <=128 MiB physical.
An active failed query gets a fixed1-second deadline anchored to the first failure;
repeated failures do not slide it. A valid query resets that deadline but does not
clear the admission recovery latch below the recovery margin.

Cancellation precedes the resource check. A completed process result precedes a
new pressure observation. Critical pressure uses the existing cooperative marker,
then owned Job/tree termination and pipe-reader retirement. Capacity and source
custody remain held until actual return. Resource failure is typed and independent
of the subscriber cancel Event; it bypasses both Demucs and BS CPU fallback.
Actual secondary supervision/read failure keeps its existing stronger diagnostic.

## Boundaries
Do not gate arbitrary StemWorkerPool targets: Copy and migration use that pool too.
Keep result/publication/cleanup polling live while admission waits. Win32 queries
are sequential, not an atomic snapshot; caller and system commit are distinct.
This policy cannot prevent sudden allocations by other processes or guarantee a
model will fit. No change to PCM accounting, inference chunking or model lifetime.
