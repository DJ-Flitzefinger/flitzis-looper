# Project Context

## Purpose
Dj Flitzefinger's Scratch-Looper is a professional DJ looping application for live performance and music production. It provides a grid-based interface for triggering audio loops with advanced features like stem separation, BPM control, pitch adjustment, and real-time mixing. The application is designed for DJs, producers, and live performers who need a powerful audio looping tool with sophisticated audio processing capabilities.

## Tech Stack
- Python >=3.13.9
- demucs >=4.0.1 (music source separation)
- madmom (audio and music signal processing)
- matplotlib >=3.10.7 (audio waveform visualization)
- numpy >=2.3.5 (numerical computing)
- pedalboard >=0.9.19 (audio effects processing)
- pyo (digital signal processing)
- setuptools >=80.9.0
- soundfile >=0.13.1 (audio file I/O)
- torch >=2.9.1 (machine learning)
- torchaudio >=2.9.1 (audio utilities)
- tkinter (GUI framework)

## Project Conventions

### Code Style
- Line length: 100 characters maximum
- Use ruff formatter for consistent code style
- All Python files should conform to PEP 8 standards
- Use 4 spaces for indentation (no tabs)
- Use absolute imports when possible
- Group imports in standard order: standard library, third-party, local
- Use explicit imports (avoid `import *`)
- Imports must only appear at the top of a file
- Use type hints for function parameters and return values
- Follow Google Python style guide for docstrings
- Variables: snake_case
- Functions: snake_case
- Classes: PascalCase
- Constants: UPPER_SNAKE_CASE
- Private members: prefixed with underscore (_private)

### Architecture Patterns
- Component-based desktop application with clear separation between:
  - Audio processing modules (`audio/`)
  - Core application logic (`core/`)
  - User interface components (`ui/`)
  - Utility functions (`utils/`)
- Centralized state management in `core/state.py`
- Event-driven architecture with reactive updates
- Modular design with separate components for different audio operations
- The project uses tkinter for GUI development
- Audio processing is handled with pyo, pedalboard, and soundfile
- Configuration is stored in JSON format

### Testing Strategy
- Type checking with mypy
- Code style checking with ruff
- Manual testing of audio features
- Integration testing of UI and core logic interaction
- Focus on audio processing edge cases, UI component interactions, state management scenarios, and error handling

### Git Workflow
- Free-form commits format
- Keep commits focused and atomic
- Create feature branches for new functionality
- Submit pull requests for review
- Run all checks locally before submitting PRs

## Domain Context
The application is designed for professional audio use cases including:
- Live DJ performance with real-time loop triggering
- Music production with stem separation capabilities
- Audio analysis with BPM detection
- Real-time audio mixing and effects processing
- Professional audio looping workflows

Key domain concepts include:
- Audio loops organized in a 9x9 grid
- Stem separation (vocals, drums, bass, other)
- BPM (beats per minute) synchronization
- Pitch and speed control
- Multi-bank system for organizing loops
- Real-time stem mixing with individual volume and EQ control
- Master volume and effects processing

## Important Constraints
- Python version requirement: >=3.13.9
- System dependency on liblo for OSC support
- Audio processing requires real-time performance considerations
- Cross-platform compatibility (Linux, Windows, macOS)
- Memory-efficient handling of audio buffers
- Low-latency audio processing requirements

## External Dependencies
- demucs for music source separation
- madmom for audio signal processing
- matplotlib for waveform visualization
- numpy for numerical computations
- pedalboard for audio effects
- pyo for digital signal processing
- torch and torchaudio for machine learning-based audio processing
- soundfile for audio file I/O
- liblo system library for OSC support