## ADDED Requirements

### Requirement: Optional automatic BPM range

The system SHALL provide a persistent configurable automatic BPM range, disabled
by default with dormant50..170 BPM bounds. Inclusive bounds SHALL be finite,
within30..300 and strictly ordered. Each future automatic-analysis request SHALL
capture one validated option before queuing; settings edits SHALL NOT reinterpret
existing PAD analysis, pending requests, manual/Tap or restored/copied metadata.

#### Scenario: Broad default or already in-range primary
- **WHEN** range is disabled or a successful primary value is inside its captured range
- **THEN** the original scalar and method SHALL remain exact
- **AND** this option SHALL NOT start additional QM work

#### Scenario: Independently supported half/double alternative
- **WHEN** an original successful NN v4/v5 value is outside the captured range
- **AND** unchanged own-QM analysis independently admits an inside value
- **AND** published periods satisfy symmetric half/double agreement within5ms over the complete common extent
- **THEN** the system MAY select the precise QM scalar with explicit schema7 source/math/range provenance
- **AND** raw NN observations, origin, master and existing publication owners SHALL remain unchanged

#### Scenario: No supported candidate or optional-stage failure
- **WHEN** an enabled request has no qualified inside scalar or its optional stage fails
- **THEN** that request SHALL return no automatic BPM with a clear range diagnostic
- **AND** independent key outcomes and existing previous-positive retention SHALL remain valid
- **AND** an outside primary SHALL NOT leak through failure, panic or late serialization fallback
- **AND** cancellation or source supersession SHALL retain whole-request fences

#### Scenario: Restore and scientific fallback
- **WHEN** saved metadata is restored/copied or analysis uses an unconfigured scientific backend
- **THEN** restore/Copy SHALL validate saved evidence without DSP or range reinterpretation
- **AND** new scientific automatic output outside an enabled snapshot SHALL be filtered without invented NN provenance
