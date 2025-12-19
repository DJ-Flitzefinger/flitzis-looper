## Context
- UI toolkit: Dear PyGui (DPG) inside `src/flitzis_looper/ui.py`
- Viewport fixed at 960×630 (per `bootstrap-ui`)
- Legacy reference: Tk constants/colors in `old-project/flitzis_looper/core/state.py`

## Goals / Non-Goals
- Goals:
  - Render a 6×6 pad grid (36 pads) plus 6 bank selector buttons.
  - Keep the layout stable (predictable item tags) for follow-up interaction work.
  - Use a legacy-inspired color palette.
- Non-Goals:
  - Trigger/stop audio (handled by `add-performance-pad-interactions`)
  - Loading/unloading pad audio from the UI

## Decisions
- Decision: Build the grid using a DPG table
  - Rationale: A table makes a 6×6 layout straightforward and predictable.
- Decision: Use deterministic item tags for pads and bank buttons
  - Pads: `pad_btn_01` .. `pad_btn_36`
  - Banks: `bank_btn_1` .. `bank_btn_6`
- Decision: Adopt the legacy palette as defaults
  - Background `#1e1e1e`, inactive pad `#3a3a3a`, bank inactive `#cc7700`, bank active `#ffaa00`, text `#ffffff`.

## Risks / Trade-offs
- DPG styling is not pixel-identical to Tk; target “close enough” first, then iterate.

## Open Questions
- None for this change; pad actions are specified in the follow-up change.
