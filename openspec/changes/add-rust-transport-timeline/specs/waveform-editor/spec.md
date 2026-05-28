## ADDED Requirements

### Requirement: Waveform Editor Shows A Zero-Amplitude Reference
The waveform editor SHALL render a horizontal zero-amplitude reference line across the waveform
plot.

The zero-amplitude line SHALL remain visible alongside the waveform, loop region, playhead, and
musical grid overlays without changing playback, loop marker, or audio-thread behavior.

#### Scenario: Zero line is visible in the waveform plot
- **GIVEN** the waveform editor is open for a loaded pad
- **WHEN** the waveform plot is rendered
- **THEN** a horizontal line is drawn at amplitude `0.0`
- **AND** the line spans the currently visible time range

### Requirement: Waveform Grid Shares The Trigger Quantization Basis
The waveform editor SHALL render its musical grid and loop snapping on the same 1/64-note unit
basis used by trigger quantization and Rust pad timing metadata.

The finest loop editor musical grid line spacing SHALL be one sixteenth of a beat in 4/4. This is
the same subdivision exposed as the minimum `1/64` trigger quantization grid step, while the
default trigger quantization Settings value remains `1/16`.

The waveform editor grid anchor SHALL be the same per-pad timing anchor published to Rust for
pad timing metadata. Adjusting the per-pad Grid Offset SHALL update this published timing anchor
so the visible loop-editor grid and Rust pad timing metadata stay aligned.

The waveform editor grid anchor SHALL remain stable when other pads are started, stopped,
paused, retriggered, or unloaded. Toggling trigger quantization, changing pitch/speed, enabling
BPM lock, or enabling key lock SHALL NOT move the source-side loop editor grid unless the
performer explicitly edits the pad's loop/grid settings.

#### Scenario: Finest loop editor line spacing matches minimum quantization
- **GIVEN** an effective BPM is available
- **AND** the waveform editor is zoomed far enough for the finest musical grid to be readable
- **WHEN** the waveform editor renders the musical grid
- **THEN** adjacent finest grid lines are spaced one sixteenth of a beat apart
- **AND** the `1/64` trigger quantization grid uses the same subdivision interval

#### Scenario: Grid offset updates Rust pad timing metadata
- **GIVEN** a loaded pad has an effective BPM
- **WHEN** the performer adjusts the waveform editor Grid Offset
- **THEN** the waveform editor grid lines move by that sample offset
- **AND** the control layer publishes the shifted grid anchor as the pad timing metadata used by
  Rust playback timing

#### Scenario: Other pad playback does not move the editor grid
- **GIVEN** the waveform editor is open for pad 2
- **AND** pad 2 has a visible musical grid
- **WHEN** pad 1 starts, stops, or is retriggered
- **THEN** pad 2's waveform editor grid lines remain at the same source-side times

#### Scenario: Quantize toggle does not move the editor grid
- **GIVEN** the waveform editor is open for a loaded pad
- **WHEN** trigger quantization is enabled or disabled
- **THEN** the pad's waveform editor grid anchor remains unchanged
