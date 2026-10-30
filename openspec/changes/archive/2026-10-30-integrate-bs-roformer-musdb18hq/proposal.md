# BS-RoFormer MUSDB18HQ offline separation

## Why
The performer needs the actual four-source BS-RoFormer MUSDB18HQ model, CUDA execution
and a quick return to the existing Demucs separator without changing prepared playback.

## What Changes
- Add a bounded selector for the pinned BS-RoFormer MUSDB18HQ and Demucs htdemucs.
- Run the exact upstream network/checkpoint through an offline subprocess adapter.
- Install explicit, verified model assets separately; generation never downloads models.
- Keep both adapters behind the existing file/artifact request and shared publication path.
- Lock a CUDA-enabled Windows Torch runtime compatible with the available Blackwell GPU.

## Non-goals And Realtime Constraints
No automatic beat-analyzer cutover, model substitution, plugin hosting, Rust app port,
new live inference, or human listening/device acceptance. The callback performs no
model loading, inference, I/O, GIL/UI access, blocking or heavy allocation. Existing
source freshness, leases, complete sets and native acceptance remain authoritative.

## Impact
Settings/persistence, separator worker, dependencies/setup and regression validation.
