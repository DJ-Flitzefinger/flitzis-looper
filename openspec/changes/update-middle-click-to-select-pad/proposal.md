# Change: Replace Middle-click Context Menu With Pad Selection

## Why
The middle-click context menu duplicates actions already accessible through the left sidebar. Repurposing middle-click for selection makes pad inspection and editing faster while reducing UI surface area.

## What Changes
- **BREAKING**: Remove the per-pad context menu that opens via middle mouse click in the performance pad grid.
- Middle mouse click on a pad selects that pad (so its details and actions are shown in the left sidebar).
- All pad actions remain available through existing left sidebar buttons (Load/Unload/Analyze/etc.).

## Impact
- Affected specs: `performance-pad-interactions`, `performance-ui`
- Affected code:
  - `src/flitzis_looper/ui/render/performance_view.py` (pad input handling / popup logic)
  - `src/flitzis_looper/ui/render/sidebar_left.py` (Load/Unload/Analyze actions)
- UX impact: No context menu is available; users select a pad (middle-click or existing left-click selection) and use the sidebar actions.

## Decisions
- Middle-click on an already-selected pad is a no-op.
- Middle-click was the only context-menu trigger; remove all traces of the context menu UI and its identifiers.
