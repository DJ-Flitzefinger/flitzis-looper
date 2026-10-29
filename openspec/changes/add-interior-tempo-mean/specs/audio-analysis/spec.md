## ADDED Requirements

### Requirement: Interior mean is the final automatic tempo fallback

Successful precise results SHALL precede an interior mean. After completed,
unlimited neural section-conflict evidence, the existing source-bound QM owner
SHALL first attempt its unchanged precise estimate once. A supported QM result
SHALL take priority, including final range rejection without falling through to mean.

Only completed section-conflict failures of both estimators MAY produce a mean.
The mean SHALL use fresh original-index local fits in the three fixed source-time
windows [D/5,2D/5), [2D/5,3D/5), [3D/5,4D/5), where D is the owned loaded duration.
All six fits SHALL pass existing local support, residual and outlier checks.
Exactly one common QM BPM factor among0.5,1,2 SHALL make the maximum/minimum of
the six local periods at most1.01. The selected value SHALL be the unrounded neural
elapsed-window-time weighted arithmetic mean. Range SHALL filter that final value.

Resource, search, cancellation, invalid, insufficient and pure ambiguity refusals
SHALL NOT be reclassified as supported means. New schema10 SHALL retain original
strict evidence and distinguish independently precise QM from a mean. A mean SHALL
NOT claim full-source constant phase, unique musical meter or realtime tempo following.

#### Scenario: Fluctuating interior supports an automatic mean
- **GIVEN** both precise methods complete with section conflicts
- **AND** all six fixed interior fits satisfy the common pulse-family rule
- **WHEN** the final fallback completes within its source and resource guards
- **THEN** the automatic result is explicitly marked as a mean estimate
- **AND** the existing ordinary scalar publication path receives that value

#### Scenario: A precise result cannot be downgraded
- **GIVEN** a PAD has a supported precise result for its current unchanged source
- **WHEN** a newer analysis returns a mean for that same source
- **THEN** its earlier BPM, grid and method remain together without another tempo write
- **AND** an independent matching key result may still complete

#### Scenario: Failed work preserves current intent
- **WHEN** a fallback fails, becomes stale or is cancelled
- **THEN** current source, manual/Tap intent and valid prior results remain protected
- **AND** no partial new BPM/method tuple is published
