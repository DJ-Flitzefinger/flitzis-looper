# Finite Seek and physical-domain software validation

This bounded P4b step extends the accepted NormalLoop implementation at
production k=0 KEY LOCK. Whole P4b, initial finite KEYLOCK-load/default-loop
admission and P5a/P5b, nonzero KEY/B5, music/resources and Human visual/device/
hearing gates remain open.

## Production behavior and authority

The existing effective-voice capture chooses its actual source and physical loop.
A held finite KEY LOCK source prepares the loop for in-loop seeks, the prefix
through loop end for Intro, or loop start through actual source end for Tail.
Source zero and complete duration remain metadata truth. The same retained range
readers, checked peak admission, WindowWork/control ACK and Native preparation
lanes own these ranges. Coverage checks copied SourcePlayback/SourceReadPlan,
both fractional taps and both active selection sides, including rate smoothing,
virtual P/integer H seams, lookahead and natural return to NormalLoop.

Every explicit seek advances voice generation and discards pending/adopted
Native/FIFO and filter history, even at equal phase. It preserves canonical
rate target and smoothing intent. Physical loop changes compare both next
physical tap addresses and interpolation weight; a coordinate-preserving end
edit can still require a cut. Clean Native/recycle capacity is admitted before
resetting usable audio. Continuous edits and storage-only full-to-finite return
may retain adopted chronology with its original reader pins; pending candidates
remain exact-window/source/timing/selection/history/checkpoint bound.

Source A retained after bank replacement keeps its original lease, loop, timing,
selection and fresh retained-voice Native permit. New bank B cannot authorize A
or revive a pre-replacement candidate. Wet old-A worker tests explicitly admit
B as a complete source under the existing KEYLOCK load policy; their A backing
and Seek/Intro/Tail preparation are finite. These cases do not prove initial
finite KEYLOCK admission for B. Non-KEY/complete-backed Seek and user ALL remain
explicit complete exceptions. No total track-duration/disk-PCM limit is added.

The callback performs fixed guards/scans, scalar clock work and existing owner
exchanges. Range IO, Native construction/reset/catch-up and large retirement
remain off realtime. No additional pool, scheduler, reader registry or fifth
live instrumental component is introduced.

## Meaningful evidence and retained limits

Actual worker/control tests extend the existing resident seek, nonresident
Intro/Tail/paused seek and old-source lease tests. Their independent complete
PCM is built from original WAV integers; an algebraic source clock and raw
Rubber Band/FIFO plus chronological EQ oracle independently model Intro/Tail
and loop return. They observe actual finite read/allocation receipts, own ACK,
fresh captures, actual Native catch-up and exact adoption, continued wet output,
both taps, FIFO/cursor, pause/resume, rate ramps, physical edits and ALL return
under irregular callback partitions. Additional finite productive-history and
same-StemSet/mask transaction cases preserve their prior assertions.

Failed initial runs remain recorded: an active-transition coverage fixture
incorrectly expected its captured contract to survive ramp completion; the raw
oracle incorrectly inserted initial mode-ON dry warming after an already
effective-ON source cut; the finite replacement-bank fixture encountered the
retained initial KEYLOCK-load guard; nested large Debug mixer fixtures exceeded
the test thread stack. Corrections retain exact pending fences, Native latency,
the original old-source semantics and substantive horizons/tolerances.

The first broad Debug attempt returned 1066 passes and three failures; the
analysis crate was not reached. It exposed two production control-order bugs:
same Normal markers after a timing refresh could cut complete-source chronology,
and same-callback Stem selection consulted a pre-edit playback domain. Shared
cut admission now preserves unchanged Normal geometry, while selection proves
the copied next-render domain. The storage-only preservation test now rejects
locally held Intro/Tail targets with missing prefix/suffix and physical bounds
one frame outside its backing; its exact 3897-frame wet suffix is retained.
Three focused Debug regressions then passed. Those earlier attempts do not
replace final broad validation against the final source.

A further real-executor regression first exposed inconsistent KEY-ON warming
in its independently built complete fixture, then an unconditional Normal-clock
rebase during RelocateResident application. The fixture now uses the matching
public KEY-ON transition. Scalar and resident marker application share the
actual unchanged-Normal-geometry rule before changing marker state; equivalent
clamped geometry also preserves the canonical clock. The test proves an actual
finite WindowWork ACK and Native adoption, then queued timing publication and
an exact manual executor binding at a virtual P/H seam, followed by 3456 frames
of wet complete-PCM equality. Its manually supplied timing binding does not
claim a timing-analysis worker or WindowWork timing-publication proof.

The first broad Release attempt returned 1068 passes and two failures before
analysis tests ran. One old rejection fixture reapplied identical markers;
it now makes a real continuous one-frame physical edit while retaining its
4000-frame audio/old-handle/no-adoption assertions. The finite old-source seek
fixture now waits off-thread, for at most five seconds, for the actual clean
Native/recycle reserve before each positive used-handle cut. Its immediate
second equal-phase seek still has no wait, and the failed-reserve negatives,
wet suffixes and strict state assertions are unchanged. These fixture repairs
do not relax production admission or constitute final broad acceptance.

## Final corrected software results

The final source passes both complete Rust workspace runs, with only the
device-opening test explicitly excluded. Their five nested project-guard child
summaries are separate subprocess checks and are not added to the primary totals.

| Actual check | Result |
|---|---|
| Debug workspace, four test threads | 1070 Looper + 168 Analysis passed; Looper 785.41 s |
| Release workspace, four test threads | 1070 Looper + 168 Analysis passed; Looper 206.71 s |
| Separate strict musical Release gate | 1 passed, 1089 filtered; nine other binaries ran zero tests |
| Fresh Release build/install and launcher `--check` | Both passed; launcher did not start the App |
| Full Python/FFI against the fresh Release with explicit FFmpeg | 3656 passed, 36 skipped, 19 warnings; 640.89 s |
| Current Cargo check and Rust fmt | Both passed against the final Rust inputs |
| Ruff and Mypy | Passed; Mypy checked 255 files; Python source/configuration unchanged |

Each broad native run retains 19 ignored Looper cases, one explicitly filtered
device case and one ignored Analysis documentation example. The strict musical
case is then actually invoked with `--ignored`; its success is not inferred from
the broad runs. Python JUnit contains 3692 cases, zero failures/errors and exactly
36 skips: 35 device cases and the unchanged explicit greater-than-4-GiB RF64
integration case. The final full run resolves the earlier omitted encoder cases;
the historical 18-case supplement is not added to its total. Existing warnings
and the RF64 skip are not additional acceptance evidence.

The actual loaded Release extension and target DLL are byte-identical at
29016064 bytes, SHA256
`00b6d6272202027da8f4a82f8c44d159f909ec135682eb085407e729052e84ac`.
Actual imports before and after the final Python run agree. Six changed
production files match the successful installation inputs; all thirteen changed
Rust production/test files match both final native runs and the explicit strict
gate. Debug and Release test executable identities are recorded separately from
the installed extension. The frozen staged tree and distinct independent review
are additional publication requirements; generated private receipts stay outside
the repository.

The unchanged strict gate checks 75 and 1000 cycles at at most one loaded-source
frame; both actual checkpoint boundary and onset errors are zero. Broad rendered
proofs also preserve binary64 P/integer H, 119.999/123.45
BPM, exact integer positive controls and 48 compact fixtures under three callback
partitions. The original-history Native CSV has 6480 measured rows with exact
reference equality. Its 48-kHz/rate-2/H16384 subset has 144 rows and no feasible
criterion pass: C3678 exceeds U2080 by 1598 frames, or 33.292 ms. The nominal
impulse cut loses its peak and about 96.714% of its energy. Passing the probe
preserves this measured failure; it does not close the causality gate. The
separate H32768/H65536 mixture probe is not substituted for H16384.

No software test claims an App/CPAL/device or listening run. Full P4b, P5a/P5b,
B5/nonzero KEY, resource/music/216 and final Human acceptance remain open.
