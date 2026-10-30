## Why

Global Key Lock currently overwrites loaded pads' boolean state, new loads default OFF, and the
sidebar cannot distinguish inherited intent from an explicit override or confirmed Native state.
The performer needs a durable global default with track-bound ON/OFF overrides and visible failures.

## What Changes

- Resolve each loaded pad from the current global default plus a nullable ON/OFF override.
- Version `pad_key_lock` as strict `bool | null` with `key_lock_intent_version: 2`; preserve loaded
  legacy booleans as explicit intent and migrate absent values to inheritance.
- Clear prior track origin only on matching successful new-source completion; failure retains the
  previous origin and confirmed Native state. Unload clears origin and neutralizes live state.
- Give selected loaded pads adjacent PAD ON/OFF and GLOBAL inherit controls. Keep the main global
  button tied to the default and show pending/preparing, error and confirmed Native status separately.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `time-stretch-pitch-shift`: authoritative inheritance resolution and separate requested/live state.
- `project-persistence`: versioned nullable override storage and legacy migration.
- `load-audio-files`: successful source-boundary clearing and failure preservation.
- `performance-ui`: origin colors, explicit click semantics and separate Native status.
- `play-samples`: publication of resolved per-pad requests without global override rewriting.

## Impact

Python project models, controllers, load completion/restore paths, selected-pad and main global UI,
and focused tests. The Rust callback continues to consume bounded resolved scalar state.

## Non-goals

No new Native source-preparation adoption, audible compensation, musical/device acceptance,
synchronized Quantize, plugin host, FX or full Rust migration. Existing pending safety/music gates
in `prepare-key-lock-realtime-state` remain unchanged.

## Realtime constraints

Intent resolution, JSON migration, UI and preparation/error orchestration remain outside the
callback. Live publication uses bounded scalar messages and preserves caller-visible failures.
The callback performs no allocation, disk/JSON I/O, Python/GIL/UI calls, blocking, logging,
inference, plugin loading or unbounded preparation work.
