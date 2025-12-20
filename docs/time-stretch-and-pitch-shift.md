# Signalsmith Stretch (Time-stretch and Pitch-shift)

Here are the **meaningful knobs, concepts, and practical implications** for integrating **Signalsmith Stretch** into a **live looper with low-latency multi-stream time-stretch/pitch shift**, based on the official docs and known behavior. ([Signalsmith Audio][1])

## Core Parameters & Configuration

### 1) **Block Size & Interval**

* Stretch uses block-based spectral processing (STFT-style) under the hood.
* **`blockSamples`**: FFT block length = trade-off between time vs frequency resolution. Larger blocks → better tonal quality at the cost of **latency** and CPU cost.
* **`intervalSamples`**: hop/overlap length between blocks; smaller intervals → smoother parameter changes and lower internal buffering but more CPU.
* You can call `stretch.configure(channels, blockSamples, intervalSamples)` for custom tuning. ([GitHub][2])

**Implications**

* Small blocks → *lower algorithmic latency* but more spectral leakage / poorer pitch accuracy.
* Large blocks → *better quality* for large stretches but added latency.

## Time-Stretch / Pitch Controls

### 2) **Time Stretch Ratio**

* Defined implicitly by **input vs output buffer lengths** in `process()`.
* If `outputSamples > inputSamples` → slower (stretch); if smaller → faster.
* There is **no separate ratio parameter** you set; you choose what you feed. ([GitHub][2])

**Design**

* You’ll accumulate buffers of different lengths when looping; maintain stable average for consistency.

### 3) **Pitch Shift / Transpose**

* **`setTransposeFactor()`** or **`setTransposeSemitones()`** alters pitch independent of duration.
* Optional **“tonality limit”** controls up to what frequency the frequency map is linear (helps preserve timbre). ([GitHub][2])

**Knob Logic**

* Apply pitch automation ahead of time (see latency below).
* Tonality limit protects lower harmonics from aliasing/artifacts.

### 4) **Formant Control**

* **Formant factor / base** adjusts formants relative to pitch shift for more natural sound.
* Without formant correction, pitch changes often make voices/instruments unnatural (chipmunk effect).
* This is **not perfect** (broader approximation vs e.g. PSOLA). ([GitHub][2])

**Live Tweak**

* Expose a formant factor parameter if voices are key; heavier CPU.

## Latency, Alignment & Sync

### 5) **Input/Output Latency**

* Stretch reports two latencies:

  * **`inputLatency()`** = samples of input *ahead of intended process time* that the algorithm needs before it can commit to accurate spectral content.
  * **`outputLatency()`** = samples of output *behind the actual processing time*.
* You need to feed automation:

  * pitch/time changes with **values from `outputLatency()` samples ahead**, and
  * input audio from **`inputLatency()` ahead**, to stay in sync. ([Signalsmith Audio][1])

**High-Level**

* This latency is inherent to FFT overlap and block buffering.
* Typical STFT based stretch latencies with moderate blocks fall in the **tens of milliseconds** — larger blocks = larger latency.
* Exact numbers depend on your block/interval config. Signalsmith doesn’t publish fixed values.

### 6) **Latency Compensation / Sync Strategy**

To keep multiple streams in sync:

* **Buffer alignment offset**
  Track and subtract `outputLatency()` from presentation time for the stream.
* **Shared transport clock**
  Drive all Stretch instances from a central clock; feed input ahead, advance play position by measured latency.
* **Latency buffer trimming / padding**
  Use adaptive delay lines: pad early buffers, trim later outputs to align with other streams.

Live loopers commonly do:

```
desired_output_time = playhead + outputLatency
fetch input ahead by inputLatency + margin
apply pitch/stretch
mix at global time
```

## Concepts: Technical Why It Matters

### 7) **Block Samples (STFT Processing)**

Block samples = FFT length.
In spectral stretch: audio is windowed into overlapping blocks and transformed to frequency domain. Bigger blocks → sharper frequency info but worse time precision. Smaller blocks → better timing but more spectral smearing.

Trade-off for live: **minimize block size** while retaining acceptable quality.

### 8) **Formant**

Formants are resonant peaks that define perceived vowel/timbre. Changing pitch without moving formants makes voices unnatural.
Stretch’s **formant compensation** shifts spectral envelopes relative to pitch so timbre stays recognizable at pitch changes. ([GitHub][2])

### 9) **Denormals**

Not explicitly mentioned in the Stretch docs, but relevant: FFT/dsp can produce **denormal floating numbers** near zero that dramatically slow CPU on some CPUs.
Mitigation:

* use **flush-to-zero / denormals-are-zero flags**,
* add tiny noise floor,
* use SIMD which often flush denormals.

Without mitigation, audio threads might occasionally spike CPU.

## CPU & Real-Time Viability

**Feasibility**

* feasible for **5–20 stereo streams** on modern desktop with tuned block/interval and multithreading.
* CPU cost scales with block size and stretch factor magnitude.

**Guidelines**

* Use **split computation** (`splitComputation` flag) to smooth out heavy FFT bursts across frames — reduces jitter in audio callback. ([GitHub][2])
* Use worker thread(s) and ring buffers per stream so that strict audio callback does minimal work.
* Avoid adjusting heavy knobs (formant, large FFT) mid-callback.

## Live Adjustment & Automations

* Because stretch uses blocks, changes take effect at **block boundaries**; parameter smoothing is crucial to avoid artifacts.
* Feed automation with lead time = `outputLatency()` samples.
* Interpolate parameters over multiple blocks for smooth transitions.

## Summary of What You’ll Expose in Your App

| Parameter                      | Purpose              | Impact                       |
| ------------------------------ | -------------------- | ---------------------------- |
| Stretch ratio                  | speed vs length      | Latency + spectral integrity |
| Transpose (semitones / factor) | pitch change         | spectral mapping quality     |
| Tonality limit                 | quality vs artifacts | timbre fidelity              |
| Formant factor                 | natural pitch-shift  | CPU + complexity             |
| blockSamples / intervalSamples | performance tuning   | latency vs quality           |
| splitComputation               | CPU jitter smoothing | real-time stability          |

## Risks / Uncertainties

* Latency quantization vs other DSP (EQ, loo playback) must be measured and compensated per stream.
* Worst-case cost under heavy pitch/formant changes might violate tight callbacks unless offloaded.
* Quality trade-offs not linear; small block can sound bad even if low latency.

[1]: https://signalsmith-audio.co.uk/code/stretch/
[2]: https://github.com/Signalsmith-Audio/signalsmith-stretch
