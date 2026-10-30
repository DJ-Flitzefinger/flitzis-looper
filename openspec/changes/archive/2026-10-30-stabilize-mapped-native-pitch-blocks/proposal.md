# Stabilize mapped Native pitch blocks

The original-owned mapped Native experiment currently sets pitch at each caller
chunk. A chunk can finish an older partial512-frame input block and start another,
so a map-rate knot can change the pitch of input already queued by an earlier
caller. Worker and live rendering need one deterministic block parameter law.

Qualify actual mapped input before rendering, retain the pitch of each Native
block's first input frame until that block is shifted, and share this path across
preparation and live continuation. Retain existing scalar rendering semantics.
Extend only the internal original-owned birth experiment to Key Lock and unity
pitch; public mapped start/adoption restrictions remain. This is a necessary
component for complete SYNC/REALTIME, not its acceptance or a default cutover.
