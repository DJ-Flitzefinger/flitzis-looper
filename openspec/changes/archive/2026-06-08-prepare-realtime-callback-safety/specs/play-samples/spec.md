## ADDED Requirements

### Requirement: Oversized Output Blocks Are Rendered In Bounded Chunks
The system SHALL render audio callback output buffers in chunks that fit the preallocated
real-time processing buffers.

If the audio backend delivers more output frames than the requested callback block size, the Rust
audio path SHALL split the render work into bounded sub-blocks rather than indexing past internal
buffers or resizing them in the callback. The chunking SHALL preserve immediate playback,
scheduled event offsets, loop playback, BPM-lock tempo ratio, Key Lock mode selection, stem
rendering fallback, gain/EQ application, and per-pad metering semantics.

#### Scenario: Larger-than-requested callback block remains safe
- **GIVEN** the audio backend delivers an output block larger than the requested fixed block size
- **WHEN** the callback renders the block
- **THEN** Rust renders it as bounded sub-blocks that fit preallocated processing buffers
- **AND** playback continues without panic, heap allocation, blocking, logging, disk I/O, Python
  GIL access, neural inference, or plugin loading

#### Scenario: Scheduled event offset survives chunking
- **GIVEN** a scheduled pad start targets an output frame inside an oversized callback block
- **WHEN** the callback renders the block using bounded sub-blocks
- **THEN** frames before the scheduled target are rendered before the pad starts
- **AND** the pad starts contributing at the scheduled target frame
