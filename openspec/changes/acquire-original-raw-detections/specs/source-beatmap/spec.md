## ADDED Requirements

### Requirement: Acquire unverified detections from the complete original source
The compatible legacy-QM acquisition on the existing Native SourceBeatmap job authority SHALL acquire the complete admitted original fullmix source with its original source generation, descriptor, loaded rate, extent and frame zero. It SHALL preserve leading silence, the partial tail and source data outside finite residency, independently of selected stems. It SHALL record the actual streamed mono rule/hash, actual converted analyzer input/hash/rate/extent/revision, complete detector configuration, original binary64 ODF frames, actual integer hop and original detected-downbeat index associations. It SHALL NOT widen rounded published timestamps, crop input or infer verified quarter counts, meter, coverage or calibrated uncertainty from raw detections.

The separately specified selected Beat This acquisition SHALL use the same original-source and job authority under `acquire-original-beat-this-detections`. Its distinct complete capture SHALL retain actual staged worker-input provenance, all four prediction arrays and pinned model/frontend provenance without fabricating QM ODF or internal frontend PCM hashes. This additive backend SHALL NOT weaken the legacy-QM requirements above or grant musical qualification.

#### Scenario: A finite resident window omits audible original content
- **WHEN** raw acquisition is requested from an admitted original whose resident view covers only part of its source
- **THEN** the existing complete-source reader SHALL analyze the complete original signal including material before and after that view
- **AND** raw results and input provenance SHALL describe that executed full signal and retain detections outside the physical source domain with truthful classification

### Requirement: Raw completion cannot authorize an accepted map
Raw acquisition SHALL use a typed result and dedicated private-owner completion distinct from accepted SourceBeatmap completion. Only the original current Native job/delivery SHALL resolve an opaque raw candidate. Raw begin, completion, inspection, cancellation and disposal SHALL NOT change accepted intent, its revision, durable references, effective geometry/delivery, source request epochs, timing, SYNC or audio. A raw candidate SHALL expose no map, accepted receipt, restore or durable ACK operation. Replacing weak latest-job addressing SHALL NOT revoke an older still-effective accepted owner.

#### Scenario: Accepted playback continues while raw analysis completes
- **WHEN** a pad with an effective accepted map acquires, inspects or discards a raw candidate
- **THEN** its accepted intent identity, original delivery, geometry and playback SHALL remain unchanged
- **AND** forged public completion fields, another job or an equal-content candidate SHALL NOT resolve or qualify that accepted map

### Requirement: Bound raw acquisition and retain actual ownership until retirement
Raw acquisition SHALL admit checked simultaneous stage peaks within 512 MiB before large allocations, including retained view/PCM, stream scratch/mono, converter capacities/FFT workspace, f64 analyzer, QM working storage, raw arrays and serialization/reparse. It SHALL bound raw encoding to 16 MiB and each raw/index array to 250,000 entries. Oversize/overflow SHALL refuse without cropping or weakening guards. All heavy work and final reader release SHALL remain off realtime. One raw review lease SHALL cover original capture, actual job cleanup and every raw artifact reader, with no owner cycle. Cancellation SHALL prevent stale publication while retaining resources until synchronous QM actually returns.

#### Scenario: Cancellation arrives inside an uninterruptible detector run
- **WHEN** the original raw job is cancelled while QM is running
- **THEN** it SHALL retain its source and lane admissions until actual return and cleanup
- **AND** no cancelled candidate SHALL publish usable rows
- **AND** job terminal, final raw-reader retirement and exact-birth disk cleanup SHALL remain distinct observations

### Requirement: Inspect only bounded current raw evidence
Opaque candidate info and inspection SHALL revalidate original Native source/content/placement/cancellation before and after bounded work. A query SHALL accept an event start index from zero through event_count and a maximum event count from 1 through 128. At the exclusive end it SHALL return a valid empty page. Placement-pending, stale or cancelled queries SHALL expose no current event rows. Inspection SHALL retain raw indices, original ODF frames, source-relative seconds, observed downbeat flags and physical-domain classification without assigning musical labels.

#### Scenario: An old candidate crosses a source or placement transition
- **WHEN** a raw candidate is queried after its source is replaced or while its original placement is pending
- **THEN** it SHALL report stale or placement-pending status and no current rows
- **AND** a completed Move or Swap SHALL follow its original content only when the original Native admission remains current
