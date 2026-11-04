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

#### Scenario: A user has no ASIO driver
- **GIVEN** Windows has a usable WASAPI output and no installed ASIO driver
- **WHEN** the user selects WASAPI or the normal automatic backend
- **THEN** output configuration and playback remain usable without ASIO activation or installation
- **AND** existing Linux and macOS platform backends remain available on their platforms

#### Scenario: A requested low-latency buffer is unsupported
- **GIVEN** exact driver or engine-period support has been queried for the selected format
- **WHEN** an explicit requested value is not legal or initialization rejects it
- **THEN** the request fails visibly without being reported as an accepted rounded value
- **AND** the previous known working configuration is retained or restored through checked ownership

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
