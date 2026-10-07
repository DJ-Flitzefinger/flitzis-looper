# Musical and physical loop periods

G3c measures a remaining productive timing defect. Current accepted period and
source-rate precision do not remove repeated rounding of a physical loop length.
The musical gate remains incomplete until the shared renderer is corrected and
its strict fractional rendered/onset test passes.

## Independent domains

For loaded rate `Fs`, accepted quarter period `T`, logical loop beats `b`,
once-rounded physical endpoints `a/e`, and steady source rate `r`:

```text
P = Fs * T * b             musical source frames per cycle (possibly fractional)
H = e - a                 physical source frames per cycle (integer)
P / r                     intended output frames per cycle
H / r                     current productive output frames per cycle
k * (H - P) / r           unwrapped output discrepancy after k cycles
```

Each endpoint is rounded absolutely once. Repeating `H` still repeats its rounding
error. SourcePlayback retains fractional rate epochs independently of callback
partitions but uses integer physical wrapping. SourceGrid's compatible-cycle
diagnostic wraps musical time first. It cannot prove actual PCM recurrence.

At 44.1 kHz and 120 BPM a 1/64 note is `1378.125` source frames. Physical markers
at 0 and 1378 give a 9.375-frame lead after 75 cycles and a 125-frame lead after
1000 cycles at unity rate. Fractional rates divide the output discrepancy by `r`;
they do not repair it. Whole integer-period reference loops are positive controls.

## Productive evidence contract

The hardware-free Rust tests use immutable PCM, actual source-bound acceptance,
callback publication acknowledgement and current native metadata before RtMixer
renders. Synthetic raw evidence and independently declared counts are fixture
assertions, not a new analyzer run or human musical acceptance. The private
unchanged metronome uses retained real complete G2 evidence and freshly verified
actual decoded PCM; its originals and exports remain in the local workspace.

Expected physical PCM is interpolated independently without SourcePlayback,
SourceReadPlan or productive address helpers. Rendered threshold-feature indices
are measured from output and checked separately against physical and musical
oracles. Continuous unwrapped period error, integer output sampling and feature
offset are distinct quantities. A modulo phase comparison could hide complete
cycle slips and is insufficient. Fixed, irregular and one-frame callback
partitions must agree independently of these period errors.

The strict musical acceptance probe is deliberately separate from passing
physical-output characterization. An ignored acceptance probe is an explicit
incomplete gate, not a passing test. Device/loopback, human listening and sustained
audible acceptance require actual evidence. Dry PCM results do not pass B5 native
Key Lock crop/delay/transition compensation.

## Actual fractional render

The generated accepted-owner matrix covers 63 runs at 44.1/48/96 kHz: exact
rational quarter periods near 120 BPM and true 119.999/123.45 BPM, rates 0.73,
0.75 and 1.25 as applicable, and fixed, irregular and full one-frame callbacks.
Every run renders past cycle 1000 and measures cycles 75 and 1000. Current native
accepted publication, source pins, accepted-period BPMLOCK, fractional cursor and
wrapped source-beat queries are checked. Physical outputs are identical across
partitions; the maximum independent PCM sample error is `1.86e-9`.

Generated physical markers are independently nearest-even rounded from declared
rational fixture positions and admitted as integer source-frame intent. Current
accepted fitted timing supplies BPMLOCK and source-beat queries. At the 96-kHz
half-frame endpoint tie, the fitted binary64 period is infinitesimally above the
declared rational period and deriving a marker from it can select the adjacent
frame. The evidence retains declared and admitted periods separately. This
fixture proves productive playback of the admitted physical markers; automatic
marker refresh remains its separately tested G3b2 contract. Both periods fail the
strict musical limit, so this tie sensitivity cannot pass the pending gate.

An external exact-rational oracle independently checks all 63,063 measured onset
indices and 1,134 exported PCM samples. All physical expectations agree. All 63
fractional cases fail the musical-period limit at cycle 1000. A separately invoked
strict musical gate fails with these actual results:

| Loaded rate / source rate | Musical / physical loop frames | Cycle | Continuous source-period error | Actual / musical onset output frame |
| --- | --- | --- | --- | --- |
| 48 kHz / 0.73 | 1500.25 / 1500 | 75 | -18.75 | 154109 / 154135 |
| 48 kHz / 0.73 | 1500.25 / 1500 | 1000 | approximately -250 | 2054794 / 2055137 |

The -26/-343 discrete output-frame differences are actual rendered threshold
features. They are not inferred grid positions. Passing physical characterization
and a deliberately separate failed acceptance probe leave G3 incomplete.

## Actual unchanged private control

The unchanged private 48-kHz metronome's 24,000-frame loop is an integer-period
positive control: six fixed/irregular runs at rates 1, 0.73 and 1.25 render
152,153,432 output frames with exact agreement against the independent binary64
PCM oracle and 1001 matching features each. Whole-output hashes agree between
partitions. A separate reviewer compares native-decode evidence with original
PCM24 bytes and independently checks exported windows and feature indices.

The ideal rational 73/100 comparison retains four one-output-sample threshold
differences per partition, at cycles 757, 830, 903 and 976. They arise at the
explicit amplitude threshold from admitted IEEE rate/multiplication and binary32
interpolation rounding; each is 0.73 loaded frame. They are discrete feature
sensitivity, while continuous musical/physical period difference is exactly zero
for this integer control. The admitted IEEE oracle agrees exactly. These facts do
not loosen the fractional musical-period gate or imply device/listening evidence.

## Required productive correction

G3c2 must correct compatible fractional musical repetition through the shared
source trajectory and reader. It must preserve physical persisted markers and
one accepted timing/rate owner, safe fractional boundary taps, intro/tail/seek,
same-source stem transitions, copied worker domains and productive native/FIFO/
filter-history continuity. Scaling another BPMLOCK ratio or driving resets from
UI polling does not satisfy this contract.

After correction, rerun the complete 75/1000-cycle fractional period/rate/wrap/
partition matrix and actual rendered/onset gate with independent oracles.
Unwrapped continuous source-equivalent musical error must remain within one
loaded frame; exact integer controls remain exact. C1 immutable copy-first/ABA,
B5 audible compensation and unmeasured current 96-handle/save-integrity costs
remain separate. See [native ownership](native-constant-timing.md) and
[the migration design](beatmap-sync-design.md).
