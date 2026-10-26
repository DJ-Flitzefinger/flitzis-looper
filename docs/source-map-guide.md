# Source Maps: fixed tempo, variable tempo and 4/4

A source map relates positions in the original song to musical quarter notes.
It lets the engine translate musical lengths into source positions when the
song's tempo changes. Four quarter notes make one 4/4 bar; 32 bars contain 128
quarter notes.

| Intended use | Controls | Map needed? |
| --- | --- | --- |
| Match songs with constant tempo | Multi Loop, BPM Lock and SYNC on; REALTIME BPM off | No. Use each song's correct BPM, grid and loop markers. |
| Follow tempo changes within a song | Multi Loop, BPM Lock, SYNC and REALTIME BPM on | A current, qualified Source Map for each participating mapped PAD. |
| Explicitly describe an evenly spaced 4/4 song | Create manual 4/4 map | Optional constant map from the current BPM/grid; no tempo-change detection. |

The SYNC button shows your preference. Its colour does not prove map or phase
adoption. Read the requested/mode/status text in the editor. Ordinary playback
may remain available through the supported launch path while a request is pending.

## Constant tempo

Check the song's BPM and grid, keeping your chosen loop markers. For fixed-tempo
SYNC, leave REALTIME BPM off: full-song analysis is unnecessary. If you want an
explicit constant map, open **Adjust Loop**, expand **Source Map**, and choose
**Create manual 4/4 map**. This records the current grid as your manual 4/4
interpretation. It does not measure changing tempo.

## Variable tempo and full-song analysis

1. In **Adjust Loop > Source Map**, choose **Analyze original full mix (Beat This)**.
   It analyzes the original whole song independently of the loop. Completion
   initially means unreviewed detector observations, not a ready variable map.
2. Use **Inspect detections**. Enter **Raw event index** and **Quarter-note
   coordinate**, then **Add/update label** for reviewed original events. Supply
   2–64 labels in chronological order. Consecutive quarter notes can be 0, 1, 2;
   a complete 4/4 bar advances by 4. Missing/extra detections must not silently
   change that count.
3. Choose **Build source map from labels**. New intervals start with uncertain
   coverage and unknown meter; the map retains the original detections.
4. In the map's interval controls, select an anchor span and review its
   **Quarter-note count**. **Coverage: Trusted > Set coverage** records reviewed
   timing. **Meter: Verified 4/4 > Set meter** records the actual meter.
   **Set downbeat** records a known bar-start label. These choices describe your
   review; they do not perform the review automatically.
5. Reviewed coverage must include the whole intended loop. Gaps or unknown meter
   cannot authorize automatic bars across that span. Repeat for each mapped PAD,
   then enable REALTIME BPM and inspect the actual status. The supported dry
   common-phase path currently requires Key Lock off and zero pitch offset;
   other paths retain their availability/refusal messages.

There is one raw review slot. **Open PAD N analysis** returns to its owner.
Successfully accepting that review releases the slot after retirement completes.
**Discard candidate** removes an unwanted raw candidate without clearing an
already accepted Source Map.

## Pending maps and recovery

Opening the editor or restoring a project must preserve its timing preferences,
loop length and markers. With an unavailable map, a waveform can show ordinary
scalar geometry; that display grants no mapped-playback or phase acknowledgement.
Passive refreshes defer unavailable mapped loop changes, preserving existing live
geometry. Map edits still require qualified coverage. A missing map must not make
startup or editor rendering fatal; deleting the saved project is unnecessary.
