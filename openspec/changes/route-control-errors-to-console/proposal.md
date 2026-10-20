## Why

Short-lived control failures are difficult to read in performance sidebars and can displace
controls. The performer requested diagnostics in the already opened command window until a
separate log window is introduced.

## What Changes

- Report rejected actions and asynchronous diagnostic changes through one control-thread console
  sink with timestamp, operation, optional pad number, and reason.
- Remove error details, error tooltips, and error styling from performance panels and dialogs;
  retain normal preparation, confirmed mode, progress, and retry controls.
- Deduplicate unchanged polled diagnostics while reporting a new deliberate failed attempt again.
- Keep historical failures as diagnostics rather than permanent pad-start admission authority.

## Non-goals

No separate log window, log files, new audio/control path, altered Native admission guards,
automatic replay, or changes to audio callback work. No disk I/O, logging, Python/GIL access,
allocation, blocking, or inference is added to the realtime path.
