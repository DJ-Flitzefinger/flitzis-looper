# Complete-source streaming: implementation and validation

X11-LONG/E11-03 now prepares complete FullMix and five aligned stem artifacts through
bounded blocks and immutable disk generations. Complete disk extent is separate from
scratch admission and the requested live loop/ALL allocation. This report retains
historical storage implementation evidence and KEYLOCK-OFF measurements from
2026-10-11; those runs do not certify the later saved finite ON admission work.
The bounded 2026-10-12 saved physical ON NormalLoop policy is described
[in the residency contract](pcm-cache-residency.md#initial-saved-finite-keylock-on--2026-10-12).
Initial focused runs passed four low-level admission checks; the corrected public
wrapper now passes all four physical ON histories in 6.12 seconds. Its failed
attempts remain recorded. Impacted cold-load/residency/cold-store integrations and
private Release/preflight/Python gates pass at the scopes recorded there.
Initial BPM-lock/accepted musical timing and ordinary external Loader incoming
timing/default-eight-bars admission remain open,
as do full P4b/P5b, X11-LONG/MATERIAL, resource/music and Human gates. Independent
final-tree review remains a separate publication gate.

## Implemented paths

`sample_loader/cold/stream.rs` continuously decodes the retained original into complete
disk PCM, then uses one continuous Rubato state for playback conversion, delay trim,
integer-ceiling extent and bounded tail flushing. Full original, decoder, playback and
signed-f64-channel-mean-to-f32 mono integrity remain mandatory. Fresh warm validation
scans complete contents and EOF before reading the requested resident interval. Native
loading with finite bounds and finite restoration retain only their requested live PCM.
Material preparation uses a one-frame metadata seed and complete lease; subscribers
then read their actual finite selection. Automatic analysis retains its own preparation
budget and terminal error alongside a valid source.

Python completion cleanup now retires the old source's analysis/task state before
processing the new assignment's supplied analysis error. Newer Manual/TAP/accepted
timing keeps its own fields and setters. The public-order mocked controller tests
exercise blocked analysis during loading, new-source success, genuinely new-source
analysis and late old events; Native source/analysis-budget evidence is separate.

Ordinary `LoaderController.load_sample_async` still passes no resident hint to the Native
loader, so its initial live allocation requests the complete source and may exceed the
real RAM budget. `SourceSuccess` processing subsequently invokes
`initialize_loaded_pad_defaults`, which selects the default eight bars. That later
initialization cannot repair an initial whole-source allocation failure. This ordinary
UI/default-loop integration remains open in the existing P4b/P5b work; it is separate
from bounded disk preparation and the historical finite Native/restore proofs with
KEYLOCK off. For trustworthy saved physical ON bounds in the supported NormalLoop
context, one native policy now resolves selection against the incoming verified
source's actual rate and extent: proper partial ranges use `KeyLockFiniteLoop`, while
resolved `[0,N)` ranges use `KeyLockFullTrack`. Unknown or absent finite geometry
retains explicitly admitted complete demand. Complete storage, source identity and
integrity remain unchanged, and actual selected allocation/overlap limits still
apply. This bounded selection work has passing runtime receipts; it does not close
the remaining initial timing, ordinary Loader or full P4b/P5b obligations.

`stem_cache/streamed.rs` scans complete source/component data for global onset maxima,
then finds the first 5-percent threshold crossing and reads the bounded analysis window.
It preserves the legacy four-second window, 0.5-second pre-onset, +/-0.25-second lag,
ascending tie rule, PCM16 MIN=-1/otherwise-divide-by-32767 conversion, established f32
component order and signed offset with exact padding/tail. All five WAV/PCM hashes,
source/mono binding and exact geometry are verified before descriptor-last eligibility;
fresh pair reopening repeats full verification through retained sealed readers.

Python `pcm16_wav.py` provides positioned integer-PCM RIFF/RF64 reading, exclusive
PCM16 writing and complete hash/EOF verification. Small writes retain the legacy
44-byte RIFF header; large writes use RF64 `ds64` 64-bit counts. `stem_artifacts.py`
streams model-output resampling and instrumental summation without complete upstream
arrays or a total-u32 target cap, retaining the frozen small-file requantization oracle.
BS-RoFormer uses the shared writer and verifies closed outputs before promotion. The
Demucs adapter retains the original CLI/inference and streams PCM16 encoding through
FFmpeg with RF64 support; packet 41 executes the dedicated TorchCodec/FFmpeg sample
parity tests. Model inference,
input/output tensors, activations and whole Demucs/model RSS remain a separate scaling
problem. No model run is included in the evidence below. Pure offline helpers live outside controller package
initialization, and vendor source/license edits participate in uv's explicit cache keys.

Native stem parsing accepts exact RIFF/RF64 PCM16 geometry and compatible extensible
PCM headers. FullMix uses the actual Symphonia registry with `Rf64Reader`, retaining
`rf64-wave-ds64-flitzi-demux-v1` provenance. Heavy preparation remains outside the
realtime callback. The historical storage implementation left callback/hot-path source
unchanged; independent whole-source review checks the control/background boundary
as part of final acceptance.

## Actual complete-source runs

These are historical 2026-10-11 storage runs with KEYLOCK OFF.

Packet 61 executed the configured productive Native parent through PyO3 dispatch to
`AudioEngine.load_sample_async`, source-ticket capture and public pair preparation.
Ten separately identified child processes completed: one prepare and one genuinely
fresh warm child for each case. Warm children assert `warm=true`, unchanged cache path,
source identity and decoder identity; the first four also retain the same five-artifact
pair identity. These are Rust test executables hosting Python. Installed-extension
import/preflight validation is a separate gate.

The tables below bind packet 61/LONG attempt 4 on the settled path-protection index
and corrected parent timeout branch. Packet 36 remains predecessor-source evidence.
The corrected, unexecuted timeout branch writes an exact child/phase/PID ownership
receipt and fails the parent without forcibly killing the child. No timeout-branch
runtime result or forced interruption of retained-reader I/O is claimed.

All cases use 48-kHz stereo. The PCM byte count is one complete f32 source extent and,
for the first four cases, the extent of each of the five aligned PCM artifacts. Times
are child work times including public preparation and integrity/lifecycle checks.

| Case | Complete frames | PCM bytes per extent | Duration (s) | Prepare / warm time (s) | Complete five-stem pair |
| --- | ---: | ---: | ---: | ---: | --- |
| Small | 144,007 | 1,152,056 | 3.000 | 0.268 / 0.157 | Both phases |
| Medium | 8,000,013 | 64,000,104 | 166.667 | 3.524 / 1.074 | Both phases |
| Large | 134,300,001 | 1,074,400,008 | 2,797.917 | 64.365 / 25.472 | Both phases |
| Boris | 16,103,550 | 128,828,400 | 335.491 | 7.321 / 2.639 | Both phases |
| RF64 | 1,073,750,007 | 8,590,000,056 | 22,369.792 | 82.984 / 32.557 | Source/parser only |

Large individually exceeds ten minutes and 1 GiB of complete source PCM. Synthetic
fixtures are generated in bounded partitions 4093/8191/65521 with independent complete
source, mono and all-five component oracles. FullMix PCM16 decoding uses /32768;
the independent stem oracle separately preserves MIN=-1/otherwise-/32767.

Boris uses a protected read-only copy of the reported original and sealed copies of
the existing five integer WAVs in isolated output. Its original SHA256 is
`64d318544c17b649080bea55889b7e154be1c2cda35a481e21f99bd1a5b8165a`.
The independent prior-algorithm oracle checks all five complete PCM hashes, signed
mono and actual offset -2456 in both phases. Its 335.49-second duration supplements
the large fixture. User source/config/current cache remain protected; the test does
not establish a live PAD4 assignment or a model fingerprint.
The separate read-only post-packet-61 observation of the protected original retains
the baseline SHA256, 13,471,040-byte extent and modification time; it is separate
from the productive child's isolated-copy receipt.

Packet 06's Python test writes every sample of an actual 4,295,000,108-byte RF64 file,
independently hashes header/payload, reopens and verifies every byte and the last sample.
Packet 61 consumes that same file through the Native stem parser and public Symphonia
source path, with independent hashes for their distinct normalization conventions and
full-source fresh warm verification. RF64 receipts have `actual_public_pair=false`,
no pair/stem oracle, zero live component bytes and four source leaves. This proves
large common Python codec -> Native parser/FullMix interoperability; complete five-RF64
pair preparation and model inference are outside this actual run. Forced-RF64 small
model-output/oracle and above-RIFF target-admission tests cover the Python artifact
consumer in the focused packet 41/42 runs. Packet 63 repeats the actual complete
>4-GiB writer/reopen test on the frozen helper. These runs do not establish complete
five-RF64 pair preparation or model inference.

## Measured memory and owner return

These measurements belong to the historical 2026-10-11 KEYLOCK-OFF runs.

The following byte counts distinguish incremental Rust allocations in each measured
phase from Windows lifetime process peaks. Rust observations use a test-only System
allocator; they do not account for Python or other native-library allocations. Windows
PeakRSS/PeakCommit include interpreter initialization and are not a scratch limit.

| Case / phase | Rust load additional peak (B) | Rust pair/parser additional peak (B) | Windows PeakRSS (B) | Windows PeakCommit (B) |
| --- | ---: | ---: | ---: | ---: |
| Small / prepare | 26,495,712 | 1,298,472 | 245,886,976 | 252,465,152 |
| Small / warm | 26,494,367 | 465,846 | 245,846,016 | 252,461,056 |
| Medium / prepare | 180,945 | 1,714,423 | 221,417,472 | 227,176,448 |
| Medium / warm | 177,256 | 465,987 | 219,688,960 | 225,759,232 |
| Large / prepare | 307,459 | 1,718,415 | 222,220,288 | 228,241,408 |
| Large / warm | 177,459 | 465,966 | 220,168,192 | 226,078,720 |
| Boris / prepare | 277,483 | 2,181,228 | 222,826,496 | 228,151,296 |
| Boris / warm | 243,876 | 2,181,448 | 221,745,152 | 227,512,320 |
| RF64 / prepare | 181,246 | 135,003 | 218,841,088 | 225,681,408 |
| RF64 / warm | 178,233 | 135,003 | 218,587,136 | 225,402,880 |

Cold source scratch reservation is 10,551,296 B and fresh warm reports zero decode
scratch. Alignment upper bounds are 2,828,400 B for small and 3,596,288 B otherwise;
these are reservation/static bounds, not measured RSS. RF64's pair phase measures
the complete parser scan, not aligned-pair construction.

Each primary selection retains 96,000 B for frames 12,000..24,000. Non-RF64 cases
retain four live components totaling 384,000 B and a pre-existing sounding voice of
256 B. Large prepare observes two actual overlapping cold workers and a distinct
8,000,019-frame secondary source with its own 96,000-B live selection, success,
identity and complete decoder/playback verification. Old audio continues through
headless callbacks with finite nonzero output. These observations certify this
specific overlap, not arbitrary voice counts or device/hearing quality.

Large source loading succeeds while automatic analysis terminally reports
`automatic analysis exceeds its PCM preparation budget: analysis PCM limit exceeded: complete automatic analysis transient bytes`.
Small automatic analysis actually succeeds. Large explicit ALL restoration reports
`warm PCM conversion exceeds whole-job budget`, preserving the existing small
selection. Source validity, analysis demand and explicit ALL residency therefore have
separate outcomes. This packet does not execute Python UI error presentation/cleanup.

Before each child exits, real tickets/prepared objects and component/old/selected PCM
weak references return; shutdown verifies cold workers/queue/admitted counts (0,0,0).
The sealed-source closure probe is still false: acknowledged ProjectAssets saved
claims and three saved pair path owners retain complete cold files (RF64 has zero
pair owners). After successful child exit, the parent contains-checks and actually
rename/restores every listed leaf on Windows: 17 for ordinary source+pair cases,
21 for large prepare with secondary source, 17 for large warm and four for RF64.
Only this post-exit operation establishes `post_process_sealed_handles_returned=true`.
It does not prove sealed-file closure before exit or whole-program unload retirement.

## Fault coverage and retained validation state

The following validation state records the historical 2026-10-11 storage delivery.

Native tests executed in packets 09/39/50/55/56 include complete five-WAV/five-PCM first/late
mutation, missing/truncated/extra EOF rejection, finite warm selection with late PCM
damage and forged mono/decoder binding rejection. `streamed_decoder_and_resampler_cancel_during_actual_complete_work`
cancels after actual nonempty output; `mid_complete_verification_cancel_returns_reader_and_preserves_owned_rollback`
cancels during the reopened full scan. Pair cancellation checks returning operations
at AlignmentScan, PcmConvert, PcmWrite, PcmVerify, PcmRename and CommonReopen; a separate
post-PairCommitted signal preserves the already verified generation and readers.

`productive_mid_write_read_only_handle_fault_rolls_back_only_owned_staging` performs
a real successful PCM write, substitutes that attempt's handle with a read-only handle
and observes the next OS write error. `conversion_cancel_after_a_returning_write_and_precise_writer_error_are_visible`
injects StorageFull at a bounded output write. Pair tests inject the second PcmWrite/
PcmVerify operation and explicit write/flush/reopen/rename/common-descriptor boundaries,
preserving reused WAVs and permitting retry. `allocation_failure_after_productive_global_scan_preserves_files_and_retries`
injects before the third `try_reserve_exact`, after complete first-pass scan/digests.
These are precise isolated faults, not measured drive exhaustion, host OOM or power loss.

The focused Python tests in packets 41/42 additionally inject OSError on the third
temporary write, mutate the final payload byte after durable flush and before full
reopening, and inject MemoryError on the seventh bounded payload allocation after prior productive I/O.
They preserve the previous five-output generation. Demucs tests exercise an actual
unsupported encoder failure and late nonfinite output with natural process return.
Packet 41 reports 199 passed and three skipped; packet 42 reports 64 vendor passes in
an isolated import-mode run. These focused results retain their recorded source
snapshots; packets 64/68 additionally validate the final installed environment.

Packet 49 executes the ignored independent codec boundary probe on 17 actual fixtures:
three each FLAC, Ogg/Vorbis, AIFF, AAC and ALAC, plus two MP3. All 17 productive streamed
decoder PCM SHA256 values and complete provenance objects equal the retained pre-stream
decoder results. Independent FFmpeg comparisons retain the explicit compressed-codec
origin/priming conventions. Each fixture's continuous streamed playback at 44.1, 48 and
96 kHz matches the unchanged fully padded FFT oracle within 2e-6, with equal transform
metadata. All three forged oversized ALAC configurations are rejected. This is one
executed Native probe over 17 fixtures, not 17 separate tests or a new model run.

Packet 45 passes four path-index parity/removal/seeded-work tests, one real cold/paired
reader equivalence test and the unchanged discard saturation test. The actual work-shape
receipt records 2,291,712 old symmetric path comparisons versus 7,168 index lookups for
1,024 targets and 373 six-path reader shapes, with 1,865 exact keys, 3,736 prefix keys
and 517,409 encoded key bytes. Key bytes exclude hash-table/allocator overhead and RSS;
this observation does not prove the cause of packet 39's timeout. The discard test
retains its eight-I/O budget, ownership assertions and existing deadlines; its isolated
run completes in 6.83 seconds. Registered history/assignment scans and full identity/proof
hashing remain under the existing asset gate.

| Packet | Actual result and scope |
| --- | --- |
| 09 focused Native | 98 passed, 0 failed, 2 ignored, 941 filtered in looper; other workspace suites filtered. |
| 06 productive Python RF64 | 1 passed in 7.13 s; common writer/reader full >4-GiB payload. |
| 36 productive parent | Parent 1 passed, 1040 filtered, 236.24 s; all ten configured children each 1 passed. Wrapper exit 0, 289.45 s. |
| 10 broad Debug | **Retained failure:** 1015 passed, 6 failed, 19 ignored, 1 device filtered. Wrapper falsely returned 0 because it omitted captured native exit propagation. Specific source/test defects were repaired; broad synchronization/analysis timing causes remain unproved and the raw negative remains retained. |
| 32 first productive attempt | Failed: harness temporary cold-leases guard self-deadlocked while obtaining source manifest after load. Exact owned failed child stopped; no file operation was interrupted. |
| 35 second productive attempt | Compile failed at receipt `json!` recursion expansion; no productive child result. |
| 39 broad Release | **Retained failure:** 1020 passed, 1 failed, 19 ignored, 1 device filtered; discard saturation times out waiting for the first eight job paths to return, before pair retirement. Cause remains unproved. Individual fault tests passed within this failed broad run. |
| 41 focused Python source | 199 passed, 3 skipped, 18 warnings in 19.96 s across PCM16/RF64, stem artifacts, Demucs output parity/faults, generation/separators and loader tests; wrapper exit 0. |
| 42 isolated Python vendor | 64 passed in 2.47 s with `--import-mode=importlib`; wrapper exit 0. |
| 44 initial path index | **Retained failure:** 3 parity tests failed, 1 work-shape test passed; wrapper exit 101. The first mismatch was empty protected path versus Windows `C:`. Pair/discard commands did not execute after that failure. |
| 45 corrected path index | 4 path tests, 1 actual cold/paired-reader test and 1 unchanged discard test passed; wrapper exit 0. Explicit terminal-empty prefix fixes packet 44's actual Windows mismatch. This focused pass does not erase packet 39 or replace the broad rerun. |
| 49 independent codec probe | 1 Native test passed over 17 actual fixtures; all 17 streamed decoder hashes/provenance agree, all three playback rates match the independent FFT oracle, and all 3 forged ALAC configurations reject; wrapper exit 0. |
| 50 broad Release | Looper 1026 passed, 0 failed, 19 ignored, 1 device filtered in 271.90 s; Analysis 85 unit and 83 integration tests passed, 1 doc test ignored; wrapper exit 0. Productive Rust was settled, but Python Loader/key authoring continued during the run. This is broad Rust evidence, not final frozen-source/runtime acceptance. |
| 52 actual release launcher | `start-release.bat --build-only` compiled and installed Release successfully; no App start. Installed PYD SHA256 `83767e581ba0ab73f9f26e6a684561684d8908307c325e480f7736bd266b141c`, 28,745,728 B. |
| 53 initial timing fixtures | **Retained failure:** 175 passed, 2 failed, 2 device skipped. Manual/TAP fixtures used invalid legacy anchor -0.125 and therefore depended on old-source analysis. |
| 54 corrected timing fixtures | 177 passed, 0 failed, 2 device skipped in 11.81 s. Fixtures use their valid own anchor +0.125; all timing/field/no-setter assertions remain. Productive Loader unchanged. |
| 55 frozen broad Release | Looper 1026 passed, 0 failed, 19 ignored, 1 device filtered in 266.88 s; Analysis 85 unit + 83 integration passed, 1 doc test ignored; wrapper exit 0. All six packet-10 failing names pass. Eight actual integration images were hashed prospectively before execution and retain those hashes afterward; packet 50's missing historical image hashes are not reconstructed. |
| 56 full Debug | Looper 1026 passed, 0 failed, 19 ignored, 1 device filtered in 976.88 s; Analysis 85 unit + 83 integration passed, 1 doc test ignored; wrapper exit 0, 1036.89 s. All actual integration images are bound before/after with equal hashes; the Looper image was actually rebuilt. Productive inputs were frozen; two later test-only style fixes are exercised by final Python validation. |
| 57 / 59 Ruff | Packet 57 retained two test-style failures. Splitting one conjunction and combining two setup assignments preserves assertions; packet 59 passes all `src` Ruff checks. |
| 58 / 60 static checks | mypy passes all 239 source files. Whole Rust formatting and all 13 changed Python files pass; the two unchanged whole-Python baseline formatting failures remain outside this diff. |
| 61 final productive parent | LONG attempt 4: parent 1 passed, 1045 filtered, 242.93 s; ten configured fresh children each pass, wrapper exit 0 in 244.04 s. Settled index and safe timeout harness; actual measurements are the tables above. No timeout branch executed. |
| 62 official OpenSpec | Official `npx @fission-ai/openspec@latest validate adopt-pad-owned-pcm-residency --strict` passes the final affected change. Other active/base/archive changes remain untouched. |
| 63 final Python RF64 | 1 passed in 7.08 s; writes every payload byte above 4 GiB and fully verifies hash, EOF and last sample on the frozen helper. |
| 64 full installed Python | 3577 passed, 36 skipped, 18 warnings in 261.74 s; wrapper exit 0. JUnit proves all nine actual TorchCodec/FFmpeg clamp/rescale/none x 1/7/4096 oracle cases executed without skip/error. The 36 skips are 35 explicit device cases plus the separately executed large RF64 proof from packet 63. Final controller/timing fixture/style tests pass. |
| 65 / 66 installed Release | Actual `start.bat --check` passes without starting App. Imported Native profile is Release; installed PYD and current Release DLL have equal SHA256 `83767e581ba0ab73f9f26e6a684561684d8908307c325e480f7736bd266b141c`. Actual installed vendor/helper hashes match their final source; pure offline imports do not import Native. |
| 67 final Cargo check | `uv run --no-sync cargo check --manifest-path rust/Cargo.toml` passes; wrapper exit 0. |
| 68 final installed vendor | 64 tests pass in 2.23 s with isolated `--import-mode=importlib`; wrapper exit 0 after the final Release install/import binding. |

Earlier whole-Python formatting recorded two unchanged baseline failures (`controller/stem_cache.py`
and `controller/stem_job.py`); all changed files pass the final formatting gate.
The 2026-10-11 software command gates pass at the scopes above. Independent whole
frozen-tree review is recorded separately before publication. Filtered analysis suites, no-op
helper children, scope audits and strict spec validation cannot stand in for these gates.
No App, CPAL/device, recorder, hearing or model run is claimed here.

Raw evidence remains local under `scratch/goal-long-source-streaming-fix-20261011/`:
`61-native-long-summary-v2.json` derives from `native-long-attempt-4/parent.json`
and its ten child logs/receipts. Packet 36 and `root-productive36-scope-audit.md`
remain predecessor evidence, not final frozen-tree acceptance. Packets 41/42/44/45/49/50/55/56/61/63/64/68
retain their individual command receipts and raw logs; `codec-stream-attempt-1` contains
packet 49's 17-fixture decoder/provenance/playback evidence. This maintained report
records their bounded facts without copying private audio or raw fixture data into Git.
