# AGENTS.md - Guidelines for Agentic Coding Agents

## Build/Lint/Test Commands

### Development Setup
```bash
uv sync --frozen
```

### Running Tests
```bash
# Type checking
uv run mypy .

# Code style checking
uv run ruff check .

# Run unit tests
uv run pytest

# Run both checks (as shown in README)
uv run mypy .
uv run ruff check .
```

### Running Single Test
```bash
# Run all tests
uv run pytest

# Run specific test file
uv run pytest tests/test_core/test_sample.py

# Run tests with specific marker
uv run pytest -m unit

# Run tests with verbose output
uv run pytest -v
```

### Formatting Code
```bash
# Format code
uv run ruff format .

# Check formatting without changes
uv run ruff format --check .
```

## Code Style Guidelines

### Imports
- Use absolute imports when possible
- Group imports in standard order: standard library, third-party, local
- Use explicit imports (avoid `import *`)
- Follow isort configuration in ruff_defaults.toml

### Formatting
- Line length: 100 characters maximum
- Use ruff formatter for consistent code style
- All Python files should conform to PEP 8 standards
- Use 4 spaces for indentation (no tabs)

### Types
- Use type hints for function parameters and return values
- Follow Google Python style guide for docstrings
- Leverage mypy for static type checking

### Naming Conventions
- Variables: snake_case
- Functions: snake_case
- Classes: PascalCase
- Constants: UPPER_SNAKE_CASE
- Private members: prefixed with underscore (_private)

### Error Handling
- Use contextlib.suppress() for simple exception handling
- Log errors appropriately with the logger
- Prefer specific exception handling over broad except clauses
- Use try/except blocks around external resource access

### Docstrings
- Follow Google Python style guide for docstrings
- Document all public functions, classes, and methods
- Include parameter types and descriptions
- Include return value descriptions for non-trivial functions

### Additional Notes
- The project uses tkinter for GUI development
- Audio processing is handled with pyo, pedalboard, and soundfile
- Configuration is stored in JSON format
- Use the state management system in core/state.py for global variables
- Follow the existing patterns for UI component creation in ui/ directory
