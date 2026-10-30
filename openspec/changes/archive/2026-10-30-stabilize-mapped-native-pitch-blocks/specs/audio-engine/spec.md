## ADDED Requirements

### Requirement: Original mapped Native pitch follows actual input block heads
Original-owned mapped Native rendering SHALL qualify actual input and frame count
before mutating Native or adapter state. Each complete Native block SHALL use the
pitch at its first input frame, retained across caller partitions and map knots.
Key Lock SHALL derive h/r from actual mapped input; Key Lock off SHALL use h.
Preparation and live rendering MUST share this parameter law and the same Native
and FIFO ownership. Existing scalar rendering semantics MUST remain unchanged.

#### Scenario: A later caller completes a partial block across a map knot
- **WHEN** its input rate differs from the rate of the block's first queued frame
- **THEN** the old block retains its head pitch and the next block uses its own head pitch
- **AND** equivalent caller partitions produce the same actual Native stream

### Requirement: Mapped parameter qualification preserves custody and failure atomicity
Typed input SHALL require canonical original geometry, a readable single span and
valid pitch. Invalid descriptors, lengths, channels or FIFO capacity SHALL fail
before activation, lead insertion, pitch changes or input writes. Silent physical
birth history MUST use frozen birth pitch without source progression. Failed
rendering SHALL NOT publish successful input/history/output receipts. Pending
freshness and effective immutable source/OwnACK lifetime MUST remain distinct.
Descriptors MUST bind the current original actual-input endpoint, input serial
and non-reused preparation incarnation at both mint and consumption. Successful
nonzero rendering alone SHALL advance these input witnesses; silent prefix SHALL
advance no source address. Counter overflow and reset/reprepare ABA MUST fail closed.

#### Scenario: A copied checkpoint or malformed buffer is supplied
- **WHEN** original input qualification or complete adapter preconditions fail
- **THEN** Native and FIFO state remain unchanged and no successful receipt is produced

#### Scenario: A same-owner descriptor survives another render or reprepare
- **WHEN** its input serial, actual endpoint or preparation incarnation is stale
- **THEN** consumption rejects it before changing Native, FIFO, pitch or output state
- **AND** zero-frame calls cannot advance its witness or revive a stale descriptor

### Requirement: Internal variable pitch evidence does not open public start policy
The original-owned future-birth experiment SHALL permit Key Lock and zero-semitone
pitch while retaining its other source, permit, timing, history and coverage guards.
This component MUST NOT change public mapped birth/adoption policy or ordinary
native_required. Direct C-ABI comparison MUST retain original raw-prefix and
actual/logical coordinate evidence. Block-constant pitch MUST NOT be reported as
sample-exact tonal, phase, musical, B2, default-cutover or Human acceptance.

#### Scenario: A map starts at unity pitch and later changes rate
- **WHEN** the internal original-owned experiment prepares and continues that stream
- **THEN** it retains its Native/FIFO owner across unity and nonunity spans
- **AND** public integration remains separately gated on Native ownership from birth
