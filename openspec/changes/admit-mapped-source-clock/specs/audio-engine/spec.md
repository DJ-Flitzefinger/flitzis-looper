## ADDED Requirements

### Requirement: Mapped playback retains original qualified ownership
Mapped launch/preparation SHALL require the original Native-qualified map and artifact bound to the admitted source, content, loop and current request. A Python map object, serialized receipt, hash or equal geometry alone MUST NOT authorize audio. The existing launch and preparation claims SHALL atomically validate the captured ownership and retain old effective audio on stale or unavailable work.

#### Scenario: Stale map or source cannot claim
- **WHEN** a captured mapped request becomes stale through source, map intent, loop or placement changes before claim
- **THEN** it SHALL NOT adopt its stale trajectory or invent an ACK
- **AND** actual preceding effective owners SHALL remain valid until their normal retirement

### Requirement: Mapped chronology and lifetime use existing paths
The existing fixed copied source clock, actual-read receipts and checkpoints SHALL carry exact mapped trajectory/endpoint identities without retaining mutable voice state. Voice and preparation work SHALL separately retain original owning pins. Genuine read chronology and F-to-E projection SHALL use the same canonical trajectory. Displaced final owners SHALL retire off realtime through the existing retirement mechanism, with pressure preserving required ownership.

#### Scenario: Read history survives copying without granting authority
- **WHEN** mapped actual reads are captured for source preparation or catchup
- **THEN** their copied coordinates SHALL match the actual mapped reads and canonical next endpoint
- **AND** historical replay SHALL require genuine history guards and separately retained original source/reader pins, while future canonical projection SHALL additionally require the original qualified owning map
- **AND** a copied marker alone SHALL grant no preparation or publication permission

#### Scenario: Unsupported render tuple remains truthful
- **WHEN** a mapped request requires a Native pitch or adoption tuple not yet integrated
- **THEN** the engine SHALL preserve preceding effective audio and report the precise unavailable tuple
- **AND** it SHALL NOT silently toggle Key Lock, substitute scalar playback or claim successful SYNC
- **AND** Key Lock OFF nonzero pitch SHALL remain an explicit integration obligation rather than a mathematical incompatibility claim

#### Scenario: Initial mapped envelope preserves effective controls
- **GIVEN** the initial admitted dry mapped envelope with Key Lock off, zero total pitch shift, speed one, BPM Lock on, a valid stable master epoch and an original execution frame equal to its target
- **WHEN** a requested seek, pause, stem transaction, pitch processing, master replacement or delayed launch lacks its mapped continuation proof
- **THEN** it SHALL refuse before changing the effective voice or publishing a successful operation effect
- **AND** source replacement and explicit stop SHALL retain the established source and final-owner retirement rules
- **AND** all these transitions SHALL remain required integration work for the complete SYNC feature

#### Scenario: Omitted map rows recover after source replacement
- **WHEN** an existing runtime publication omits optional map rows after unload, replacement or original-job cancellation
- **THEN** it SHALL retain prior SYNC intent and revision while discarding obsolete Native selection
- **AND** it SHALL publish other valid PAD updates without allowing stale mapped launch or a requested-SYNC scalar fallback
- **AND** an explicitly supplied stale original delivery SHALL still reject atomically

#### Scenario: Equal global rate preserves the actual epoch
- **WHEN** the public global-rate path repeats the effective rate and master period while a mapped launch is pending or active
- **THEN** it SHALL preserve the actual master epoch, the original launch target and canonical source reads
- **AND** it SHALL NOT reanchor unchanged musical timing at the current callback frame

#### Scenario: Retirement reserves actual additional owners
- **WHEN** a callback operation may retire mapped and scalar voices
- **THEN** admission SHALL cover every actual retained mapped launch owner in addition to established scalar reservations
- **AND** scalar-only operations SHALL retain their established retirement thresholds and retry behavior
