# Design

## One output authority

Keep the Python `AudioEngine` object stable so existing controllers retain their
authority. A prepared output owner advances through preparation, initialization,
start, installed operation and checked retirement. Backend-specific code owns
device negotiation and the final physical-format boundary. The existing callback
state, transport, scheduler, source identities and sample-frame clock remain the
render authority.

The backend owner supports platform CPAL, Windows WASAPI shared mode and native
Windows ASIO. Automatic backend selection retains the normal platform backend;
it never means the first installed ASIO driver. An explicit backend request is
not silently replaced after failure.

## Preferences and observations

Store one validated output preference containing backend/device identity, optional
rate, optional buffer frames and selected physical left/right channels. Automatic
rate and buffer choices have backend-defined meanings that are shown explicitly.
Malformed output preferences recover locally without discarding other project
fields. Persist a successful Apply, retaining the previous known working request
until the replacement and its source restoration complete.

Separate these facts in the API and UI:

- Requested backend, endpoint, rate, buffer policy and output channels.
- Actually initialized endpoint, logical format, driver-native format and rate.
- ASIO preferred/legal/prepared buffer frames, or WASAPI requested/initialized
  engine period and allocated buffer capacity.
- Observed callback sizes and freshness, and driver-reported latencies if present.
- Applying, restoring, unavailable, uncertain-close and failed/recovered state.

Refreshing an ASIO driver list reads installed identities only. It does not load
each driver. Capability queries that require activating a selected driver occur
only under stopped admission, with exclusive ownership of that driver instance.
Capabilities and channel pairs are requalified after changing its actual rate.

## ASIO boundary

Use the exact ASIO SDK 2.3.4 archive dated 2025-10-15, SHA-256
`d5ebf0c20dd2c5f43771fd0c1418f4b361bf52434ee670097cfa6b3a335e2eca`.
The acquired interface headers and their licenses offer GPLv3, which this project
uses. Retain pristine headers, licenses and provenance under the Rust source tree.
No SDK download, bindgen/Clang or third-party driver installation occurs at build
or runtime. Existing MSVC/`cc` compiles the small Windows-only bridge.

Keep the IASIO virtual interface in C++. Export a fixed-width C ABI with checked
sizes, alignment and offsets. SDK `long` and `ASIOBool` remain 32-bit on Win64.
Load only the explicitly selected registered ASIO identity on its owned COM
control apartment. Do not use SDK global driver-list helpers or CPAL/asio-sys
callback registries.

Register exactly two output descriptors, using the selected physical channel
indices. Keep the internal render format stereo. Each returned native plane has
its own sample type and preallocated conversion plan. Silence every plane owned
by Looper when required; unleased physical channels remain untouched. This does
not claim multiclient compatibility for an untested driver.

Validate minimum, maximum, preferred and granularity. Negative one denotes
doubling from the minimum; positive granularity denotes steps from the minimum.
Zero is specified for a fixed `min=max=preferred` tuple. A wider zero-granularity
range does not authorize arbitrary requested values. Buffer creation remains an
authoritative failure boundary. Actual returned/prepared geometry is reported
independently of the request.

Support the relevant native integer and float endpoint formats without changing
internal float DSP. In particular, `Int32LSB16/18/20/24` stores a right-aligned
signed value with sign extension, unlike ordinary `Int32LSB`. Tests cover byte
order, negative values, saturation, rounding and per-channel routing.

## Callback and checked ownership

A dedicated control owner retains the complete session, callback context and
retirement worker from construction. The Engine owns a bounded control handle;
it does not place ownership inside a temporary error string. A candidate occupies
the prepared owner slot before backend initialization can retain a callback.
Only installed owners may supply current-format descriptors.

ASIO callbacks have no user-data field. Publish one stable session pointer and
use fixed atomic entry/in-flight and reentrancy guards before accessing mutable
render state. Render/scatter callbacks perform no blocking locks, allocation,
logging, UI/Python/GIL access, driver reconfiguration or process work. Notifications
publish bounded atomic reasons. Reset, resync, buffer/rate changes, clock loss and
latency changes are handled on the existing control path. A rate-change value of
zero invalidates timing. Do not acknowledge unsupported selector capabilities.

Successful ASIO stop is a required callback barrier: the SDK prohibits subsequent
buffer-switch calls after its successful return. Require all in-flight entries to
finish, clear the session pointer, dispose buffers and release driver/COM ownership
on its owning thread, then drop the Rust callback and successfully join the
retirement worker. Keep completed stages across retries. A failed stop, disposal
or join remains uncertain and retains its original owner; no takeover or new
stream is admitted on that evidence. Retirement-worker failure is sticky, including
after its JoinHandle was consumed. Final Engine destruction must not accidentally
free a retained callback by merely ignoring a close error.

ASIO sample-position/system-time validity flags and reported output latency feed
the existing clock-mapping authority. Invalid timestamps are unavailable, not
fabricated hardware observations. Reported latency may include driver FIFO and
safety buffering; it is not measured round-trip or acoustic latency.

## WASAPI and other platforms

Extend the pinned WASAPI path narrowly to query legal engine periods for the
exact selected format through `IAudioClient3`. A fixed period must be a multiple
of the fundamental period within the returned range. Initialize it through
`InitializeSharedAudioStream`, without silently rounding an explicit request.
Automatic mode may use the returned default period. Legacy automatic/default
operation, if required, is distinguished from exact-period support.

Current engine-period observations are instantaneous and have explicit freshness;
allocated buffer capacity and callback extent remain separate. Free query-owned
format allocations. WASAPI operation does not load an ASIO driver. Linux/macOS
keep their CPAL backends and skip the Windows SDK bridge.

## Controlled Apply and recovery

Before effects, validate the request and obtain authoritative Native admission
covering actual playback, paused/retained voices, queued starts, placement and
control ownership. Freeze new GUI/MIDI/background admissions while the transaction
owns the change. Require recording/capture, arrangement, Hold, Loader, analysis,
stem and other source readers to settle; cancellation alone is not END.

Retain complete persistent intent and the previous configuration. Verify old
callback, dispatcher/MIDI and source-reader retirement, then rebuild the stream
and runtime on the same Engine. Clear stale feedback/signature/selection caches.
Restore persistent control intent before ordinary Loader work, then restore
selected stems through their existing model-free paths after fresh source ACKs.
Never copy old accepted receipts or mutable DSP history into the new epoch.

Preserve loop positions in seconds and grid-offset time across rate changes.
`pad_grid_offset_samples` cannot simply retain the same integer under a different
rate. Record and test the chosen rate conversion and rounding bound. Do not
automatically resume playback.

On failure after old-stream close, rebuild the prior configuration and restore
through the same fresh paths. Double failure leaves truthful stopped/error state
and a usable Settings retry path. Uncertain retirement forbids replacement.
Startup with an unavailable saved request likewise preserves project data and
provides usable Settings without claiming a silent backend fallback.

Assigned Pair restoration supports another negotiated rate, including Auto.
Schema1 exact conversion retains its canonical identity and rejects resampling
metadata. Schema2 records the executed Original decoder clock, coherent raw
five-WAV geometry, target geometry, actual FFT delay and exact nested-ceiling
tail. A private Original witness binds completed decoding to sealed file/root/
material identities and full bytes; JSON cannot supply that witness. Decoder
and FFT/alignment scratch use actual host promises before their allocations.

Recovery enters the existing Engine Cold lane under the same output-epoch
linearization as output admission, including without an installed stream.
Qualified leases acquire their Original witness before inventory locking and
retain it through fresh resealing and physical retirement. Ordinary source
publication still requires the actual current source, epoch and OwnACK.

When a saved Pair needs another target clock, Native verifies the old complete
Pair and derives the replacement from its original WAV generation. The existing
atomic selection writer commits the new identity before owner transfer. Output
rollback retains the original Pair readers and restores its saved selection
through that writer before fresh original-rate source/component admission.

Queued Pair workers capture an opaque restore scope from the genuine scoped
control call. Execution revalidates the same Engine, epoch, restoration permission
and healthy output, releasing the Engine borrow before invoking worker code.
An ended or foreign transaction cannot admit work through that scope.

Accepted stopped mode receipts remain owned state after control adoption. The
verified output reset retires those exact owners outside cache locks; their
audible-effect state does not certify output or retirement. Actual pending
placement publication uses the declared admission-pressure protocol and the
existing bounded retry queue. Fresh component interest requeues a previously
disk-only verified demand and still requires its own current Native ACK.

## Verification

Use injected drivers and headless real callback/retirement fixtures for native
formats, legal buffers, sparse routing, notifications, reentrancy, sticky close
failures and exact owner END. Cover capability validation, field-local persistence,
startup failure, pending-owner refusal, rollback/double failure and complete
48-to-96-kHz source/stem/clock/grid restoration with stale-receipt rejection.
Verify missing ASIO drivers do not break WASAPI import/start or headless probes.
Include SDK source/license and normal Native/Portable build custody.

The hardware-free test emits provisional runtime/source evidence. Saved-project
cold-file leases retain their original process custody after App and Engine
shutdown. Accept all-file reader END only from a supervising verifier after the
original test process and its owning Job have actually ended. The verifier must
reject reparse paths, open every artifact file read-only with sharing disabled,
close its handles, and bind its proof to the exact process, executable, inputs
and provisional receipt. Preserve earlier failed in-process checks as failures.

Hardware-free checks do not establish actual RME behavior, device latency,
96-frame deadlines, six-wet-PAD reliability or listening acceptance. Preserve the
separate real-workload, six-OFF comparison and all original Performance gates.
