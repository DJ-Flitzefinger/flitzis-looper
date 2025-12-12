# Technology Stack Analysis

## Project Overview
- **Project Type:** Desktop Application (Audio Processing)
- **Primary Language:** Python
- **Architecture Pattern:** Component-based desktop application

## Technology Stack Table

| Category | Technology | Version | Justification |
|----------|------------|---------|---------------|
| **Core Language** | Python | >=3.13.9 | Specified in pyproject.toml |
| **Audio Processing** | demucs | >=4.0.1 | Music source separation |
| **Audio Analysis** | madmom | - | Audio and music signal processing |
| **Visualization** | matplotlib | >=3.10.7 | Audio waveform visualization |
| **Numerical Computing** | numpy | >=2.3.5 | Audio signal processing |
| **Audio Effects** | pedalboard | >=0.9.19 | Audio effects processing |
| **Audio Synthesis** | pyo | - | Digital signal processing |
| **Audio I/O** | soundfile | >=0.13.1 | Audio file reading/writing |
| **Machine Learning** | torch | >=2.9.1 | Deep learning for audio processing |
| **Audio Utilities** | torchaudio | >=2.9.1 | Audio-specific torch utilities |

## Architecture Pattern
- **Component-based desktop application** with clear separation between:
  - Audio processing modules (`audio/`)
  - Core application logic (`core/`)
  - User interface components (`ui/`)
  - Utility functions (`utils/`)

## Key File Patterns Detected
- `pyproject.toml` - Python project configuration
- `flitzis_looper/__main__.py` - Application entry point
- Audio processing modules in `audio/` directory
- Core application logic in `core/` directory
- UI components in `ui/` directory

## Development Environment
- **Dependency Management:** UV (based on uv.lock)
- **Code Quality:** mypy, ruff (dev dependencies)

## Framework Characteristics
- **Desktop Application Framework:** Custom Python-based framework
- **Audio Processing Pipeline:** Modular design with separate components for different audio operations
- **UI Architecture:** Component-based with widgets and dialogs
