# Refactor TransportController

## Summary
Extract three focused controller subcomponents from `TransportController` to reduce class complexity and improve maintainability:
1. **BPMController** (~112 lines) - manages manual BPM overrides, tap BPM detection, and master BPM computation
2. **GlobalModesController** (~32 lines) - manages multi-loop, key lock, and BPM lock global modes
3. **PlaybackOrchestrator** (~36 lines) - manages pad triggering, stopping, and playback orchestration

## Motivation
The `TransportController` class has grown to 593 lines with ~40 public methods, violating Ruff's `PLR0904` complexity threshold. This makes the class difficult to understand, test, and maintain. The three identified candidate groups have clear responsibilities and minimal coupling to the main controller, making them ideal for extraction.

## Goals
- Reduce `TransportController` method count to comply with Ruff `PLR0904` threshold
- Improve code organization by grouping related functionality
- Maintain stable public API through `TransportController` delegation
- Preserve all existing behavior required by dependent specs
- Improve testability of extracted components

## Non-Goals
- Changing any observable behavior
- Adding new functionality
- Breaking existing tests or UI integration
- Creating new public APIs outside controller package
