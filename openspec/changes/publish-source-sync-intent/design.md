## Existing authority

SourceBeatmapSelection owns the original Native delivery, immutable artifact and
fresh source qualification. InputMappingController publishes complete runtime
rows, retaining reserved rows through actual arrangement completion. CapturedPadStart,
TransportTimeline, SourcePlayback and the existing reader/mixer remain the sole
launch, time and render authorities. Existing Residency prepares physical coverage.

## Independent request and observed effect

The new `pad_source_sync_intent` table stores strict per-content objects with
`requested` and checked u64 `revision`. Its neutral value is false/zero. It is
independent of map selection revisions, map preparation, BPM Lock and Key Lock.
An explicit changed request advances its own revision without wrapping.

The complete source-map runtime row carries the separate request revision, an
original freshly qualified delivery or None, and the exact quarter extent only
when it comes from explicit automatic-bars intent. Deferred rows retain the same
full previous publication. GUI and device actions converge on one controller;
capture must observe current runtime intent, including same-frame GUI changes.

Callback feedback identifies actual current-source voice state, the adopted
request revision and map revision. Python compares this with current intent and
qualified map; it never derives effective audio from map-ready or enqueue success.
An unchanged old mapped voice may remain effective after a request changes, but
does not acknowledge the new request. Mixed or stale source observations cannot
become a successful current adoption. This block does not implement a live switch:
the existing actual start/retrigger path adopts supported changes.

## Extent and persistence

Automatic mapped loops use L=4*bars and S(B(actual start)+L). Rounded physical end
and resident-window acceptance do not redefine L. A manual physical end instead
uses the canonical B(end)-B(start) interval and never inherits old stored bars as
exact musical intent. Map unavailability/coverage failure preserves prior audio.

Copy gets independent SYNC intent and fresh source qualification; Move/Swap and
Bank reuse their existing content field authority. Neutral snapshot fields are
omitted to preserve historical canonical seals. Complete historical project images
may omit the new table, while malformed/unknown fields remain rejected. Damaged
new metadata retains the unsafe-save fence and original Config bytes. New-source
replacement/unload clears intent; same-content analysis preserves it.

## Delivery boundary

The currently supported dry envelope remains Key Lock off, total pitch zero,
speed one, BPM Lock on, stable actual master epoch and E=T. Unsupported Native,
transition and delayed tuples retain precise refusal and old audio. Automatic
fullmix acquisition, both Key Lock states/all required rates/pitches, continuous
adoption and the original acoustic/resource/Human gates remain open.
