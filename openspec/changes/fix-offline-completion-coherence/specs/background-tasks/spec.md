## ADDED Requirements

### Requirement: Terminal analysis settles after derived control refusal
For an exact current analysis request, terminal success feedback SHALL settle that request even when its derived Native control publication raises an expected admission error. The system SHALL expose that error and continue processing independent events. It SHALL preserve already completed key metadata and any already admitted control or analysis state without claiming rejected state was adopted. An early refusal SHALL NOT manufacture analysis adoption. No later frame SHALL replay or recapture the terminal action automatically.

#### Scenario: Native rejects an early or late derived analysis publication
- **GIVEN** a matching reanalysis result and another independent running analysis
- **WHEN** restoring its authority or applying derived loop controls is refused
- **THEN** the completed request and its progress are removed and the refusal is visible
- **AND** valid completed metadata remains truthful and the independent event is processed
- **AND** only a fresh explicit analysis request can start new work
