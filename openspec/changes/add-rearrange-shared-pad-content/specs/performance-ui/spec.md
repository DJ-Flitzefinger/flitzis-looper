## MODIFIED Requirements

### Requirement: Arrangement Diagnostics Use The Existing Console
The system SHALL report technical arrangement, loop-context and shutdown failure
details through the existing deduplicated control-thread console diagnostics.
The performance UI SHALL NOT display raw exception messages in the loop editor,
pending-close surface or their tooltips. Ordinary readiness, retry/cancel controls
and truthful pending progress SHALL remain available without creating another
error-reporting owner or changing the underlying operation's outcome.

#### Scenario: A loop-context preparation fails
- **WHEN** preparation of a copied pad's playback context fails
- **THEN** the actual failure SHALL be reported in the console with its pad/operation context
- **AND** the editor SHALL retain its stable layout and existing applicable retry/cancel controls without displaying technical failure text

#### Scenario: Close waits for an arrangement owner
- **WHEN** close must wait for actual Native acknowledgement, durable settlement or retained ownership
- **THEN** the existing always-reachable close poll SHALL continue servicing those owners
- **AND** the pending-close surface SHALL show compact progress within a viewport-responsive width with its title readable at supported font scales
- **AND** changed technical failures SHALL be reported through the existing console even when no GUI frame is rendered
- **AND** hiding technical text SHALL NOT fabricate cancellation, acknowledgement, retirement or permission to tear down audio

### Requirement: Bank Selector
The system SHALL expose six banks of36 pads and preserve normal bank selection when Re-Arrange is off, while active Re-Arrange SHALL route bank gestures to exact BankCopy or BankClear targets.

The six Bank1..6 selector controls SHALL remain below the pad grid, SHALL
highlight the currently selected bank and SHALL start with Bank1 selected.

Other banks except current source SHALL be clearly marked preferably slow pulse.
Left-click other bank SHALL copy complete current bank including empty slots,
keep source selected and SHALL NOT navigate or self-copy. Occupied target SHALL
ask "Overwrite Bank B?" with OVERWRITE/CANCEL; empty target needs no overwrite
warning. Right-click any bank including current SHALL ask "Clear Bank B?" with
CLEAR BANK/CANCEL for exactly that clicked bank. Cancel/failure SHALL leave complete
state unchanged, and hover/end of pad drag SHALL NOT produce bank actions.

#### Scenario: Other occupied or empty bank copied
- **WHEN** active Re-Arrange left-clicks other bankB
- **THEN** occupiedB needs exact overwrite confirmation and emptyB does not
- **AND** source stays selected for further copies, copied playback stays stopped

#### Scenario: Clear current or other exact bank
- **WHEN** active Re-Arrange right-clicks bankB and confirms CLEAR BANK
- **THEN** exactlyB clears atomically even if current, only removed target playback ends
- **AND** CANCEL or failed admission causes no partial assignment/voice/hold changes

#### Scenario: Normal selector and drag fence
- **WHEN** Re-Arrange is off a bank is clicked, or active drag merely hovers/releases over a bank
- **THEN** normal click selects its bank, while consumed drag performs no bank action/navigation

#### Scenario: Confirmation retains the original complete gesture
- **WHEN** an occupied-copy or any-bank-clear warning is opened
- **THEN** the common confirmation owner SHALL retain the exact original BankRequest and its captured source/target images and Native participants
- **AND** acceptance SHALL consume that same request at most once through the existing Bank controller, without recapture or following later bank selection
- **AND** cancellation, Escape or closing the warning SHALL discard only that unreserved request
- **AND** stale content or failed admission SHALL preserve the complete before-state and report the actual failure without granting a second acceptance
- **AND** an unsolicited Native rejection after publication but before acceptance SHALL remain an explicit failed Bank action in the existing deduplicated console feedback; it SHALL NOT appear as silent successful completion or deliberate cancellation
- **AND** deliberate cancellation SHALL NOT acquire a fabricated rejection error and an existing more specific failure SHALL remain intact

#### Scenario: A held right button cannot repeat Clear
- **WHEN** Re-Arrange receives a fresh right-click on a bank with no PAD gesture owning the pointer
- **THEN** it SHALL request the exact clicked bank's clear warning once
- **AND** merely holding the right button, dragging over the selector or releasing a consumed PAD gesture SHALL NOT open or replace a bank warning

#### Scenario: Bank 1 is selected by default
- **WHEN** the UI is started with Re-Arrange off
- **THEN** Bank 1 is visually indicated as selected below the pad grid

#### Scenario: Selecting a different bank updates the selection
- **GIVEN** Re-Arrange is off
- **WHEN** the user selects Bank 3
- **THEN** Bank 3 is visually indicated as selected
- **AND** Bank 1 is visually indicated as not selected
