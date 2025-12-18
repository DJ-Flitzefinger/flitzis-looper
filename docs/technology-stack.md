# Technology Stack Analysis

## Project Overview
- **Project Type:** Desktop Application (Audio Processing)
- **Primary Language:** Python
- **Architecture Pattern:** Component-based desktop application

## Technology Stack Table

| Category | Technology |
|----------|------------|
| **Core Language** | Python |
| **Audio Processing** | demucs |
| **Audio Analysis** | madmom |
| **Visualization** | matplotlib |
| **Numerical Computing** | numpy |
| **Audio Effects** | pedalboard |
| **Audio Synthesis** | pyo |
| **Audio I/O** | soundfile |
| **Machine Learning** | torch |
| **Audio Utilities** | torchaudio |

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
