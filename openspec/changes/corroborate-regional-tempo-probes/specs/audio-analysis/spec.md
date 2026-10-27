## ADDED Requirements

### Requirement: Version Five Counterprobes May Use A Strict Corroborated Thirds Hull
Policy five SHALL retain existing preferred common-region paths. Only if none is
eligible SHALL it consider one continuous hull of all published regions. Those
regions SHALL consist of two or three unique exact canonical source thirds,
include the exact selected region and a strictly extending witness, and all meet
`abs(region.bpm / selected.bpm - 1) <= 0.00025`. Any conflicting, duplicate or
non-third region SHALL refuse this fallback without filtering or smaller searches.

The hull SHALL use every intervening original event, original detector ordinal,
one candidate-neutral strict fit and one shared mask/pivot. Existing five-iteration
robust fitting, minimum 24 observations/30 seconds, exclusion/coverage/residual
limits and candidate checks SHALL remain in force. The exact selected BPM SHALL
remain the fractional candidate; the hull free fit SHALL NOT replace it.

#### Scenario: Compatible thirds precede an irregular outro
- **WHEN** strict published thirds support the selected fractional tempo but the preferred middle and complete run do not qualify
- **THEN** one strict continuous hull SHALL be evaluated with all intervening original events
- **AND** integer and fractional results SHALL report the actually supported source interval
- **AND** success SHALL NOT certify excluded material, beat labels, whole-song constancy or musical alignment

#### Scenario: A phase step or missing beat lies between the witnesses
- **WHEN** independently fitted thirds disagree in phase or raw event count through their common interval
- **THEN** the unchanged common fit and shared candidate checks SHALL retain that discrepancy
- **AND** local rephasing, renumbering or a convenient narrower fallback SHALL NOT manufacture support

### Requirement: Versioned Hull Provenance Preserves Cached Performer Intent
The App SHALL preserve versions one through four with their original validation
and accept the new provenance only under version five. It SHALL independently
reconstruct the complete qualifying canonical-third set, preferred-middle
availability, each strict witness fit and the exact candidate-neutral hull
mask/free fit before accepting successful probe metrics. Omitted qualifying
regions SHALL NOT manufacture fallback eligibility or reduce the support span.
Forged bounds, masks, support membership or metrics SHALL be rejected.

Valid cached v4 estimates SHALL NOT trigger fresh neural work merely because their
counterprobe was inconclusive. Manual/TAP, accepted source timing, source identity,
raw arrays, maps, loop markers and grid offsets SHALL retain their existing priority
and ownership. The common object SHALL remain included in metadata byte accounting
without increasing the published region limit above five.

#### Scenario: A saved v4 inconclusive result is reopened
- **WHEN** the source is restored with valid v4 tempo evidence and performer overrides
- **THEN** the old evidence SHALL remain readable under v4 rules without automatic neural refresh
- **AND** a strict Copy roundtrip SHALL preserve its original estimate and overrides

#### Scenario: A copied v5 record substitutes a convenient mask
- **WHEN** its common fit omits different original events than the strict candidate-neutral discovery
- **THEN** validation SHALL reject the forged provenance even if recomputed candidate metrics would pass
