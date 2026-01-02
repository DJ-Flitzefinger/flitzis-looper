## Context
- The app’s control plane is Python UI + controller state; the audio plane is a Rust real-time mixer.
- Python and the audio thread communicate via a fixed-capacity SPSC ring buffer (`rtrb`) carrying `ControlMessage` enums.
- A global speed control already exists end-to-end (UI → Python → `ControlMessage::SetSpeed` → mixer stores a value) but **the mixer does not apply speed to playback yet**.
- The UI already exposes `KEY LOCK` and `BPM LOCK` toggles, but these only update Python project state and do not affect audio.
- Per-pad BPM/key metadata exists (auto analysis + manual overrides), but is currently used for display/state only.

## Goals / Non-Goals
- Goals:
  - Make the global speed/tempo slider change audible playback in real-time for all currently playing voices.
  - Support BPM lock and Key lock behaviors while keeping the audio callback real-time safe.
  - Use a high-quality real-time time-stretch/pitch-shift implementation (Signalsmith Stretch via `signalsmith_dsp`).
  - Keep the implementation robust under frequent slider drags (best-effort updates, smoothing, bounded CPU spikes).
- Non-Goals:
  - Beat-phase quantized start/stop or cross-pad phase alignment.
  - Exposing advanced Signalsmith parameters (formants, tonality limit, block size) to the UI.
  - Implementing a separate master BPM entry UI (anchor-to-current-pad is sufficient for now).

## Proposed Architecture (recommended)
### Audio-thread state
Add a tempo/key state struct held by the mixer:
- `global_speed: f32` (existing)
- `bpm_lock_enabled: bool`
- `key_lock_enabled: bool`
- `master_bpm: Option<f32>` (set when BPM lock is enabled)
- Per-pad metadata arrays (NUM_SAMPLES):
  - `pad_bpm: [Option<f32>; NUM_SAMPLES]`

### Message protocol
Extend `ControlMessage` to inform the audio thread about:
- lock mode changes (`SetBpmLock(bool)`, `SetKeyLock(bool)`)
- master BPM updates (`SetMasterBpm(f32)`)
- per-pad BPM metadata updates (`SetPadBpm{id,bpm}`)

The control-plane computes **when** to set the master BPM (on BPM lock enable, anchored to the selected pad) and sends those values; the audio thread computes per-voice parameters every render based on stored metadata.

### Per-voice DSP
Each active `Voice` owns (or is paired with) a Signalsmith Stretch processor instance configured at voice creation. The processor produces the voice’s audio stream at the current tempo ratio and transpose, which is then mixed into the output buffer.

Key constraints:
- No heap allocation in the audio callback: all DSP instances and scratch buffers must be preallocated or drawn from fixed-capacity pools.
- Parameter updates are smoothed and only applied at safe boundaries (block boundaries), using Signalsmith’s recommended lead-time behavior.

## Tempo + Pitch Model
Define:
- `speed` = global speed slider value (0.5..2.0)
- `effective_bpm(pad)` comes from manual BPM override, else analysis BPM.
### Tempo ratio per pad
- If BPM lock is OFF:
  - `tempo_ratio = speed` (all pads speed up/slow down equally)
- If BPM lock is ON and `master_bpm` is set:
  - The system treats `master_bpm` as the single global BPM.
  - When the speed slider changes, the control-plane updates `master_bpm` as:
    - `master_bpm = selected_pad_bpm * speed`
  - For a pad with known BPM: `tempo_ratio = master_bpm / pad_bpm`
  - If pad BPM unknown: fall back to `tempo_ratio = speed`

This achieves “tempo alignment” by BPM (same beat duration), but does not guarantee beat-phase alignment.

### Transpose per pad
- If Key lock is OFF:
  - `transpose_semitones = 0.0`
- If Key lock is ON:
  - Compensate tempo-induced pitch change so tempo changes don’t change perceived pitch:
    - tempo pitch shift factor (varispeed) is approximately `tempo_ratio`
    - compensation in semitones: `-12 * log2(tempo_ratio)`
    - `transpose_semitones = compensation`

If BPM lock is OFF but Key lock is ON, `tempo_ratio` is still derived from `speed`, so key lock still prevents tempo from affecting pitch.

## Signalsmith Stretch Integration Notes
Based on `docs/time-stretch-and-pitch-shift.md`:
- Stretch ratio is controlled implicitly by input vs output lengths in `process()`; for playback speed `tempo_ratio`, the stretch ratio is approximately `1 / tempo_ratio`.
- Parameter changes should be fed with lead time (≈ `outputLatency()` samples) and smoothed to avoid artifacts.
- Choose an initial configuration that balances quality and latency (e.g., block 1024, interval 256), with a path to future tuning.
- Use `splitComputation` if supported by `signalsmith_dsp` to reduce worst-case CPU spikes.

## Alternatives Considered
1) Naive resampling (linear interpolation) in the callback
- Pros: very simple, very low CPU
- Cons: poor quality, no key lock, artifacts on large ratios
- Status: rejected (does not meet “high-quality” requirement)

2) Signalsmith Stretch directly in the audio callback (recommended starting point)
- Pros: simplest high-quality solution, minimal threading complexity
- Cons: CPU cost scales with voices; must be carefully configured to avoid glitches
- Mitigations: fixed limits (MAX_VOICES), conservative block/interval, `splitComputation`, no allocations, parameter smoothing

3) Worker-threaded stretch processing (future optimization path)
- Pros: isolates heavy FFT work from the CPAL callback; reduces jitter
- Cons: significantly more complex (per-voice ring buffers, scheduling, backpressure)
- Status: not required initially, but design keeps the door open (voice processors could be moved behind a producer/consumer boundary)

## Risks / Trade-offs
- Added algorithmic latency from STFT buffering.
  - Mitigation: keep block size moderate; accept a small, consistent output delay.
- CPU spikes under many active voices + aggressive parameter changes.
  - Mitigation: `splitComputation`, hard voice cap, parameter smoothing, and (if needed) future worker threads.
- Pitch stability under rapid tempo changes.
  - Mitigation: parameter smoothing and bounded rate-of-change on tempo_ratio.

## Migration Plan
- No persisted data migrations required.
- Runtime behavior changes:
  - Global speed becomes audible.
  - BPM lock / Key lock toggles begin affecting playback.
- Backward compatibility:
  - Existing projects without BPM/key metadata continue to play; lock modes degrade gracefully.
