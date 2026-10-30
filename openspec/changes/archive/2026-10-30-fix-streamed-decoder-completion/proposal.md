## Why

The streamed decoder currently treats the 65th consecutive demux DecodeError as
EOF and can publish a decoded prefix as a complete immutable source.

## What Changes

Fail the original streamed transaction on persistent decode errors. Preserve
actual EOF, the bounded tolerance of isolated errors and their original chronology.
Exercise the real codec/writer/source-manifest path through an injected reader.

## Non-goals

No replacement decoder, codec policy overhaul, original-file mutation, fabricated
source extent or realtime work. Decode and cleanup remain off realtime.
