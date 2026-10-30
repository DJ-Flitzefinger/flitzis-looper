## ADDED Requirements

### Requirement: Inferred ordinal fallback preserves accepted scalar results
The system SHALL return every successful v4 estimate and its serialized evidence
unchanged. Only valid unavailable v4 input MAY enter a bounded inferred-ordinal
fallback. Raw detector events and source/PCM/model/frontend/device identities SHALL
remain unchanged. Inferred evidence SHALL grant no playback-map, phase, source,
clock, launch or timing authority.

#### Scenario: Previously successful analysis
- **WHEN** complete v4 analysis has a supported scalar BPM
- **THEN** the system SHALL retain its exact value, policy, counterprobe and serialized evidence
- **AND** it SHALL NOT run the new fallback or add an empty evidence field

#### Scenario: Missing or extra events
- **WHEN** v4 cannot estimate BPM but unchanged observations support a coherent inferred beat sequence
- **THEN** the fallback MAY return a scalar estimate with explicit original-index to integer-ordinal assignments
- **AND** duplicate selection, rounding and fitted-line consistency SHALL be deterministic and replayable
- **AND** input observations SHALL NOT be rewritten or renumbered

### Requirement: Scalar fallback exposes distributed evidence and conflicts
The fallback SHALL evaluate fixed complete-source and canonical-section spans,
including unsupported holes and sustained conflicting sections. Ranking scores
alone SHALL NOT establish acceptance. Same-period phase competitors, half/double
and nonharmonic alternatives SHALL remain visible. Credible candidate truncation
or unresolved coherent competition SHALL NOT produce a strong unqualified result.

#### Scenario: Coherent body with conflicting edges
- **WHEN** a body fit agrees locally but an edge contains sustained coherent incompatible phase or tempo
- **THEN** the system SHALL retain the conflict instead of hiding it as intro/outro exclusion
- **AND** it SHALL NOT claim a supported constant fit from two convenient passing windows

#### Scenario: Long and short source evidence
- **WHEN** duration permits separated10second observations
- **THEN** both probes SHALL use actual retained support and one common long-span reference
- **AND** all fixed section outcomes SHALL remain available in the evidence
- **AND** a genuinely short original SHALL use explicit beat-count/span qualification rather than long-track thresholds
- **AND** a short observed island in a long original SHALL NOT become a short-source result

#### Scenario: Compatible local detection density
- **WHEN** local candidates describe nested predicted pulse lattices within the existing finer tolerance over the whole fixed section
- **THEN** missing alternate detections or finer subdivisions SHALL NOT alone be labeled a local tempo-change contradiction
- **AND** one common integer stride and winding SHALL cover both section endpoints
- **AND** genuinely shifted or drifting local models, global competition and common-reference probes SHALL retain their existing guards

#### Scenario: Integer counterhypothesis
- **WHEN** an integer candidate is tested against the fractional fit
- **THEN** both SHALL use exactly the same assigned observations, mask and global pivot
- **AND** neither SHALL independently refit phase or alter assignments to improve its comparison
- **AND** finite-resolution integer preference SHALL NOT be described as proof of exact physical tempo

### Requirement: Inferred evidence remains bounded control metadata
The system SHALL admit new search, assignment and serialization memory before
allocation within the existing512MiB preparation limit. Fallback resource refusal
SHALL remain explicit and SHALL NOT invalidate an existing successful v4 result.
Heavy fitting and validation SHALL remain outside realtime. Current source/request,
cancellation, OwnACK and retirement guards SHALL remain authoritative.

#### Scenario: Memory or search saturation
- **WHEN** new fallback memory cannot be admitted or credible candidates exceed the search capacity
- **THEN** the result SHALL expose the limitation without increasing512MiB or silently dropping competition
- **AND** it SHALL NOT fabricate a successful BPM, phase or source receipt

#### Scenario: Superseded analysis during search
- **WHEN** the current request is canceled during candidate generation, fitting or source-sized scans
- **THEN** the first observed cancellation SHALL remain latched for that operation
- **AND** supersession SHALL remain distinct from mathematical or allocation refusal
- **AND** it SHALL NOT publish a partial successful or unavailable method as the current source result

#### Scenario: Transient proposals and changed winner
- **WHEN** bounded candidate generation produces more transient original/refined rows than require full fitting
- **THEN** proposal fuel, complete recovery-family selection and exact full-fit admission SHALL remain separately bounded
- **AND** filtering SHALL NOT omit a failed refinement's original witness or silently truncate later generation
- **AND** every selected winner change SHALL trigger reconsideration of earlier winner-dependent skips
- **AND** only complete generation against an unchanged winner SHALL establish search completion
- **AND** a prior actual fuel, range, family or required-fit refusal SHALL remain visible

#### Scenario: Persisted method and current guidance
- **WHEN** version5 evidence is loaded, copied or restored
- **THEN** the existing method DTO SHALL replay its declared mapping and metrics through an explicit version5 branch
- **AND** version1–4 validation, BPM equality and execution receipts SHALL retain their existing guards
- **AND** UI/Copy validation SHALL NOT rerun the full discovery search or neural inference
- **AND** the BPM controls SHALL show understandable current automatic uncertainty and existing analysis errors
- **AND** busy, manual or Tap state SHALL NOT be mislabeled by historical automatic evidence
