# flitzis-looper TODOs

## Bugs

### High Priority
- Fix blind exception catching (over 30 instances of `except Exception:`)
- Resolve type errors in UI widgets (15 mypy errors in toolbar, stems_panel, and loop_grid)
- Fix attribute access on Canvas objects that don't exist

### Medium Priority
- Address overly complex functions (20+ functions exceeding complexity thresholds)
- Fix import placement issues (imports inside functions causing potential cyclic dependencies)
- Add missing type annotations for variables and callback dictionaries
- Fix db display missing on gain controls

## Chores

### Code Quality
- Refactor complex functions (break down functions with complexity >10)
- Move function-level imports to module level
- Replace generic exception handling with specific exception types
- Add proper logging for caught exceptions
- Enable stricter mypy checking

### Testing
- Expand unit tests for core audio processing functions
- Create integration tests for UI components
- Add end-to-end tests for complete workflow scenarios
- Set up CI/CD pipeline integration
- Implement test helpers for UI components
- Create test fixtures for audio data
- Mock external dependencies (pyo, soundfile)

## Features

### UI/UX Improvements
- Implement config presets
- Move BPM window to sidebar with per-pad EQ buttons
- Add volume normalizer

### Future Upgrades
- MIDI integration
- Option menu
- Touch-mode selectable in options

### Performance
- Improve loop triggering speed (faster on 8 bars, slower on more bars)
- Waveform editor snappiness
