# Preserve cold loads across unrelated publication

## Why

A valid asynchronous cold load currently treats a transient placement publication marker as source cancellation. An unrelated settings edit, including the global Key Lock start behavior, can therefore leave a restored pad unavailable with `cold load cancelled`.

## What Changes

Keep the original cold request and source capture through transient publication. Distinguish pending observation from definitive slot replacement or retirement, and publish only through the existing request, epoch, proposal and Native claim guards.

## Impact

Cold preparation and the existing worker publication path distinguish typed transient placement conflicts from terminal rejection. Original cancellation and source authority remain mandatory; bounded retries release control locks before waiting. The audio callback gains no logging, I/O, allocation, blocking or additional work. Device-free regressions exercise real publication and callback acknowledgement.

## Non-goals

No source recapture, settings reset, cache reset, new worker, scheduler or DSP path is introduced. Release installation and device acceptance remain separate integration work.
