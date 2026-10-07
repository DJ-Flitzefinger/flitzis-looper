## MODIFIED Requirements

### Requirement: Output Time And Source Position Are Distinct
The system SHALL distinguish Rust-owned output-frame time, virtual fractional
source timing position and admitted physical PCM interpolation taps. Output time
SHALL remain owned by transport/scheduler, and active Rust mixer voices SHALL own
source progression shared by full mix and matching stems. A virtual fractional
seam position MAY reach the physical exclusive end within the admitted rounding
mismatch, but SHALL NOT be used as an unchecked PCM index.

#### Scenario: Quantized trigger chooses output time without source seeking
- **GIVEN** an admitted source and a quantized pad trigger
- **WHEN** the scheduler chooses the trigger's output-frame entry time
- **THEN** the source begins at its effective source start
- **AND** quantization does not seek the source to the transport position

#### Scenario: Virtual phase enters the extended seam
- **GIVEN** P=1500.25 loaded frames and physical half-open length H=1500
- **WHEN** the virtual source phase reaches 1500.125
- **THEN** timing retains that virtual fractional phase
- **AND** the reader interpolates from physical knot 1499 to loop start at P
- **AND** no PCM access uses the physical exclusive end as an index

### Requirement: Loop Regions Resolve To Source-Frame Ranges
The system SHALL preserve integer physical half-open loop endpoints while using
the compatible musical loop period from the active voice's effective source-bound
accepted timing for productive fractional source progression. The musical period
SHALL equal loaded rate times accepted quarter period times compatible logical
beats, without applying another H/P playback-rate factor.

Manual, Tap, Legacy and incompatible arbitrary loops SHALL retain physical
repetition without claiming sustained musical alignment. Missing/invalid physical
bounds SHALL retain existing full-source fallback. Intro/tail/explicit seeks,
pause/resume and live edits SHALL use the shared source trajectory. All source
and interpolation taps SHALL remain inside admitted physical PCM.

#### Scenario: Live loop edit preserves an in-range playhead
- **GIVEN** a pad is actively playing
- **AND** its current source-frame position is inside the newly published loop region
- **WHEN** the loop region reaches the audio thread
- **THEN** subsequent rendering continues from the current source-frame position
- **AND** the voice does not restart from the loop start

#### Scenario: Live loop edit clamps an out-of-range playhead
- **GIVEN** a pad is actively playing
- **AND** its current source-frame position is outside the newly published loop region
- **WHEN** the loop region reaches the audio thread
- **THEN** subsequent rendering clamps the voice to the new loop start

#### Scenario: Fractional compatible duration retains persisted endpoints
- **GIVEN** current acknowledged accepted timing with P=1500.25 loaded frames and physical length H=1500
- **WHEN** the productive mixer renders 75 and 1000 cycles at a fractional rate
- **THEN** actual unwrapped musical boundary error stays within one loaded frame
- **AND** the stored integer endpoints and authoritative source rate are unchanged
- **AND** the final admitted PCM knot interpolates to loop start at P

#### Scenario: An old voice retains its pinned accepted trajectory
- **GIVEN** an active voice retains its actual source and effective accepted timing
- **WHEN** bank replacement or unavailable admission changes current pad metadata
- **THEN** the old voice continues its pinned source and musical domain
- **AND** replacement timing cannot relabel that trajectory

#### Scenario: Live edits and explicit seeks retain bounded source reading
- **GIVEN** an active voice with a compatible fractional loop
- **WHEN** an in-range edit, pause/resume or explicit intro/tail seek occurs
- **THEN** the shared trajectory preserves the appropriate fractional phase and seek policy
- **AND** no interpolation tap reads beyond admitted PCM

#### Scenario: Period-only refresh crosses a virtual seam
- **GIVEN** an accepted loop with unchanged physical bounds and virtual phase 1500.125 at P=1500.25
- **WHEN** accepted period changes to P=1499.75 or accepted timing clears to H=1500
- **THEN** canonical phase retains the corresponding wrapped residue 0.375 or 0.125
- **AND** actual chronological native/FIFO/filter history continues without a phase-zero reset
- **AND** stale prepared domains remain rejected

## ADDED Requirements

### Requirement: Copied Preparation Preserves The Fractional Loop Domain
The system SHALL copy and compare the complete physical and fractional musical
loop domain for source reading and native prepared adoption. Full mix, same-source
stems and transition sides SHALL consume one source trajectory. Continuous native,
FIFO and filter history SHALL follow actual productive feed, and a stale prepared
domain SHALL NOT adopt even when its current wrapped position matches.

#### Scenario: Equal phase does not authorize a different future period
- **GIVEN** copied preparation and live feed with matching source phase but different fractional period bits
- **WHEN** the prepared contract or adoption is checked
- **THEN** the different domain is rejected
- **AND** ongoing productive history remains the effective owner

#### Scenario: Stems and native preparation cross a fractional seam
- **GIVEN** matching admitted full/stem PCM and a current accepted fractional domain
- **WHEN** a source transition or native worker continuation crosses loop wrap
- **THEN** both selections and copied worker feed use the same safe seam and source rate
- **AND** independent actual native output verifies continuation
