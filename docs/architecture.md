# System Architecture

## Executive Summary

**Flitzis Looper** is a monolithic Python desktop application for audio processing and looping. The system is designed with a component-based architecture that separates audio processing, core application logic, user interface, and utility functions.

## Technology Stack

| Category | Technology |
|----------|------------|
| **Core Language** | Python |
| **Audio Processing** | demucs, madmom, pedalboard, pyo |
| **Machine Learning** | torch, torchaudio |
| **Numerical Computing** | numpy |
| **Visualization** | matplotlib |
| **Build System** | none |
| **Code Quality** | mypy, ruff |

## Architecture Pattern

**Component-Based Desktop Application** with the following characteristics:

- **Modular Design:** Clear separation of concerns between audio processing, core logic, and UI
- **Event-Driven:** UI events trigger core application operations
- **Real-time Processing:** Audio operations performed with low latency
- **State Management:** Centralized application state with reactive updates

## Data Architecture

### Audio Data Flow
```
User Interaction → UI Components → Core Logic → Audio Processors → Audio Output
```

### State Management
- Centralized in `core/state.py`
- Reactive updates to UI components
- Persistent configuration storage

## API Design

### Internal APIs
- **Core → Audio:** Function calls with audio buffers
- **UI → Core:** Event-based communication
- **Core → UI:** State updates and notifications

### External Dependencies
- **Audio Libraries:** demucs, madmom, pedalboard, pyo
- **ML Framework:** PyTorch for audio processing

## Component Overview

### Audio Processing Components
- **BPM Detection:** Tempo analysis and synchronization
- **Loop Management:** Audio loop recording and playback
- **Pitch Processing:** Pitch detection and manipulation
- **Stem Separation:** Source separation (vocals, drums, bass, etc.)
- **Audio Server:** Real-time audio streaming and processing

### Core Application Components
- **Application Core:** Main application lifecycle and coordination
- **Bank Management:** Sample and loop organization
- **BPM Control:** Tempo synchronization logic
- **Configuration:** Settings and preferences management
- **State Management:** Centralized application state
- **Volume Control:** Audio volume management

### UI Components
- **Main Window:** Primary application interface
- **Loop Grid:** Visual representation of audio loops
- **Stems Panel:** Stem separation controls
- **Dialogs:** Configuration dialogs (BPM, Volume, Waveform)
- **Widgets:** Custom UI elements (EQ Knobs, VU Meters)
- **Toolbar:** Quick access to common functions

## Source Tree

See [Source Tree Analysis](./source-tree-analysis.md) for detailed directory structure.

## Testing Strategy

### Quality Assurance
- **Static Analysis:** mypy for type checking
- **Code Style:** ruff for formatting and linting
- **Manual Testing:** Functional testing of audio features
- **Automatic Testing:** Core logic/unit tests

### Recommended Test Coverage
- Audio processing edge cases
- State management scenarios
- Error handling and recovery

## Performance Considerations

### Optimization Areas
- **Audio Processing:** Buffer size optimization
- **Real-time Operations:** Low-latency audio pipelines
- **Memory Management:** Efficient audio buffer handling
- **UI Responsiveness:** Non-blocking operations

See [`todos-optimizations.md`](./todos-optimizations.md).

## Architecture Diagram

```mermaid
graph TD
    A[User] --> B[UI Layer]
    B --> C[Core Logic]
    C --> D[Audio Processors]
    D --> E[Audio Output]
    C --> F[State Management]
    F --> B
    G[Configuration] --> C
    H[External Libraries] --> D
```

## Future Evolution

See [Research Documents](./research/index.md).

### Migration Paths
- Gradual refactoring to maintain compatibility
- Comprehensive testing for backward compatibility

## Conclusion

Flitzis Looper represents a well-structured monolithic desktop application with clear separation of concerns. The component-based architecture facilitates maintainability and extensibility while providing real-time audio processing capabilities.
