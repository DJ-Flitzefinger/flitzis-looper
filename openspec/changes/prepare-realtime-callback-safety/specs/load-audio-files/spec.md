## ADDED Requirements

### Requirement: Loaded Sample Replacement Retires Old Audio Handles Outside The Callback
The system SHALL keep loaded sample replacement and unload operations real-time safe when old
audio handles are released.

When a loaded sample slot is replaced or unloaded in the audio callback, any old full-mix sample
handle and associated prepared-stem handles SHALL be moved to bounded non-audio cleanup instead
of being deallocated directly on the callback thread.

#### Scenario: Replacing a loaded sample defers old handle cleanup
- **GIVEN** a sample slot already contains a loaded full-mix buffer
- **WHEN** a new loaded sample publication for the same slot reaches the audio callback
- **THEN** the old buffer handle is retired through non-audio cleanup
- **AND** the new buffer becomes the slot's loaded full-mix source
- **AND** the callback performs no disk I/O, blocking wait, logging, Python/GIL access, neural
  inference, plugin loading, or large audio-payload deallocation
