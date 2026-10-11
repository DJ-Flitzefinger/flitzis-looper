# Refine loop editor rendering and performance actions

## Why

The Human reports a vertically displaced duplicate waveform and upper number row
while zooming. The published renderer has one raw/envelope branch; a second draw
or a particular scaling cause has not been established. Unload and Analyze also
execute directly beside Adjust Loop, making performance mistakes easy.

## What changes

E11-04 requires stable actual ImPlot geometry and one visible waveform/number row
through zoom, readiness, resize, font and DPI changes. E11-16 adds content-bound
warning confirmation for performer Unload and Analyze actions. E11-17 moves Adjust
Loop between the Pad and BPM sections with matching separators. E11-18 replaces
the old icon-only far-right close contract with a visible `CLOSE LOOP EDITOR`
button immediately right of Grid Offset, using the existing close action.

The single bounded delivery is `X11-UI`, based on the published `e9da7e1` KEYLOCK
source. Software completion requires the productive draw/action evidence and
independent review recorded in the tasks. Final Human visual repetition remains
open through H-FINAL; source inspection or headless frames cannot close it.

## Non-goals and realtime boundary

No change to source coordinates, grid origin, loop markers, playback/transport,
analysis backend selection, or audio DSP. No new editor or controller framework.
Render reads remain cached and bounded; decoding, file scans, hashes, analysis,
persistence and confirmation validation stay outside the audio callback.
Full Rust-port planning/implementation and Slice8 remain outside this program.
