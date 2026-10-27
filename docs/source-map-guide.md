# BPM, SYNC and source maps

BPM describes an average tempo. A source map also describes where detected beats
occur in the original song. Extra decimal places alone do not make an estimate
accurate. BPM analysis checks stable interior sections and tests nearby integer
tempos against separate sections; it keeps a fractional estimate when the
evidence does not support the integer.

| Intended use | Controls | Source map |
| --- | --- | --- |
| Constant rate playback with accurate BPM | Multi Loop and BPM Lock; SYNC off | No map required. Each PAD keeps its own loop markers and BPM. |
| Keep detected beats aligned, including recording-speed variations | Multi Loop, BPM Lock and SYNC on | Prepared automatically; no manual map creation required. |
| Correct a detector mistake or describe a constant grid manually | Adjust Loop > Source Map | Optional advanced editing. Explicit manual choices take priority. |

SYNC follows detected beat positions using the current master tempo. There is
one SYNC control; old REALTIME BPM MIDI bindings address that same control. The
old saved tracker preference remains readable for compatibility. The supported
dry mapped path requires Key Lock off and zero pitch offset; other
representations retain their availability/refusal messages.

Grid Offset chooses the beat phase; the loop start chooses the part of the song
to play. SYNC corrects fractional beat differences and preserves an already
matching whole-beat offset. It does not automatically choose the same musical
phrase, bar or kick/snare pattern in two different songs.

## Normal use

1. Load the songs and let their BPM analysis finish. Keep the selected loop starts
   and Grid Offset; choose, for example, 8 and 64 bars in Adjust Loop.
2. Enable Multi Loop and BPM Lock with the intended reference PAD selected.
3. Enable SYNC. Maps are reused from analysis or prepared in the background.
   Saved automatic maps are source-verified after reopening.
4. Start the PADs and inspect the status. The button shows your preference;
   an applied mode alone does not prove current grid phase or audible alignment.

Automatic maps contain estimated detections. A loop bar operationally means four
detected quarter notes; 64 bars contain 256. This convention does not claim a
verified time signature or first downbeat. Gaps and uncertain timing remain
unavailable instead of inventing missing beats. Map timestamps remain in song
seconds when playback resamples 44.1 kHz to 48 kHz. Physical loop markers use the
loaded playback rate.

## Optional manual correction

**Create manual 4/4 map** describes an evenly spaced grid using the current BPM
and Grid Offset. It does not detect speed changes. **Analyze original full mix
(Beat This)** in the advanced editor opens the separate raw review workflow:
inspect detections, label quarter-note coordinates, build a map, then explicitly
review counts, coverage and meter. This is optional for normal automatic SYNC.
A verified 4/4 label records a manual musical judgement.

There is one raw review slot. **Open PAD N analysis** returns to its owner.
**Discard candidate** releases an unwanted review without clearing the accepted
map. Manual selection, correction and **Clear** supersede automatic work; late
analysis cannot restore an explicitly cleared selection.

## Pending maps and recovery

An unavailable map must not make startup or opening Adjust Loop fatal. Ordinary
geometry may remain visible while qualification is pending; that display does
not authorize mapped playback. Existing audio and its actual owner remain until
a supported replacement is adopted. Loading a stopped PAD does not give it
authority over the active cohort's status or phase confirmation.

Listening remains necessary: correct software coordinates do not prove that
detected beats coincide with audible kick and snare through all 64 bars.
