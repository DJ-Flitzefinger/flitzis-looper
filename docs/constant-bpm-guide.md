# BPM and constant-tempo loops

The current runtime uses accurate BPM estimates and constant-rate BPM Lock.
Source SYNC, REALTIME BPM, source-map editing and live beat-phase correction have
been removed. The complete previous implementation is preserved in the remote
`SYNC-REALTIME-BPM` branch. See [the archive record](sync-archive.md).

BPM analysis checks stable interior sections and tests a nearby integer tempo
against separate sections. It retains the fractional estimate when the evidence
does not support that integer. The accepted detector and its method schema 4
remain unchanged by this removal. More decimal places alone do not establish
accuracy. Finite segment checks cannot guarantee exact tempo for every song.

The selected fractional estimate and integer candidate share separate probe
windows and one original-event phase. Each window retains ten seconds when enough
complete-source material is available. Short original audio uses the existing
adaptive check. Intro/outro and unstable sections are excluded by the established
quality criteria; a short resident loop does not shorten an available full song.

## Mean estimates and possible drift

Successful precise estimates take priority. When the existing precise methods
finish without a supported value because sections conflict, the final fallback
can use an elapsed-time weighted mean of three sections between 20% and 80% of
the full source. All three sections must independently support the same pulse
family in both detectors, using one common half/double normalization. Existing
local coverage, residual and outlier checks still apply; insufficient or incomplete
analysis does not produce a mean. Each section needs at least 30 seconds of support,
so short or irregular sources can remain unavailable.

A selected mean is marked **Possible BPM drift** on its PAD, with
**Mean estimate · middle 60%** in the BPM details. This is an approximate constant
scalar for ordinary loops and BPM Lock. Playback does not follow changing source
tempo. The label indicates uncertainty; it does not establish the physical cause
of detector disagreement. Reproducible analysis is not a guarantee of musical accuracy.

Reanalysis cannot replace a supported precise result of the same unchanged source
with a mean. Its previous BPM, grid and method remain together, while an independent
key result can still complete. New content does not inherit another source's result.
Manual and Tap BPM keep their existing priority. A configured automatic range filters
the final new value; it does not retrospectively qualify an older retained result.

## Normal use

1. Use **start.bat** to start the delivered matching Release. Developers rebuilding
   changed Native sources use **start-release.bat** as described in Development.
2. Load the songs and let BPM analysis finish. Use half or double when the
   detector chooses the wrong rhythmic level. Tap BPM or manual BPM provides a
   correction for difficult material.
3. Set Grid Offset and each loop's start/end in Adjust Loop. A bar is four beats.
   For example, choose 8 bars on one PAD and 64 bars on another.
4. Enable BPM Lock with the intended reference PAD selected. It is available with
   Multi Loop OFF and ON. Start PADs using the ordinary quantized controls; switching
   PADs or Multi Loop preserves the established master tempo and Lock reference.

With Multi Loop OFF, START/STOP uses the last PAD that actually played through an
explicit start or resume. Right-click STOP retains that target for the next START.
Changing editor selection does not change it. If no valid playback target exists,
an explicit START can use the loaded selection. STOP ALL remains global, as do
the existing recording group controls.

Turning Multi Loop OFF stops peers and keeps the last playable target at its
current position. It does not retrigger or resume a paused, stopped or held target.
An admitted transition may remain pending until the audio callback can complete
it; a refused transition leaves the previous mode and voices intact.

Grid Offset sets the source grid's phase; loop start selects the musical entry.
Grid lines and physical loop markers use the same loaded sample positions.
Analytical BPM and musical loop periods keep their full precision. Wrong phase,
wrong rhythmic level, incorrect BPM or actual tempo fluctuations can still
produce audible disagreement. This runtime does not correct changing recording
speed during playback.

Tap BPM keeps the full current sequence and iteratively fits its beat grid,
rejecting strongly misplaced taps while allowing previous exclusions to return
when later evidence supports them. A pause longer than three seconds or a source
change starts a fresh sequence. More independent accurate taps improve the fit;
systematic human timing drift cannot be inferred away automatically.

## Processing device

Long tracks can finish BPM analysis while reporting that optional key analysis
was skipped because it needs too much preparation memory. A fresh track then
shows an unknown key. Reanalysis preserves the previous detected key and manual
source-key correction when key work is skipped or fails; a matching successful
key result can replace the old detection and clear its preexisting correction.
The BPM result remains available independently.

The sidebar shows the last confirmed BPM inference device separately from its
processing stage. CUDA inference can fall back once to a fresh CPU run after a
CUDA execution failure. **CPU (CUDA fallback)** identifies that result; old data
without a device receipt shows **Device unknown**. STEM inference device remains
visible while the later alignment stage runs. Availability or a queued job alone
is not evidence that a GPU processed the source.

## Fresh project

Fresh projects use a 1/16 trigger-quantization step with quantization initially
disabled, FLAC recording at 24 bit with compression 8, and BS-RoFormer MUSDB18HQ for future
stem requests. Explicitly saved choices remain authoritative; changing defaults
does not rewrite user settings or existing recordings/stems.

This architecture change intentionally provides no backward compatibility. The
Human starts with a fresh samples directory. Development tooling does not delete
private samples, settings or previous artifacts.
