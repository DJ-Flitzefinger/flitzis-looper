## 1. Spec Delta
- [x] 1.1 Add delta spec for waveform-editor grid behavior (zoom-dependent subdivisions + styling)
- [x] 1.2 Validate change with `openspec validate waveform-editor-musical-grid --strict`

## 2. Implementation (later step)
- [x] 2.1 Implement drawing changes (single musical grid; remove/disable old non-musical grid)

## 3. Tests / QA
- [x] 3.1 Add tests if applicable (or document manual QA steps if UI-only)

## 4. Manual QA
- [ ] 4.1 Default zoom: bar lines only; every 4 bars drawn stronger
- [ ] 4.2 Medium zoom: beat lines visible; bar lines drawn stronger
- [ ] 4.3 Close zoom: 1/16-note lines visible; beat lines drawn stronger
- [ ] 4.4 Extreme zoom: 1/64-note lines visible
- [ ] 4.5 Pan left/right: grid stays aligned (no drift) while panning
- [ ] 4.6 BPM change (manual override vs analysis): grid updates and stays aligned to snapped markers
- [ ] 4.7 Marker alignment: snapped loop start/end positions coincide with visible grid lines at suitable zoom
