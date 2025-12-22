# Design: Python Test Suite Update

## Context
Following the December 22, 2025 refactoring (commit 5f24758) that split `FlitzisLooperApp` into `LooperController`, `ProjectState`, `SessionState`, and `UiContext`, the existing Python test suite is fundamentally broken. All imports and test fixtures reference removed classes and APIs, causing test failures.

## Goals
1. Provide comprehensive test coverage for new architectural layers
2. Verify read-only state enforcement through ReadOnlyStateProxy
3. Ensure all controller actions properly validate inputs and update state
4. Maintain test-driven development practices for future features
5. Prevent regressions in the new layered architecture

## Non-Goals
- Testing UI rendering components (Dear PyGui integration tests)
- Testing Rust audio engine directly (covered by Rust unit tests)
- Testing Pydantic itself (already well-tested library)

## Decisions

### 1. Separate Test Files by Component
**Decision**: Create separate test files for each major component rather than one large test file.

**Rationale**:
- Easier to navigate and maintain
- Matches the modular architecture
- Clear separation of concerns
- Faster parallel test execution

**Files**:
- `test_controller.py` - Business logic
- `test_models.py` - State validation
- `test_ui_context.py` - UI interaction layer
- `test_readonly_state_proxy.py` - Access control

### 2. Mock AudioEngine for Controller Tests
**Decision**: Mock the AudioEngine in all controller tests to avoid real audio device requirements.

**Rationale**:
- Tests run in CI environments without audio hardware
- Focus on controller logic, not audio backend
- Faster test execution
- Isolated unit tests (test one thing at a time)

**Implementation**:
```python
with patch("flitzis_looper.controller.AudioEngine", autospec=True):
    controller = LooperController()
```

### 3. Test Behavior, Not Implementation
**Decision**: Test public APIs and observable behavior, not internal state transitions.

**Rationale**:
- Tests resilient to internal refactoring
- Focus on user-visible behavior
- Encourages well-designed public APIs

**Examples**:
- ✅ Test that `controller.load_sample(sample_id, path)` updates `project.sample_paths[sample_id]`
- ❌ Don't test that controller._project._sample_paths is set

### 4. Comprehensive Error Handling Tests
**Decision**: Explicitly test all error conditions and edge cases.

**Rationale**:
- Prevent runtime errors in production
- Document expected error behavior
- Ensure validation is working

**Test cases**:
- Invalid sample IDs (< 0, >= NUM_SAMPLES)
- Non-finite float values (NaN, inf)
- Out-of-range values for volume and speed
- Attempts to modify read-only state

### 5. State Validation Through Pydantic
**Decision**: Trust Pydantic's validation for basic type checking, but test domain-specific validators.

**Rationale**:
- Pydantic is well-tested
- Focus on our custom validation logic
- Reduce test duplication

**What to test**:
- ✅ Custom validators like `validate_sample_id()`
- ✅ Field constraints (ge, le for numeric ranges)
- ✅ Default value factories
- ❌ Basic type annotations (str, int, bool)

## Risks / Trade-offs

### Risk: Test Maintenance Burden
**Mitigation**: Clear test naming conventions, minimal test duplication, focus on behavior over implementation.

### Risk: Brittle Tests Due to Mocking
**Mitigation**: Mock at module boundaries (AudioEngine), not internal methods. Use `autospec=True` for safety.

### Risk: Incomplete Coverage of New Features
**Mitigation**: Task list explicitly enumerates all needed tests. Can use coverage reports to verify.

### Trade-off: Many Small Test Files vs Few Large Files
**Choice**: Small files for better organization. CI can run `pytest src/tests/` to run all tests easily.

## Migration Plan

### Phase 1: Remove Broken Tests (Immediate)
1. Delete `test_app.py`
2. Comment out broken imports in `test_state.py` and `conftest.py`
3. Verify no old references remain (`rg "FlitzisLooperApp\|AppState" src/tests/`)

### Phase 2: Create New Test Infrastructure (Next)
1. Update `conftest.py` with new fixtures
2. Create test files: `test_models.py`, `test_controller.py`
3. Create supporting test utilities if needed

### Phase 3: Test UI Layer (After Core)
1. Create `test_ui_context.py`
2. Create `test_readonly_state_proxy.py`
3. Verify read-only state enforcement works

### Phase 4: Validation (Final)
1. Run all tests and verify 100% pass
2. Check linting and type hints
3. Verify no broken imports remain

## Open Questions
None - the refactoring is complete, we're just updating tests to match.