# Key analysis

Key detection classifies the complete source into a musical label. It does not
transpose playback, enable Key Lock or choose BPM. Performer correction and pitch
intent are described in [pitch and Key Lock](pitch-and-key-lock.md).

## Owners and complete signal path

The productive caller is
[app_analysis.rs](../../rust/crates/looper/src/audio_engine/app_analysis.rs).
The analysis crate's [key_detection/mod.rs](../../rust/crates/analysis/src/key_detection/mod.rs)
exposes the dense scientific API and complete file adapter over the same DSP
kernel. [cqt.rs](../../rust/crates/analysis/src/key_detection/cqt.rs) prepares model
features; [inference.rs](../../rust/crates/analysis/src/key_detection/inference.rs)
owns serialized ONNX inference; [mapping.rs](../../rust/crates/analysis/src/key_detection/mapping.rs)
maps the 24 classes to key names.

```text
complete verified source mono
  -> complete 44.1 kHz f32le owned file
  -> centered librosa-compatible CQT, 105 bins
  -> magnitude, log1p, omit highest-frequency row
  -> 1 x 1 x 104 x T f32 model input
  -> 24 logits, argmax class and softmax confidence -> key name
```

The pinned frontend uses 24 bins/octave, fmin 65 Hz and hop 8820 frames (200 ms).
Magnitude-only basis/STFT projection matches the established `phase=False`
contract. The model's variable time dimension preserves all source frames;
there is no excerpt, crop or long-track quality reduction. Silence/insufficient
input can complete as unknown; the minimum useful input is 8820 frames.

The exported `keynet.onnx` preserves the trained 104-row input and 24-class
output. Model search is executable directory, cwd, assets/models, models, assets;
the first applicable resource matters. The cached serialized ORT session avoids
loading weights per call. Internal confidence describes a classifier result,
not musical correctness. `KeyResult` exposes key name/confidence;
the productive loader publishes key/status/error rather than a logits DTO.

## Complete file frontend and memory

The current frontend uses job-owned anonymous files for mono input and successive
octave signals. [librosa_cqt/signal.rs](../../rust/crates/analysis/src/key_detection/librosa_cqt/signal.rs)
provides common memory/file range reads;
[librosa_cqt/cqt.rs](../../rust/crates/analysis/src/key_detection/librosa_cqt/cqt.rs)
and [stft.rs](../../rust/crates/analysis/src/key_detection/librosa_cqt/stft.rs)
share transform mathematics. Bounded file reads replace simultaneous whole-track
octave vectors. CQT response/frontend matrices still scale with source duration.

Initial source conversion keeps the pinned Rubato FFT delay trimming and complete
tail. Octave downsampling keeps the pinned Sinc full/partial/flush/prefix/zero-tail
semantics. Rubato here is resampling, not a phase-vocoder pitch backend. Dense and
file adapters retain one shared numerical path, checked finite data/extent and
cancellation. Anonymous scratch lifetimes remain job-owned outside realtime.

For N converted frames, the default file-CQT working estimate is
`(ceil(N / 8820) + 1) * 105 * 52 + 64 MiB + 64 KiB`, with checked arithmetic.
The productive admission also counts retained caller PCM, neural response/wire,
bounded I/O and converter/FFT/Sinc peaks. It does not claim that dropping one
snapshot frees another holder. The preparation ceiling remains 512 MiB; cached
ORT/model arenas, neural activations elsewhere and reclaimable OS file cache are
outside this accounting. It is not a process-tree RSS guarantee. See
[BPM admission](bpm-analysis.md#complete-source-preparation-and-admission).

## Independent outcomes and publication

| Outcome | Meaning |
| --- | --- |
| `Ready` | Classification completed, including a legitimate unknown for silent/insufficient input. |
| `NotRequested` | Tempo-only work did not request Key. |
| `SkippedBudget` | Optional Key preparation was not admitted; BPM admission is independent. |
| `Failed` | Model/frontend/execution failed; it is not relabelled a successful unknown. |

[key_metadata.py](../../src/flitzis_looper/controller/key_metadata.py) captures
content instance, source path/generation/digest/frames/rate and independent
analysis/correction epochs at real request admission. A job result cannot update
a replacement occupant or erase a correction made after that admission. Matching
successful detection can replace the earlier detection and its preexisting
correction; a skipped/failed reanalysis preserves them. A failed BPM component
can still merge a matching completed Key without clearing valid BPM. Initial
import publishes only through the matching own-source loader success.

Raw source Key/version, manual source correction, transposition and display
projection remain separate. Unknown classification is not a fabricated tonic.
Load/cancel/stale/error and source retirement use the common
[media ownership](media-and-pcm.md#publication-cancellation-and-retirement) and
[state polling](state-input-and-ui.md#polling-feedback-and-errors) rules.

## Evidence and limits

The 2026-10-30 complete-file repair retained dense/file and independent Sinc/CQT
parity, exact admission boundaries/overflow, full-tail/nonfinite/cancellation and
source-bound metadata checks. The authorized full CHASER/control import checks
returned Cm and Am with matching OwnACK. These are actual model outputs, not
musical ground truth, listening, device or MIDI acceptance; no inference was
repeated for this documentation.

[Historical Key evaluation](../key-detection.md) retains reported 604-track scores
and agreement. Its proposed causal explanation of a Python/Rust gap was not
proved; it is not a fresh evaluation of the complete-file implementation.
The [canonical E-KEY/E-MEMORY evidence](../../openspec/changes/archive/2026-10-30-evidence-index.md)
preserves original identities, old memory failures and superseded formulas.
Current [analysis](../../openspec/specs/audio-analysis/spec.md) and
[classification](../../openspec/specs/musical-key-cnn/spec.md) contracts apply.
