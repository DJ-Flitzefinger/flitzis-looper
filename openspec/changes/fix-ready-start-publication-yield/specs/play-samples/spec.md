## ADDED Requirements

### Requirement: Accepted ordinary readiness publishes without an artificial control yield
The controller SHALL continue through the existing publication checks during the same call when ordinary GUI PAD preparation returns its exact genuine Native window ticket already accepted, and SHALL publish the same original captured attack if those checks and the remaining admission budget permit it. The controller MUST NOT wait for another UI/runtime poll solely because it just obtained the ticket. Original content, input timestamp, target T, sequence, loop and exact independent edit dependency SHALL remain unchanged.

#### Scenario: An ordinary GUI start joins an already accepted window
- **GIVEN** an original captured GUI attack joins an exact Native window whose publication is already accepted
- **WHEN** preparation returns that original member ticket with sufficient remaining admission budget
- **THEN** the controller SHALL publish the same captured attack with that ticket during the original call
- **AND** publication SHALL NOT claim audible effectiveness before the original Native attack is applied

#### Scenario: A real window acknowledgement remains pending
- **GIVEN** the returned original ticket is preparing, pending or adopting
- **WHEN** the controller checks it during the original call or a later poll
- **THEN** the attack SHALL remain unpublished until that exact ticket is genuinely accepted
- **AND** a later poll SHALL reuse the original capture and ticket without recapture or re-quantization

#### Scenario: Same-call publication preserves the total attempt limit
- **GIVEN** preparation and publication share the existing total limit of eight admission attempts
- **WHEN** an accepted ticket is obtained with a publication attempt remaining
- **THEN** the original attack MAY use that remaining attempt during the same call
- **AND** if obtaining the ticket consumed the eighth attempt, the controller SHALL cancel the original attack and its member ticket without attempting a ninth publication
- **AND** cancellation SHALL NOT cancel an independently requested resident edit or another attack member

#### Scenario: Preparation crosses the original readiness deadline
- **GIVEN** an original captured attack has a bounded readiness deadline
- **WHEN** preparation returns its accepted ticket after that original deadline has expired
- **THEN** the controller SHALL reject and cancel that original attack before publication during the same call
- **AND** obtaining the accepted ticket SHALL NOT extend its deadline or bypass the existing terminal guards
