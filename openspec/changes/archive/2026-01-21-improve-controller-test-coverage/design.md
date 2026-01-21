# Design: Improve Controller Test Coverage

## Overview

This design adds ~130 unit tests across the `flitzis_looper.controller` module to achieve comprehensive coverage. Tests follow existing patterns, use pytest fixtures, and focus on edge cases, error handling, and validation.

## Architecture

### Test Structure

```
src/tests/controller/
├── test_base_controller.py          # NEW: BaseController tests
├── test_validation.py               # NEW: validation.py tests
├── test_metering_controller.py      # NEW: MeteringController tests (split from test_pad.py)
├── test_transport_controller.py     # NEW: TransportController tests
├── test_apply_project_state.py      # NEW: ApplyProjectState tests
├── test_app_controller.py           # NEW: AppController tests
├── test_loader.py                   # EXISTING: additions
├── test_persistence.py              # EXISTING: additions
├── test_bpm.py                      # EXISTING: additions
├── test_pad.py                      # EXISTING: additions
├── test_loop.py                     # EXISTING: additions
├── test_global_params.py            # EXISTING: additions
└── test_playback.py                 # EXISTING: additions
```

### Test Organization

Tests are organized by module, following existing patterns:
- **Initialization tests**: Verify constructor behavior and state setup
- **Happy path tests**: Normal operation with valid inputs
- **Edge case tests**: Boundary conditions, null/empty inputs
- **Error handling tests**: Invalid inputs, exceptions, failures
- **Integration-style tests**: Multiple method calls, state transitions

## Test Patterns

### Fixture Patterns

Existing test files use consistent fixture patterns. New tests will follow the same approach:

```python
# Common fixtures for controller tests
@pytest.fixture
def mock_project():
    """Mock project with default state."""
    return Project(...)

@pytest.fixture
def mock_session():
    """Mock audio session."""
    return AudioSession(...)

@pytest.fixture
def mock_audio_engine():
    """Mock audio engine."""
    return AudioEngine(...)

# Controller-specific fixtures
@pytest.fixture
def base_controller(mock_project, mock_session, mock_audio_engine):
    """BaseController instance."""
    return BaseController(mock_project, mock_session, mock_audio_engine)

@pytest.fixture
def transport_controller(...):
    """TransportController with all sub-controllers."""
    ...
```

### Test Naming Convention

Tests follow `test_<method>_<condition>` pattern:
- `test_set_manual_bpm_invalid_raises` - method + condition
- `test_handle_pad_peak_message_ignores_non_finite` - method + condition
- `test_effective_region_auto_no_bpm` - method + condition

### Assertion Patterns

Tests use clear assertions with descriptive messages:

```python
# Verify state changes
assert controller.sample_id == 1, "Sample ID should be set to 1"
assert controller.is_loading is True, "Should mark sample as loading"

# Verify method calls
mock_audio.set_volume.assert_called_once_with(0.7)
mock_audio.set_pad_gain.assert_called_with(0, 0.8)

# Verify exceptions
with pytest.raises(ValueError, match="BPM must be positive"):
    controller.set_manual_bpm(-10)

# Verify no change
assert controller.manual_bpm is None, "Manual BPM should remain None"
```

## Implementation Details

### Phase 1: Core Infrastructure Tests

**BaseController tests** focus on:
- Constructor parameter storage
- `_output_sample_rate_hz` behavior with audio engine availability/errors
- `_mark_project_changed` callback behavior

**validation.py tests** focus on:
- `ensure_finite` with valid/invalid values
- `normalize_bpm` with various input types

**MeteringController tests** focus on:
- Peak message handling (clamping, validation)
- Playhead message handling (negative, non-finite)
- Exponential decay behavior (half-life, threshold)
- Message polling with missing attributes

### Phase 2: App Initialization Tests

**TransportController tests** verify:
- Sub-controller initialization (BpmController, PadController, etc.)
- Reference passing to sub-controllers
- `apply_project_state_to_audio` delegation

**ApplyProjectState tests** verify:
- Initialization stores transport reference
- Apply methods only call when values differ from default
- Apply methods call correct audio methods
- Apply methods handle edge cases (no samples, None BPM, etc.)
- Full project state application with defaults vs modified state

**AppController tests** verify:
- Initialization creates all components
- Loads project from persistence
- Applies project state to audio
- Restores samples from project
- Shutdown behavior (flush, stop audio, handle errors)
- Property accessors

### Phase 3: Metering Controller Completion

Add missing MeteringController tests to complete coverage, currently split between test_pad.py and the new test_metering_controller.py.

### Phase 4: Existing Test File Additions

Each existing test file gets focused additions for missing coverage:

- **test_loader.py**: Event handling, analysis methods, path handling
- **test_persistence.py**: Path normalization, atomic write failures
- **test_bpm.py**: Validation, edge cases, audio engine updates
- **test_loop.py**: Quantization, snap behavior, edge cases
- **test_global_params.py**: No-op behavior, edge cases
- **test_playback.py**: Loop region application, validation

## Error Handling Strategy

Tests verify controller behavior under error conditions:

1. **Invalid inputs**: Validate ValueError raises for invalid parameters
2. **Missing data**: Verify graceful handling of None/missing values
3. **Audio engine errors**: Test RuntimeError, TypeError, ValueError handling
4. **File system errors**: Test OSError handling (persistence)
5. **Malformed messages**: Test ignoring invalid audio messages

## Coverage Goals

Target coverage per module:
- **BaseController**: 100% (all methods and error paths)
- **validation.py**: 100% (all utility functions)
- **MeteringController**: 100% (all message handlers and decay logic)
- **TransportController**: 100% (initialization and delegation)
- **ApplyProjectState**: 100% (all apply methods and edge cases)
- **AppController**: 100% (lifecycle and properties)
- **Existing modules**: >90% (fill identified gaps)

## Validation Approach

After test implementation:

1. **Run all tests**: `pytest src/tests/controller/ -v`
2. **Generate coverage**: `pytest --cov=flitzis_looper.controller --cov-report=html`
3. **Lint check**: `uv run ruff check src/tests/controller/`
4. **Type check**: `uv run mypy src/tests/controller/`
5. **Review coverage report**: Identify any remaining gaps

## Risks and Mitigations

### Risk: Tests expose existing bugs

**Mitigation**: Document bugs found during test implementation, create separate change proposal for fixes. Tests should document expected behavior even if implementation needs fixes.

### Risk: Test fixture complexity grows

**Mitigation**: Reuse existing fixtures, create shared fixture module if needed, keep fixtures focused on minimal setup.

### Risk: Tests become brittle

**Mitigation**: Focus on behavior verification over implementation details, use clear assertions, avoid over-mocking.

## Trade-offs

### Simplicity vs Completeness

**Decision**: Prioritize completeness (~130 tests) over minimal test suite. Controller layer is critical to reliability; comprehensive tests prevent regressions.

**Rationale**: Existing gaps indicate many unverified edge cases. Production application needs robust controller behavior.

### Test file organization

**Decision**: Keep test_metering_controller.py separate rather than merging with test_pad.py.

**Rationale**: MeteringController is a distinct module (metering.py) with 13 missing tests. Separation follows module structure and keeps test files focused.

### Phase ordering

**Decision**: Implement tests in priority order (infrastructure → lifecycle → gaps).

**Rationale**: Infrastructure tests validate base behavior needed by other controllers. Lifecycle tests validate app startup/shutdown critical to all features. Gaps in existing tests are lower priority as those modules already have partial coverage.
