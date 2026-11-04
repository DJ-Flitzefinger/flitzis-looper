# Configurable audio output with native ASIO and capable WASAPI

## Why

The engine currently selects the platform default endpoint and rate, then requests
512 frames through CPAL. Settings cannot select an audio backend, endpoint, rate,
buffer policy or physical stereo output pair. A driver control-panel setting does
not describe every layer of the application stream. Pinned CPAL's Windows shared
path does not query legal low-latency engine periods; enabling its optional ASIO
feature also introduces callback locks and incomplete routing and teardown.

## What changes

- Provide one output preference and Settings surface for backend, endpoint,
  channel pair, sample rate and buffer policy, including explicit automatic choices.
- Add generic native Windows ASIO output using the pinned SDK interface and a
  small C++ bridge. Preserve the existing renderer, scheduler and retirement owner.
- Provide a format-specific WASAPI3 period query/build path while retaining
  explicit default-mode behavior where exact periods are unavailable.
- Distinguish requested, installed and currently observed configuration, buffer
  capacity, callback geometry and reported driver latency.
- Apply changes only through checked stopped-state admission, callback/reader
  retirement, fresh runtime epochs and ordinary complete source/stem restoration.
- Keep a failed explicit ASIO selection visible; retain WASAPI as an independent
  usable backend, and preserve existing Linux/macOS backends.
- Include source/license provenance, reproducible Windows/Portable builds and
  meaningful hardware-free lifecycle and routing checks in this delivery.

## Impact and limits

Audio Engine ownership, callback boundary adaptation, clock mapping, startup,
Settings, persistence, Loader restoration and Portable source custody are affected.
Internal DSP quality and the six productive PAD target remain unchanged. Actual
96 kHz/96-frame six-wet and six-OFF measurements and listening acceptance remain
separate requirements. A nominal one-millisecond buffer period is not total
acoustic latency or evidence of reliable deadlines.

This bounded delivery covers backend selection, stopped Legacy restoration,
same-rate assigned-Pair restoration and fault/rollback handling. After source
startup completes, assigned paired stems require the installed rate explicitly;
Auto or another rate is refused before output retirement. Full cross-rate
assigned-Pair conversion-v2 restoration with actual OriginalClockProof and
recovery remains required. The fixed-rate refusal does not complete that work.
