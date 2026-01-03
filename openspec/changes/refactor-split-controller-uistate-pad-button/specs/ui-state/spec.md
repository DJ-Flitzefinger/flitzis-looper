## ADDED Requirements
### Requirement: UI computed state is decomposed into selector components
The UI state layer SHALL provide read-only computed state access while decomposing `UiState` into smaller selector components so that no single UI state class violates Ruff `PLR0904` public-method thresholds.

#### Scenario: UiState passes Ruff public-method limits
- **WHEN** the developer runs Ruff on the project
- **THEN** `src/flitzis_looper/ui/context.py` produces no `PLR0904` findings for UI state classes

### Requirement: UI actions are decomposed into action components
The UI action layer SHALL decompose audio-related actions (currently grouped in `AudioActions`) into smaller action components so that no single action class violates Ruff `PLR0904` public-method thresholds.

#### Scenario: Audio actions pass Ruff public-method limits
- **WHEN** the developer runs Ruff on the project
- **THEN** `src/flitzis_looper/ui/context.py` produces no `PLR0904` findings for audio action classes

### Requirement: UI context uses the new controller API surface
When `LooperController` is refactored into grouped sub-objects, the UI context layer SHALL be updated to call the new API directly (no legacy compatibility layer).

#### Scenario: UI compiles against new controller API
- **WHEN** the developer runs the existing Python test suite
- **THEN** UI context code imports and calls the controller successfully

### Requirement: UI state remains read-only
The UI state surface SHALL remain read-only from UI code. Direct assignment to the proxy-wrapped project/session state via `UiState` MUST raise an `AttributeError` instructing the caller to use controller actions.

#### Scenario: Attempted mutation raises a clear error
- **GIVEN** UI code has access to `ctx.state.project`
- **WHEN** the UI attempts to set an attribute on that object
- **THEN** an `AttributeError` is raised indicating state is read-only

### Requirement: UI refactor preserves existing UI semantics
The UI refactor SHALL preserve the observable UI semantics required by existing specs, including pad label basename formatting (`performance-ui`), pad loading progress behavior (`async-sample-loading`), and peak meter rendering (`per-pad-metering`).

#### Scenario: Existing UI tests remain valid
- **WHEN** the existing Python test suite is executed
- **THEN** UI context and render-related tests continue to pass
