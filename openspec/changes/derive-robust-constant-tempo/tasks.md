## G2a: offline evidence and fit core

- [x] Introduce validated complete raw evidence and source-bound explicit quarter-note
  count hypotheses without modifying legacy or diagnostic publication.
- [x] Implement bounded robust multi-position fitting, distant-region/coverage checks,
  preserved origin, residual diagnostics and conditional period uncertainty.
- [x] Test fractional tempo, quantization, outliers, missing/extra events, count ambiguity,
  count steps, real tempo variation, unsupported coverage and malformed inputs.
- [x] Update maintained architecture/API docs and run full project checks plus official
  strict validation. Review diff for callback changes and unintended publication.

## G2b: adapters, count support and PCM refinement

- [ ] Add lossless full-result adapters and supported count proposals for the selected
  backend evidence; preserve original source/backend identities and raw revisions.
- [ ] Implement conservative comparable-attack refinement, explicit count evidence,
  displacement/provenance and uncertainty; ambiguous signals remain unsupported.
- [ ] Prove the actual exact-WAV <=1 loaded-frame slope gate over measured 0..599.5 s,
  all 1200 pulses, and report 600-s extrapolation separately; test true fractional and
  variable tempo independently without tuning frozen B2 limits.
- [ ] Validate the complete G2 result and hand off source-bound accepted timing to G3.
