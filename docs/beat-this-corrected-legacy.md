# Corrected legacy comparator evidence

The corrected QM comparator is an explicitly invoked, offline diagnostic. It
reuses `analyze_bpm_raw`/`QmRawAnalysis`, the shared pipeline also used by ordinary
`analyze_bpm`, after the repaired integer sample-hop conversion and bounded
Rubato tail handling. It does not select QM in settings, change the analysis
default, adopt timing or supply independent musical labels. The selected future
Beat This default remains gated by [the full B2 protocol](beat-this-acceptance.md).

Historical legacy output and every original candidate/publication failure remain
unchanged. A fresh comparator supplements that history; it does not rename an
old result as corrected or turn successful provenance into musical acceptance.

## Native execution and source domains

The hardware-free producer cold-loads the complete immutable original through
the existing copy-first source loader, uses the finite playback bank and native
command acknowledgement, and reserves an ordinary `OfflineAnalysisJob`. Its
complete-source reader and mono export describe the whole loaded source even
when only a small playback window is resident. No app, GUI, CPAL stream, device
or recorder is opened.

The job exposes `analyze_corrected_legacy(analyzer_output_path=None)`
and `finish_corrected_legacy(result_json)`. The producer passes an explicit
absolute analyzer output path to retain the complete actual binary64 input;
no detector input is reconstructed from its hash. The diagnostic derives
complete 44100-Hz analyzer input from the native loaded-rate mono export using
the existing analysis converter, then calls the shared raw QM pipeline once.
Loaded PCM, mono export and resampled analyzer PCM retain separate rate, channel,
frame-count and content identities. The analyzer rate must not be substituted
for the loaded rate; an analyzer frame is not a loaded sample or device frame.
Source frame zero, the complete ceiling-derived resampled tail and leading
silence remain intact without fitting an offset or cropping input.

The diagnostic mono adapter accumulates each interleaved frame's arithmetic
channel mean in f64 and rounds once to f32. Ordinary legacy analysis uses its
existing f32 channel mapper. These methods can produce different sample bits
for multichannel sources. The comparator therefore records its actual mono
rule and does not assert byte parity with ordinary loading merely because the
resampler and QM tracker are shared. Neither adapter is a second beat authority.

QM is native Rust analysis: it does not execute the optional Beat This Python
worker, load Beat This weights or produce neural logits. The actual executing
test EXE, its retained producer sources and used dependency/build identities are
bound independently. `loaded_libraries` records actual loaded DLL/module paths
and retained dependency hashes separately from installed files. Loading a module
does not prove that every function it contains executed; QM, optional KeyNet and
Python numerical helpers retain their actual procedure/use declarations. An installed
PYD unused by this embedded producer remains an explicitly unused separate
artifact. The engineering evaluator uses pure Python/Fraction arithmetic and
imports no native extension. A separately invoked numerical fitter must bind
its own executing extension; its identity does not describe QM generation.

## Complete raw contract and native publication

`corrected-qm-native-v1` has `schema_version: 1` and `diagnostic_only: true`.
It binds pad/request/source/generation, loaded mono identity and origin zero,
44100-Hz analyzer identity, actual integer ODF hop, transform revision and all
eight requested `AnalysisConfig` fields. A configuration snapshot does not claim
that previously unused tracker settings have become active.

| Representation | Retained complete values |
| --- | --- |
| Raw QM | Binary64 detector frames and beat/downbeat source seconds; unsigned 64-bit downbeat raw indices. |
| Compatibility projection | Binary32 scalar BPM and complete beat/downbeat/bar seconds, projected from the same raw analysis without another detector run. |
| Native result, finish and completion | The original bounded diagnostic packet, exact finish input and actual identity-matched native event. |

`result-envelope.raw.json` is the producer's retained byte copy of the returned
diagnostic result; it is not an `OfflineAnalysisService.JobSnapshot`. The native
completion event is read independently from the actual event queue.

The raw object declares `float64-le/base64+uint64-le/base64`: canonical padded
standard Base64 of complete uncompressed little-endian binary64 values and
unsigned 64-bit indices. Compatibility declares `float32-le/base64`. Every
position remains in its original order. Downbeat raw
indices associate the retained QM downbeats with their original beat frames;
bars preserve the legacy convention. The reader does not sort, deduplicate,
shift, trim, round or use event JSON numeric arrays as replacement raw evidence.

Native finish accepts only the exact raw JSON string produced by that job's
successful native analysis and still-current source/request identity. Reformatting
or duplicate-key JSON cannot replace the retained success, even with equivalent
parsed values.
Mutated, invented, foreign, stale or cancelled packets cannot publish. Finish,
ordered native observations, one identity-matched completion, actual cleanup and
same-source subsequent admission are separate evidence obligations. The retired
temporary export path is retained as history; the complete export copy is a
distinct content binding. A finish receipt alone proves neither complete array
parity nor source-to-analyzer execution.

All parsing, hashing, resampling, analysis, encoding and cleanup occur outside
the realtime callback. Existing bounded PCM/job ownership remains authoritative;
the comparator does not raise a production limit or perform callback work.
One reservation claims either native KeyNet or corrected QM. Reciprocal checks
under the shared transition lock prevent their full PCM stages from coexisting;
the claim persists through retirement. An unstarted-job abort rejects a claimed
QM job before trying its PCM mutex, so it cannot wait for QM while holding GIL.

## Fixed reader profiles

`corrected_legacy_models.py` defines the strict complete QM contract;
`corrected_legacy_reader.py` imports only fixed independently reviewed profiles.
The reader rehashes their private artifacts and validates their semantic
relationships. Candidate-provided hashes or arbitrary registration receipts
cannot create a supported producer profile.

`corrected_legacy_provenance.py` checks the executing EXE, complete native
module partition and one-to-one imported Python source mappings against the
actual trace. `corrected_legacy_sources.py` checks retained producer sources
against identical preflight/postflight manifests and the source copies embedded
in the EXE. A complete snapshot maps every preflight source/build entry to one
retained byte copy with the same original path, size and hash. Original checkout
paths remain provenance metadata; the reader
rehashes the retained copies, so later checkout edits cannot redefine a frozen
producer. Retained dependency bytes and external module metadata have distinct
roles; an observed path without retained bytes is not a content-hash claim.

The profile binds the frozen original source and explicit T01/T02 alias where
needed, its sealed copied bytes and cold manifest/transform, complete loaded
interleaved PCM, loaded-rate mono export, actual 44100-Hz analyzer PCM, request,
raw packet, finish, retained result copy, actual completion, producer/runtime/dependency bytes and
retirement evidence. Full listening mono PCM must match the independent loaded
inventory. Binary64 raw arrays, unsigned index arrays and binary32 compatibility
arrays must agree bit-exactly through publication, including signed zero.
Rehashing a receipt cannot bypass identity, timebase, array, lifecycle or encoding
validation.

The reader is offline Python. It runs no detector or native engine and has no
authority over project persistence, accepted timing, saved/manual/TAP values or
live playback. Its supported comparator profiles are distinct from supported
Beat This candidate profiles. Drafting a scoring plan continues to choose the
approved Beat This candidates; it never chooses QM as the candidate.
Corrected comparator selections are optional and added by the draft command only
with `--include-corrected-legacy`. All selections are validated after the genuine
seal and before any backend artifact is opened.

## Engineering comparison and sealed musical scoring

`corrected_legacy_evaluation.py` provides a pure API, invoked by an explicit
private harness rather than an engineering CLI. The harness validates the
existing unlabelled inventory and complete source/PCM material for mechanical
same-source comparisons; it does not seal a reference or call musical scoring.
The API compares complete approved T01-T05 comparator
and candidate chains as engineering observations. It retains their entire raw
arrays and identities, count/extent differences and separately named numerical
timing diagnostics. Backend disagreement is not detector error without an
independent reference. An ordinal beat sequence is an unverified count hypothesis;
good numerical fit does not establish quarter notes, meter or bar identity.

The frozen `corrected-legacy-engineering-v1` policy requires identical complete
original-byte and loaded mono identities for both backends. It binds each raw
array by count, declared representation and SHA-256, retaining all values beside
the identity. Complete beat and downbeat event ordinals have separate
equal-weight OLS fits computed with exact rational representations of the
supplied binary64 times; the reported period,
intercept and every residual are numerical diagnostics. This separate audit fit
does not replace `selected-backend-bpm-v1`, its binary64 production fitting or G2
robust/feasibility decisions. Local intervals use every consecutive original pair.
Beat ordinals do not certify quarter-note counts; downbeat ordinals do not
certify musical bars or meter.

Beat and downbeat disagreements reuse the frozen bounded monotone matcher at
10/20/40/70 ms. They retain matched original index pairs, unmatched indices on
each side and signed Beat-This-minus-QM errors with bias and absolute p50/p95/max.
Neither side is called the reference; no precision/recall/F1 or musical
missing/extra verdict is produced. Middle `[20%,80%)` and early/central/late
thirds retain their original member indices for each complete source. Each
region has separate complete regional beat/downbeat ordinal fits, all local
intervals and fixed-tolerance disagreement arrays. Its matching indices are
local to that regional sequence, with explicit maps to the unchanged global
indices. Regional comparisons remain separate from global matches, including
cross-boundary pairs; they never replace the complete-track comparisons or
hide endpoint events. No region creates new independently validated exclusions,
selects a winner or establishes representative playback timing.

QM's actual `hop / 44100` detector timebase is separate from Beat This's 20-ms
logit lattice. The engineering report records their conditional halfwidths as
`hop / 88200` seconds and `0.01` seconds respectively. These values do not
establish an acoustic-onset or musical-beat bound. Any reported scalar
BPM projection and the existing [G2 evidence](constant-tempo-summary.md) must
retain their own scope. Neither a detector lattice nor a finite fit establishes
acoustic uncertainty or a universal one-loaded-frame timing bound. Engineering
comparisons do not adjust thresholds, drop difficult tracks, fit offsets or
replace the complete arrays with a selected region.

The [reference-first scoring workflow](beat-this-scoring-workflow.md) can
explicitly include a supported corrected comparator alongside the selected
candidate after validating a genuine complete independent ReferenceSeal. That
path uses the same frozen temporal matcher against the independent labels for
each backend and retains their separate provenance. Comparator inspection cannot
precede reference validation or substitute for it. Engineering evaluation can
run without labels precisely because it returns no musical score or acceptance.

## Observed complete-track results

The 2026-10-08 hardware-free collection completed all five native Debug and all
five native Release jobs. Every original raw and compatibility array, including
the scalar projection, is bit-identical between profiles for each track. Loaded
mono and analyzer identities also agree. The five fixed Release profiles are
`b2-corrected-qm-native-release-t01-v1` through `t05-v1`; they retain the actual
successful native chains separately from the selected Beat This candidate chains.

The complete engineering reports retain all eight QM arrays, all four selected
candidate arrays, global and regional fits, every interval, matched pair and
unmatched index. Independent recomputation checked the five complete reports'
exact rational fits and bounded matching, including all regional original-index
maps. The following rounded display summarizes those engineering observations.
QM/BT columns show corrected QM followed by the selected Beat This candidate.
Ordinal rates are fitted events per minute; their musical units remain unverified.
The 70-ms columns count monotone backend pairs, without a musical correctness claim.

| Track | Complete beats QM/BT | Complete downbeats QM/BT | Full beat ordinal rate QM/BT (per minute) | Beat pairs at 70 ms | Downbeat pairs at 70 ms | Native QM job wall Debug/Release (s) |
| --- | --- | --- | --- | --- | --- | --- |
| T01 | 379 / 377 | 94 / 95 | 94.015521 / 94.006886 | 375 | 0 | 20.255266 / 1.960947 |
| T02 | 256 / 300 | 64 / 75 | 89.975064 / 105.271113 | 256 | 64 | 14.090438 / 1.384761 |
| T03 | 358 / 430 | 90 / 112 | 96.092763 / 132.416068 | 240 | 48 | 18.937754 / 1.772762 |
| T04 | 662 / 618 | 166 / 239 | 110.927095 / 100.797164 | 206 | 79 | 29.210447 / 2.966974 |
| T05 | 1153 / 1258 | 288 / 334 | 133.315820 / 146.323109 | 1049 | 266 | 51.630939 / 4.102234 |

Job wall time starts immediately after successful native admission and ends after
`job_done`. It includes complete export, retained-artifact copying/hashing, analysis,
diagnostic packet writes, retirement and finish. It excludes admission work, initial
cold loading and the subsequent event audit/readmission. These single observed
durations do not establish comparative inference performance, human correction time
or resource acceptance. Existing native `staging_stats` cover export/key staging,
not whole QM or process peak RSS.

Earlier Debug attempts exceeded the ignored collector's 120-s and 300-s cold-load
acknowledgement deadlines before analysis; those failed cohorts and their errors remain
retained and unsupported. The successful collection used a 900-s ignored-test guard;
production limits did not change. Historical legacy output, original candidate
failures and the 242 previously retained private bindings remain unchanged.

Actual human sessions remain separate: six balanced T03-T05 sessions must retain
recorded operations, active intervals, absolute caps, critical downbeats and the
20% operations/time comparison with the zero-baseline rule. Analysis wall time
is not human correction time. Observed process durations, file sizes and retained
PCM bytes are not measured live RSS, aggregate peak memory or resource acceptance.

The full B2 reference/count/meter/bar/class/group/uncertainty, musical-score,
human, cutover/restore and final hearing/device gates remain open. No engineering
report or fixed profile enables the analyzer default or changes live timing.
