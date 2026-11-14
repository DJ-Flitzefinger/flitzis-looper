## ADDED Requirements

### Requirement: Native ONNX Is The Stem Inference Path

Windows x64 stem generation SHALL route both saved separator choices to the
qualified native ONNX workers. HTDemucs SHALL use FFmpeg/FFprobe for input
decode and FFmpeg for PCM16 WAV encoding. Generation and codec work SHALL
remain off realtime.

#### Scenario: The codec cannot decode the source

- **WHEN** a required tool is missing or fails during a stem decode
- **THEN** generation reports a codec error without invoking inference
- **AND** the previous assignment remains available

#### Scenario: A saved set can be verified

- **GIVEN** an explicitly saved set with complete and verified source and cache
  identities
- **WHEN** the project restores the assignment
- **THEN** the existing adoption path restores it without model inference
