# Tests

This directory contains all unit and integration tests for the Flitzis Looper project.

## Structure

- `test_audio/` - Tests for audio processing modules
- `test_core/` - Tests for core application logic
- `test_ui/` - Tests for user interface components
- `test_utils/` - Tests for utility functions
- `conftest.py` - Shared pytest fixtures and configuration

## Running Tests

To run all tests:

```bash
pytest
```

To run tests with verbose output:

```bash
pytest -v
```

To run tests with coverage:

```bash
pytest --cov=flitzis_looper
```

## Test Markers

- `unit` - Unit tests
- `integration` - Integration tests
- `slow` - Slow running tests

To run only unit tests:

```bash
pytest -m unit
```
