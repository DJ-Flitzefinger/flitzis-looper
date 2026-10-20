## ADDED Requirements

### Requirement: Independent launch preserves proved constant source program
The system SHALL preserve a callback-proved constant source/control-program interval across a completely admitted same-content Normal launch when the active unpaused pre-birth render observation reaches the actual effect E exactly, the previous voice's admitted ContentKey matches the new action's current key, the new launch retains the same content/source/reader/Normal-loop domain, source-control revision and constant rate, and at most one active voice owns the content. The continuation SHALL rebase only the logical checkpoint needed for subsequent observations; it SHALL NOT retain or fabricate predecessor PCM, Native/FIFO history, filter state or voice lifetime authority. An absent, stopped, paused or ambiguous predecessor SHALL NOT gain an earlier rendered observation from current bank metadata or a matching physical PCM owner. Every later delivered independent retrigger SHALL retain its original capture T, truthful execution E, frozen pitch and separate prepared owner, including equal or opposing notes. Its full selected physical beginning SHALL remain complete without consuming unheard E-T source/filter output; required heavy preparation SHALL use the existing off-realtime worker and sole scheduler without introducing an unnecessary worker wait for an otherwise ready dry birth. Source phase SHALL begin at actual E independently from the actual accepted rate-step clock evaluated there. The callback SHALL perform bounded scalar work only and SHALL NOT allocate, prepare Native, replay audio, block, log, perform I/O or access Python/GIL.

#### Scenario: Earlier birth lies between another gesture and its delivery
- **GIVEN** unchanged constant source/loop/rate controls and a fully prepared independent retrigger A
- **WHEN** B is captured before or at A's effect, A starts, and B is delivered afterward
- **THEN** A's permitted birth alone SHALL NOT erase B's already proved source/control interval
- **AND** B SHALL retain its own original T/E and complete atomic pitch-plus-start effect
- **AND** both equal and different notes under both launch policies SHALL remain distinct attacks
- **AND** B's entire emitted beginning from E SHALL match an independent full-beginning Native/EQ reference with the same admitted rate state without slicing either emitted output
- **AND** B SHALL NOT claim the source phase of an on-time T birth or borrow A's Native, filter state or lifetime

#### Scenario: Real discontinuity cannot inherit a prior constant observation
- **GIVEN** an earlier constant observation followed by Stop, Pause, arbitrary Seek, source replacement, control ABA, changed loop/readers/stems, nonconstant rate, unavailable source reads or a clock gap/reversal
- **WHEN** a later original retrigger needs the preceding historical interval
- **THEN** the system SHALL retain the existing source/control and gain/EQ fences and refuse unproved history
- **AND** a later matching endpoint or independent birth SHALL NOT reopen that earlier interval

#### Scenario: Independent onset does not authorize an unsupported glide
- **GIVEN** a fresh original has no supported past-T or paused-glide admission proof
- **WHEN** an earlier independent birth has preserved a constant scalar observation
- **THEN** that observation SHALL NOT certify the missing variable-rate history
- **AND** source/control ABA, cancellation, original ACK and whole-effect guards SHALL remain mandatory
