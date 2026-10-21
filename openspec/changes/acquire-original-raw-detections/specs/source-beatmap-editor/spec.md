## ADDED Requirements

### Requirement: Review raw detections separately from accepted source maps
The English Source Map editor SHALL expose Analyze original full mix, separate queued/running/cancel-retiring/failed/unreviewed state, bounded detection inspection and explicit discard/reacquire through the existing loader selection owner. It SHALL state that raw quarter-note count, meter and uncertainty are unverified. Raw presence SHALL NOT change accepted selection, waveform grids, snapping, bar application, loop boundaries, scalar BPM, master timing, SYNC or playback. Accepted edits MAY continue beside an unreviewed immutable candidate without promoting or modifying it.

#### Scenario: An unreviewed result is displayed beside an accepted map
- **WHEN** raw analysis completes while an accepted map remains selected
- **THEN** the editor SHALL show its raw state and at most 128 Native-returned events per page separately from accepted geometry
- **AND** event indices and detected-downbeat flags SHALL remain detector observations without quarter, bar, meter or musical-coverage authority

### Requirement: Fence a single transient raw review attempt
The existing controller SHALL own one transient raw review slot with original job/delivery/candidate, content/source identity and a distinct monotonic attempt token. Analyze SHALL refuse while that content has accepted preparation or another raw review owns the slot. It SHALL NOT call accepted-map admission, mutate persistent ProjectState or emit a durable changed callback. Every action/page query SHALL fence source/content and attempt as well as Native currentness. Copy, Bank and restart SHALL NOT inherit raw state; Move/Swap SHALL follow the original content. Removed content and shutdown SHALL cancel only their original work while retaining retiring handles through actual retirement.

#### Scenario: An identical reanalysis follows discard
- **WHEN** a new attempt produces identical raw bytes after the previous candidate is discarded
- **THEN** an old press/release or page action SHALL refuse through its old attempt token
- **AND** equal data revision SHALL NOT redirect that gesture to the new attempt

#### Scenario: Discard waits for actual temporary and raw-reader release
- **WHEN** discard or source removal requests cancellation
- **THEN** the controller SHALL retain cancel-retiring until the original job reports terminal and its own raw_review_retired receipt is true
- **AND** it SHALL NOT claim a free review slot from cancellation_requested, job terminal alone or a pending exact-birth disk deletion

#### Scenario: Placement becomes pending after the first loader lookup
- **WHEN** the original completion passes its first loader lookup and a placement publication makes the later original Native resolution or current source-generation capture temporarily unreadable
- **THEN** the loader SHALL retain the same original event and private delivery for a later bounded poll without cancelling the job or changing accepted state
- **AND** a genuinely different source generation SHALL still cancel only that original raw attempt, while unrelated Native errors SHALL remain visible
