## Why

A rapid original piano retrigger can be delivered after an earlier accepted
retrigger has started another voice generation. That earlier independent birth
currently erases the callback's constant source/control observation, so the later
gesture reports preparation_failed despite unchanged source, loop and rate.

## What Changes

- Keep constant source/control-program observations distinct from voice DSP births.
- Continue a proved observation only across a fully admitted same-content Normal
  launch with an unchanged constant source, rate, loop and reader contract.
- Retain every original gesture, frozen pitch tuple, T/E, Native owner and attack.
- Emit every fresh physical beginning at its own E without unheard E-T source or
  filter consumption; preserve the accepted rate-step clock at E independently.
- Preserve separate gain/EQ intent fences and every real discontinuity guard.

The Human's 2026-10-20 full-onset revision replaces this change's earlier T-born
suffix oracle. The scalar observation and independent owner foundations remain.

## Non-goals

No general lifecycle-reset removal, guessed glide history, fabricated PCM/Native
history, callback allocation/preparation, timestamp shift, attack coalescing,
additional scheduler, device deadline or listening acceptance.
