# Change: Show pad filenames and legacy active color

## Why
Today, performance pads only show their numeric index. Once audio is loaded, the performer has no at-a-glance feedback about what content is on each pad. The legacy app displayed the loaded file name and used a green active state, which made performance workflows faster and less error-prone.

## What Changes
- Pad button labels reflect pad content:
  - Empty pads display their pad number (1–36).
  - Loaded pads display the loaded audio file’s basename (filename only, no directory path).
- Pad labels update immediately after load/unload actions.
- Active pads use the legacy active background color `#2ecc71` (legacy `COLOR_BTN_ACTIVE`) with active text color `#000000`.
- Out of scope for this change:
  - BPM display/state in pad labels
  - Bank-specific pad assignments / per-bank labels
  - Ellipsis/truncation for long filenames (can be added later if needed)

## Impact
- Affected specs: `performance-ui`
- Affected code (expected): `src/flitzis_looper/ui.py`, `src/flitzis_looper/app.py`
- Validation: `uv run pytest`, `uv run ruff check src`, plus a manual UI smoke run
