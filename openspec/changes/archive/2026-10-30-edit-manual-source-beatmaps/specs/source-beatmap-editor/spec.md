## ADDED Requirements

### Requirement: Manual Map Creation Uses Actual Source Custody
The system MUST offer explicit manual quarter-note 4/4 map creation in the existing
Loop Editor from captured scalar source origin and period. The original bounded
Native source-map job SHALL derive and verify actual decoder identity, preserve
source/content/request ownership, and publish through the existing durable selection.
Manual provenance SHALL NOT fabricate raw detections, inference or musical acceptance.

#### Scenario: A performer explicitly creates a manual map
- **WHEN** valid current source scalar timing is accepted for manual 4/4 construction
- **THEN** a freshly source-qualified immutable map becomes selected only after Config commitment
- **AND** scalar timing, master time, other PADs and existing source markers remain unchanged

### Requirement: Editor Coordinates Come From One Qualified Map
The system MUST use the same current source-qualified immutable Native map revision
for visible grid, snap, explicit bar endpoints and local source BPM. Native viewport
queries SHALL be bounded to at most 2048 points with explicit refusal; Python SHALL
NOT parse full map geometry or implement another interpolation. Unsupported coverage
and meter SHALL remain explicit. Local source BPM MUST follow the retained or live
source playhead, including seek/wrap/pause, and remain distinct from master target.
The UI SHALL identify map editing as editor-only and audio as scalar, including after
stop/retrigger. Pending or stale revisions MUST NOT be advertised as live audio.

#### Scenario: A nonuniform source segment is displayed and snapped
- **WHEN** its supported beat is drawn and explicitly selected as a loop marker
- **THEN** both use the same inverse source coordinate followed by final loaded-frame rounding
- **AND** local source BPM comes from that same revision rather than average or master BPM

### Requirement: Explicit Map Loop Edits Preserve Physical Marker Ownership
Variable-map snap MUST use the existing Native later-tie subdivision policy and final
loaded-frame rounding. Scalar snapping SHALL retain its existing nearest-even policy.
Explicit map bar operations MUST require supported quarter-note 4/4 throughout the
whole interval and compute the endpoint as S(B(start)+4*bars). They SHALL materialize
physical source markers through the existing source-generation-guarded loop owner.
Map creation, correction, clearing or restore alone MUST NOT resnap or recompute
existing markers. Unsupported operations MUST refuse without partial marker mutation.

#### Scenario: A new map revision changes local tempo
- **WHEN** it becomes the selected editor revision without an explicit loop edit
- **THEN** both persisted and published source markers remain unchanged
- **AND** a later explicit supported bar edit uses the new revision as one atomic marker intent

### Requirement: Sample Correction And Translation Produce Undoable Revisions
The system MUST support source-sample positioning of an accepted musical beat and a
separate signed whole-map translation. Corrections SHALL validate strict monotonicity,
retain raw detections and uncertainty, record exact sample index or signed offset,
loaded rate, evidence, parent revision and inverse operation, and produce a new
immutable revision. Translation SHALL preserve musical intervals and never reanchor
master or other PADs. Undo SHALL apply the captured inverse as a new validated revision.
Beat-count/downbeat repair and creative phase SHALL remain distinct operations.

#### Scenario: A copied map receives a one-sample beat correction
- **WHEN** the correction finishes for its still-current content, source and intent
- **THEN** only that content selects the new persisted revision, retaining raw evidence
- **AND** the original copy and source markers stay unchanged
- **AND** undo and fresh model-free restore retain the corresponding exact geometry

### Requirement: Failed Or Stale Editor Work Preserves The Predecessor
Manual creation/correction MUST use the original Native job/private delivery and
existing compact selection/Config/assignment/retirement owners. Stale content, source,
intent or captured map revision SHALL refuse. Failed save, invalid correction, queue
pressure or cancellation MUST preserve the predecessor and retire only proven unused
candidates. No new worker, scheduler, map authority or unsafe deletion SHALL be added.

#### Scenario: An edit races source replacement or a failed Config write
- **WHEN** its worker completion is stale or its durable write fails
- **THEN** it cannot replace the current selected map or move source markers
- **AND** the original job and exact candidate asset proof retire through existing owners
