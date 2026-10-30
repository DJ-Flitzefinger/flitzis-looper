## ADDED Requirements

### Requirement: Late original starts preserve the complete admitted output history
A delayed existing-source continuation with a proved unchanged gain and PAD DSP trajectory SHALL prepare both source processing and filter state through E minus T before its effective frame E. The first and every subsequent emitted continuation sample SHALL match the corresponding suffix of an independent on-time continuation at original T under the same admitted controls. Original T MUST remain unchanged. Heavy preparation SHALL use the existing off-realtime lane and the actual source or Native output. Preparation SHALL NOT reconstruct filter history from pre-T Native warm-up, an old voice's filter state or a cropped delayed output. A small dry continuation history MAY use fixed callback storage under the aggregate bound specified below. New ordinary births and fresh Piano SET_RETRIGGER births instead SHALL retain their complete selected beginning and begin a fresh filter chronology at actual E under preserve-full-start-onset; they SHALL retain the original scalar gain/EQ/source stability guards without consuming unheard E-T output. SET without Retrigger, resume and genuine existing-source replay SHALL retain their distinct continuation contracts.

#### Scenario: Stable nonneutral EQ accompanies delayed wet continuation
- **GIVEN** independent on-time and delayed existing-source continuations share the same stable source, gain, EQ, rate, pitch and start policy
- **WHEN** the delayed start's real resident acknowledgement postpones its effective frame beyond original T
- **THEN** its first emitted sample and full continuation match the independent on-time Native and EQ suffix
- **AND** the callback performs only bounded prepared-state adoption

#### Scenario: Stable nonneutral EQ accompanies delayed dry continuation
- **GIVEN** the admitted source output needs no Native pitch processing
- **WHEN** that original existing-source continuation becomes effective after T
- **THEN** its filter state reflects the exact dry source output from T through E
- **AND** preparation does not introduce unnecessary Native processing

### Requirement: Dry filter preparation preserves bounded responsive execution
A proved dry existing-source continuation history of at most 32 frames MAY complete at the original available callback frame without a worker lead, provided all such preparation in that callback consumes at most 128 channel-frames in aggregate. The callback MUST use control-allocated scratch and already prepared filter coefficients, MUST validate the existing source taps, and MUST NOT allocate, construct Native, calculate new filter coefficients or mutate the live filter before the original claim. The budget MUST be shared across continuation actions and groups and MUST reset only once at the callback control-drain boundary. Longer or over-budget continuation histories SHALL use the same existing preparation worker with a separate small DSP-only lead, without inheriting the Native lead. A selected lead SHALL NOT be represented as a guaranteed realtime deadline without measured evidence; existing readiness and refusal guards MUST remain in force. New ordinary and fresh Piano dry births SHALL require neither this history budget nor a DSP-only lead merely for E-T, because their full source begins at actual E; other genuine readiness requirements SHALL remain authoritative.

#### Scenario: A stable dry gesture reaches the callback one frame after T
- **GIVEN** a dry existing-source continuation has proved source, upstream gain and DSP history and the callback has sufficient aggregate budget
- **WHEN** its original work is admitted one frame after T
- **THEN** E remains that callback frame and every emitted sample matches the independent original-T source and EQ suffix
- **AND** no worker wait, allocation, Native construction or coefficient preparation is introduced

#### Scenario: Multiple dry actions exhaust one callback budget
- **GIVEN** multiple dry existing-source continuation actions each individually fit the 32-frame bound
- **WHEN** their combined history would exceed 128 channel-frames in one callback
- **THEN** later work uses the existing DSP-only worker path without resetting the aggregate budget per action
- **AND** original T and source, DSP, readiness and atomic adoption guards remain intact

### Requirement: Fresh piano glides preserve accepted rate state independently from onset
The system SHALL preserve the actual accepted source-rate state evaluated at E for a supported fresh Piano SET_RETRIGGER, including its actual ratio, rate target and remaining active-frame interval before the next smoothing step. Its physical source phase and fresh filter chronology SHALL begin with the complete selected beginning at actual E, without processing E-T as unheard new-source or filter output. A final target ratio or a newly reset smoothing clock MUST NOT substitute for the proved actual rate trajectory. Required Native preparation SHALL retain one action-owned source/Native/FIFO candidate off realtime and validate it against the original source/control/action authority at the actual effect. Native prehistory SHALL remain distinct from the first emitted new-source content. Unproved past-T glides, paused pending glides and unsupported selection/read histories SHALL remain explicit whole-action refusals; this requirement SHALL NOT infer their missing trajectory from a later endpoint.

#### Scenario: Immediate piano retrigger during a proved source-rate glide
- **GIVEN** the existing supported admission path proves an unchanged accepted rate trajectory and its exact rate-step state at E
- **WHEN** the existing worker prepares that original piano retrigger
- **THEN** all emitted source/Native/EQ output SHALL retain the independently declared complete beginning under that rate-step state without a T-to-E suffix cut
- **AND** original T SHALL remain immutable, actual E SHALL remain truthful, and the old voice SHALL remain intact until the whole effect claim
- **AND** Native construction, reset and heavy preparation SHALL remain off realtime

#### Scenario: An unavailable glide is not created by a full phase reset
- **GIVEN** the existing admission guards refuse an unproved past-T or paused pending glide, source/control ABA or unsupported active stem transition
- **WHEN** a fresh Piano retrigger requests its complete physical beginning
- **THEN** it SHALL remain refused without mutating old audio, controls or feedback
- **AND** resetting physical phase SHALL NOT fabricate the missing rate clock, source reads or original authority

### Requirement: Late DSP reconstruction retains chronological authority
The system SHALL retain sufficient original DSP and upstream gain authority to certify a genuine continuation's prepared T-to-E trajectory, including input that returns to the same values after an intervening edit. A fresh full birth SHALL retain its original scalar DSP/gain/source intent and ABA fences without inventing or consuming an unheard filter prefix. An unproved or changed applicable contract SHALL reject the original work before changing voices, filters, feedback or placement. Successful adoption SHALL transfer the matching audio and filter state atomically through the existing original claim, without replaying the gesture or substituting current controls for its past.

#### Scenario: DSP or gain is edited after the retained preparation contract
- **GIVEN** an original continuation owns a proved DSP/gain trajectory or a fresh full birth owns original scalar DSP/gain intent
- **WHEN** a relevant edit or ABA cycle invalidates that trajectory before adoption
- **THEN** the original work is refused without partially resetting existing audio
- **AND** a later independent gesture can acquire fresh authority
