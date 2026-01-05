## Context
`ProjectState` (`src/flitzis_looper/models.py`) is a Pydantic model with `validate_assignment=True` and already includes fields that are explicitly described as persistent state (e.g., `sample_paths`, `sample_analysis`, pad gain/EQ, and global settings).

The current app entrypoint (`src/flitzis_looper/ui/__init__.py`) constructs `LooperController()` directly, which currently always starts with a fresh `ProjectState()`.

The Rust audio engine dynamically chooses an `output_sample_rate` from the active audio device configuration. The Rust sample loader already decodes and resamples arbitrary formats to the engine’s output sample rate.

## Goals / Non-Goals
- Goals:
  - Restore `ProjectState` on startup from `./samples/flitzis_looper.config.json`.
  - Persist changes to `ProjectState` automatically with a debounce (≤ 1 write per 10 seconds).
  - Make a session portable by copying all user-loaded audio into `./samples/` as WAV and referencing those files in `ProjectState`.
  - Avoid crashes on startup when expected sample files are missing or unusable.
- Non-Goals:
  - Persisting `SessionState` (runtime/UI ephemeral state).
  - Implementing waveform editor loop-point persistence (that is TODO #8/#9).
  - Supporting multi-project workspaces or explicit “save as/open project” UI (this is a later feature).

## Decisions
### Decision: Project directory layout
- The project root is the current working directory (CWD).
- The project assets live under `./samples/`.
- The project config file path is `./samples/flitzis_looper.config.json`.

Rationale: Matches the user request and keeps “portable session” as a single folder.

### Decision: Path representation in `ProjectState`
- `ProjectState.sample_paths[*]` should point to project-local paths under `./samples/` (portable), rather than external absolute paths.
- Store paths as relative strings where possible (e.g., `samples/foo.wav`) to keep JSON portable across machines.

Rationale: A user can copy the entire `samples/` folder and keep working.

### Decision: WAV cache sample rate
- Cached WAV files are encoded at the engine output sample rate *at the time of load*.
- On restore, the app validates cached WAV sample rate against the current engine output sample rate.
- If the cached WAV sample rate mismatches, the pad is ignored (and no crash occurs).

Rationale: The engine output sample rate can change across devices/systems; ignoring mismatches matches the request.

### Decision: Debounced persistence mechanism
- Use a lightweight “dirty flag + last write timestamp” approach:
  - Mark dirty on any `ProjectState` mutation.
  - Periodically flush to disk (e.g., from the UI frame loop) when dirty and at least 10 seconds since last flush.
  - Also attempt a best-effort flush during app shutdown.

Rationale: Avoids background threads and keeps I/O off the audio thread. The UI loop already runs continuously.

### Decision: Name collisions in `./samples/`
- Primary intent is to keep the original basename.
- If a file with the target basename already exists but is not the same content, choose a deterministic disambiguation strategy (e.g., append a short hash suffix).

Rationale: “Original name” conflicts with real-world collisions; deterministic naming avoids silent overwrites.

## Risks / Trade-offs
- Writing to disk from the UI loop can stutter if the JSON gets large; mitigations include debouncing, atomic writes, and keeping the JSON small.
- Cached WAVs at output sample rate make portability across devices with different sample rates less seamless; the request explicitly allows ignoring mismatches.
- Filename collision policy needs to be predictable to avoid confusing the user.

## Migration Plan
- If `flitzis_looper.config.json` does not exist, start with defaults.
- If config exists but cannot be parsed/validated, start with defaults and keep the UI usable.
- On first successful save, the config file becomes the source of truth.

## Resolved Questions
- Collision policy: deterministic rename (suffix with a stable identifier) to avoid overwriting.
- Sample rate mismatch handling: ignore the sample on restore.
- Cached WAV encoding: encode using the same sample format as the engine’s in-memory sample buffers (currently `f32`), to minimize conversion during fast load.
- Unload cleanup: when a pad is unloaded, delete its corresponding cached file under `./samples/` if present; missing files are ignored.
