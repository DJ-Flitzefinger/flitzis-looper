## ADDED Requirements

### Requirement: Adapt a qualified all-dry mapped cohort through the existing global rate claim
A changed global speed/master-period tuple with active mapped voices MAY apply through the existing global rate coordinator only when every active participant is admitted to the supported dry cohort. It SHALL capture and validate every participant's actual source/reader permit, placement, generation, continuity and effective tuple, including each mapped voice's original qualified geometry. It SHALL NOT replace those owners with a later selected-map request. It SHALL require BPM Lock on or that actual mapped voice's frozen explicit global SYNC ON choice under add-global-tempo-sync-policy, zero mapped pitch, Key Lock off and a valid actual master period with supported whole-map local rates. The explicit choice SHALL NOT create a master tempo or overwrite requested BPM Lock. Legacy global-null policy SHALL retain the BPM Lock requirement. Active Native ownership, retained Native input, required Native history, paused mapped state and unsupported transitions SHALL refuse the entire effect without partial mutation.

#### Scenario: Coupled fader changes a supported mixed dry cohort
- **WHEN** the existing public speed/master-period operation reaches a settled callback boundary with qualified mapped and scalar dry participants
- **THEN** the system SHALL preflight every mapped replacement and source participant before claiming the effect
- **AND** it SHALL install the global tuple, transport and mapped playback replacements through the existing atomic claim at that one actual boundary
- **AND** it SHALL retain original voice generations, map/source owners, launch targets and preceding content

#### Scenario: A participant cannot join the dry effect
- **WHEN** a participant is stale, has unavailable reader coverage, requires an unsupported Native/transition path, or any whole-map local rate is out of range
- **THEN** the system SHALL refuse the complete effect with a truthful unavailable result
- **AND** preceding audio, the effective global tuple, master timing and every participant SHALL remain intact
- **AND** it SHALL NOT clip individual map segments or silently select scalar playback

### Requirement: Preserve settled-boundary preparation and truthful rate state
Dry mapped rate capture SHALL use its actual settled execution boundary without the Native preparation delay or positive copied-map history projection. Temporarily unsettled command/parameter drains SHALL retain Preparing until a settled boundary is available. Requested, pending and effective observations SHALL remain separate. Superseded, cancelled and stale captures SHALL NOT partially apply. Generic live-pitch, Key-Lock, source-history and delayed mapped paths SHALL retain their existing admission gates.

#### Scenario: Parameter drains are temporarily unpaired
- **WHEN** a dry mapped rate request arrives before its command and parameter fences are settled
- **THEN** it SHALL remain preparing with the old effective audio and timing
- **AND** the first valid settled boundary SHALL capture and claim the effect without inventing a worker delay or bypassing freshness checks

### Requirement: Admit ordinary dry mapped starts after qualified nonunity rate changes
An ordinary mapped start or retrigger at nonunity stored speed MAY use the actual qualified master epoch when Key Lock is off, total pitch is zero, the frozen matching-enable choice above is satisfied and execution equals the original target. The canonical map SHALL use the actual master beat once, without a second speed multiplier. Native pitch, delayed execution, ordinary mapped Pause/Resume, mapped Seek and unsupported trajectories SHALL remain gated. Qualified dry waveform-hold continuation MAY use only the bounded own-ACK Stage A contract of resume-dry-mapped-playback; that exception SHALL NOT admit a rate change while a mapped voice is paused.

#### Scenario: Stop and retrigger after a supported coupled fader change
- **WHEN** the user starts or retriggers the mapped pad after a qualified nonunity speed/master-period effect
- **THEN** the existing ordinary original-source claim SHALL begin at its actual frozen target using the current qualified epoch
- **AND** it SHALL preserve the complete selected beginning and actual mapped feedback without enabling any unproven Native path
