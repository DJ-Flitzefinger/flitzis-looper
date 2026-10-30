# Source SYNC recovery architecture

This is the recovery entry for the implementation preserved on
`SYNC-REALTIME-BPM`. It is documentation of an unsuccessful experimental feature,
not authority to restore it to main or resume an old execution program. Main is
constant-BPM. This consolidation changes no code, runtime, model or acceptance.

## Frozen identities and acceptance

| Identity | Meaning |
| --- | --- |
| `949cff1dae4b979639dca1cbc1e86c586387cd21` | Archived accepted **constant** BPM/Tap baseline. The Human reported more than 100 bars without audible drift for the tested constant-tempo pair. This was not SYNC acceptance. |
| `27ef7f878f55c0955835ca536af8df6ef4a89a28` | Preserved implementation and later regional-probe work. The probe was reviewed and typechecked, but not installed or musically accepted. SYNC failed the musical test and remained unaccepted. |
| `dbc80d3c117dcec50996feef2e8c59f1649388a5` | Main's frozen source-document audit baseline, including the later full-source Key repair. Its code is not present on this older branch merely because this document describes that boundary. |
| `9f3ca2d19ad11b0218ccc5ef5dc5353c9981f341` | Main's documentation audit and disposition ledger. It decides which mixed original sections are retained; it does not certify historical programs. |

The constant listening report identified Veteran's Memorial at
91.89220756855694 BPM and King Size (Instrumental) at 94 BPM. Tap returned
91.91132496949297 BPM for the former, with slight but acceptable long-term drift.
These observations concern those songs and settings, not arbitrary material.
The [original removal receipt](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/dbc80d3c117dcec50996feef2e8c59f1649388a5/docs/sync-archive.md)
retains the precise decision. Private songs, settings, installed runtimes, models
and local diagnostic files remain outside Git and were neither published nor deleted.

## One clock and one source trajectory

Rust's permanent output-frame clock and `TransportTimeline` own master musical
position `M(n)`. A versioned, monotone source map `B(s)` and inverse `S(b)` express
quarter coordinates against original source time. Editor, preparation, reader and
diagnostics must use the same interpolation and revision. Loaded playback frames
derive from source seconds and the actual decoder/resampling identity once;
repeated rounding cannot redefine a beat, loop or source origin.

Quantize selects entry intent. SYNC controls the subsequent mapped source
trajectory. BPM Lock must not multiply a second tempo ratio into that trajectory.
The coupled master fader already changes the master period; multiplying its speed
again would double the effect. Scalar dry peers keep their own established rate
glide. Ordinary scalar clocks, accepted constant periods, loops and fractional
grid precision remain useful independent foundations on main.

For explicit Grid Offset `g`, the phase lattice is `B(g)+Z`; correction reduces
`B(g)+M(n)-B(s(n))` modulo one quarter. Loop start `A` remains the chosen musical
entry. Without explicit `g`, the preserved launch-relative law uses modulo `Q`.
Use `C=1` or `C=Q` consistently for debt and winding reconstruction; retain `Q`
for physical folding, windows and domain identity. Choose the nearest feasible
winding, positive on ties, then the other adjacent winding under the same bounds.
The second winding can exceed half a beat. Explicit phase admission still needs
positive integral `Q`; fractional ordinary playback is a separate domain.
Equivalent beat phase does not erase full origin/revision identity. Unknown meter
does not certify bars, downbeats, phrases or kick/snare parity.

Rate trajectories must integrate to the canonical coordinates. Feeding local
slopes into the scalar smoother accumulates phase debt. Silent clipping, repeated
seeks, peak chasing or a second scheduler cannot replace a qualified trajectory.
Source-rate overflow, missing coverage and unsupported state retain refusal.

## Source, content and control ownership

Saved preference, available map, pending preparation, effective programme and
actual rendered read are separate states. GUI/MIDI intent travels through the
existing controller and runtime publication paths. The later single-SYNC policy
superseded the earlier separate REALTIME BPM tracker workflow; old bindings on
this branch address that policy. This history is not a compatibility requirement
for main, where those controls and jobs were removed.

The branch automatically obtains/restores estimated maps for ordinary SYNC tests;
manual map creation and Raw review are advanced corrections. Operational bars
count four detected quarters without proving meter. Immutable raw beats, downbeats
and logits remain distinct from explicit count/coverage/meter interpretation and
accepted corrected records. Missing beats are not repaired by snapping every beat
to a transient. A beat can lie in silence or a syncopated passage.

Original bytes, decoder/PCM identity, content ticket, map/window revision, loop,
pitch, master epoch and publication request fence preparation and adoption.
Copy shares immutable material where valid but retains independent pad intent
and requires fresh Native qualification. A source replacement, cancel, stale
request, failed publication or exhausted capacity cannot adopt old work into a
new content generation. No requested status can substitute for the current
source's own acknowledgement.

Mode/map/program ACK does not prove current grid phase. Current phase evidence
requires a complete actual source read with settled current content, map, policy
and programme identity. Even that is software-coordinate evidence, not audible
synchronization. Pause/Hold, source/grid edits, retrigger and master changes revoke
old observations. Refusal preserves previous effective audio and truthful feedback.

## Preparation, launch, PCM and stems

All decoding, inference, map construction, editing, hashing and heavy preparation
run outside the callback. Reuse immutable source/material custody, reader plans,
existing publication reservations and last-reader retirement. The callback must
not perform I/O, blocking locks, Python/GIL calls, logging or inference.

The qualified dry branch path requires Key Lock OFF, total pitch zero, qualified
map/coverage, current actual master epoch and ordinary launch `E=T`. Its explicit
global policy or legacy BPM Lock path does not open arbitrary delayed execution,
Native/pitch representation or fallback to scalar playback for requested mapping.
Dry global-rate adoption settles command/parameter drains, captures one boundary
and atomically claims the whole supported cohort. Active Native ownership,
retained Native input, paused mapped state, stale readers, unsupported transitions
or whole-map rate overflow refuse the entire effect. Equal-period speed edits do
not silently reanchor a pending start; a genuine epoch change fences old work.

Qualified dry waveform Hold uses the caused-pause ticket and complete cohort masks.
It retains the next source point while master time continues. Release prepares a
checked copied-address translation; ordinary dry Pause/Resume reuses the qualified
projection. These domains do not grant Native/pitch support or master-rate changes
while paused. Ordinary nonmapped Hold/Pause/Resume remains live on main.

Pitch `k` must never enter `M`, `B/S`, loop addressing or beat counts. Key Lock
holds the intentionally transposed pitch as tempo changes. Preserve the distinct
source-rate, musical pitch and Native compensation parameters. Actual input,
logical source position and audible output are different clocks.

Stems of one source share trajectory, wrap and revision. Coordinate agreement
does not prove that independently nonlinear processed stems sum to the processed
full mix. Preserve selected/pending/effective stem state, immutable artifacts,
source leases and exact retirement; active stem transitions retain their own gates.
Readable material, lazy finite residency, complete-source access and accepted
constant-timing OwnACK remain independent work. This branch must not be described
as containing main's later full-source file-backed CQT repair or all later residency
fixes. Recovery must reconcile actual implementations before any future change.

## Native future-input ledger and causal limits

Copied spans authorize only their declared endpoints, not arbitrary future input.
Original future-input ownership retains the actual Native/FIFO owner and refreshes
logical and actual spans independently. For raw stream cut `D`, silent physical
birth prefix `H` and block `B`, preparation consumes `F=D+B-1` adapter frames,
with real source input after the `H` zeros. After `j` emitted frames the labels are:

- logical target `T+j`;
- raw Native output `D+j`;
- actual next input `T+(F-H)+j`.

This ledger enables continuity experiments; it does not choose the musical cut
or prove output `D` belongs to intended time `T`. Early output stays visible.
Suffix equality and nominal API delay cannot conceal first-attack loss. Existing
prepared adoption and live-read consumers need independent intended-output proof
before any public nonzero mapped admission can open.

The retained causal fixture has `C3678-U2080=1598` frames, or 33.292 ms at
48 kHz, of required pretarget response. Strict first sound at `T` discards that
response even with unlimited preparation. A future target or different renderer
needs its own measured onset/retention policy; it cannot assume this limit away.

## Retained negative evidence and rejected shortcuts

The original [renderer comparison](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/accepted-map-render-comparison.md)
retains full fixture geometry, PCM/DLL/executable hashes, predicates and domains:

| Observation | Result and limit |
| --- | --- |
| Cold mapped Native/FIFO acoustic predicates | 0/64 within all original bounds. Cold-start delay does not by itself invalidate productive prepared history. |
| Offline R3, Key Lock ON, phase zero | 2/16 acoustic passes, 14 failures. OFF requires variable pitch the fixed-pitch API cannot automate; phase17 is unsupported. |
| Tonal windows | Native 105 pass / 7 outside / 16 insufficient; Offline 19 / 9 / 4 under the original energy/cycle/15-cent guards. |
| Caller partition pairs | 80 Native and 20 Offline pairs bit-identical under the declared canonical protocol; this is not arbitrary callback/pitch-transition acceptance. |
| Four gain components | Raw residual <=1.49e-8; independently processed residual about .338 maximum/.106 RMS. No actual separator/mask/lifetime acceptance. |

The owning-future measurement later retained **72 complete streams** matching the
separate C-ABI stream, caller partitions and source-coordinate assertions. All
60 isolated responses retained peak and bounded completeness, but **0/60 passed
the unchanged q10/q50 timing gates**. There were **40 empty timing intervals** and
**44 empty joint timing/retention intervals**; tones passed 72/96 windows. Response
deformation makes a single delay insufficient. A Drum-D analysis fallback cannot
repair that renderer response.

The original nearest-rounded B5 map gives rate `2.0000000000000004` and remains a
strict refusal. A separately declared supported fixture moves knot3 to the adjacent
lower binary64 second (about -2.13e-11 loaded frames). Do not relabel the original
record as accepted. Complete raw beginnings and fixed end windows remain evidence;
finite capture-tail checks do not prove internal Native-tail exhaustion.

Earlier Short/Medium comparisons and aligned Medium +7 isolated-event successes
are component evidence only. Standard/HighConsistency realtime Stretcher and the
nondefault Signalsmith canonical prerequisite failed all four retained kick cases.
Do not promote a different window, suffix crop, translation, gain normalization,
seam fade or test-only backend into an accepted productive renderer. A 57.5-ms
asymmetric window overlap and integral phase debt retain their original scope.

The legacy downbeat defect cast `0.01161` seconds to a zero sample increment;
the actual ODF hop was 512 at 44.1 kHz. Restoring units made scores audio-dependent,
not musically qualified. Beat This's nominal 20-ms lattice is not sample-accurate
beat truth. Published comparisons and numerical consistency do not establish
musical acceptance. Windows ONNX/export, neural/warp and portability variants
remain unexecuted or separately unqualified as recorded by their original sources.

## Recovery source map and unpassed gates

These are immutable original locators. Their dates, line numbers, old statuses,
requirements and command examples are historical provenance, not current startup
instructions. Complete reports remain recoverable even when main removes duplicates.

| Frozen source | Information retained |
| --- | --- |
| [Design, 27ef7f87](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/beatmap-sync-design.md) | Phase law; independent pitch; Raw acquisition/interpretation; editor/count/coverage; dry rate/Hold; future-input ledger; 72-stream failure. |
| [Research, 27ef7f87](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/beatmap-sync-research.md) | Comparator defect, rejected/conditional renderer approaches, bounds, 57.5-ms overlap and causal/preparation limits. |
| [Guide, 27ef7f87](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/source-map-guide.md) | Former controls, manual map/review workflow and uncertainty; not main's user manual. |
| [Diagnostic protocol, 27ef7f87](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/sync-diagnostics.md) | Requested/control/raw/cohort distinction; opt-in ten-minute/16-MiB trace, privacy, gaps/drops and incomplete crash captures. No current-main logging claim. |
| [Main audit, 9f3ca2d1](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/9f3ca2d19ad11b0218ccc5ef5dc5353c9981f341/docs/audit/source-disposition.json) | Original blobs/SHA256 and exact mixed spans: architecture782–791, development814–894, Portable320–330, offline5–36, program1–49 and nested timing/pitch/stem/JSON histories. |

Actual branch code starts at `rust/crates/looper/src/audio_engine/`:
`source_beatmap{,/record,/query}.rs`, `source_sync_policy.rs`,
`source_map_admission.rs`, `mapped_birth.rs`, `mapped_source_clock.rs`,
`source_reader.rs`, `prepared_source.rs`, `source_feed_history.rs`,
`stretch_processor.rs` and `sync_diagnostics.rs`.
The associated change directories preserve detailed contracts, results and unchecked
tasks: `adopt-live-source-sync-policy`, `adapt-dry-mapped-global-rate`,
`resume-dry-mapped-playback`, `own-mapped-native-future`,
`measure-owned-mapped-native-output` and `compare-accepted-map-renderers`.
These paths belong to this frozen branch; main's later archival may relocate its
own copies without changing branch provenance.

Independent full B2 references/ReferenceSeal, balanced real groups/classes, real
scores and six paired Human correction sessions remain unpassed. Raw/synthetic
arrays, accepted modes/maps and numerical stress success cannot substitute.
Renderer/onset, continuous/transition/resource/RT, prepared-stem, device and Human
listening acceptance remain separately open. The later productive BeatThis
`neural-stable-regions-v4` scalar policy on main does not retroactively pass them.
Historical full-program JSON/unchecked requirements remain lossless history,
not an active Goal or a completion report.

Any future explicitly authorized recovery must reconcile current source and valid
contracts, preserve one clock and source/ACK/reader authority, and qualify each
affected representation. Productive optimization targets 2/4/6 simultaneous PADs;
synthetic performance stress is at most ten. Above ten, performance problems or
glitches are possible; there is no device guarantee or hard ten-PAD app cap.
Internal 32-voice correctness is a separate ownership boundary. No full Rust port,
Slice8, automatic default cutover or new feature implementation follows here.
