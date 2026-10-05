# Automatic initial signal placement

The existing native decode/resample/channel-map worker owns complete interleaved
loaded PCM and the loaded sample rate. A focused non-realtime helper scans that
data with constant extra memory before publication; it neither builds mono nor
uses a viewport envelope. Detection across individual channels preserves
anti-phase stereo and activity confined to one channel.

For a valid finite signal, find global peak absolute amplitude. The activity
threshold is max(1e-5 full scale, peak * 0.001). Find the first frame reaching
that threshold in any channel and retain up to 5 ms preceding audio, clamped
to frame zero. The result is an integer loaded frame divided by the loaded
rate as f64 seconds. No signal, invalid metadata or unusable detection yields
no candidate; the existing initial zero fallback remains.

This fixed heuristic avoids treating sub-threshold codec/noise residue as a
useful start. It can still choose an intro, isolated transient or noise above
threshold and can exclude the quietest part of a long fade. Pre-roll protects
the beginning of attacks but is not a downbeat alignment guarantee. Fractional
BPM is preserved and never rounded as part of detection.

The existing request identity and stale-load rejection apply to the candidate.
Python consumes the optional load-success metadata only for a new assignment,
through the shared default-initialization/persistence/publication path. It does
not musically snap that automatic marker, which could move it later than the
detected attack. Auto-loop duration still follows effective BPM and 8 bars.
Project restore, reanalysis and later grid/marker edits never rerun initial
placement or shift a saved region. Grid origin and source audio are independent.
