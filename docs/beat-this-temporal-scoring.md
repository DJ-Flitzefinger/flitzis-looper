# Offline B2 temporal metric core

The pure Python `beat_matching` and `beat_scoring` modules implement temporal
diagnostics for the [frozen B2 protocol](beat-this-acceptance.md). Their policy is
`b2a-private-pilot-v1-temporal-core-1`. They read no files, verify no reference
seal, run no inference and produce no acceptance decision. The actual five
independent full-span references and paired human correction measurements remain
missing; no actual musical score is reported by this implementation slice.

Every report retains `input_certification: "unchecked_by_metric_core"`,
`musical_acceptance: "pending"` and `default_adoption: "blocked"`. Perfect
supplied timestamps do not certify the source, annotation independence,
quarter-note counts, intended bar identities or audible behavior. Normal
analysis routing, persistence, manual/TAP intent and playback remain unchanged.

## Pure APIs and supplied extent

`flitzis_looper.analysis.beat_matching.match_beats` matches two complete,
strictly increasing nonnegative timestamp sequences. It returns original index
pairs and the exact summed absolute error, with a rounded diagnostic projection.

`flitzis_looper.analysis.beat_scoring.score_timing_events` accepts reference
`TimingLabel` values, complete prediction seconds, supplied `Region` values and
the complete duration. Regions must tile `[0, duration)` exactly, starting at
zero and retaining the complete tail. Each event must lie within that exclusive
end; all reference labels must lie in metrical regions. A boundary event belongs
to the following half-open region. Nonfinite, unordered, out-of-extent or
structurally incomplete input fails explicitly.

`score_track_timing` projects the supplied `ReferenceTrack` and complete
`BeatPredictions` into separate beat/downbeat reports. The reference extent must
equal its native loaded frame count divided by its sample rate. Complete logits
must have equal lengths, remain finite and satisfy the count bound. Selected
critical reference bars receive 40-ms timing diagnostics with their supplied
feature role and bar ID. Candidate bar identity remains `unchecked`;
quarter-count/bar-identity and paired-correction decisions remain `pending`.

These shape checks do not establish that any input came from the actual frozen
source, complete native analysis or independent listening. The core has no file
importer, CLI or receipt writer. The separate
[private workflow](beat-this-scoring-workflow.md) revalidates receipts and native
lineage before calling this core.

## Frozen matching and uncertainty

For each tolerance, the monotone one-to-one match maximizes cardinality, then
minimizes summed absolute timestamp error, then selects the lexicographically
smallest sequence of original reference/prediction index pairs. Matching treats
the validated binary64 timestamps and binary64 tolerance as their exact dyadic
values. It uses no decimal rounding, epsilon expansion or greedy nearest-neighbor
substitute. The exact cost determines ties; its rounded projection does not.

| Point tolerance | Maximum eligible reference uncertainty half-width |
| --- | --- |
| 10 ms | 2.5 ms |
| 20 ms | 5 ms |
| 40 ms | 10 ms |
| 70 ms | 10 ms |

Reference uncertainty is a timing half-width. Each result retains original
eligible/ineligible reference indices and evaluated/excluded prediction indices.
Predictions are excluded only when their original timestamp lies in a supplied
ambiguous or nonrhythmic region. An ineligible reference never erases a nearby
prediction or creates an exclusion window. Thus a prediction near a wide-
uncertainty label can remain an explicit extra in a precise-tolerance report.

Matching is global across all eligible metrical events. It can join endpoints
on opposite sides of a short predeclared gap when their exact distance meets the
tolerance. Each region reports matched reference and prediction endpoint counts
separately, including `cross_region_matches`; it does not recompute an isolated
regional optimum or hide that cross-boundary association.

## Reported metrics and spans

Each tolerance reports precision, recall and F1, original matched/missing/extra
indices, signed timing bias and absolute p50/p95/maximum error. Precision uses
evaluated prediction count; recall and missing/extra fractions use eligible
reference count. F1 uses the sum of those two event counts. A zero denominator
produces `None` for that metric, and an empty match set produces no error
distribution. Empty evidence never becomes a successful acceptance case.

Error and uncertainty quantiles use linear type-7 interpolation at `(N - 1) * p`.
Signed bias is the mean prediction-minus-reference error. The companion interval
distance is `max(0, absolute_point_error - reference_uncertainty_halfwidth)` on
the **same point-match pairs**. It is not a separately optimized interval match
and cannot replace the frozen point-timing gates.

Temporal confidence excludes ambiguous time; trusted metrical coverage counts
only metrical time. Those fractions remain separate from eligible-event
denominators. Region reports preserve every supplied region, its original
boundaries/kind and endpoint counts. The core invents no startup, break or tail
cutoffs: `regional_scope` is
`supplied_regions_only_no_inferred_startup_break_tail`. Actual independently
declared regional scope is required for later regional acceptance claims.

The longest matched run uses consecutive original reference and prediction
indices within one reference region and one prediction region. An unmatched/ineligible reference,
an intervening prediction or either endpoint sequence's region boundary splits the run. It ranks
runs by reference endpoint duration, then event count, then earliest reference
index. The result retains original indices, event count, endpoint seconds and
region identity; it extrapolates no coverage beyond those endpoints. Unmatched
eligible-reference runs also retain their original endpoints and split at
reference-index discontinuities or region boundaries.

## Explicit resource failures

Each complete reference/prediction/logit sequence is bounded to 250000 events;
regions are bounded to 10000. A tolerance may contain at most 1000000 eligible
reference/prediction edges. Exceeding the matching bound raises
`BeatMatchingLimitError`; invalid scoring input also fails explicitly. There is
no prefix score, silent truncation, replacement algorithm or partial pass.
These are offline metric limits, separate from the unchanged frozen runtime
resource gates. All allocation and matching belongs outside realtime paths.

## Acceptance and next integration boundary

The [private seal-bound workflow](beat-this-scoring-workflow.md) now revalidates
independent reference receipts before opening candidates and binds approved
complete historical native source/PCM/request/raw/envelope lineage. Generic
self-hashed arrays cannot establish lineage. No independent reference is derived
from candidates; offset fitting, trimming and post-hoc exclusions remain forbidden.

The full B2 program remains open: complete independent T01-T05 labels and
recording-group/class certification; actual corrected-legacy comparison and
paired held-out human correction sessions; musical count/phase/critical-bar
assessment; versioned selected-backend BPM and representative-region evaluation;
gated default cutover; saved/manual/accepted-result preservation and model-free
cache restore; end-to-end availability, independent outcomes and atomic-result
validation; maintained documentation and full required checks. Existing G2/G3
constant-tempo evidence/adoption APIs remain reusable foundations. The temporal
core completes none of those parent gates. Actual human hearing/device acceptance
and necessary corrections remain open for the final pre-port phase.
