# Source Tree Analysis

## Project Structure Overview

```
flitzis_looper/
├── __init__.py               # Package initialization
├── __main__.py               # Main entry point → Calls core.app.main()
├── audio/                    # Audio processing modules
│   ├── __init__.py
│   ├── bpm.py                # BPM (Beats Per Minute) detection
│   ├── loop.py               # Audio looping functionality
│   ├── pitch.py              # Pitch detection and manipulation
│   ├── server.py             # Audio server/streaming
│   ├── stems_engine.py       # Stem separation engine
│   └── stems_separation.py  # Stem separation algorithms
├── core/                     # Core application logic
│   ├── __init__.py
│   ├── app.py                # Main application class → Entry point
│   ├── banks.py              # Sample/loop banks management
│   ├── bpm_control.py        # BPM control logic
│   ├── config.py             # Application configuration
│   ├── loops.py              # Loop management
│   ├── state.py              # Application state management
│   ├── stems_control.py      # Stem separation control
│   └── volume_control.py     # Volume control logic
├── ui/                       # User interface components
│   ├── __init__.py
│   ├── dialogs/              # Dialog windows
│   │   ├── __init__.py
│   │   ├── bpm_dialog.py     # BPM configuration dialog
│   │   ├── volume.py         # Volume control dialog
│   │   └── waveform.py       # Waveform visualization dialog
│   ├── loop_grid.py          # Loop grid interface
│   ├── main_window.py        # Main application window
│   ├── stems_panel.py        # Stem separation panel
│   ├── toolbar.py            # Application toolbar
│   └── widgets/              # Custom UI widgets
│       ├── __init__.py
│       ├── eq_knob.py        # Equalizer knob widget
│       └── vu_meter.py       # VU (Volume Unit) meter widget
└── utils/                    # Utility functions
    ├── __init__.py
    ├── logging.py            # Logging utilities
    ├── math.py               # Mathematical utilities
    ├── paths.py              # Path handling utilities
    └── threading.py          # Threading utilities
```

## Critical Folders Explained

### `audio/` - Audio Processing Engine
- **Purpose:** Core audio processing functionality
- **Key Components:**
  - BPM detection and synchronization
  - Audio looping and playback
  - Pitch detection and manipulation
  - Stem separation (isolating vocals, drums, bass, etc.)
  - Audio server for real-time processing
- **Integration Points:** Used by core application logic

### `core/` - Application Business Logic
- **Purpose:** Main application functionality and state management
- **Key Components:**
  - Application entry point and lifecycle management
  - Sample/loop banks for organizing audio content
  - BPM control and synchronization
  - Configuration management
  - State management for application state
  - Stem separation control
  - Volume control
- **Entry Points:** `app.py` contains the main application class

### `ui/` - User Interface Layer
- **Purpose:** Graphical user interface for the application
- **Key Components:**
  - Main application window
  - Loop grid for arranging and triggering loops
  - Stem separation panel for controlling audio separation
  - Dialogs for configuration (BPM, volume, waveform)
  - Custom widgets (EQ knobs, VU meters)
  - Toolbar for quick access to functions
- **Integration Points:** Connects to core logic via event handlers

### `utils/` - Utility Functions
- **Purpose:** Shared utility functions across the application
- **Key Components:**
  - Logging infrastructure
  - Mathematical utilities for audio processing
  - Path handling for file operations
  - Threading utilities for concurrent operations

## Entry Points and Integration

**Primary Entry Point:**
- `flitzis_looper/__main__.py` → `flitzis_looper.core.app.main()`

**Key Integration Flow:**
1. User interacts with UI components
2. UI triggers events to core application logic
3. Core logic processes requests using audio modules
4. Audio modules perform DSP operations
5. Results are returned to core and displayed in UI

## Multi-Part Structure
This is a **monolithic application** with all components contained within a single repository. No separate client/server architecture detected.

## Annotated Directory Tree

```markdown
project-root/
├── .editorconfig          # Code style configuration
├── .gitignore             # Git ignore patterns
├── .python-version        # Python version specification
├── pyproject.toml         # Python project configuration and dependencies
├── README.md              # Project documentation (minimal)
├── ruff_defaults.toml     # Code formatting configuration
├── uv.lock                # Dependency lock file
├── .bmad/                 # BMAD configuration and agents
├── .roo/                  # Roo configuration
├── .vscode/               # VSCode workspace configuration
├── docs/                  # Generated documentation (this file)
│   ├── project-scan-report.json
│   ├── technology-stack.md
│   ├── comprehensive-analysis.md
│   └── source-tree-analysis.md
└── flitzis_looper/        # Main application package
```

## Summary
This source tree analysis provides a comprehensive overview of the monolithic Python desktop audio application structure, with clear separation between audio processing, core logic, user interface, and utility components.
