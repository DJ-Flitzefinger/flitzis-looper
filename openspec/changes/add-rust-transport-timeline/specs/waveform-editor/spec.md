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
basis used by trigger quantization.

The finest loop editor musical grid line spacing SHALL be one sixteenth of a beat in 4/4. This is
the same subdivision exposed as the minimum `1/64` trigger quantization grid step, while the
default trigger quantization Settings value remains `1/16`.

#### Scenario: Finest loop editor line spacing matches minimum quantization
- **GIVEN** an effective BPM is available
- **AND** the waveform editor is zoomed far enough for the finest musical grid to be readable
- **WHEN** the waveform editor renders the musical grid
- **THEN** adjacent finest grid lines are spaced one sixteenth of a beat apart
- **AND** the `1/64` trigger quantization grid uses the same subdivision interval
