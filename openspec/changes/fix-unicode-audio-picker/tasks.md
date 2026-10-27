## Implementation
- [x] Replace narrow-path selection with exact Unicode helper-process results.
- [x] Retain one captured PAD/token and cancel only its owned process before reuse.
- [x] Bound and validate UTF-8 IPC, preserve load diagnostics and closing settlement.
- [x] Check Unicode routing, malformed/error/cancel/reopen behavior and real owned-child cancellation.
- [x] Check affected loader/UI closing regressions, lint/types and independent final diff.

Publication and actual clean main/tracking/remote equality are recorded in the
existing focused execution log and the owned picker evidence receipt.

Actual private FLAC decoding, FLAC/MP3 tempo comparison and 8/32/64-bar playback
evidence remain the immediately following focused task; no musical acceptance is
claimed by picker tests. The automated regressions open no real picker, App or
audio device. A separate controlled Windows picker check selected the exact
Human FLAC path and confirmed terminal cleanup without starting the Looper/audio.
