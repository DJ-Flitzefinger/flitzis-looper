# Change: Add performance UI grid + banks

## Why
The current UI is a minimal "hello world" shell, but the legacy application’s primary workflow is a performance view built around a 6×6 pad grid and bank switching. Adding this layout unblocks implementing pad interactions and later loop features while keeping the change incremental.

## What Changes
- Replace the placeholder "hello world" content with a performance UI view.
- Render a 6×6 pad grid (36 pads) and a row of 6 bank buttons (legacy parity).
- Introduce minimal UI state for the selected bank (and placeholders for per-pad metadata).
- Apply a legacy-inspired theme (colors, spacing) aligned with the old Tk UI.

## Impact
- Affected specs: `bootstrap-ui` (remove placeholder label), `performance-ui` (new capability)
- Affected code: `src/flitzis_looper/ui.py` (layout), `src/flitzis_looper/app.py` (UI state)
- Out of scope: Pad audio interactions (handled in change `add-performance-pad-interactions`)
