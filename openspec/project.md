# Project Context

## Purpose
A high-performance audio looper application that enables real-time sample triggering and manipulation with zero-latency audio processing, leveraging Rust for real-time DSP and Python for UI and control logic.

## Tech Stack
- Rust (for real-time audio DSP)
- PyO3 (for Python-Rust FFI)
- CPAL (audio backend)
- rtrb (real-time ring buffer)
- Python (for UI and control logic)
- Maturin (build system)
- Dear PyGui (GUI)
- Ruff (code linting)
- Mypy (type checking)
- Pytest (testing)

## Project Conventions

### Code Style
- Rust: Follows standard Rust formatting with `rustfmt`
- Python: Uses Ruff with `line-length = 100` and Google-style docstrings
- Indentation: 4 spaces (`.editorconfig`)
- Naming: Rust uses snake_case for functions/variables, CamelCase for types; Python follows PEP8

### Architecture Patterns
- Hybrid Rust/Python architecture: Rust handles real-time audio DSP in a dedicated thread; Python manages UI, I/O, and control
- Lock-free message passing: Uses `rtrb` ring buffer for zero-allocation communication between Python and audio threads
- GIL avoidance: Audio thread runs independently of Python’s GIL using native OS threads
- Sample pre-loading: Audio files loaded on Python thread; only sample IDs and pointers are sent to audio thread
- Immutable audio state: Audio engine state (volume, samples) updated via atomic or shared references to avoid locks
- Single Producer, Single Consumer (SPSC): Ring buffer designed for one producer (Python) and one consumer (audio thread)

### Testing Strategy
- Unit tests: Rust tests in `rust/src/lib.rs` using `#[cfg(test)]`
- Integration tests: Python tests in `src/tests/test_all.py` using `pytest`
- Type checking: `mypy` enforces type safety in Python
- Linting: `ruff` enforces style and catches bugs in both Python and Rust via PyO3 bindings
- Real-time safety: No runtime allocations or blocking operations in audio thread (verified via code review)

### Git Workflow
- Main branch: `main` (protected)
- Feature branches: `feature/short-description` (e.g., `feature/add-velocity-control`)
- Commit messages: Conventional Commits format (`feat:`, `fix:`, `docs:`, etc.)
- Pull requests: Required for all changes into `main`
- CI: Automated build and test on PR via GitHub Actions (`CI.yml`)

## Domain Context
- Real-time audio processing requires zero-latency, no heap allocations, and no blocking operations in the audio thread
- Python and Rust communicate via a lock-free ring buffer (`rtrb`) using Rust enums as the message protocol
- Audio samples are pre-loaded as `Arc<Vec<f32>>` on the Python thread and referenced by ID in the audio thread
- The GIL must never be held during audio callback execution
- CPAL is used as the cross-platform audio backend
- All file I/O and heavy processing must occur on the Python thread, never the audio thread

## Important Constraints
- Audio thread must never allocate memory, block, or call into Python
- All audio messages must be serializable as Rust enums with no dynamic allocation
- Sample loading must occur on the Python thread only
- Must support Linux audio backend via CPAL
- Build system must use Maturin for PyO3 compatibility
- No external dependencies beyond Rust crate ecosystem and Python stdlib

## External Dependencies
- CPAL: Cross-platform audio library (Rust crate)
- PyO3: Python-Rust FFI (Rust crate)
- rtrb: Real-time ring buffer (Rust crate)
- Maturin: Build system for PyO3 projects
- WAV decoder: Custom or external (e.g., `wav` crate)
- Python 3.14+: Modern type hints
- Dear PyGui
