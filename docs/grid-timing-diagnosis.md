# Grid timing drift: diagnosis and correction direction

Investigation date: 2026-10-06. Runtime baseline: `35b00c5`.
Status: reproduced and diagnosed; G1 source-coordinate precision is implemented.
Automatic tempo-summary correction and live accepted-period unification remain pending.

The investigation uncovered a significant, long-standing weakness in automatic
BPM derivation: coarse detector endpoint errors become a constant tempo error
that propagates into the scalar grid and dependent loop operations. A correct
first anchor and a displayed `120.00` do not establish an exact 120-BPM grid.
The interval-mean implementation is present at least since `a66434a` (2026-05-31),
as confirmed by Git blame. The numerical reproduction below applies to the
tested current baseline; it does not establish identical historical outputs.

## Reference and reproduction

The private `metronom_120_BPM.wav` reference is 600 seconds of mono PCM24 at
48,000 Hz: 28,800,000 frames and 1,200 byte-identical half-second blocks.
All pulse onsets, 1% amplitude crossings and peaks lie at `n * 24000` frames.
Every one of the 1,199 pulse intervals is exactly 24,000 frames. Its SHA-256 is
`96ffe98cf44215719b0b57d605d6dc586c9c4e763ad3d47d512d0ba787d204ef`.
The first pulse is at frame zero; the last is at 599.5 seconds.

A separate muted native probe loaded it at 48 kHz as stereo. The complete
native mono export equals every normalized source sample: zero mismatches and
zero maximum amplitude error. This rules out a decode/load time-axis slope for
this tested path. Other output rates, live DSP and acoustic output were not
verified by that comparison. The unchanged installed extension SHA-256 was
`96fa310f95cb6cb8385131283138de8648340354d13ff0cf22042cc2159686bd`.

Normal automatic analysis publishes **120.00128936767578 BPM**. The BPM field
formats this as **120.00**, while the editor uses its full effective value:

```text
grid_seconds = anchor_seconds + beat_index * 60 / effective_bpm
```

With the correctly initialized anchor at zero:

| Reference position | Calculated grid position | Grid early, loaded frames at 48 kHz |
| --- | --- | --- |
| 10.0 s | 9.999892553848165 s | 5.157415288 |
| 30.0 s | 29.999677661544496 s | 15.472245864 |
| 599.5 s | 599.4935586031975 s | 309.187046521 |
| 600.0 s, extrapolation | 599.9935532308899 s | 309.444917284 |

There is no pulse at 600 seconds; that row is a mathematical extrapolation.
Its displacement is about 6.447 ms. The 10/30-second values reproduce the user's
approximately 5.1/15.4-sample observations. Screenshots alone cannot recover the
historical raw BPM or complete project settings. Setting manual BPM to exactly
120.0 removes this scalar-grid slope for the reference.

## Root cause and propagation

[`calculate_bpm`](../rust/crates/analysis/src/lib.rs) averages adjacent detector
intervals. For ordered positions `t[0] ... t[N-1]`, its calculation telescopes:

```text
mean_interval = sum(t[i] - t[i-1]) / (N-1)
              = (t[N-1] - t[0]) / (N-1)
BPM           = 60 * (N-1) / (t[N-1] - t[0])
```

Thus internal beat-position evidence does not improve the fitted slope; only
the two endpoints and interval count determine it. Missed/extra beats can
also corrupt that count. Here the tracker emits integer onset-detection frames
with an actual 512-sample hop at 44,100 Hz, about 11.610 ms. It reports 1,199
beats, omitting the source pulse at zero. The first/last detector frames are
43 and 51,636, with 1,198 intervals:

```text
detected span = (51636 - 43) * 512 / 44100 = 598.993560090703 s
true span for the same count              = 599.0 s
endpoint span error                      = -6.439909297 ms
derived BPM                              = 120.00129014595002
published native f32 BPM                  = 120.00128936767578
```

Coarse endpoint timing becomes a persistent tempo bias. The final `f32` BPM
cast contributes a much smaller additional error; it is not the main cause.

[`visible_grid_lines`](../src/flitzis_looper/ui/waveform_grid.py) projects a
regular scalar-BPM raster, rather than drawing each raw `beat_grid.beats`
position. Its formula computes absolute lines directly: repeated rounded
addition is not responsible for this slope. The independently stored activity
anchor can be correct while the period is wrong. Display formatting in
[`sidebar_left.py`](../src/flitzis_looper/ui/render/sidebar_left.py) conceals
that distinction. Effective BPM also controls snapping and automatic loop
duration, so the consequence extends beyond drawing. Moving the origin or
loop start cannot correct the period. Loop-relative labels affect numbering,
not the source positions of grid lines.

## Separate precision and loop findings

- At the investigated baseline, waveform bounds and returned X used `f32` seconds in the
  [native engine](../rust/crates/looper/src/audio_engine/mod.rs). At 599.5 s,
  16 adjacent 48-kHz frames collapse to seven distinct X coordinates; a
  one-frame query can round both bounds to the same value. Seek/playhead
  interfaces also contain `f32` seconds. At 10/30 s, waveform X rounding is
  too small to explain the reproduced 5/15-frame slope.
- Grid offsets already use whole loaded frames. Continuous musical boundaries
  and snapped physical markers can differ by at most half a frame. Fractional
  musical periods are valid: at 44.1 kHz, a 120-BPM 1/64-note subdivision is
  1,378.125 frames. Round each absolute physical boundary once, never repeatedly
  add an independently rounded interval.
- Live source playback wraps an integer physical loop length. Arithmetic
  probes show a separate cumulative phase risk when this rounded length differs
  from the intended musical period. Preserved fractional source-rate epochs do
  not remove that discrepancy. No rendered-DSP or acoustic onset measurement
  was collected for this risk; it is not the cause of the static screenshot.

The existing source/output-clock separation, actual-hop correction, activity
anchor and resampler-tail repair remain justified. These findings do not
support reverting them or blaming the source audio.

### G1 coordinate correction

The focused `preserve-scalar-source-precision` change now shares the pure Rust
scalar projection across editor lines, snap and automatic endpoints, and preserves
binary64 waveform/seek/playhead/duration addressing. Untouched BPM edits preserve
the full effective value; intentional fractional BPM remains supported. See
[scalar source coordinates](scalar-source-coordinates.md) for the API and remaining
native binary32 BPM/rate boundary.

A separate actual-WAV public API comparison at 599.5 seconds used 16 consecutive
48-kHz frames. Before G1, 11 of 16 one-frame queries were empty, and the combined
window returned 14 points with five distinct binary32 X values. After G1, every
one-frame query returned exactly its addressed frame and the combined window
returned exactly 16 points with 16 distinct binary64 X values. This probe used a
muted isolated engine without playback or analysis; different query bounds from
the earlier arithmetic probe explain its different baseline distinct-X count.
Generated sparse tests additionally cover 44.1/48/96 kHz at 600/1800 seconds,
seek/telemetry payloads and source clamps without large dense fixtures.

Independent manual-120 grid/snap/auto tests cover all 1200 pulse positions
`n*24000`, plus fractional 119.999/123.45, subdivisions and signed/off-grid origins.
These are coordinate proofs. The automatic estimator has not changed, so the
120.00128936767578-BPM scalar slope remains G2; native accepted period/revision and
musical-versus-physical live loop behavior remain G3. No audible SYNC is certified.

## Correction direction and evidence limits

The selected future automatic analyzer remains **Beat This 1.1.0, final0,
minimal**, after acceptance. There will be no QM-versus-neural Settings selector.
Use a verified GPU or CPU with the same model; retain manual/TAP authority and
saved legacy results. GPU acceleration does not itself improve detector timing
resolution or repair downstream timing arithmetic.

On this full native mono export, the existing CPU FP32 worker completed in
39.0207 seconds and returned 1,200 beats exactly at `0, 0.5, ... 599.5` seconds,
yielding exactly 120 BPM. It also marked every pulse as a downbeat. The
unaccented fixture provides no independent bar-phase truth. Its 0.5-second
spacing aligns with Beat This's 20-ms output lattice; this result does not prove
sample-accurate detection on general music or complete musical acceptance.

Tempo derivation needs supported region/count selection, a robust fit using
multiple positions, distant-window validation and explicit uncertainty.
Cropping the old endpoint estimator to the middle 20-80% worsens this fixture
to about 120.00314085 BPM. A diagnostic all-position linear fit to published QM
times gives 119.99999727 BPM, about 0.655 frame of slope error over 600 seconds.
This is evidence for a better estimator, not a validated production policy.
Do not replace the independently confirmed origin with a fitted intercept.
Integer 120 is supported by this reference's exact sample intervals; proximity
to an integer alone is insufficient to round real fractional tempos.

The pending timing correction must give editor, snap, automatic loop duration,
native pad timing, MIDI runtime metadata and playback one authoritative accepted
timing revision. Keep raw predictions separate from accepted constant periods
or variable maps, preserving source identity, units, count and edit provenance.
Use exact frame addressing or `f64` seconds through long-file interfaces and
validate fractional musical loop periods separately before further live SYNC
changes. The [migration design](beatmap-sync-design.md) and existing OpenSpec
acceptance gates remain the implementation boundary.

The separate Windows stem investigation found `torch 2.12.0+cpu`,
`torch.version.cuda == None` and `USE_CUDA=0`; the actual backend exposes CPU
only despite the driver recognizing an RTX 5090 Laptop GPU. The current lock's
CUDA dependencies are Linux-only. This explains the tested Windows environment,
not the exact uninspected Linux installation reported by another user. No
package, driver or lock change was performed. A durable compatible CUDA profile
still needs an actual GPU operation and short Demucs separation test.

For the later Rust runtime, evaluate the existing Rust `ort`/ONNX background
boundary for model inference with CPU and tested CUDA providers. Export,
frontend, postprocessing and separator parity require model-specific proof.
PyTorch-free shipping inference is the target, not current behavior; this does
not initiate the full application Rust port. See the existing
[setup guide](stem-generation-setup.md) and
[ONNX Runtime provider documentation](https://onnxruntime.ai/docs/execution-providers/).

## Validation scope

The investigation checked all reference source frames, complete native mono
parity, the actual unchanged automatic analysis, real project grid formulas,
estimator comparisons, long-position coordinate arithmetic and a full CPU Beat
This run. Existing focused waveform-grid, transport-loop, BPM and sidebar tests
passed (168 tests). These checks establish the diagnosis, not a repaired runtime,
general model acceptance or audible synchronization. Raw audio and local probe
artifacts remain private and are not included in this repository documentation.
