# Change: revise-waveform-editor-transport-controls

## Why

The waveform editor transport controls must be changed to use reliable, icon-only geometric shapes (triangle/square/bars) rather than font glyph icons, and the intended semantics must be clarified:

- View-Jump-Start/End must move ONLY the waveform view, without affecting playback state.
- All five buttons must act as triggers that fire on mouse-down.
- Play must always restart the loop from the beginning on every press, even if already playing.
- Stop must stop playback and reset the playhead to the loop start.

## What Changes

- Modify the waveform-editor requirement "Waveform editor provides transport and navigation controls" to:
    - Replace the previous combined Play/Pause UI with separate Pause, Play, Stop buttons.
    - Add view-only navigation buttons: View-Jump-Start and View-Jump-End.
    - Specify geometric, icon-only shapes (no text labels, no font glyph dependency).
    - Specify trigger-on-press semantics for all five buttons.
    - Specify Play always restarts from loop start each press.
    - Specify Stop stops playback and resets to loop start.
    - Specify View-Jump controls do not change playback state.
- All existing other toolbar controls remain unchanged and are positioned to the right of the five new buttons.

## Impact

- Waveform editor toolbar UI changes.
- Existing playback logic is reused; input handling must trigger on mouse-down.
- Tests need updates for trigger-on-press, play-restart semantics, stop+reset semantics, and view-only navigation.
