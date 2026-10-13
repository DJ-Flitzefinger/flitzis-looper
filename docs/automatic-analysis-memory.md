# Complete automatic analysis memory — 2026-10-13

The reported six-minute stereo MP3 now completes the existing automatic BPM/key
workflow and source adoption at both 44100 and 48000 Hz. The analysis admission
limit remains **512 MiB**. This is an analysis-owned allocation bound, separate
from total process RSS/commit and model/ORT storage.

## Cause and repair

The previous admission charged complete interleaved and mono f32 copies. It also
omitted the actual CQT real/imaginary STFT and magnitude matrices. For this duration
the first 32768-point CQT octave alone retained hundreds of MiB of spectral data.
Removing only the input copies would therefore have been insufficient.

The complete immutable playback PCM is now borrowed. At 44100 Hz the existing f32
stereo mean is widened directly into one f64 input; at other loaded rates the
existing f32 mono/Rubato conversion executes once and its buffers retire before
the concurrent BPM/key pipelines. Both pipelines borrow that same f64 input.
The key frontend keeps the same CQT and inference semantics without widening it
again. No signal is cropped, re-decoded or analyzed by a different detector.

CQT synthesizes the existing centered zero padding for one FFT frame at a time.
Window generation, FFT dimensions, magnitude operations and accumulation order
remain unchanged. The first octave borrows the mono input. Sinc octave conversion
reserves the exact final capacity, executes the same process/flush calls and
discards only the suffix previously removed by truncation. Compact complete
octave responses remain available for the existing stack and frontend.

## Allocation accounting

For source frames `F`, channels `C`, rate `R`, converted frames
`T = ceil(F * 44100 / R)` and separately held finite-view bytes `H`, admission uses
`4*F*C + H + max(preparation, concurrent analysis)`. This maximum is justified by
the preparation allocations retiring before the scoped concurrent pipelines.
All allocations within each stage are summed.

Preparation includes mono f32, actual FFT rate dimensions/plans/scratch, delayed
output capacity and its overlap with f64 widening. At 44100 Hz it owns only `8*T`.
Concurrent analysis includes the shared `8*T` plus **both** CQT and BPM bounds.

- CQT: `6*T + 52*105*(ceil(T/8820)+1) + 64 MiB`. The first term covers simultaneous
  half/quarter f64 octave buffers. The second conservatively covers retained
  octave responses, stacked real/imaginary output, magnitude and f32 frontend.
  The fixed term derives from the maximum default 32768 FFT: 1024 bytes/point
  for original/scaled/magnitude bases and wavelet construction, 512 for FFT
  plans/scratch/window/input/output, and 512 for pinned Sinc tables/construction,
  chunks, vector headers and rounding. Tests check the pinned octave dimensions,
  absence of early downsampling and actual frame/resampling capacities.
- BPM: `128*D + rows*(3*128*8 + 4*24 + 2*8) + floor(T/16)*8 + 2 MiB`, where
  `D = ceil(T/512)+1` and `rows = ceil(D/128)`. This covers ODF/DP/backtracking/raw
  and legacy timelines including Vec growth, RCF/delta/psi trellises and outer
  headers, the complete downbeat decimation, and default FFT/transition/threshold
  workspaces. The borrowed shared input is charged separately.

Dimensions and byte arithmetic reject overflow before large allocation. Genuine
oversize, separately retained finite views and large coprime FFT units still reject.
Existing explicit preflight runs before request/epoch mutation. Source import,
cancel/stale fencing, rollback and matching callback ACK remain unchanged.

## Actual authorized MP3 evidence

Fresh isolated Release test processes exercised the real immutable staging,
complete analysis and callback source adoption. The 48000-Hz case uses the actual
public incoming-load API; the 44100-Hz case uses the existing cold worker with its
actual alternate format. No stream/device/App or installed extension was started.

Symphonia 0.5.5 decoded **16,602,624 stereo frames at 44100 Hz** with the established
gapless-disabled policy, retaining the declared 1105 delay and 1335 padding frames.
The ffprobe preliminary extent is 2440 frames shorter, matching the sum of the
declared delay and padding. Actual analysis uses the verified decoder/playback
extent, not that preliminary metadata.
Complete decoder/playback byte lengths, EOF digests and preserved original bytes
were independently checked by the import fixture.

| Actual loaded rate | Playback frames | Duration seconds | Analysis bound MiB | Lifetime peak RSS MiB | Sampled private peak MiB | Baseline current private MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 44100 | 16,602,624 | 376.4767347 | 436.7935 | 582.4492 | 594.5000 | 227.8438 |
| 48000 | 18,070,884 | 376.4767500 | 447.9956 | 581.6094 | 588.3086 | 216.0859 |

Both cases produced **161.6186829 BPM, 1014 beats and 254 downbeats/bars**, with
`analysis_error=None` and their own source ACK. Analysis/import elapsed 2.7796 and
3.1078 seconds in these isolated processes. These are observed software results,
not a musical ground-truth label or a speed guarantee. Key was truthfully
`unknown`; successful model inference and key-label acceptance are not evidenced.

Lifetime process peaks include the embedded Python/runtime, fixtures, allocator
retention and all import work. The 1-ms private sampler is an observed lower
bound. Neither RSS, sampled private nor a subtraction of baseline is an allocation
proof or a claim that the entire process fits 512 MiB. Model/ORT storage remains
outside the analysis-owned bound and requires its separate program gates.

## Verification and preserved limitations

- Independent dense STFT/response and growing-Sinc oracles preserve binary bits
  at centered boundaries, tail, default FFT dimensions, custom windows and
  decimation remainders/capacities. Shared mono matches the previous complete
  channel/rate oracle at mono/stereo 44100/48000/96000 Hz, including BPM/grids.
- 91 integrated analysis unit cases and 11 BPM/CQT reference integration cases
  pass. Focused source/finite-bank, explicit-budget, cancel/stale, queue/shutdown,
  manual/TAP and actual ordinary-loader incoming histories pass.
- Retained negative logs contain two corrected fixture failures: a supposed
  coprime rate actually divisible by 7, and an alternate-rate predecessor still
  bound to the old fixture's 48000-Hz ownership. No budget, productive guard,
  assertion tolerance or original oracle was weakened.
- The private Release wheel builds without installation; Release/API/Portable/
  controller/UI preflight passes. The three affected Python Loader files pass
  250 cases with two explicitly skipped device cases.

Logs remain outside the repository in `scratch/goal-mp3-analysis-20261013`.
The installed Human Native, running App/settings and external source remain
unchanged. Full musical/model/resource/216-pad, Copy/ReArrange, recording and
actual Human device/listening acceptance remain open. This delivery does not
close the original Pre-Rust Goal or its parent gates; full Rust port/Slice8 stays
outside the authorized boundary.

## Delivery closure

The private Release build took 24.16 seconds; Native SHA256 is
`f867958e97c559c87fd4fb4d76da3550dd678c5c73d73335ffc84ec2dd0f0f3d`.
After the test-only process-counter module was shared, numerical/runtime results
and this unchanged production binary are explicitly reused. The counter function
bodies and production behavior are unchanged; final all-targets Clippy compiles
the shared registration and retains exactly the previous 77 warning messages /
117 message-file pairs, with no new pair. Existing warnings remain.

Both affected OpenSpec changes validate strictly. All 17 changed/new files are
UTF8/LF without BOM; local Markdown links, Rustfmt and diff whitespace pass.
The one independent final source/evidence review passes with no open findings.
Normal main publication and clean-remote equality are recorded separately in the
active chat's local handoff. No full-parent or Human acceptance is claimed.
