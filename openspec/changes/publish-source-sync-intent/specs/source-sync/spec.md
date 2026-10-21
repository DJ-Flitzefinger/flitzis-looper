## ADDED Requirements

### Requirement: SYNC intent belongs independently to playable content
The application SHALL expose one independent per-content SYNC request through
GUI and Learn/MIDI actions using the same controller. It SHALL persist a strict
requested boolean and checked u64 revision independently of map selection,
BPM Lock and Key Lock. A changed request MUST advance its own revision without
wrapping and MUST NOT silently change other controls or cancel map preparation.

#### Scenario: SYNC changes while a map restores
- **WHEN** the user changes SYNC while the original selected map is restoring
- **THEN** the original map preparation owner SHALL remain independent
- **AND** current SYNC intent SHALL persist without a fabricated effective ACK
- **AND** BPM Lock and Key Lock SHALL retain their requested values

### Requirement: Existing runtime publication carries genuine map custody
The existing complete input-runtime publication SHALL carry the separate SYNC
request revision and only a current original source-qualified Native delivery.
Requested SYNC without a usable map MUST remain requested but unavailable; it
MUST NOT fall back to scalar launch or reconstruct authority from receipt data.
Same-frame GUI capture and mapped device capture SHALL observe current intent.
Reserved arrangement rows MUST retain their whole previously published state
until the existing actual completion/mirror permits replacement.

#### Scenario: A saved map is unavailable after Copy
- **WHEN** copied content requests SYNC before its own fresh map restoration succeeds
- **THEN** its runtime request SHALL contain no fabricated qualified delivery
- **AND** mapped launch SHALL refuse without replacing preceding effective audio
- **AND** unrelated valid PAD updates SHALL remain publishable

### Requirement: Effective SYNC is confirmed by actual voice adoption
The engine SHALL expose bounded callback-owned current-source observations of
active mapped playback, adopted request revision and map identity. The GUI MUST
distinguish request, map preparation, pending/refused adoption and actual effective
playback. Neither a qualified editor map, enqueue nor a scalar voice SHALL count
as mapped adoption. Stale, overlapping or mixed voice observations MUST NOT
acknowledge a new request. Old effective audio MAY remain until supported actual
restart/retrigger adopts a replacement.

#### Scenario: A new request retains the old audible map
- **WHEN** intent or selected map changes while the old mapped voice remains audible
- **THEN** feedback SHALL retain that actual voice's original revision and map identity
- **AND** the new request SHALL remain pending rather than inheriting its ACK
- **AND** stop, source replacement or unload SHALL NOT leave a fabricated active state

### Requirement: Automatic mapped bars retain exact musical extent
Explicit automatic mapped bars SHALL resolve L=4*bars and their unrounded end
from the same qualified canonical map. Existing Residency SHALL prepare and
acknowledge rounded physical coverage before effective launch. A manual physical
end MUST NOT infer exact bars from rounded coordinates or old stored bars.

#### Scenario: Rounded PCM coverage does not redefine the requested cycle
- **WHEN** an automatic mapped loop ends between physical source frames
- **THEN** its runtime row SHALL retain the exact quarter-note extent
- **AND** rounded residency coverage SHALL NOT redefine its musical cycle
- **AND** unavailable whole-interval coverage SHALL preserve preceding effective audio

### Requirement: SYNC persistence preserves historical content contracts
Copy SHALL retain independent SYNC intent and require fresh map qualification;
Move/Swap and Bank SHALL follow existing content ownership. Neutral intent SHALL
be omitted from musical snapshot seals. Historical complete images MAY omit only
the new field alongside already authorized historical omissions. Python, Native
and Portable validators MUST agree on strict added metadata. Malformed intent
SHALL retain original Config bytes and the existing unsafe-metadata save fence.
Same-content analysis SHALL preserve intent; actual replacement/unload SHALL clear it.

#### Scenario: Legacy journal and a nonneutral SYNC copy coexist
- **WHEN** an old exact sealed image restores beside a new copied SYNC request
- **THEN** its historical bytes and seal SHALL remain unchanged
- **AND** the new copied intent SHALL remain independent
- **AND** unknown fields, wrong types and revision overflow SHALL still refuse

### Requirement: Initial control integration preserves remaining SYNC obligations
This control integration SHALL preserve all unsupported mapped tuple/transition
refusals of the current initial envelope and SHALL NOT present itself as complete
continuous SYNC. Automatic trusted fullmix acquisition, genuine Native/FIFO and
history integration, both Key Lock states, all required rates/pitches, both start
policies and actual resource/music/device/Human/parent gates MUST remain required.

#### Scenario: A requested Native tuple is not integrated
- **WHEN** a SYNC request needs an unsupported mapped Native or transition tuple
- **THEN** the established precise refusal and preceding effective audio SHALL remain
- **AND** the UI SHALL NOT claim effective adoption or silently toggle other controls
