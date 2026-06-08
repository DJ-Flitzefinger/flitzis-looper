## ADDED Requirements

### Requirement: Neutral Internal Rust DSP Foundation
The system SHALL introduce an internal Rust DSP/FX foundation before implementing any new visible
EQ or effect behavior.

The first foundation slice SHALL host only neutral no-op or test-only processing in a per-pad
chain. It SHALL preserve current public Python APIs, current performer-facing controls, current
project persistence semantics, and current audio output within floating-point tolerance.

#### Scenario: Neutral foundation preserves playback
- **GIVEN** the DSP foundation has been added
- **AND** no visible DSP node is enabled
- **WHEN** the mixer renders an active pad
- **THEN** the rendered output matches the pre-foundation path within floating-point tolerance
- **AND** the existing per-pad EQ controls remain unchanged

### Requirement: DSP Parameters Use Typed Bounded State
The system SHALL represent accepted DSP parameter targets with typed fixed-size identities and
Rust-owned smoothing state.

Future DSP parameter identities SHALL NOT contain callback-local pointers, Python objects, file
paths, plugin handles, dynamically sized metadata, or UI object references. Accepted continuous
targets SHALL use the bounded parameter path before they are applied to DSP state, and the audio
side SHALL smooth target changes before sample processing.

#### Scenario: Continuous DSP target is bounded before smoothing
- **GIVEN** a future mapped controller targets a DSP parameter
- **WHEN** the controller derives an accepted finite target value
- **THEN** Rust receives that target through a typed bounded parameter identity
- **AND** the DSP state smooths from the previous value toward the target before processing audio

### Requirement: DSP Chain Processing Preserves Realtime Constraints
The system MUST keep DSP chain processing realtime-safe in the CPAL audio callback.

DSP processing in the callback MUST NOT perform disk I/O, JSON access, Python/GIL access, UI
calls, blocking waits, logging, neural inference, plugin loading/scanning, heavy allocation,
unbounded loops, or long-running work. Node state and scratch storage needed by the first
foundation slice SHALL be prepared outside realtime rendering or stored as fixed-size state owned
by the Rust audio engine.

#### Scenario: DSP callback work stays bounded
- **GIVEN** a neutral per-pad DSP chain has been prepared
- **WHEN** the audio callback renders a block
- **THEN** the chain processes only already owned audio buffers and fixed-size state
- **AND** it does not allocate, block, log, touch disk, acquire the Python GIL, scan plugins, or
  run neural inference

### Requirement: Later Isolator EQ Replacement Is Separate
The system SHALL treat the later 3-band DJ isolator replacement as a separate OpenSpec-backed
behavior change after the neutral DSP foundation exists.

The foundation slice SHALL NOT replace the current hardwired per-pad EQ, add a visible filter,
delay, reverb, phaser, flanger, stem effect, deck/group/master effect, plugin host, or new UI
control. The later isolator replacement SHALL use the DSP foundation instead of patching the
current hardwired mixer EQ path.

#### Scenario: Foundation does not implement isolator behavior
- **GIVEN** only the DSP foundation slice has been applied
- **WHEN** the performer uses the existing low, mid, and high EQ controls
- **THEN** the current EQ behavior and UI mapping remain the active behavior
- **AND** no new isolator node, visible effect, plugin host, or new performer control is present
