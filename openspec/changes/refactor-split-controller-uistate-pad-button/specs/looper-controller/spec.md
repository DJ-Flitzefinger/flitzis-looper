## ADDED Requirements
### Requirement: Controller is decomposed into focused modules
The Python control layer SHALL provide a `LooperController` entrypoint while decomposing controller responsibilities into smaller focused modules/classes so that no single class violates Ruff `PLR0904` public-method thresholds.

In addition, the controller’s public API SHALL be grouped by responsibility (e.g. loader/analysis, transport/playback, metering) rather than exposing a single flat set of methods, and it SHALL NOT provide a legacy compatibility flat-method API.

This is an internal-quality requirement intended to preserve behavior while improving maintainability.

#### Scenario: Controller passes Ruff public-method limits
- **WHEN** the developer runs Ruff on the project
- **THEN** `src/flitzis_looper/controller` produces no `PLR0904` findings

### Requirement: Stable `LooperController` import path
The system SHALL keep the import path `from flitzis_looper.controller import LooperController` valid even if the controller implementation is moved into a package directory.

This requirement does not imply preserving the legacy flat-method API; call sites within this repo will be updated to the new grouped controller surface.

#### Scenario: Existing imports remain valid
- **GIVEN** existing UI and tests import `LooperController` from `flitzis_looper.controller`
- **WHEN** the controller is refactored into a package
- **THEN** those imports continue to resolve without changes

### Requirement: Controller does not catch blind exceptions for analysis
The controller SHALL NOT catch a blind `Exception` when invoking audio analysis APIs. To keep scope minimal, it SHALL catch `RuntimeError` for `AudioEngine.analyze_sample_async` failures and report the error in controller/session state as it does today.

#### Scenario: Analysis error handling is Ruff-compliant
- **WHEN** the developer runs Ruff on the project
- **THEN** `BLE001` is not reported for controller analysis-triggering code

### Requirement: Controller refactor preserves existing behavior
The controller refactor SHALL preserve observable behavior required by existing specs, including (but not limited to) `async-sample-loading`, `audio-analysis`, `play-samples`, `multi-loop-mode`, `pad-manual-bpm`, and `per-pad-metering`.

#### Scenario: Behavior remains compliant with existing specs
- **WHEN** the existing Python test suite is executed
- **THEN** controller-dependent tests continue to pass
