## Control-thread reporting

One App-owned sink writes persistent lines to stderr in the existing command window. Explicit
UI action boundaries report immediately. Existing controller snapshots are observed during
normal Python control polling so asynchronous errors remain readable after transient state
changes. Reporting never executes in Rust, a callback, or an audio worker.

The sink tracks the last diagnostic per operation/pad. An unchanged observation emits no line;
recovery clears the observation. A distinct deliberate action starts a new attempt even when
its refusal reason matches the previous attempt. Repeated held-button stop calls share their
attempt and do not produce one line per rendered frame.

Only operation, numeric pad, and a bounded single-line reason are printed. Known private source
paths/names and other absolute paths are redacted. No source samples, payloads, configuration
dumps, or traceback/local-variable dumps are emitted. Existing typed admission catches remain
typed; unexpected programming exceptions still propagate.

Controller failure state continues to serve existing transaction ownership and diagnostics.
Rendering removes detailed errors without treating historical failures as Native start guards.
