# Accepted constant timing foundation

G3a adds `flitzis_looper_analysis::tempo_acceptance`. This is a pure offline/control
API. G3b2a-h supply native current-pad acceptance and acknowledgement, precise
native/Python grid/period/loop/control consumers, source-bound MIDI/prepared stems
and productive continuous/prepared voice/native/FIFO history; see
[native adoption](native-constant-timing.md). Acceptance remains explicit.
Ordinary automatic analysis, manual/TAP, saved legacy projects and physical
wrapping keep their established policy. G3b2f2 supplies productive native ownership
and numerical timed-adoption proof. G3b2g persists supported COMPLETE native QM
evidence with source verification and fresh acknowledged loader adoption.
G3b2h supplies transactional current-source/accepted GLOBAL START/STOP batches,
including productive MIDI actions and execution feedback. General accepted
publication/derived-refresh orchestration remains incomplete; loader-specific
refresh does not close that boundary.

## Explicit construction

`AcceptedConstantTiming::from_raw` owns a `BoundTempoEvidence`, recomputes the
existing summary from all supplied count hypotheses and requires a uniquely
supported verified interpretation. `from_comparable_attacks` also requires the
exact verified complete `PcmBinding` and `IndependentQuarterEvidence`, reruns the
existing bounded full-source refinement and fits its complete feature sequence.
Neither constructor accepts a caller-mutated summary/refinement as proof.

Both require `IndependentTimingOrigin { seconds, provenance }` and
`TimingAcceptanceDecision { policy_version, provenance }`. Acceptance is an
explicit caller assertion under a named policy. Validation checks consistency
and numerical support; it cannot prove the assertion's independent musical truth
or turn the G2 engineering policy into a general musical acceptance gate.
Unsupported, ambiguous and unverified candidates produce typed errors.

The fitted binary64 seconds-per-quarter is authoritative within the record.
There is no binary32 BPM roundtrip or integer-tempo preference. The chosen signed
origin remains separate from source zero, the bound evidence's original origin
and the fit's diagnostic intercept. Accessors borrow immutable retained evidence,
summary, optional refinement/independent quarters, origin and acceptance decision.

## Revision and uncertainty

`accepted-constant-timing-v1:<sha256>` identifies the entire accepted record.
Canonical encoding uses explicit lengths/tags and exact little-endian floating
bits. It binds original/PCM content and loaded rate/extent, backend/raw/configuration
and complete request/generation evidence, source and timing-bound provenance,
all evaluated rational counts and diagnostic fits, selected count interpretation,
feature policy and complete refinement/independent evidence, accepted period/error
state, selected origin/provenance and acceptance policy/provenance.

Different count denominators, equivalent encodings, count provenance, declared
error state, origin or acceptance decisions produce different record identities
even when the raw revision or numerical period is unchanged. The raw revision
does not substitute for this identity. Ephemeral request/generation fields are
also checked during adoption; they never replace content hashes.

Declared position uncertainty, conditional period sensitivity, residuals and
distant-window diagnostics remain distinct. No statistical confidence or acoustic
alignment claim is added. All hashing, complete PCM scans, fitting, allocations
and record retirement occur outside the audio callback.

## Control-only adoption guard

`TimingAdoptionGuard::new` retains metadata from a verified current `PcmBinding`
and explicit `TimingIntent`. It owns no PCM or engine state. In Automatic intent,
`issue_ticket` admits a new request and invalidates earlier tickets. Tickets are
opaque and belong to one guard and one monotonically increasing revision.

`adopt` checks the current ticket, Automatic intent and the record's complete
binding against the guard's current binding. Successful adoption invalidates
reuse of the ticket. Failed adoption preserves the previous accepted record and
guard state. A newer ticket alone keeps the last accepted record available.

`set_intent`, `replace_source` and `unload` invalidate pending work and clear
the automatic record. Manual, Tap and Legacy intent cannot issue or adopt
automatic results. Even an edit retaining the same intent enum must call
`set_intent`; returning to Automatic does not revive an old ticket. Revisions and
guard identifiers reject exhaustion instead of wrapping.

`check_binding` is also available as a pure full-binding predicate. A matching
caller-supplied snapshot cannot establish live freshness. G3b2a captures actual
source/request/timebase in a native opaque ticket and feeds this guard under
current ownership at explicit publication. It separately rechecks real Arc,
generation, request, digest and timing intent, including bounded callback adoption.
The pure guard alone does not establish these facts.

## Remaining consumer and loop gates

| Boundary | Required next integration |
| --- | --- |
| Editor, snapping, automatic loop ends | G3b2c uses one current binary64 period/origin/full revision and actual accepted extent/rate, retaining physical rounding and manual authority; caller-owned derived refresh follows adoption. |
| Native SourceGrid | G3b2a uses the acknowledged binary64 period/origin/full revision directly; ordinary legacy timing stays compatible. |
| Native transport reference/master, output clock and BPMLOCK | G3b2b/c consumes acknowledged source/output periods directly with binary64 rate/epoch ownership and Python locked speed/master controls. |
| MIDI metadata | G3b2d binds actual native source/authority and complete accepted revision to one guarded loop/launch effect, including scheduled execution and fresh failed-direct fallback. |
| Controller GLOBAL START/STOP including MIDI | G3b2h binds every affected current native source/authority and complete acknowledged accepted projection to one all-or-none scheduled transaction, preserving restore intent until actual execution. |
| Prepared source and same-source stems | G3b2e binds capture/admission/rendering to current source/authority/full accepted projection and retains one source trajectory. |
| Productive Key Lock/voice/DSP history | G3b2f1 binds continuous native/FIFO/filter/pinned voice history; G3b2f2 adds worker-owned actual source-specific native/FIFO continuation and current-permit timed transactional adoption. |
| Persistence and legacy restore | G3b2g persists complete supported native QM evidence with exact identity/bits, explicit Manual/Tap/Legacy intent and fresh source-verified native adoption. |

G3b1 protects productive stem preparation with actual loaded-source pointers,
content identities, current request/preparation epochs, isolated worker artifacts
and callback feedback; see [prepared publication](prepared-stem-publication.md).
Its generic epoch does not replace this accepted revision. G3b2a connects native
adoption and SourceGrid; G3b2b connects current native authority and native
period/rate consumers; G3b2c adds Python projections and locked controls, G3b2d
adds runtime MIDI pad triggers. G3b2e binds productive prepared-source/stem
admission to full current accepted revision/period/signed origin; retained
same-source PCM refreshes only on effective native timing adoption/clear and
shares one SourcePlayback trajectory. G3b2f1 binds actual continuous productive native/FIFO
history to canonical source feed and complete effective timing, retaining old
voice source/timing ownership independently of bank replacement. G3b2f2 adds
actual worker processing of 4096 copied canonical active frames from pinned
PCM/stems, retained native/FIFO ownership, full current source/load/preparation/
authority/runtime/accepted permits and exact timed transactional adoption. Failed,
stale, unready, late or saturated work retains old effective audio/history.
Productive numerical output/ownership/failure tests establish that native gate.
G3b2g source-verifies supported COMPLETE native QM persistence and fresh loader
adoption while preserving historical accepted identity separately from runtime
request ownership; unsupported evidence schemas fail closed.
G3b2h binds controller GLOBAL START/STOP, including MIDI, through the same native
current source/authority resolver and exact full accepted period/signed-origin
comparison. Whole-batch admission and scheduled execution preserve all previous
audio/loop/transport/restore state on stale/unavailable/capacity failure, and old
pinned voice ownership cannot be relabeled by current bank replacement. Native
playback messages own active/paused truth and must fit as a complete batch before
audio effects; actual batch execution feedback changes restore intent. Loader
callbacks prune unloaded/replaced ids before late acknowledgement can restore
them. General explicit acceptance/derived loop/master refresh
orchestration remains G3b2 work. Neutral warmed reserves and the separate test-only
source preparation fixture do not substitute for the productive native owner. Later B5
audible crop/delay/transition compensation and C1 copy-first/ABA proof stay separate.
G3c separately proves
musical period versus rounded physical duration over 75/1000 cycles, fractional
periods/rates and callback partitions. Rendered DSP/onset/device evidence is still
required for audible sustained SYNC. G3a changes no physical wrap policy and does
not pass these gates or the frozen B2 musical/default gate.
Current 96-native-handle setup/RAM and source-verifying persistence I/O/CPU costs
remain unmeasured; historical 64-handle measurements do not cover them.

Focused public API tests:

```powershell
.\scripts\run-rust-tests.ps1 -CargoArgs @('--package', 'flitzis-looper-analysis', '--test', 'tempo_acceptance')
```

See [G2 evidence](constant-tempo-summary.md), [scalar coordinates](scalar-source-coordinates.md)
and [the shared-map design](beatmap-sync-design.md).
