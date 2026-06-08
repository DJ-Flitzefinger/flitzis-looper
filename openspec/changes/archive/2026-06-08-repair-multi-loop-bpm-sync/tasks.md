## 1. Reproduction And Grid Stability

- [x] 1.1 Add deterministic Rust reproduction for BPM-locked Multi Loop drift across first pass and repeated wraps.
- [x] 1.2 Cover speed `1.0`, `1.25`, `1.5`, and `2.0`, fixed and variable callback segments, Key Lock off/on, same-BPM controls, and different-BPM matched pads.
- [x] 1.3 Cover retrigger as a phase reset in the reproduction.
- [x] 1.4 Add Python controller tests for Loop Editor Grid Offset and snapped loop-start stability under playback sync changes.

## 2. Specification And Architecture

- [x] 2.1 Create this focused OpenSpec change.
- [x] 2.2 Validate `repair-multi-loop-bpm-sync` with official strict OpenSpec validation.
- [x] 2.3 Select the implementation strategy after comparing fractional accumulation, output-frame anchored source addressing, boundary correction, and hybrid options.

## 3. Rust Timing Repair

- [x] 3.1 Thread absolute segment output-frame bounds into the mixer if output-frame anchored addressing is selected.
- [x] 3.2 Add only the fixed-size voice sync metadata required for BPM-locked phase-stable source addressing.
- [x] 3.3 Preserve immediate trigger, quantized trigger, stop, pause/resume, seek, retrigger, live loop edit, Key Lock, Gain/EQ, metering, and prepared-stem behavior.
- [x] 3.4 Keep missing BPM metadata on the documented global-speed fallback.
- [x] 3.5 Keep the audio callback free of disk I/O, JSON, Python/GIL access, UI work, blocking locks, logging, neural inference, plugin loading, unbounded loops, heavy allocation, and long-running work.

## 4. Regression Coverage

- [x] 4.1 Convert the ignored drift reproduction into passing regression coverage when the repair lands.
- [x] 4.2 Add focused Rust coverage for prepared stems sharing the repaired BPM-locked timing path.
- [x] 4.3 Add focused Rust coverage for missing BPM metadata fallback and retrigger reset behavior.

## 5. Validation

- [x] 5.1 Run `openspec validate repair-multi-loop-bpm-sync --strict`.
- [x] 5.2 Run `uv run cargo check --manifest-path rust/Cargo.toml`.
- [x] 5.3 Run `uv run cargo test --manifest-path rust/Cargo.toml`.
- [x] 5.4 Run focused Python controller tests for loop/global/playback stability.
- [x] 5.5 Run `uv run pytest` if Python-facing behavior changes beyond tests.
- [x] 5.6 Run `uv run cargo fmt --manifest-path rust/Cargo.toml --check`, `uv run ruff check src`, and `git diff --check` before the repair is considered complete.
