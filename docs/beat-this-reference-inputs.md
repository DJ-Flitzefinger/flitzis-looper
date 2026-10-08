# Private B2 reference and correction input workflow

This bounded CLI prepares empty drafts and validates/seals human input for the
[frozen B2 protocol](beat-this-acceptance.md). It runs no inference, scoring,
automatic labeling, timer collection, result adoption or playback changes.
Reference-only sealing is supported. Every receipt retains pending musical
acceptance and blocked default adoption; reference receipts retain pending
correction measurements. Valid input is not an acceptance result.

The separate [temporal metric core](beat-this-temporal-scoring.md) accepts supplied
in-memory timestamps and regions only. It verifies no seal or candidate lineage
and retains unchecked input certification, pending musical acceptance and blocked
default adoption. Connecting these receipts to complete native candidates through
a private scoring workflow remains the next integration boundary; neither this
input CLI nor the metric core supplies missing human observations.

## Private identity and listening material

Run commands from the active repository with its existing Python environment:

```powershell
$workspace = "D:\NEUES\Windows Dokumente\Dokumente\CODING-PROJEKTE\flitzis-looper"
uv run --no-sync python -m flitzis_looper.analysis.reference_inputs draft --workspace "$workspace" --identities "scratch/b2-reference/loaded-identities.json" --output "scratch/b2-reference/reference-draft.json"
```

Use a fresh output filename when a draft/receipt already exists. Every output is
created exclusively inside the workspace, outside `repo/`. No overwriting or
external private-file access is allowed. Original sources, PCM, annotations,
session artifacts and receipts remain outside Git.

The original manifest SHA-256 remains
`df0d3e62731c58383c83fbb7f6474f6534648eb873d1045f6238b81d28fe8ac5`;
the scoring-addendum SHA-256 remains
`056d6db87684252844b3b7243bee647063e31396826083d43ccd731051ec925d`.
The verified private native identity inventory SHA-256 is
`7952861334862194f2de40b5da7b74d0e2a7e121bd717bffb310a9da4983cabb`.
This additional identity binding preserves the original protocol and limits.
The inventory retains independently checked native export identities and their
evidence lineage. The CLI never reads worker responses, prediction envelopes or
historical result summaries to prepare or seal references.

All and only T01-T05 must match this byte-verified inventory exactly, including
their original-source hashes and native loaded PCM rate/frame count/origin/hash.
T01/T02 retain development status, T03/T04/T05 retain held-out status, and R01
remains resource-only. A self-hashed replacement PCM or a declaration that an
original MP3 is already in the loaded domain cannot satisfy the identity check.

Version 1 requires the actual complete native loaded mono file, origin zero,
float32 little-endian, one channel. Sealing verifies every original source's
frozen byte size/hash and every actual PCM's exact byte extent/hash and finite
samples using bounded reads. The five private listening WAV wrappers, when
prepared, contain those same float32 samples; annotation tools must preserve
that rate, frame-zero origin and complete extent. The annotation listening
provenance must identify the actual material/tool and any wrapper verification.
Missing materialization is an input blocker. There is no decode-offset fitting
or independently decoded-source mapping import in this version.

## Independent full references

The draft has `status: "draft"`, null human declarations and no beat/bar/region
labels. It is deliberately invalid. After independent human work, complete the
same schema and set `status: "ready_for_seal"`. Do not change the inventory,
source identities, frozen split or thresholds.

- Provide annotator, revision, `predictions_seen: false`,
  `independent_listening: true`, listening provenance, assessor and certification
  provenance. These are actual human declarations, never inferred by the CLI.
- Set the complete end to `frame_count / sample_rate_hz`. Regions must tile
  `[0, end)` exactly with metrical, nonrhythmic or ambiguous classifications and
  independent provenance. `beat_unit_quarters` is required for each metrical
  region, and null for others. It makes the pulse interpretation explicit:
  quarter pulses use 1, eighth pulses 0.5, dotted-quarter pulses 1.5.
- Each beat has increasing loaded seconds, timing uncertainty, a continuous
  integer `count`, continuous `quarter` coordinate, `bar_id` and
  `quarter_in_bar`. Each bar has a unique increasing identity, independent
  downbeat seconds/uncertainty, `start_quarter` and meter numerator/denominator.
  A bar spans `numerator * 4 / denominator` quarters. Negative count/quarter
  origins are valid; event seconds must remain in the loaded extent.
- Beats belong to the declared metrical region and their time/quarter position
  must belong to their identified bar. Adjacent beats advance the count by one
  and the quarter coordinate by the previous pulse unit. Meter and pulse changes
  can occur at independently labeled boundaries. Certified skipped counts/bar
  advances require a matching `gap_transitions` entry across a predeclared
  nonmetrical interval, with previous/next count/bar identities,
  `quarter_advance` and provenance. Unknown counts cannot silently restart.
- `uncertainty_ms` is a timing **half-width**, not a full interval width. At
  least 90% of independently labeled metrical beats and, separately, downbeats
  must have half-width <=10 ms for 40/70-ms eligibility. At least 90% of time
  must be confidently classified. Receipts report temporal confidence, trusted
  metrical time and the two eligible event denominators separately. The frozen
  2.5/5-ms half-widths at 10/20 ms remain future scoring requirements.
- Provide all three critical feature roles: first unambiguous bar, post-break
  reentries and independently selected late-track bars. First/reentry identities
  must match the corresponding supplied reference bars. Each role includes
  provenance and either selected bar IDs or an explicit absence reason. The CLI
  adds no new late-track cutoff and does not infer musical absence from a short
  label list. Human certification owns that judgment.
- Certify recording groups and all eight class assessments (constant tempo,
  drift, ramp, abrupt tempo change, swing, sparse, meter change, non-4/4), with
  present/absent status and provenance. Multiple group IDs support compilations
  and excerpts. No group may occur in both development and held-out references.
  Globally absent classes are reported; missing classes do not become coverage.

```powershell
uv run --no-sync python -m flitzis_looper.analysis.reference_inputs seal-reference --workspace "$workspace" --input "scratch/b2-reference/reference-v1.json" --output "scratch/b2-reference/reference-seal-v1.json"
```

The receipt binds the original JSON bytes and inventory/protocol identities.
Changing a sealed bundle, inventory, original source or PCM invalidates later
session work. A receipt is a content checksum record, not a digital signature
or proof that listening, completeness, labels or declarations are truthful.
The validator cannot discover omitted real beats or establish musical accuracy.
Independent assessment and later scoring remain required.

## Actual paired correction measurements

After sealing the independent reference, prepare and confirm the shared human
workflow and six-session schedule for both backends on T03/T04/T05:

```powershell
uv run --no-sync python -m flitzis_looper.analysis.reference_inputs draft-order --workspace "$workspace" --reference-seal "scratch/b2-reference/reference-seal-v1.json" --output "scratch/b2-reference/order-draft.json"
uv run --no-sync python -m flitzis_looper.analysis.reference_inputs seal-order --workspace "$workspace" --reference-seal "scratch/b2-reference/reference-seal-v1.json" --input "scratch/b2-reference/order-v1.json" --output "scratch/b2-reference/order-seal-v1.json"
uv run --no-sync python -m flitzis_looper.analysis.reference_inputs draft-corrections --workspace "$workspace" --reference-seal "scratch/b2-reference/reference-seal-v1.json" --order-seal "scratch/b2-reference/order-seal-v1.json" --output "scratch/b2-reference/corrections-draft.json"
```

Complete the schedule's annotator/tool/workflow/endpoint/provenance and set
`ready_for_seal` before sealing it. The suggested AB/BA/AB order is balanced
for three pairs; the validator allows other balanced orders. Every paired
session must then use the same confirmed annotator, editing tool, procedure and
complete corrected endpoint, and occur in the exact frozen chronological order.
Reference sealing precedes order sealing; order sealing precedes all sessions.

Each actual session records `human_measured: true`, source SHA, reference
revision and receipt SHA, candidate/corrected artifact paths/hashes, UTC start/
end and phase identities. Each phase links actual input/output artifacts,
operation counts (inserts, deletes, moves, count, meter, phase, gap) and actual
active human intervals with measurement provenance. Intervals must be ordered,
nonoverlapping, nonfuture and within their phase/session. The CLI sums the
reported intervals; it never creates a stopwatch result. Every phase with
operations requires its own positive active time; another phase's time cannot
cover an unmeasured edit phase. Session-wide zero active time is
representable only with zero operations and an explicit human reason. Critical
outcomes retain every sealed selected bar identity, measured timing error and
bar-identity correctness, including honest failures.

Producer identity records a workspace-contained implementation path/SHA,
configuration and provenance. Beat This requires the frozen model SHA and
selected CPU FP32 configuration; legacy requires the repaired QM configuration
and `legacy_units_fixed: true`. The implementation may be the actual repository
native extension or worker entry script; it is hashed read-only without loading
it. A backend must retain the same producer identity across its three sessions.
Artifact bytes are hashed only after both receipts are validated. Hashes prove
which bytes were supplied; the claimed producer/session relationship and human
measurements still require independent provenance, not a backend name alone.
Producer provenance must retain the actual legacy analysis configuration or
the selected worker environment/frontend identities used to produce the
candidate. Configuration names and file hashes alone cannot prove those runtime
parameters or establish that the hashed binary contains the units repair.

Set the completed correction bundle to `ready_for_validation`, then run:

```powershell
uv run --no-sync python -m flitzis_looper.analysis.reference_inputs validate-corrections --workspace "$workspace" --reference-seal "scratch/b2-reference/reference-seal-v1.json" --order-seal "scratch/b2-reference/order-seal-v1.json" --input "scratch/b2-reference/corrections-v1.json" --output "scratch/b2-reference/corrections-receipt-v1.json"
```

The resulting receipt reports actual supplied operation/time totals, with
comparative improvement and musical acceptance pending. It does not apply F1,
count, critical-downbeat, resource or 20%-improvement gates. Failed measured
outcomes remain valid evidence. Without genuine paired measurements,
correction-burden improvement remains unproven.

All input JSON is bounded to 16 MiB with strict fields, finite numbers, strict
integer/boolean types and duplicate/extra-key rejection. Each reference allows
at most 250000 beats/downbeats, 10000 regions/gap transitions and 256 recording
groups. Each correction session allows 256 phases and each phase 10000 active
intervals. Candidate/edit artifacts are bounded to 16 MiB; implementation files
to 512 MiB. These input limits do not revise the frozen runtime resource gates.
