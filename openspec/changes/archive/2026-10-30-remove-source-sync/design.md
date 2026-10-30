# Constant-tempo runtime boundary

The accepted detector, method schema 4 and robust Tap BPM remain unchanged.
Ordinary BPM Lock uses the existing scalar rate and musical loop-period path.
Source SYNC and REALTIME BPM have no current control or executable runtime path.
Beat This raw events and the verified worker protocol remain necessary for BPM
estimation; they do not constitute a selected playback map.

Remove manual/automatic selected maps, map review/editor jobs, launch projections,
adoption/retry, phase correction and their diagnostics. Preserve ordinary source
generation/launch witnesses, OwnACK, InputRuntime captures, scalar grids, resident
preparation, key/pitch, Multi Loop and scheduler/reader retirement invariants.

The Human explicitly rejects backward compatibility and will clear the samples
directory himself. Implement only current project, input and sealed Copy/Pair/Bank
schemas. Do not add migration, legacy decoders, tombstones or automatic cleanup.
Existing private files remain untouched by development tooling.

The waveform uses loaded PCM frames at the engine output rate. Display each
scalar grid position after the same nearest-even physical-frame rounding as loop
markers. Keep analytical beat coordinates and playback timing continuous; display
rounding must not accumulate into subsequent beat positions.

The new Native API marker `BPM_ONLY_RUNTIME_API_VERSION=1` distinguishes this
runtime from an old installed extension. A stale extension requires rebuilding;
feature removal is not complete while an older Native is still installed.

Software regressions and source review do not establish device/hearing acceptance.
The Human's positive listening report applies to the accepted constant-tempo
baseline, not the removed SYNC path or an untested candidate.
