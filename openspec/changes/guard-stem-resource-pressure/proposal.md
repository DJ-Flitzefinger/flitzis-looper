## Why
The bounded stem pool limits jobs, but production admission does not observe host
commit or physical headroom. Existing NN216 instrumentation measured severe commit
pressure. A failed CUDA command can also start a second CPU attempt.

## What Changes
Reuse the measured Windows caller/system query for a conservative stem admission
and worker-entry policy. Stop critically pressured owned commands through existing
tree/read retirement and report a resource failure before backend CPU fallback.
Keep existing jobs, fairness, cancellation, readers and publication authority.

## Non-goals
No model RAM estimate, guaranteed OOM prevention, new scheduler/pool, foreign process
termination, Rust callback work, model run, automatic failed-job retry, or musical
acceptance. Unsupported hosts retain the bounded pool with an explicit status.
All sampling/locks/process retirement remain on control/offline worker paths; no
GIL, blocking, allocation or OS query is added to realtime audio.
