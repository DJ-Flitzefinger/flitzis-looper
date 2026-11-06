## ADDED Requirements

### Requirement: Explicit Output Configuration Preserves Requested and Effective State
The application SHALL offer output backend, endpoint, sample rate, buffer policy
and physical stereo-channel selection, including explicit automatic choices.
It SHALL validate actual support and distinguish requested, installed and observed
configuration without silently replacing an explicit backend request.

#### Scenario: A user selects an ASIO output
- **GIVEN** a registered ASIO driver is available on Windows
- **WHEN** the user explicitly selects its identity and applies a supported configuration
- **THEN** the application uses native ASIO for the selected output channels
- **AND** internal rendering remains stereo with the existing DSP and scheduler authority
- **AND** WASAPI is not silently substituted after ASIO failure

#### Scenario: Windows automatic output prefers ASIO and driver geometry
- **GIVEN** automatic backend/device selection and a successful non-acquiring ASIO inventory with at least one registered identity
- **WHEN** output starts on Windows
- **THEN** acquisition selects the lowest canonical ASIO identity in deterministic order
- **AND** automatic rate and buffer retain that driver's current sample rate and preferred legal buffer frames
- **AND** requested Auto intent stays distinct from the resolved identity and actual geometry
- **AND** explicit supported backend/device/rate/buffer/channel preferences remain authoritative

#### Scenario: A user has no ASIO driver
- **GIVEN** Windows has a usable WASAPI output and complete successful ASIO discovery returns no identities
- **WHEN** the user selects the automatic backend without a selected endpoint
- **THEN** output configuration and playback remain usable without ASIO activation or installation
- **AND** existing Linux and macOS platform backends remain available on their platforms

#### Scenario: ASIO discovery or driver activation fails
- **GIVEN** automatic Windows output selection
- **WHEN** ASIO discovery is incomplete or fails, or the selected driver's activation or format fails
- **THEN** the real error remains visible rather than being treated as an empty inventory
- **AND** no second backend takes over an unverified original owner

#### Scenario: A saved automatic preference includes a platform endpoint
- **GIVEN** a saved Auto request contains an existing WASAPI endpoint ID
- **WHEN** startup restores the preference
- **THEN** the endpoint retains its WASAPI interpretation rather than being treated as an ASIO CLSID
- **AND** all explicit saved geometry and routing remain intact

#### Scenario: Startup initially shows PADs
- **GIVEN** ordinary asynchronous audio startup and saved-source restoration
- **WHEN** the first UI frames render before performance admission
- **THEN** the inactive PAD grid remains visible without Native-dependent selectors or input/playback admission
- **AND** readiness reveals the ordinary PAD view without latching Settings open
- **AND** manual Settings navigation persists, while actual startup errors expose corrective Settings and their reason

#### Scenario: A requested low-latency buffer is unsupported
- **GIVEN** exact driver or engine-period support has been queried for the selected format
- **WHEN** an explicit requested value is not legal or initialization rejects it
- **THEN** the request fails visibly without being reported as an accepted rounded value
- **AND** the previous known working configuration is retained or restored through checked ownership

#### Scenario: Manual audio geometry requires explicit values
- **GIVEN** the selected output uses automatic sample rate or buffer geometry
- **WHEN** the user changes either field to manual entry
- **THEN** the application requires an explicit positive value before applying
- **AND** it does not substitute a device-independent example as a request
- **AND** unfinished or invalid numeric edits cannot be applied as an older value or Auto

#### Scenario: A different output is selected
- **GIVEN** a draft contains device-specific sample rate, buffer or physical routing
- **WHEN** the user selects a different backend or device
- **THEN** that geometry, routing and unfinished numeric edits reset to Auto
- **AND** changing only physical routing does not cancel a numeric edit

#### Scenario: ASIO reports output capabilities
- **GIVEN** the selected explicit ASIO driver has a healthy installed observation
- **WHEN** Settings offers values from its previous capability queries
- **THEN** it distinguishes the snapshot from an exhaustive device control-panel list
- **AND** rate suggestions contain only the initialized rate and successful individual probes
- **AND** buffer suggestions follow that driver's reported geometry at the observed rate
- **AND** a planned different or incomplete rate does not reuse the old buffer geometry
- **AND** selection is explicit, manual entry remains available and Apply revalidates support

#### Scenario: Output observations differ from a request
- **GIVEN** an output stream has actually been initialized
- **WHEN** Settings displays its state
- **THEN** requested policy, installed rate/format, prepared buffer or engine period, buffer capacity and observed callback geometry remain distinct
- **AND** unavailable or stale observations and driver-reported latency are identified accurately

### Requirement: Output Changes Retain Complete Ownership and Musical Intent
Output reconfiguration SHALL use authoritative stopped admission and verified
callback, dispatcher and reader retirement. It SHALL restore complete sources,
selected stems and persistent musical intent with fresh runtime epochs and ACKs.

#### Scenario: Active or pending work fences Apply
- **GIVEN** playback, retained/paused voices, pending starts, Hold, recording, arrangement or required source readers remain active
- **WHEN** a user requests output reconfiguration
- **THEN** the application refuses or waits truthfully without changing the active configuration
- **AND** no requested source, control sequence, musical beginning or ownership receipt is lost

#### Scenario: Sample rate changes after checked retirement
- **GIVEN** all required owners have actually settled and the old output is verified closed
- **WHEN** a new sample rate is installed
- **THEN** ordinary Loader and stem-restoration paths create fresh source/timing/input bindings
- **AND** loop and grid positions preserve their time meaning within the stated rounding bound
- **AND** old acknowledgements and mutable DSP history cannot certify the new runtime
- **AND** playback does not restart automatically

#### Scenario: Backend replacement or cleanup fails
- **GIVEN** the application retains its previous configuration and complete project intent
- **WHEN** replacement fails after verified close
- **THEN** it rebuilds and restores the previous configuration or exposes a truthful stopped retry state
- **AND** uncertain stop, disposal, dispatcher or retirement-worker completion retains its original owner and forbids takeover

### Requirement: Native Audio Callbacks Remain Bounded and Driver Independent
Native ASIO output SHALL use preallocated format/routing storage and immutable
callback ownership, without callback locks, allocation, logging or control-side
driver operations. Notifications SHALL defer recovery to the control path.

#### Scenario: A driver requests reset or reports clock loss
- **GIVEN** an ASIO session owns its registered callbacks and output planes
- **WHEN** a driver requests reset/resync or reports an invalid or changed sample clock
- **THEN** bounded callback-safe state records the condition for control-side recovery
- **AND** no callback rebuilds streams, changes sample rate or fabricates current timing
- **AND** callback data and readers remain alive until successful stop and actual retirement

#### Scenario: An ASIO recording seals before its final output buffer is submitted
- **GIVEN** an original mix capture belongs to an active ASIO session and seals at frame S
- **WHEN** the writer has spooled the exact capture interval [E,S)
- **THEN** success requires the same session's contiguous submission to cover that interval after final callback checks and any required successful output-ready call
- **AND** a gap, terminal fault or verified detach before coverage leaves the spool incomplete
- **AND** a later stream fault preserves a take whose interval was already confirmed

#### Scenario: Only selected physical outputs belong to the session
- **GIVEN** a valid distinct physical left/right pair was selected
- **WHEN** native ASIO buffers are created and rendered
- **THEN** only those selected output descriptors are activated and converted from stereo float DSP
- **AND** silence clears every owned plane without touching unleased physical channels

### Requirement: Audio Output Build Inputs Are Reproducible
The Windows audio bridge SHALL use pinned, licensed SDK source and checked build
inputs included in normal Native and Portable source custody. Runtime WASAPI
operation SHALL not require an ASIO SDK or driver installation.

#### Scenario: A clean source or Portable build enables native ASIO
- **GIVEN** the tracked pinned SDK interface headers and license/provenance files
- **WHEN** the Windows Native extension is built
- **THEN** the existing checked compiler builds the bridge without mutable SDK downloads or global configuration changes
- **AND** normal source fingerprints and Portable inclusion checks cover the actual compiled inputs

### Requirement: Assigned Pairs Follow the Negotiated Output Clock
Assigned paired stems SHALL support explicit rate changes and Auto through complete
Original-clock-qualified conversion without changing the exact schema1 policy.
Schema2 SHALL retain all five complete outputs, channel order and source origin.

#### Scenario: Nested clock rounding produces a tail
- **GIVEN** an actual executed Original decoder clock and five coherent sealed PCM16 WAVs
- **WHEN** their natural converted extent exceeds the Original-derived target extent
- **THEN** complete conversion consumes all raw frames and finite FFT tail
- **AND** the descriptor records the actual single delay and exactly derived tail trim
- **AND** all five prepublication and write completions agree before full reopening

#### Scenario: Recovery qualifies a saved cross-rate Pair
- **GIVEN** a saved schema2 Pair and an Engine with no installed audio stream
- **WHEN** the existing recovery worker acquires a bounded read-only Cold reservation
- **THEN** actual sealed Original decoding and memory admission qualify its private clock proof
- **AND** capture, fresh receipt reopening and retirement resealing require that held proof
- **AND** metadata alone grants no lease, receipt, source ACK or cleanup authority

#### Scenario: Pair restoration fails after committing a new selection
- **GIVEN** the original Pair readers and saved selection remain held by an output transaction
- **WHEN** a new target-rate selection commits but subsequent restoration fails
- **THEN** rollback atomically restores the original saved selection
- **AND** fresh original-rate source and component ACKs precede resumed control admission

#### Scenario: A queued Pair worker restores inside the current output transaction
- **GIVEN** a genuine current restoration call captured an opaque worker scope
- **WHEN** the bounded worker prepares an existing assigned Pair
- **THEN** the scope revalidates the same Engine, epoch, restoration permission and healthy output
- **AND** a stale scope is refused before the worker callback runs
- **AND** the scope grants no source-admission authority to another Engine
- **AND** pending placement remains unaccepted and retries only through declared bounded admission

#### Scenario: Auto reuses an unchanged verified Pair
- **GIVEN** disk verification completed before component interest was requested
- **WHEN** the restored selection requires components or preload
- **THEN** the verified demand re-enters the existing bounded scheduler
- **AND** readiness waits for its exact current registration and component acknowledgements
