## ADDED Requirements

### Requirement: A separate canonical pitch candidate preserves timing and evidence
The private Signalsmith experiment SHALL consume the existing complete canonical
PCM at q1/p2 under both declared caller patterns and SHALL use its own unity
control. It MUST use one frozen default preset, fixed seed, actual pre-signal
latency queries and a source-derived predeclared output coordinate. It MUST NOT
fit delay, alter gain, crop Raw, seek away the onset or change the established
timing, original-energy, peak-retention and complete-tail criteria.

#### Scenario: A pitch candidate misses the original attack
- **WHEN** any required kick case or own unity control fails a fixed criterion
- **THEN** actual positive and negative measurements are retained separately
- **AND** the candidate is rejected for matrix expansion or productive adoption

#### Scenario: A finite API offers no terminal availability query
- **WHEN** the experiment supplies the complete input and the declared ending protocol
- **THEN** every returned streaming and flush sample is retained with its actual extent
- **AND** a bounded ending capture is not described as proof of internal tail exhaustion
- **AND** unavailable complete windows cannot pass and no silent padding is fabricated

### Requirement: The experimental native owner remains isolated and bounded
The experiment SHALL pin actual upstream revisions and licenses and SHALL keep
the C++ build behind an explicit nondefault Cargo feature. Its opaque owner MUST
be exclusive and non-Send/non-Sync, validate FFI scalars and slice extents, contain
C++ exceptions, retain no borrowed PCM pointers and destroy the handle off
realtime. Input, process, output, ending and simultaneously held owned sample
arrays MUST have checked finite bounds. Opaque library RAM and allocator metadata
MUST NOT be claimed as covered by sample-array bounds.

#### Scenario: An ordinary installation builds without the probe feature
- **WHEN** the normal locked Release or Portable pipeline builds the application
- **THEN** the experimental C++ code and optional compiler helper are not built
- **AND** existing public APIs, Native dependency and adoption guards remain unchanged

#### Scenario: A caller supplies invalid or excessive input
- **WHEN** a scalar, channel extent, finite-sample check or resource bound fails
- **THEN** the request fails before the corresponding FFI call or buffer growth
- **AND** no partial result is represented as qualified audio
