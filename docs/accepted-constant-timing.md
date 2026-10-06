# Accepted constant timing foundation

G3a adds `flitzis_looper_analysis::tempo_acceptance`. This is a pure offline/control
API with no production caller. Existing automatic BPM, editor/native consumers,
manual/TAP, saved projects and realtime wrapping remain unchanged. The record and
guard prepare the next G3 integration boundary; they do not complete it.

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
caller-supplied snapshot cannot establish live freshness. The next slice must
serialize actual source/request and timing-intent changes at the engine's
publication boundary and feed this guard there. No live job is protected by this
new guard until that integration exists.

## Remaining consumer and loop gates

| Boundary | Required next integration |
| --- | --- |
| Editor, snapping, automatic loop ends | Use one accepted binary64 period/origin/revision, retaining physical endpoint rounding and manual authority. |
| Native SourceGrid, transport reference/master and BPMLOCK | Remove silent derivation through binary32 BPM; preserve ratio and source-epoch ownership. |
| MIDI metadata | Include current source and accepted timing revision alongside exact effective endpoints. |
| Prepared source/Key Lock state | Bind preparation to the current source and timing revision; retire stale work under the actual owner. |
| Same-source stems | Retain one source trajectory and check content/generation before prepared publication; shape or path/mtime alone is insufficient. |
| Persistence and legacy restore | Preserve manual/TAP and saved legacy intent through an explicit source-verified migration contract. |

G3b integrates these consumers and current-pad authority. G3c separately proves
musical period versus rounded physical duration over 75/1000 cycles, fractional
periods/rates and callback partitions. Rendered DSP/onset/device evidence is still
required for audible sustained SYNC. G3a changes no physical wrap policy and does
not pass these gates or the frozen B2 musical/default gate.

Focused public API tests:

```powershell
.\scripts\run-rust-tests.ps1 -CargoArgs @('--package', 'flitzis-looper-analysis', '--test', 'tempo_acceptance')
```

See [G2 evidence](constant-tempo-summary.md), [scalar coordinates](scalar-source-coordinates.md)
and [the shared-map design](beatmap-sync-design.md).
