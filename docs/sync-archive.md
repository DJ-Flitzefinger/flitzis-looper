# Archived source SYNC and REALTIME BPM

The Human requested complete removal from main on 2026-10-27 after confirming
constant-BPM playback over more than 100 bars and useful long-sequence Tap BPM.
The former live SYNC path had failed the same musical test.

The remote branch [SYNC-REALTIME-BPM](https://github.com/DJ-Flitzefinger/flitzis-looper/tree/SYNC-REALTIME-BPM)
preserves the full implementation and history:

- Accepted BPM/Tap baseline: `949cff1dae4b979639dca1cbc1e86c586387cd21`.
- Archive tip: `27ef7f878f55c0955835ca536af8df6ef4a89a28`, including unpublished
  regional-probe work. That extra work was reviewed and typechecked but was not
  installed or musically accepted. Main retains the accepted detector policy.

The Human reported PAD1 Veteran’s Memorial.flac at 91.89220756855694 BPM and
PAD2 King Size (Instrumental) at 94 BPM with no audible drift over more than
100 bars. Tap BPM returned 91.91132496949297 BPM on PAD1, with slight but acceptable
long-term drift. These observations are specific listening evidence, not a
100-percent guarantee for arbitrary material. SYNC was not accepted.

Recover the branch when explicitly revisiting variable-tempo playback. Do not
restore obsolete controls, map jobs or phase correction into main implicitly.
Private source files, installed model/runtime environments and local diagnostic
artifacts are outside Git and have not been published or deleted.
