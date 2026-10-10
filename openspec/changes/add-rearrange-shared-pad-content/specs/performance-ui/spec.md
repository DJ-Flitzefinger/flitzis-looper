## MODIFIED Requirements

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

#### Scenario: Bank 1 is selected by default
- **WHEN** the UI is started with Re-Arrange off
- **THEN** Bank 1 is visually indicated as selected below the pad grid

#### Scenario: Selecting a different bank updates the selection
- **GIVEN** Re-Arrange is off
- **WHEN** the user selects Bank 3
- **THEN** Bank 3 is visually indicated as selected
- **AND** Bank 1 is visually indicated as not selected
