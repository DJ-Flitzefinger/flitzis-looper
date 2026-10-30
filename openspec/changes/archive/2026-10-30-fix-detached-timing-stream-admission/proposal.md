# Fence detached timing completion to its stream

Accepted timing publication and saved timing restore retain a producer while work runs detached. Shutdown removes the stream, but the completion can still enqueue into its retained old producer and report a pending publication that cannot be consumed.

Require the originally captured producer to remain the current stream producer before first publication, preserving all source, timing, capacity and placement guards.

## Non-goals

No stream restart, device access, source recapture or callback work is introduced.
