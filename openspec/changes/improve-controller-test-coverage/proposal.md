# Improve Controller Test Coverage

## Summary

Add comprehensive test coverage for the `flitzis_looper.controller` module to ensure reliability and catch regressions. Currently, 6 modules have no dedicated tests, and existing test files have significant gaps in coverage. This change will add ~130 new tests across all controller modules.

## Motivation

The controller layer is critical to application reliability, managing sample loading, playback, persistence, and project state. Current test coverage is insufficient:
- 6 modules have **no dedicated tests** (AppController, BaseController, MeteringController, validation.py, TransportController, ApplyProjectState)
- 7 well-tested modules still have **significant gaps** (loader, persistence, bpm, pad, loop, global_params, playback)
- ~130 missing tests identified in `docs/controller-tests.md`

Without comprehensive tests, regressions may go undetected, and edge cases (error handling, invalid inputs, boundary conditions) remain unverified.

## Goals

1. Add dedicated tests for 6 modules without test files
2. Fill gaps in 7 existing test files
3. Achieve comprehensive coverage of error handling, validation, and edge cases
4. Ensure all controller modules are production-ready with reliable behavior

## Scope

**In scope:**
- Create `test_base_controller.py` (8 tests)
- Create `test_validation.py` (8 tests)
- Create `test_metering_controller.py` (13 tests)
- Create `test_transport_controller.py` (3 tests)
- Create `test_apply_project_state.py` (21 tests)
- Create `test_app_controller.py` (14 tests)
- Add 18 tests to `test_loader.py`
- Add 12 tests to `test_persistence.py`
- Add 15 tests to `test_bpm.py`
- Add 3 tests to `test_pad.py`
- Add 28 tests to `test_loop.py`
- Add 5 tests to `test_global_params.py`
- Add 8 tests to `test_playback.py`

**Out of scope:**
- Changes to production controller code
- Changes to test infrastructure or framework
- Integration tests beyond unit tests
- Performance or load testing

## Non-Goals

- Modifying controller implementation (only tests)
- Adding new test frameworks or tools
- Changing test structure or organization
- Documentation updates beyond test code

## Dependencies

None. This is a testing initiative that depends only on existing controller code.

## Related Changes

None. This is an independent testing effort.

## Success Criteria

- [ ] All ~130 new tests pass
- [ ] All tests follow existing test patterns and conventions
- [ ] Coverage report shows significant improvement
- [ ] All tests run cleanly with pytest

## Risks

- **Medium:** Tests may expose existing bugs in controller code, requiring fixes
- **Low:** Test fixture complexity may grow; mitigated by reusing existing patterns
- **Low:** New tests may be brittle; mitigated by following existing patterns

## Timeline

Estimated effort: ~130 tests × 15-30 minutes per test = 32-65 hours
Suggested approach: Batched implementation in priority order
