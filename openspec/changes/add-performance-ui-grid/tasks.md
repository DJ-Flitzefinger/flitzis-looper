## 1. Implementation
- [x] 1.1 Replace "hello world" with the performance layout in `src/flitzis_looper/ui.py`.
- [x] 1.2 Add minimal UI state for the selected bank (1..6).
- [x] 1.3 Render a 6×6 pad grid (36 pads) with stable tags (`pad_btn_01` .. `pad_btn_36`).
- [x] 1.4 Render 6 bank selector buttons (`bank_btn_1` .. `bank_btn_6`) and visually highlight the active bank.
- [x] 1.5 Apply a legacy-inspired theme (dark background, high-contrast buttons).
- [x] 1.6 Ensure new UI closely resembles old UI in terms of structure, positioning, and layout.
- [x] 1.7 Manual sanity: run `python -m flitzis_looper` and confirm the grid + bank row render.

## 2. Validation
- [x] 2.1 Run `uv run ruff check src`.
- [x] 2.2 Run `uv run mypy src`.
- [x] 2.3 Run `uv run pytest`.
