## ADDED Requirements

### Requirement: Superseding ordinary controls withdraw prior committed effect receipts
The callback SHALL withdraw an older committed stem operation's effective claim when a later accepted ordinary mode or mask change supersedes its selection intent. Returning to identical fields in the same callback drain SHALL NOT restore that receipt. Merely queueing or rejecting a command SHALL NOT invent callback adoption. Historical component residency SHALL remain truthful; supersession SHALL NOT be mislabeled as a Native processing failure or discard any source/reader owner. The receipt SHALL describe its complete requested mode/mask tuple: an accepted mask change while FullMix retains cached components also supersedes that tuple, even though the audible FullMix selection does not change. Rejected and unchanged tuple requests SHALL preserve the current receipt.

#### Scenario: Ordinary selection change follows effective committed stems
- **GIVEN** an effective committed all-stems receipt and a running source
- **WHEN** a later accepted ordinary mode or mask change reaches the callback
- **THEN** the previous effective claim becomes rejected without changing residency history
- **AND** actual PCM follows the normal selection/ramp path with Native and FIFO custody preserved

#### Scenario: Ordinary selection changes away and back in one drain
- **GIVEN** an older effective receipt
- **WHEN** accepted ordered controls change and restore its visible mode or mask
- **THEN** subsequent frames cannot resurrect that older operation's effect claim

#### Scenario: Cached components are retained while FullMix remains audible
- **GIVEN** an effective committed FullMix receipt with actual preload-retained components
- **WHEN** accepted ordinary mask controls change or change-and-restore its requested mask
- **THEN** the old complete mode/mask receipt becomes terminal without changing FullMix PCM or component custody

### Requirement: Stopped stem adoption validates the transition it will actually perform
When no active voice owns a PAD, committed stem adoption SHALL validate the new selection using the inactive transition that the actual claimed effect will install. It SHALL NOT require an old complete-set reader solely for a ramp that will never run. Source, candidate, launch, publication, identity and capacity guards SHALL remain enforced. A paused active voice SHALL retain its real transition, two-reader and retained-Native requirements.

#### Scenario: A stopped PAD replaces an already selected complete set
- **GIVEN** V1 remains selected after its PAD voice has stopped and a valid V2 candidate owns the current source
- **WHEN** the actual committed adoption is considered
- **THEN** preflight validates V2 with the cleared stopped transition and can admit its own complete acknowledgement
- **AND** it neither borrows V1 reader authority nor changes active or paused reader requirements
