# Development Guide

## Prerequisites and Dependencies

### System Requirements
- **Python**
- **Operating System:** Linux, Windows, macOS (untested)

## Environment Setup

### Installation
1. Clone the repository:
   ```bash
   git clone https://github.com/DJ-Flitzefinger/flitzis-looper.git
   cd flitzis-looper
   ```

2. Install dependencies:
   ```bash
   uv sync
   ```

## Local Development Commands

### Running the Application
```bash
# Run the application
uv run python -m flitzis_looper
```

### Testing
```bash
# Run type checking
uv run mypy .

# Run code formatting
uv run ruff format .

# Run linting
uv run ruff check .
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

## Build Process

In the future, we want to provide a portable .exe for Window using [Nuitka](https://nuitka.net/).

## CI/CD Pipeline

No CI/CD configuration files found. Recommended setup:

1. **Testing:** Run `uv mypy .` and `uv ruff check .` on push
2. **Build:** Create wheel, sdist and .exe (Nuitka) on tags (to be done)

## Contribution Guidelines

### Code Style
- Follow PEP 8 guidelines
- Use type hints for all functions
- Run `uv run ruff format .` before committing
- Run `uv run ruff check .` to ensure code quality

### Commit Conventions
- Use conventional commit format (free-form accepted)
- Keep commits focused and atomic

### Testing Requirements
- Run `uv run mypy .` and `uv run ruff check .` before comitting
- Add tests for new features when possible

## Getting Started for New Developers

1. Install dependencies and set up environment
2. Run the application to understand the current functionality
3. Explore the codebase starting from `core/app.py`
4. Check existing issues or create new feature branches
5. Follow the contribution guidelines for all changes
