## Why

The actual Medium +7 aligned birth now passes its eight isolated-event cases.
Existing Live and original-source offline experiments still fail the retained
44.1 kHz, source-rate .5 kick prerequisite. Known empty quantile intervals cannot
be repaired by a constant delay or a more accurate detector.

The current library also exports R3 Stretcher realtime Standard/HighConsistency.
Feeding it the already mapped canonical PCM with q=1 is a distinct bounded
candidate. Standard enables the multiwindow path absent from Live; its merit
must be measured rather than assumed.

## What Changes

- Add a separate test-only realtime owner using the existing C ABI and DLL.
- Reuse the exact original kick and independent canonical transcript for four
  prerequisite captures, plus a unity padding/coordinate control.
- Retain every raw sample and documented pre-signal padding/delay observations;
  preserve the existing acoustic budgets and previous negative results.

## Non-goals

No production backend/profile/guard change, full pitch/rate matrix, dynamic pitch,
callback allocation or deadline proof, actual owning-future/Mixer integration,
automatic musical-map qualification or Human/device acceptance.

## Realtime

The experiment runs off-thread in cfg(test) only. The library's realtime API name
does not establish this application's callback safety or resource envelope.
