## ADDED Requirements

### Requirement: Prepared global groups settle capacity failures with intact custody
A prepared GLOBAL START SHALL reject as a whole when playback feedback, voice or complete-effect retirement capacity is insufficient, once at least one existing retirement slot can own the entire original group. Zero retirement capacity SHALL retain the original group and scheduler identity without an effect or final callback destruction. Placement and window publication gaps SHALL preserve their original bounded retry authority. No retry SHALL change original T or frozen E.

#### Scenario: Feedback cannot cover every start
- **GIVEN** a valid prepared group and fewer feedback slots than affected PADs
- **WHEN** its effect is checked with a slot available to retire that group
- **THEN** the entire group is rejected with no partial loop, voice or feedback effect
- **AND** original work retires together through the existing off-realtime owner

#### Scenario: Retirement recovers before the original effect
- **GIVEN** zero retirement slots keep a valid original group queued before admission
- **WHEN** whole-effect capacity returns while original E is still current
- **THEN** that same group may execute once at its unchanged E
- **AND** no new capture or timing request supplies permission

#### Scenario: Retirement recovers after a prepared effect expired
- **GIVEN** zero retirement capacity retained an original prepared group through E
- **WHEN** capacity returns after E
- **THEN** the original group refuses without a late attack or changed E
- **AND** its complete owners retire only when the existing retirement lane can accept them

#### Scenario: The complete callback owner closes
- **GIVEN** an unexecuted prepared group held by the runtime and a scalar scheduler record
- **WHEN** complete callback teardown releases both owners off realtime
- **THEN** its original ticket becomes rejected without any playback effect
- **AND** all retained work and source owners are released through their existing custody
