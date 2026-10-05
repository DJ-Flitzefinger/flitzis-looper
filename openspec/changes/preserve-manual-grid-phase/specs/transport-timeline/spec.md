## MODIFIED Requirements

### Requirement: Beatgrid And Downbeat Metadata Integrates With Pad Timing Metadata
The system SHALL publish bounded precomputed pad timing metadata that retains the same finite
signed source-grid origin and effective BPM used by Adjust Loop.

Analysis onset preparation SHALL use the first downbeat when finite/non-negative, otherwise the
first beat when finite/non-negative, otherwise zero, with existing onset normalization.
The control layer SHALL round onset at the loaded
source rate, add the persisted signed sample offset, and publish sufficient precision to retain
source-frame phase. Rust SHALL represent that origin in the loaded-buffer source-frame domain
without rejecting a finite negative origin or clipping it to the sample bounds. Invalid/non-finite
input SHALL be rejected or replaced by the documented neutral fallback before phase calculation.

Metadata preparation and beat-grid vectors SHALL remain outside the audio callback. Publication
SHALL NOT reset master time or seek/retrigger playing voices. An already selected, incomplete
one-time bootstrap MAY retry after relevant metadata becomes valid; ordinary other-pad metadata
SHALL NOT select a master reference or reanchor the transport.

#### Scenario: Corrected negative source origin is preserved
- **GIVEN** the source grid has an origin before source frame zero
- **WHEN** its bounded timing metadata reaches Rust
- **THEN** Rust retains the signed origin and derives source beat/bar phase from it
- **AND** no unchecked negative source read occurs

#### Scenario: Fallback analysis is shared with editor
- **GIVEN** a pad has no valid downbeat but has a valid first beat
- **WHEN** its editor/native timing metadata is prepared
- **THEN** both use that beat plus the same signed offset
- **AND** both use zero onset if neither a valid downbeat nor beat is available

#### Scenario: Other-pad metadata preserves master phase
- **GIVEN** the permanent master phase has been established
- **WHEN** another pad's BPM, grid origin or analysis metadata is published
- **THEN** the master musical position and output-frame clock are unchanged
- **AND** existing voices are not sought or retriggered

### Requirement: Rust-Owned Permanent Global Transport Timeline
The system SHALL maintain a permanent global transport timeline inside Rust audio-thread-owned
state with a monotonic output-frame clock and complete musical beat position.

The timeline SHALL initialize with the stream and advance by rendered output frames, including
silence. It SHALL store validated output rate/master BPM, a four-beat bar model and a musical
position reference. Python SHALL request changes through bounded messages and SHALL NOT own the
sample-frame clock. Only deliberate explicit transport/sync operations or completion of the
selected one-time session bootstrap SHALL establish a pad-derived master phase.

Normal pad start, stop, pause, retrigger, unload, loop wraps and stem changes SHALL NOT reset or
reselect the master clock. A successful start of an already selected pending bootstrap reference
MAY complete that bootstrap. Silence SHALL NOT rearm completed bootstrap.

#### Scenario: Silent callbacks retain continuous frame time
- **GIVEN** a callback begins at output frame F with no active pads
- **WHEN** it renders N frames
- **THEN** the permanent output clock is F + N
- **AND** no pad reference is selected or completed bootstrap rearmed

#### Scenario: Stop does not reselect the master
- **GIVEN** two pads are playing after master bootstrap
- **WHEN** either pad stops or unloads
- **THEN** the output clock and master musical progression remain continuous
- **AND** the remaining pad is not selected as a new master

### Requirement: Explicit Sync Can Anchor Transport Downbeat From A Playing Pad
The system SHALL retain a deliberate repeatable request to anchor master musical position from
the selected active pad's canonical source-grid phase.

Rust SHALL read only bounded audio-thread-owned pad id, output frame, BPM/rate, signed source origin
and current source playhead. A valid explicit request SHALL use the pad's complete source beat
position, from which beat/bar phase is derived. Missing/inactive pad or unavailable required
metadata SHALL leave transport unchanged. Successful explicit sync SHALL consume any remaining
one-time bootstrap opportunity. Automatic source-grid anchoring SHALL occur only through the
dedicated selected-reference bootstrap contract.

#### Scenario: Deliberate sync retains complete source beat
- **GIVEN** a selected active pad has valid grid/BPM metadata and is at source beat 18.5
- **WHEN** its explicit sync request succeeds
- **THEN** master musical position at that output frame is 18.5 beats
- **AND** its bar phase is 2.5 beats
- **AND** bootstrap cannot later overwrite that deliberate anchor

#### Scenario: Inactive explicit reference leaves transport unchanged
- **GIVEN** an existing master position and an inactive requested pad
- **WHEN** deliberate sync is requested
- **THEN** the output clock and musical position remain unchanged

### Requirement: Master BPM Is Owned And Validated In Rust
The system SHALL store validated master BPM in Rust and use the same accepted value for transport
grid timing and BPMLOCK tempo matching.

An accepted master-BPM update SHALL preserve the complete musical beat position at the current
output frame, including loop-cycle progression, and then progress using the new tempo. It SHALL
NOT reset frame time or seek, stop, restart or retrigger active voices. The derived beat/bar phase
SHALL therefore also be continuous. Invalid/non-finite/non-positive BPM SHALL preserve the
previous valid state.

BPMLOCK reference setup MAY issue the dedicated one-time bootstrap request. Mode toggles, pitch,
KEYLOCK, ordinary pad metadata and playback SHALL otherwise not reanchor phase.

#### Scenario: Master tempo change preserves beat count beyond one bar
- **GIVEN** master musical position is 18.5 beats at output frame F
- **WHEN** valid master BPM changes from 120 to 90 at F
- **THEN** musical position at F remains 18.5 beats
- **AND** later frames progress from 18.5 at 90 BPM
- **AND** active source playheads are not moved by the update

#### Scenario: Invalid master tempo preserves state
- **WHEN** a BPM update contains zero, negative, NaN or infinite BPM
- **THEN** the previous valid master BPM, frame time and musical reference remain unchanged

## ADDED Requirements

### Requirement: Source Phase Mapping Uses One Canonical Signed Grid
The system SHALL implement bounded Rust source/master musical mapping from canonical signed origin,
effective source BPM, loaded source rate and effective half-open source loop region.

Source beat position at frame S SHALL equal `(S - origin) / frames_per_beat`; beat and four-beat
bar phase SHALL use Euclidean modulo. Loop-start phase SHALL use the same formula. For a compatible
loop, master musical beat B SHALL be wrapped relative to the loop-start beat by the exact musical
loop tick length before conversion to source frames within the physical half-open loop.
Different supported tempos and loop lengths SHALL retain this shared musical phase rather than
assuming source frame zero or loop start is always the downbeat.
Phase arithmetic SHALL use the accepted native BPM parameter value promoted to sufficient working
precision; it SHALL NOT imply exact decimal representation beyond the native BPM precision.

The mapping SHALL remain a foundation/diagnostic until synchronized-launch activation. It SHALL
not seek voices, modify persisted markers or claim audible latency alignment. Full mix and
prepared stems SHALL share the same source-frame domain and eventual mapped address.

#### Scenario: Negative origin produces positive source phase
- **GIVEN** source rate 48,000 Hz, BPM 120 and signed origin -4,800 frames
- **WHEN** source phase at frame zero is calculated
- **THEN** source beat position and beat/bar phase are 0.2 beats

#### Scenario: Loop start retains its source phase
- **GIVEN** BPM 120 at 48,000 Hz, origin 12,000 and loop start 24,000
- **WHEN** loop-start phase is calculated
- **THEN** the phase is 0.5 beats
- **AND** loop start is not treated as a new downbeat

#### Scenario: Different compatible loop lengths share bar phase
- **GIVEN** two prepared loops of two and four whole bars at different valid source BPMs
- **WHEN** the same master musical beat is mapped into each source loop
- **THEN** both source positions represent the same beat/bar phase
- **AND** each wraps within its own half-open loop bounds

#### Scenario: Fractional BPM loop rounding does not accumulate drift
- **GIVEN** a compatible loop whose ideal musical duration is non-integral in source frames
- **WHEN** the same loop-cycle phase is mapped many master cycles later
- **THEN** source position is derived from the exact musical tick period
- **AND** integer-frame loop rounding does not accumulate timing error on successive wraps

### Requirement: Phase Mapping Reports Musical Loop Compatibility
The system SHALL report musical compatibility for valid prepared constant-tempo loops whose nearest
positive 1/64-note tick count divides 64 or is a multiple of 64, within one source frame of rounding error.

Compatibility SHALL compare source-frame length against that exact musical tick duration. Short
compatible loops SHALL repeat evenly within a four-beat master bar; whole-bar loops SHALL retain
bar phase over wraps. Invalid BPM/rate, empty/reversed loop or missing metadata SHALL report
unavailable mapping. Other valid physical lengths SHALL be marked incompatible and MAY use physical
loop wrapping as an internal fallback without a sustained synchronization claim. Tempo ratios
clipped outside the supported `0.5..2.0` range SHALL also not receive that claim. Compatibility SHALL
NOT reject otherwise valid ordinary playback, rewrite loop markers or imply variable-tempo support.

#### Scenario: Integer-frame rounded whole bar is supported
- **GIVEN** a valid fractional source BPM makes an exact four-beat duration non-integral in frames
- **AND** the stored loop length differs from a supported exact tick duration by at most one frame
- **WHEN** compatibility is checked
- **THEN** compatibility is true and mapping uses the stored half-open loop bounds

#### Scenario: Half-bar loop repeats within master bar
- **GIVEN** a valid two-beat loop
- **WHEN** musical compatibility and loop-relative mapping are calculated
- **THEN** the loop is compatible and repeats twice within each four-beat master bar
- **AND** its source bar labels do not imply a four-beat source duration

#### Scenario: Off-grid loop is not claimed to remain synchronized
- **GIVEN** a valid physical loop whose musical length neither divides nor spans a whole bar
- **WHEN** compatibility is checked
- **THEN** it is marked incompatible
- **AND** current loop-start playback and bounded physical wrapping remain available

### Requirement: Session Master Bootstrap Uses A Selected Reference Once
The system SHALL bootstrap master musical position at most once per stream from the reference
explicitly requested through existing selected-pad/BPMLOCK enable or restore setup.

The first accepted bootstrap request SHALL latch its pad id. Completion SHALL require that reference
to be active with valid BPM/rate and signed source-grid metadata. Rust SHALL anchor master musical
position at the current output frame to the reference's complete source beat without resetting
output time or seeking existing voices. Missing availability SHALL defer completion and retry only
for that latched reference when it successfully starts or relevant metadata becomes valid.
Bootstrap SHALL observe the same normalized source playhead as the next renderer read after loop
edits or explicit seeks; an obsolete voice timeline anchor SHALL NOT override that source position.

Further requests SHALL NOT replace a latched/completed reference. Unloading an incomplete reference
SHALL clear that pending reference while retaining the unused opportunity for a later request.
Successful bootstrap or explicit deliberate sync SHALL consume the opportunity until a new stream
is initialized. Silence, stop/restart, reference changes, ordinary metadata, wraps and stem masks
SHALL NOT rearm it. The bootstrap state SHALL be runtime-only, not persisted or a new UI control.

#### Scenario: First selected reference completes after metadata arrives
- **GIVEN** bootstrap was requested for selected pad 2 before its metadata was valid
- **AND** a later request names pad 3
- **WHEN** pad 2 is active and its valid signed grid/BPM metadata arrives
- **THEN** Rust completes bootstrap from pad 2's complete source beat
- **AND** pad 3 never replaces the latched reference
- **AND** existing source playheads remain unchanged

#### Scenario: Silence does not reopen completed bootstrap
- **GIVEN** bootstrap completed from pad 2 and all pads subsequently stopped
- **WHEN** a later reference request or new pad playback occurs
- **THEN** master musical progression continues
- **AND** bootstrap does not run again

#### Scenario: Unloaded incomplete reference can be selected again explicitly
- **GIVEN** bootstrap is pending for unavailable pad 2
- **WHEN** pad 2 is unloaded and a later bootstrap request names pad 3
- **THEN** the incomplete reference is cleared before accepting pad 3
- **AND** no other-pad playback silently selects a replacement

### Requirement: Accepted Pending Starts Retain Their Output Target
The system SHALL preserve an accepted scheduled start's absolute output target, stable sequence
and original optional captured input timestamp across later BPM, phase-bootstrap and metadata updates.

Updates SHALL NOT reround, reschedule, reorder or silently cancel an accepted event. New requests
SHALL use the updated master grid. During this foundation, execution SHALL use the latest accepted
effective loop start and BPM metadata under existing launch semantics; no phase catch-up SHALL be
activated. Existing stop/unload and missing-source safety behavior SHALL remain unchanged; this
foundation SHALL NOT introduce a new pending-start cancellation policy.

#### Scenario: Master BPM edit does not move a pending start
- **GIVEN** a start was accepted for absolute output frame F with captured timestamp T
- **WHEN** master BPM changes before F
- **THEN** the accepted target remains F and timestamp remains T
- **AND** new starts use the phase-continuous updated grid

#### Scenario: Loop edit changes source start without changing pending target
- **GIVEN** a start was accepted for output frame F
- **WHEN** the effective loop start changes before execution
- **THEN** the event executes at F using the latest accepted loop start
- **AND** its stable ordering and optional timestamp remain unchanged

### Requirement: Phase Foundations Preserve Current Launch Policy
The system SHALL retain current immediate or future-grid loop-start playback while signed phase
mapping and one-time bootstrap foundations are introduced.

The source mapping and captured-input nearest-target diagnostics SHALL NOT activate nearest-boundary
launches, middle-of-loop phase starts, late catch-up, pre-roll or latency compensation. Persisted loop
markers SHALL remain source editing intent. Subsequent activation SHALL require its own explicit
OpenSpec delta and audible device/DSP validation.

#### Scenario: Phase metadata does not activate source seeking
- **GIVEN** a valid signed origin and compatible loop whose start is not a downbeat
- **WHEN** the pad is triggered with current quantization enabled
- **THEN** the current future-boundary policy starts it at the effective loop beginning
- **AND** no diagnostic source mapping changes that start

### Requirement: Signed Phase Foundations Are Bounded And Realtime Safe
The system SHALL compute phase, bootstrap and pending-event decisions using bounded scalar Rust
state and existing preallocated scheduler/voice storage.

The callback SHALL NOT allocate, block, access Python/GIL or disk, log, run inference/analysis,
scan plugins, construct DSP state or iterate unbounded metadata. Invalid arithmetic SHALL report
unavailable phase without panic or unchecked source access.

#### Scenario: Unusable mapping is a bounded failure
- **WHEN** source metadata or musical arithmetic is unusable
- **THEN** Rust returns unavailable mapping and preserves current playback safety
- **AND** no analysis, allocation, blocking, logging, disk access or GIL work occurs
