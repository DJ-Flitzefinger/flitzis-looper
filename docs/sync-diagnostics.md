# Timing log for a listening test

This diagnostic build records timing observations so a reported drift can be
investigated using the actual loaded sources and settings. It does not change the
tempo algorithm or establish that synchronization sounds correct.

1. Start the updated Looper normally. Keep the two loaded songs and their existing
   settings. No new BPM map is required to record a timing log.
2. Click **Start Timing Log** below SYNC / REALTIME BPM before starting the PADs.
3. Play PAD2 and PAD3 together with the short and long loops that reproduce the
   drift. If another PAD normally repeats eight bars, include it in the test.
4. Click **Heard Drift** when kick/snare audibly move apart. Keep playing long
   enough to cover the 64-bar loop, then click **Stop Timing Log**.

The file location appears below the buttons. Logs are written in `diagnostics/`
beside the current project configuration. Tell the developer the test has finished;
the developer can read the local file without copying console output.

The trace contains requested loop/grid/BPM/pitch/STEMS settings, analysis beat
positions and up to five callback cohorts per second. Every cohort gives all active
voices at one common output-frame boundary, including the actual source-reader
receipt, rates, loop geometry and clock ownership markers. Inactive loaded PADs
remain identifiable through the separate control state. Raw feedback is separately
timestamped and does not grant current source or phase authority.

Native input coordinates describe what the time stretcher consumed. They do not
identify which transient is audible at that instant. Logical/grid agreement alone
does not prove that the detected grid matches the music. The listening test remains
necessary: the actual songs' kick and snare must stay aligned through all 64 bars.

Logging is opt-in, transient and limited to ten minutes or 16 MiB per file. It
records no audio, song names or source file paths. A background worker writes and
flushes JSONL; gaps, limits and dropped events are explicit. A logging failure stops
the trace without changing playback. Stop drains the bounded queue; application
shutdown allows a bounded final flush. A crash can leave a partial file without a
stop record, which must be treated as incomplete evidence. Existing files are kept.

If the installed Native engine is older, the logging button is unavailable until
the verified Native update and an application restart. Do not replace a Native
module held by a running Looper.
