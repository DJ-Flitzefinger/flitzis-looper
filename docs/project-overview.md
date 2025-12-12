# Project Overview

## Flitzis Looper - Audio Processing Desktop Application

**Version:** 7.0
**Type:** Monolithic Python Desktop Application
**Primary Language:** Python
**Architecture:** Component-Based Desktop Application

## Executive Summary

Flitzis Looper is a sophisticated audio processing desktop application designed for musicians, producers, and audio engineers. The application provides comprehensive tools for audio looping, beat detection, pitch manipulation, and stem separation.

## Tech Stack Summary

| Category | Technologies |
|----------|-------------|
| **Core Language** | Python 3.13.9+ |
| **Audio Processing** | demucs, madmom, pedalboard, pyo |
| **Machine Learning** | PyTorch, torchaudio |
| **Numerical Computing** | NumPy |
| **Visualization** | Matplotlib |
| **Code Quality** | mypy, ruff |

## Architecture Type Classification

**Monolithic Desktop Application** with the following characteristics:

- Single cohesive codebase
- Component-based architecture
- Real-time audio processing
- Modular design with clear separation of concerns

## Repository Structure

**Monolith** - All components contained within a single repository:

- **Audio Processing:** `flitzis_looper/audio/`
- **Core Logic:** `flitzis_looper/core/`
- **User Interface:** `flitzis_looper/ui/`
- **Utilities:** `flitzis_looper/utils/`

## Quick Reference

### Tech Stack
- **Primary Language:** Python
- **Audio Framework:** Custom audio processing pipeline
- **UI Framework:** Custom Python-based UI components

### Entry Point
- **Main Entry:** `flitzis_looper/__main__.py`
- **Core Application:** `flitzis_looper/core/app.py`
- **Execution:** `python -m flitzis_looper`

### Architecture Pattern
- **Component-Based Desktop Application**
- **Event-Driven Architecture**
- **Modular Audio Processing**
- **Centralized State Management**

## Key Features

### Audio Processing Capabilities
- **BPM Detection:** Automatic tempo analysis and synchronization
- **Audio Looping:** Recording, playback, and manipulation of audio loops
- **Pitch Processing:** Pitch detection and manipulation
- **Stem Separation:** Isolation of vocals, drums, bass, and other instruments
- **Real-time Processing:** Low-latency audio operations

### User Interface Features
- **Loop Grid:** Visual arrangement and triggering of audio loops
- **Stem Separation Panel:** Control over audio source separation
- **Configuration Dialogs:** BPM, volume, and waveform visualization
- **Custom Widgets:** EQ knobs, VU meters, and other audio-specific controls

### Core Functionality
- **Bank Management:** Organization of samples and loops
- **State Management:** Centralized application state with reactive updates
- **Configuration:** Persistent settings and preferences
- **Volume Control:** Comprehensive audio volume management

## Technology Highlights

### Audio Processing Engine
- **demucs:** State-of-the-art music source separation
- **madmom:** Audio and music signal processing toolkit
- **pedalboard:** Comprehensive audio effects processing
- **pyo:** Powerful digital signal processing library
- **PyTorch:** Machine learning for advanced audio processing

### Development Toolchain
- **UV:** Fast Python package management
- **mypy:** Static type checking for code quality
- **ruff:** High-performance formatting and linting

## Project Metrics

- **Files Generated:** 6 documentation files
- **Scan Level:** Quick Scan (pattern-based analysis)
- **Project Type:** Desktop Application
- **Components:** 4 major component groups

## Links to Detailed Documentation

### Generated Documentation
- [Architecture](./architecture.md) - System architecture and design
- [Source Tree Analysis](./source-tree-analysis.md) - Detailed directory structure
- [Technology Stack](./technology-stack.md) - Complete technology analysis
- [Development Guide](./development-guide.md) - Development setup and workflow
- [Comprehensive Analysis](./comprehensive-analysis.md) - Conditional analysis results

### Existing Documentation
- [README.md](../README.md) - Basic project information

## Getting Started

### For Users
1. Install dependencies: `uv pip install -e .`
2. Run the application: `python -m flitzis_looper`
3. Explore the audio processing features

### For Developers
1. Set up development environment: See [Development Guide](./development-guide.md)
2. Understand the architecture: See [Architecture](./architecture.md)
3. Start with small features or bug fixes
4. Follow contribution guidelines

### For Maintainers
1. Review the comprehensive documentation
2. Understand the component-based architecture
3. Monitor dependency updates
4. Plan future enhancements based on architecture

## Next Steps

1. **Review Documentation:** Familiarize yourself with the generated documentation
2. **Explore Features:** Try out the audio processing capabilities
3. **Contribute:** Check for open issues or suggest new features
4. **Extend:** Consider adding plugins or new audio effects

## Conclusion

Flitzis Looper represents a well-architected desktop audio processing application with a strong foundation for extension and enhancement. The comprehensive documentation provides a solid basis for understanding, maintaining, and evolving the system.
