## MODIFIED Requirements

### Requirement: Replace a dry mapped master epoch without changing current phase
At an accepted dry mapped global rate boundary E, the system SHALL preserve the
shared master beat/bar phase, actual output-frame clock and each voice's exact
canonical source position and creative map phase. Every fallible projection,
coverage, source/OwnACK and complete-cohort check SHALL precede effective stores.
The system SHALL retain original marker/map ownership, raw source anchor/inverse,
retained scalar witness, launch target/reference and historical read epochs.

For an ordinary anchored voice whose Hold/resume translated its read address,
the replacement SHALL use an exactly composed ordinary basis satisfying
`b' = b + M_new(E) - M_old(E - address_shift)`, after qualifying actual master
continuity and the new horizon. Subsequent reads SHALL follow the new master
increment from E without changing the old exact latent phase. Composition MUST
NOT round the basis, replace its original inverse or recover merely from the
rounded current source pair. Original source axes and admission limits remain.

This ordinary continuation SHALL include Compact, declared/physical General and
FullFraction ordinary families through their qualified original preparation.
Actual CommonPhase escape SHALL retain its separate specified RN-pair recovery;
an already ordinary FullFraction clock MUST NOT be treated as phase escape.
New basis state SHALL remain bounded through repeated cutovers, carry no owning
source pointer and supply no Native-history or grid-phase acknowledgement.

#### Scenario: Hold and repeated master changes preserve an exact latent phase
- **WHEN** a qualified dry ordinary voice resumes from Hold and accepts a master change at E
- **THEN** its complete source pair and original creative phase remain unchanged at E
- **AND** later coordinates follow the new piecewise master through knots and wraps independently of callback partitions
- **AND** repeated changes remain exact even when their basis cannot be represented by one binary64 value
- **AND** one invalid participant leaves the entire prior cohort, clock and ownership unchanged
