# Development Guide

## Prerequisites and Dependencies

### System Requirements
- **Python Version:** >= 3.13.9
- **Operating System:** Linux, Window, macOS (untested)
- **System Library:** `liblo` (OSC support)

### Python Dependencies
Install dependencies using the package manager:

```bash
# Or using UV (recommended)
uv pip sync --locked
```

### Required Python Packages
- `demucs>=4.0.1` - Music source separation
- `madmom` - Audio and music signal processing
- `matplotlib>=3.10.7` - Audio waveform visualization
- `numpy>=2.3.5` - Numerical computing
- `pedalboard>=0.9.19` - Audio effects processing
- `pyo` - Digital signal processing
- `setuptools>=80.9.0` - Python package management
- `soundfile>=0.13.1` - Audio file I/O
- `torch>=2.9.1` - Machine learning
- `torchaudio>=2.9.1` - Audio utilities

### Development Dependencies
- `mypy>=1.19.0` - Static type checking
- `ruff>=0.14.8` - Code formatting and linting

## Environment Setup

### Installation
1. Clone the repository:
   ```bash
   git clone https://github.com/your-repo/flitzis-looper.git
   cd flitzis-looper
   ```

2. Install dependencies:
   ```bash
   uv pip sync --locked
   ```

3. Install system dependencies:
   ```bash
   # On Ubuntu/Debian
   sudo apt-get install liblo-dev
   ```

## Local Development Commands

### Running the Application
```bash
# Run the application
python -m flitzis_looper

# Or run the main module directly
python flitzis_looper/core/app.py
```

### Build Process
None yet.

### Testing
```bash
# Run type checking
mypy flitzis_looper

# Run code formatting
ruff format flitzis_looper

# Run linting
ruff check flitzis_looper
```

## Development Workflow

### Code Organization
- `flitzis_looper/audio/` - Audio processing modules
- `flitzis_looper/core/` - Core application logic
- `flitzis_looper/ui/` - User interface components
- `flitzis_looper/utils/` - Utility functions

### Common Development Tasks

**Adding a new UI component:**
1. Create a new widget in `ui/widgets/`
2. Add the component to the appropriate dialog or panel
3. Connect event handlers to core logic

**Adding configuration options:**
1. Add new settings to `core/config.py`
2. Create UI controls in the settings dialog
3. Implement save/load functionality

## Deployment Configuration

### Build Configuration
The application uses `pyproject.toml` but does not have a build configuration yet.

### Distribution
```bash
# Create source distribution
python -m build --sdist

# Create wheel distribution
python -m build --wheel
```

### Installation
```bash
pip install dist/flitzis_looper-*.whl
```

## CI/CD Pipeline
No CI/CD configuration files found. Recommended setup:

1. **Testing:** Run mypy and ruff on push
2. **Build:** Create wheel and sdist on tags
3. **Release:** Publish to PyPI on version tags

## Contribution Guidelines

### Code Style
- Follow PEP 8 guidelines
- Use type hints for all functions
- Run `ruff format` before committing
- Run `ruff check` to ensure code quality

### Commit Conventions
- Use free-form commits format
- Keep commits focused and atomic

### Pull Request Process
1. Create a feature branch
2. Implement the feature with tests
3. Update documentation if needed
4. Run all checks locally
5. Submit PR for review

### Testing Requirements
- No testing currently

## Troubleshooting

### Common Issues

**Audio device not found:**
- Ensure proper audio drivers are installed
- Check audio device permissions
- Try different audio backends

**Missing library dependencies:**
- Install `liblo` and other system dependencies
- Ensure all Python dependencies are installed

**Performance issues:**
- Check audio buffer sizes
- Optimize DSP operations
- Profile with Python profiling tools

## Project Structure Reference

```
flitzis_looper/
├── audio/          # Audio processing modules
├── core/           # Application logic and state
├── ui/             # User interface components
└── utils/          # Shared utility functions
```

## Getting Started for New Developers

1. Install dependencies and set up environment
2. Run the application to understand the current functionality
3. Explore the codebase starting from `core/app.py`
4. Check existing issues or create new feature branches
5. Follow the contribution guidelines for all changes
