# Evidence domains and incomplete acceptance

For source rate Fs, accepted seconds per quarter T and logical loop beats b,
the musical source duration is P = Fs*T*b. Physical endpoints a and e are rounded
once from absolute source positions and H = e-a. At an unclipped steady source
rate r, physical repetition is H/r output frames, while musical repetition is P/r.
The unwrapped discrepancy after k cycles is k*(H-P)/r. Binary64 epoch precision
and callback partition independence cannot remove a nonzero H-P.

Independent expectations must not call SourcePlayback, SourceReadPlan or their
addressing helpers. Rational reference arithmetic separates numerical rounding
from endpoint rounding. Actual output threshold features are measured and compared
with independently interpolated PCM; any feature offset is declared or cancelled
relative to the first occurrence. Integer output sampling and interpolation-feature
offset are reported separately from continuous recurrence: exact discrete onset
agreement does not weaken the one-loaded-frame unwrapped musical-period limit.
A scalar grid position is not audio evidence.

Each rendered fixture must publish source-bound accepted timing through the current
native guard and acknowledgement before playback. Synthetic backend/count fixtures
are explicitly test evidence, not an analyzer or human musical-acceptance result.
The private unchanged metronome is a separate positive control. Hashes, large
exports and private audio stay outside the repository.

The productive integer-wrap mismatch is a failed G3 gate. Correcting it must use
the shared source trajectory and reader, preserving explicit units, safe fractional
boundary taps, intro/tail/seek behavior, same-source stems, copied preparation
domains and native/FIFO/filter continuity. Merely scaling a second BPMLOCK ratio,
resetting from UI polling, or accepting integer physical output as musical success
does not satisfy the contract. The correction and its complete 75/1000-cycle
rendered/onset matrix remain required until actually implemented and validated.

Hardware-free dry output proves numerical and discrete PCM-feature behavior only.
Actual device/loopback and human sustained listening require their own evidence;
native Key Lock crop/delay/transition compensation remains B5.
