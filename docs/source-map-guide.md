# BPM, SYNC and source maps

BPM describes an average tempo. A source map also describes where detected beats
occur in the original song. Extra decimal places alone do not make an estimate
accurate. BPM analysis checks stable interior sections and tests nearby integer
tempos against separate sections; it keeps a fractional estimate when the
evidence does not support the integer.

Both the selected fractional estimate and the nearby integer use the same two
separate probe windows and one shared original-event phase. Each window retains
at least ten seconds when enough complete-source material is available. A genuinely
short original can use a declared shorter check; a short resident loop does not
shorten an available full song. Unchecked estimates remain estimates. These finite
checks do not prove an exact integer tempo, correct beat labels or musical alignment.

New version-five analyses can also check one continuous span supported by compatible
song thirds when the preferred middle is unsuitable. The entire span is fitted
together, including every intervening detected event. Its reported boundaries
show which part was checked; a passing prefix does not verify the rest of the song.
The fractional check keeps the selected BPM; the existing preference for a
supported nearby integer still applies. Earlier cached results remain readable.

The sidebar shows the last confirmed BPM inference device separately from BPM and
STEM processing stage. New analyses can use CUDA with one fresh CPU fallback when
CUDA execution fails. "CPU (CUDA fallback)" identifies that fallback; an old cache
without a device receipt shows "Device unknown". CUDA availability or a queued job
alone does not mean that a GPU has processed the song. STEM inference device
remains visible while the later alignment stage runs.

| Intended use | Controls | Source map |
| --- | --- | --- |
| Constant rate playback with accurate BPM | Multi Loop and BPM Lock; SYNC off | No map required. Each PAD keeps its own loop markers and BPM. |
| Experimental detected-beat alignment, including recording-speed variations | Multi Loop, BPM Lock and SYNC on for explicit tests | Prepared automatically; no manual map creation required. |
| Correct a detector mistake or describe a constant grid manually | Adjust Loop > Source Map | Optional advanced editing. Explicit manual choices take priority. |

SYNC is intended to follow detected beat positions using the current master tempo. There is
one SYNC control; old REALTIME BPM MIDI bindings address that same control. The
old saved tracker preference remains readable for compatibility. The supported
dry mapped path requires Key Lock off and zero pitch offset; other
representations retain their availability/refusal messages.

Grid Offset chooses the beat phase; the loop start chooses the part of the song
to play. SYNC corrects fractional beat differences and preserves an already
matching whole-beat offset. It does not automatically choose the same musical
phrase, bar or kick/snare pattern in two different songs.

## Current BPM validation

1. Load the songs and let their BPM analysis finish. Keep the selected loop starts
   and Grid Offset; choose, for example, 8 and 64 bars in Adjust Loop.
2. Enable Multi Loop and BPM Lock with the intended reference PAD selected.
3. Leave SYNC off and compare the loops over the intended playback duration.
   Accurate scalar BPM playback does not require a source map.
4. SYNC remains available for explicit development tests. Live transitions and
   musical alignment still require further work and acceptance. Its button shows
   the preference; an applied mode alone does not prove current grid phase.

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
